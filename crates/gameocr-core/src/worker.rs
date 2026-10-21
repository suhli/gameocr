use crate::{
    engine::{EngineFactory, OcrEngine, OcrEngineFactory},
    types::*,
};
use anyhow::{Result, ensure};
use std::{
    io::{self, BufRead, Write},
    path::Path,
    time::Instant,
};

pub fn serve(factory: EngineFactory) -> Result<()> {
    let mut engine: Option<Box<dyn OcrEngine>> = None;
    let mut output = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let request = serde_json::from_str::<WorkerRequest>(&line?);
        let shutdown = matches!(&request, Ok(WorkerRequest::Shutdown));
        let response: Result<WorkerResponse> = (|| match request? {
            WorkerRequest::RecognizeShared {
                frame,
                timestamp,
                mode,
            } => {
                let start = Instant::now();
                let loaded = engine
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Model not loaded"))?;
                ensure!(loaded.capabilities().supports(mode), "Unsupported OCR mode");
                let image = crate::shared_frame::read(&frame)?;
                let copy_ms = start.elapsed().as_secs_f64() * 1000.;
                loaded.set_frame_context(Some(frame.context));
                let mut result = loaded.recognize_rgb(image, frame.frame_id, timestamp, mode)?;
                result.metrics.total_ms = start.elapsed().as_secs_f64() * 1000.;
                result.metrics.preprocessing_ms += copy_ms;
                if let Some(p) = result.metrics.pipeline.as_mut() {
                    p.decode_ms += copy_ms;
                }
                Ok(WorkerResponse::Result { result })
            }
            WorkerRequest::RecognizeTile {
                image,
                frame_id,
                timestamp,
                mode,
                task,
                detection_resize,
            } => {
                let loaded = engine
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Model not loaded"))?;
                ensure!(loaded.capabilities().supports(mode), "Unsupported OCR mode");
                Ok(WorkerResponse::TileResult {
                    task,
                    result: loaded.recognize_tile(
                        Path::new(&image),
                        frame_id,
                        timestamp,
                        mode,
                        detection_resize,
                    )?,
                })
            }
            WorkerRequest::LoadEngine {
                protocol_version,
                config,
            } => {
                ensure!(protocol_version == 2, "Unsupported worker protocol version");
                if let Some(old) = engine.take() {
                    old.shutdown();
                }
                let start = Instant::now();
                let loaded = factory.create(config)?;
                let device = loaded.device().to_owned();
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
                cache_context,
            } => {
                let loaded = engine
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("Model not loaded"))?;
                ensure!(loaded.capabilities().supports(mode), "Unsupported OCR mode");
                loaded.set_frame_context(cache_context);
                Ok(WorkerResponse::Result {
                    result: loaded.recognize(Path::new(&image), frame_id, timestamp, mode)?,
                })
            }
            WorkerRequest::Unload | WorkerRequest::Shutdown => {
                if let Some(old) = engine.take() {
                    old.shutdown();
                }
                Ok(WorkerResponse::Unloaded)
            }
            WorkerRequest::Load { .. } => anyhow::bail!(
                "ONNX requires loadEngine protocol v2; legacy load is supported by the llama.cpp worker"
            ),
        })();
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
    if let Some(old) = engine {
        old.shutdown();
    }
    Ok(())
}
