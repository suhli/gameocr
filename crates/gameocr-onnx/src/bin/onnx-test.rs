use clap::Parser;
use gameocr_core::{
    engine::{EngineConfig, EngineFactory, OcrEngineFactory, OnnxConfig, RuntimeConfig},
    manifest::ModelManifest,
    types::*,
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    manifest: std::path::PathBuf,
    #[arg(long)]
    image: std::path::PathBuf,
    #[arg(long, value_enum, default_value = "cpu")]
    device: Device,
    #[arg(long, default_value_t = 4)]
    threads: u32,
    #[arg(long, default_value_t = 1)]
    repeat: u32,
    #[arg(long)]
    output: Option<std::path::PathBuf>,
}
fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    anyhow::ensure!((1..=100).contains(&args.repeat), "Repeat must be 1..100");
    let manifest = ModelManifest::from_slice(&std::fs::read(&args.manifest)?)?;
    let mut factory = EngineFactory::default();
    factory.register("ppocrv6", gameocr_onnx::PaddleOcrAdapter::create);
    let config = EngineConfig {
        manifest,
        directory: args
            .manifest
            .canonicalize()?
            .parent()
            .unwrap()
            .to_string_lossy()
            .into(),
        device: args.device,
        threads: args.threads,
        inference: RuntimeConfig::Onnx(OnnxConfig::default()),
    };
    let mut engine = factory.create(config)?;
    let mut results = Vec::new();
    for index in 0..args.repeat {
        results.push(engine.recognize(&args.image, index as u64, now_ms(), OcrMode::Grounding)?);
    }
    engine.shutdown();
    let json = serde_json::to_string_pretty(&results)?;
    println!("{json}");
    if let Some(path) = args.output {
        std::fs::write(path, json)?;
    }
    Ok(())
}
