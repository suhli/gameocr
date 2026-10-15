//! Developer-only NDJSON driver for the exact executor used by the desktop.
use anyhow::{Context, Result};
use gameocr_app::executor::{Cancellation, Executor};
use gameocr_core::{
    engine::EngineConfig,
    parallel::{FrameInput, ParallelConfig},
    types::{OcrMode, now_ms},
};
use serde::Deserialize;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    resources: PathBuf,
    engine: EngineConfig,
    #[serde(default)]
    parallel: ParallelConfig,
    cache: PathBuf,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Request {
    Recognize {
        image: PathBuf,
        #[serde(rename = "frameId")]
        frame_id: u64,
    },
    Cancel {
        image: PathBuf,
        #[serde(rename = "delayMs")]
        delay_ms: u64,
    },
    Status,
    Shutdown,
}
async fn emit(value: serde_json::Value) -> Result<()> {
    let mut out = tokio::io::stdout();
    out.write_all(serde_json::to_string(&value)?.as_bytes())
        .await?;
    out.write_all(b"\n").await?;
    out.flush().await?;
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: executor-bench <config.json>")?;
    let config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
    let closing = Arc::new(AtomicBool::new(false));
    let start = Instant::now();
    let mut executor = match Executor::load(
        &config.resources,
        config.engine,
        &config.parallel,
        closing.clone(),
    )
    .await
    {
        Ok(executor) => executor,
        Err(error) => {
            emit(json!({"type":"error","phase":"load","message":format!("{error:#}"),"coldStartMs":start.elapsed().as_secs_f64()*1000.})).await?;
            return Ok(());
        }
    };
    emit(json!({"type":"ready","coldStartMs":start.elapsed().as_secs_f64()*1000.,"device":executor.device,"message":executor.message,"executor":executor.status.lock().clone()})).await?;
    let mut input = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = input.next_line().await? {
        let request: Request = serde_json::from_str(&line)?;
        if matches!(request, Request::Shutdown) {
            break;
        }
        if matches!(request, Request::Status) {
            emit(json!({"type":"status","executor":executor.status.lock().clone()})).await?;
            continue;
        }
        let (path, frame_id, cancel_delay) = match request {
            Request::Recognize { image, frame_id } => (image, frame_id, None),
            Request::Cancel { image, delay_ms } => (image, 999999, Some(delay_ms)),
            _ => unreachable!(),
        };
        // Corpus decoding is outside the measured frame pipeline: WGC supplies RGB.
        let image =
            tokio::task::spawn_blocking(move || image::open(path).map(|im| im.to_rgb8())).await??;
        let active = Arc::new(AtomicBool::new(true));
        let valid = active.clone();
        let cancel = Cancellation::new(frame_id, move || valid.load(Ordering::Acquire));
        let timer = cancel_delay.map(|ms| {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(ms)).await;
                active.store(false, Ordering::Release);
            })
        });
        let frame = Arc::new(FrameInput {
            id: frame_id,
            timestamp: now_ms(),
            generation: 1,
            image,
        });
        let start = Instant::now();
        let result = executor
            .recognize(frame, OcrMode::Grounding, &config.cache, cancel)
            .await;
        if let Some(timer) = timer {
            timer.abort();
        }
        let wall = start.elapsed().as_secs_f64() * 1000.;
        let status = executor.status.lock().clone();
        match result {Ok(result)=>emit(json!({"type":"result","result":result,"wallMs":wall,"executor":status})).await?,Err(error)=>emit(json!({"type":"error","phase":"recognize","message":format!("{error:#}"),"wallMs":wall,"executor":status})).await?}
    }
    executor.shutdown().await;
    closing.store(true, Ordering::Release);
    emit(json!({"type":"closed","executor":executor.status.lock().clone()})).await?;
    Ok(())
}
