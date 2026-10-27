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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::BufReader,
        net::{TcpListener, TcpStream},
        sync::atomic::AtomicUsize,
        time::Duration,
    };
    struct DelayedEngine {
        started: mpsc::Sender<String>,
        steps: Arc<AtomicUsize>,
    }
    impl crate::engine::TranslationEngine for DelayedEngine {
        fn device(&self) -> &str {
            "cpu"
        }
        fn translate_cancellable(
            &mut self,
            r: TranslationRequest,
            cancel: Arc<AtomicBool>,
        ) -> Result<GenerationResult> {
            self.started.send(r.request_id.clone())?;
            if r.source_text == "slow" {
                for _ in 0..2000 {
                    if cancel.load(Ordering::Acquire) {
                        return Ok(GenerationResult::cancelled(r.request_id, "cpu".into()));
                    }
                    self.steps.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            Ok(GenerationResult {
                request_id: r.request_id,
                text: "complete".into(),
                device: "cpu".into(),
                truncated: false,
                stop_reason: "eos".into(),
                metrics: Default::default(),
            })
        }
    }
    fn send(stream: &mut TcpStream, request: WorkerRequest) {
        writeln!(stream, "{}", serde_json::to_string(&request).unwrap()).unwrap();
    }
    fn receive(reader: &mut BufReader<TcpStream>) -> WorkerResponse {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn translate(id: &str, text: &str) -> WorkerRequest {
        WorkerRequest::Translate {
            request: TranslationRequest {
                request_id: id.into(),
                source_text: text.into(),
                options: Default::default(),
            },
        }
    }
    #[test]
    fn running_generation_stops_and_next_request_reuses_engine_without_stale_controls() {
        for ending in ["cancel", "unload", "shutdown"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let (server, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(client.try_clone().unwrap());
            let (started, rx) = mpsc::channel();
            let steps = Arc::new(AtomicUsize::new(0));
            let steps_copy = steps.clone();
            let loads = Arc::new(AtomicUsize::new(0));
            let loads_copy = loads.clone();
            let join = std::thread::spawn(move || {
                serve_with(BufReader::new(server.try_clone().unwrap()), server, |_| {
                    loads_copy.fetch_add(1, Ordering::Relaxed);
                    Ok(Box::new(DelayedEngine {
                        started: started.clone(),
                        steps: steps_copy.clone(),
                    }))
                })
            });
            send(
                &mut client,
                WorkerRequest::Load {
                    protocol_version: PROTOCOL_VERSION,
                    config: TranslationLoad {
                        manifest: crate::catalog::hy_mt2(),
                        path: "test-double".into(),
                        device: gameocr_core::types::Device::Cpu,
                        threads: 1,
                        generation: Default::default(),
                    },
                },
            );
            assert!(matches!(
                receive(&mut reader),
                WorkerResponse::Ready {
                    supports_cancellation: true,
                    ..
                }
            ));
            send(&mut client, translate("A", "slow"));
            assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), "A");
            send(
                &mut client,
                match ending {
                    "cancel" => WorkerRequest::Cancel {
                        request_id: "A".into(),
                    },
                    "unload" => WorkerRequest::Unload,
                    _ => WorkerRequest::Shutdown,
                },
            );
            let WorkerResponse::Result { result } = receive(&mut reader) else {
                panic!("expected cancellation response")
            };
            assert_eq!(result.stop_reason, "cancelled");
            assert!(
                steps.load(Ordering::Relaxed) < 2000,
                "old generation ran to its budget"
            );
            if ending == "cancel" {
                send(
                    &mut client,
                    WorkerRequest::Cancel {
                        request_id: "A".into(),
                    },
                );
                send(&mut client, translate("C", "latest"));
                let WorkerResponse::Result { result } = receive(&mut reader) else {
                    panic!("expected latest result")
                };
                assert_eq!(result.request_id, "C");
                assert_eq!(result.stop_reason, "eos");
                assert_eq!(loads.load(Ordering::Relaxed), 1);
            } else {
                assert!(matches!(receive(&mut reader), WorkerResponse::Unloaded));
            }
            if ending != "shutdown" {
                send(&mut client, WorkerRequest::Shutdown);
                assert!(matches!(receive(&mut reader), WorkerResponse::Unloaded));
            }
            join.join().unwrap().unwrap();
        }
    }
    #[test]
    fn controls_match_identity_and_input_and_queue_remain_bounded() {
        let token = Arc::new(AtomicBool::new(false));
        let active = Arc::new(Mutex::new(Some(("active".into(), token.clone()))));
        let (tx, _rx) = mpsc::sync_channel(1);
        read_commands(
            &b"{\"type\":\"cancel\",\"request_id\":\"stale\"}\n"[..],
            &tx,
            &active,
        )
        .unwrap();
        assert!(!token.load(Ordering::Acquire));
        read_commands(
            &b"{\"type\":\"cancel\",\"request_id\":\"active\"}\n"[..],
            &tx,
            &active,
        )
        .unwrap();
        assert!(token.load(Ordering::Acquire));
        assert!(read_commands(&vec![b'x'; 262145][..], &tx, &active).is_err());
        assert!(
            read_commands(
                &b"{\"type\":\"unload\"}\n{\"type\":\"unload\"}\n"[..],
                &tx,
                &active
            )
            .is_err()
        );
        let old: WorkerResponse =
            serde_json::from_str(r#"{"type":"ready","device":"cpu","load_ms":1}"#).unwrap();
        assert!(matches!(
            old,
            WorkerResponse::Ready {
                supports_cancellation: false,
                ..
            }
        ));
    }
}
