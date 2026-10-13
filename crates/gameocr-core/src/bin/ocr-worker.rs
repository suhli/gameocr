use gameocr_core::{inference::OcrEngine, types::*};
use std::{
    io::{self, BufRead, Write},
    path::Path,
    time::Instant,
};
fn main() -> anyhow::Result<()> {
    let mut engine: Option<OcrEngine> = None;
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
        let response: anyhow::Result<WorkerResponse> = (|| match request {
            WorkerRequest::Load { config } => {
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
                result: engine
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("模型未加载"))?
                    .recognize(Path::new(&image), frame_id, timestamp, mode)?,
            }),
            WorkerRequest::Unload => {
                engine = None;
                Ok(WorkerResponse::Unloaded)
            }
            WorkerRequest::Shutdown => {
                std::process::exit(0);
            }
        })();
        let response = response.unwrap_or_else(|e| WorkerResponse::Error {
            message: format!("{e:#}"),
        });
        writeln!(output, "{}", serde_json::to_string(&response)?)?;
        output.flush()?;
    }
    Ok(())
}
