use crate::types::*;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerRequest {
    Load {
        protocol_version: u32,
        config: TranslationLoad,
    },
    Translate {
        request: TranslationRequest,
    },
    Cancel {
        request_id: String,
    },
    Unload,
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerResponse {
    Ready {
        device: String,
        load_ms: f64,
        #[serde(default)]
        supports_cancellation: bool,
    },
    Result {
        result: GenerationResult,
    },
    Unloaded,
    Error {
        message: String,
    },
}
#[cfg(feature = "inference")]
pub fn serve() -> anyhow::Result<()> {
    serve_with(
        std::io::BufReader::new(std::io::stdin()),
        std::io::stdout().lock(),
        crate::engine::TranslationFactory::create,
    )
}
type Active = Arc<Mutex<Option<(String, Arc<AtomicBool>)>>>;
type Command = (WorkerRequest, Arc<AtomicBool>);

// Inference cannot block this reader. Controls bypass the bounded command queue.
fn read_commands(
    mut input: impl BufRead,
    tx: &mpsc::SyncSender<Result<Command>>,
    active: &Active,
) -> Result<()> {
    loop {
        let mut line = Vec::new();
        let count = (&mut input).take(262145).read_until(b'\n', &mut line)?;
        if count == 0 {
            return Ok(());
        }
        ensure!(
            count <= 262144 && line.last() == Some(&b'\n'),
            "Translation IPC message too large or incomplete"
        );
        let command: WorkerRequest = serde_json::from_slice(&line)?;
        let token = Arc::new(AtomicBool::new(false));
        let shutdown = matches!(command, WorkerRequest::Shutdown);
        {
            let mut slot = active
                .lock()
                .map_err(|_| anyhow::anyhow!("Translation control lock poisoned"))?;
            match &command {
                WorkerRequest::Cancel { request_id } => {
                    if let Some((_, flag)) = slot.as_ref().filter(|(id, _)| id == request_id) {
                        flag.store(true, Ordering::Release);
                    }
                    continue;
                }
                WorkerRequest::Translate { request } => {
                    ensure!(slot.is_none(), "Translation request already active");
                    *slot = Some((request.request_id.clone(), token.clone()));
                }
                WorkerRequest::Unload | WorkerRequest::Shutdown => {
                    if let Some((_, flag)) = slot.as_ref() {
                        flag.store(true, Ordering::Release);
                    }
                }
                _ => {}
            }
        }
        tx.try_send(Ok((command, token)))
            .map_err(|_| anyhow::anyhow!("Translation command queue full or closed"))?;
        if shutdown {
            return Ok(());
        }
    }
}
pub fn serve_with(
    input: impl BufRead + Send + 'static,
    mut output: impl Write,
    mut create: impl FnMut(TranslationLoad) -> Result<Box<dyn crate::engine::TranslationEngine>>,
) -> Result<()> {
    let active: Active = Arc::new(Mutex::new(None));
    let control = active.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("translation-controls".into())
        .spawn(move || {
            let result = read_commands(input, &tx, &control);
            if let Ok(slot) = control.lock() {
                if let Some((_, flag)) = slot.as_ref() {
                    flag.store(true, Ordering::Release);
                }
            }
            if let Err(error) = result {
                let _ = tx.send(Err(error));
            }
        })?;
    let mut engine: Option<Box<dyn crate::engine::TranslationEngine>> = None;
    for command in rx {
        let (request, token) = command?;
        let shutdown = matches!(&request, WorkerRequest::Shutdown);
        let request_id = match &request {
            WorkerRequest::Translate { request } => Some(request.request_id.clone()),
            _ => None,
        };
        let response: anyhow::Result<WorkerResponse> = (|| match request {
            WorkerRequest::Load {
                protocol_version,
                config,
            } => {
                anyhow::ensure!(
                    protocol_version == PROTOCOL_VERSION,
                    "Unsupported translation protocol"
                );
                engine.take();
                let start = std::time::Instant::now();
                let e = create(config)?;
                let device = e.device().to_owned();
                engine = Some(e);
                Ok(WorkerResponse::Ready {
                    device,
                    load_ms: start.elapsed().as_secs_f64() * 1000.,
                    supports_cancellation: true,
                })
            }
            WorkerRequest::Translate { request } => Ok(WorkerResponse::Result {
                result: engine
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Translation model not loaded"))?
                    .translate_cancellable(request, token)?,
            }),
            WorkerRequest::Unload | WorkerRequest::Shutdown => {
                engine.take();
                Ok(WorkerResponse::Unloaded)
            }
            WorkerRequest::Cancel { .. } => unreachable!("controls handled by reader"),
        })();
        if let Some(id) = request_id {
            let mut slot = active
                .lock()
                .map_err(|_| anyhow::anyhow!("Translation control lock poisoned"))?;
            if slot.as_ref().is_some_and(|(current, _)| current == &id) {
                slot.take();
            }
        }
        writeln!(
            output,
            "{}",
            serde_json::to_string(&response.unwrap_or_else(|e| WorkerResponse::Error {
                message: format!("{e:#}")
            }))?
        )?;
        output.flush()?;
        if shutdown {
            break;
        }
    }
    Ok(())
}
