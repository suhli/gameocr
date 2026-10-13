use clap::Parser;
use gameocr_core::{inference::OcrEngine, types::*};
use std::{path::PathBuf, time::Instant};

#[derive(Parser)]
#[command(
    about = "真实 LightOnOCR-3 GGUF + mmproj 兼容性验证（stdout 为 JSON，stderr 为原生日志）"
)]
struct Args {
    #[arg(long)]
    model: String,
    #[arg(long)]
    mmproj: String,
    #[arg(long)]
    image: PathBuf,
    #[arg(long, value_enum, default_value = "grounding")]
    mode: OcrMode,
    #[arg(long, value_enum, default_value = "auto")]
    device: Device,
    #[arg(long, default_value_t = 4)]
    threads: u32,
    #[arg(long, default_value_t = 8192)]
    context_size: u32,
    #[arg(long, default_value_t = 2048)]
    max_tokens: u32,
    #[arg(long, default_value_t = 0.1)]
    temperature: f32,
    #[arg(long, default_value_t = 1280)]
    max_image_edge: u32,
    #[arg(long, default_value_t = 1)]
    repeat: u32,
}
fn main() -> anyhow::Result<()> {
    let a = Args::parse();
    let start = Instant::now();
    let mut engine = OcrEngine::load(InferenceConfig {
        model: a.model,
        mmproj: a.mmproj,
        device: a.device,
        threads: a.threads,
        context_size: a.context_size,
        max_tokens: a.max_tokens,
        temperature: a.temperature,
        max_image_edge: a.max_image_edge,
    })?;
    eprintln!(
        "Loaded {} in {:.0} ms",
        engine.device,
        start.elapsed().as_secs_f64() * 1000.
    );
    for i in 0..a.repeat {
        println!(
            "{}",
            serde_json::to_string_pretty(&engine.recognize(
                &a.image,
                i as u64,
                now_ms(),
                a.mode
            )?)?
        );
    }
    Ok(())
}
