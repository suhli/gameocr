//! Development-only exercise of the same official downloader used by the desktop.
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
#[tokio::main]
async fn main() -> Result<()> {
    let root = PathBuf::from(std::env::args().nth(1).context("resource root")?);
    let runtime = std::env::args().nth(2).unwrap_or("onnx".into());
    let manager = gameocr_app::cuda_runtime::RuntimeManager::default();
    let started = Instant::now();
    manager
        .ensure(&root, &runtime, Arc::new(AtomicBool::new(false)), |p| {
            eprintln!("{} {} {} / {}", p.state, p.component, p.received, p.total)
        })
        .await?;
    println!(
        "{}",
        serde_json::json!({"elapsedMs":started.elapsed().as_secs_f64()*1000., "paths":gameocr_app::cuda_runtime::paths(&root,&runtime)?, "status":manager.status()})
    );
    Ok(())
}
