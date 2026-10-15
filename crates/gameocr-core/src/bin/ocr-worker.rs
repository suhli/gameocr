use gameocr_core::engine::{EngineFactory, OcrEngineFactory};
use gameocr_core::{inference::OcrEngine, types::*};
use std::{
    io::{self, BufRead, Write},
    path::Path,
    time::Instant,
};
fn main() -> anyhow::Result<()> {
    let mut engine: Option<OcrEngine> = None;
    let mut adapted: Option<Box<dyn gameocr_core::engine::OcrEngine>> = None;
    let mut factory = EngineFactory::default();
    factory.register(
        "lightonocr",
        gameocr_core::inference::LightOnOcrAdapter::create,
    );
    let stdout = io::stdout();
    let mut output = stdout.lock();
    for line in io::stdin().lock().lines() {
        let request = match serde_json::from_str::<WorkerRequest>(&line?) {
            Ok(r) => r,
            Err(e) => {
                writeln!(
                    output,
                    "{}",
                    serde_json::to_string(&WorkerResponse::Error {
                        message: e.to_string()
                    })?
                )?;
                output.flush()?;
                continue;
            }
        };
        let shutdown = matches!(&request, WorkerRequest::Shutdown);
        let response: anyhow::Result<WorkerResponse> = (|| match request {
            WorkerRequest::RecognizeTile { .. } => {
                anyhow::bail!("GGUF uses sequential full-frame inference")
            }
            WorkerRequest::LoadEngine {
                protocol_version,
                config,
            } => {
                anyhow::ensure!(protocol_version == 2, "Unsupported protocol version");
                engine = None;
                if let Some(old) = adapted.take() {
                    old.shutdown();
                }
                let start = Instant::now();
                let loaded = factory.create(config)?;
                let device = loaded.device().to_owned();
                adapted = Some(loaded);
                Ok(WorkerResponse::Ready {
                    device,
                    load_ms: start.elapsed().as_secs_f64() * 1000.,
                })
            }
            WorkerRequest::Load { config } => {
                if let Some(old) = adapted.take() {
                    old.shutdown();
                }
                engine = None;
                let start = Instant::now();
                let loaded = OcrEngine::load(config)?;
                let device = loaded.device.clone();
                engine = Some(loaded);
                Ok(WorkerResponse::Ready {
                    device,
                    load_ms: start.elapsed().as_secs_f64() * 1000.,
                })
            }
            WorkerRequest::Recognize {
                image,
                frame_id,
                timestamp,
                mode,
            } => Ok(WorkerResponse::Result {
                result: if let Some(e) = adapted.as_mut() {
                    e.recognize(Path::new(&image), frame_id, timestamp, mode)?
                } else {
                    engine
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("模型未加载"))?
                        .recognize(Path::new(&image), frame_id, timestamp, mode)?
                },
            }),
            WorkerRequest::Unload => {
                if let Some(old) = adapted.take() {
                    old.shutdown();
                }
                engine = None;
                Ok(WorkerResponse::Unloaded)
            }
            WorkerRequest::Shutdown => {
                engine = None;
                if let Some(old) = adapted.take() {
                    old.shutdown();
                }
                Ok(WorkerResponse::Unloaded)
            }
        })();
        let response = response.unwrap_or_else(|e| WorkerResponse::Error {
            message: format!("{e:#}"),
        });
        writeln!(output, "{}", serde_json::to_string(&response)?)?;
        output.flush()?;
        if shutdown {
            break;
        }
    }
    Ok(())
}
