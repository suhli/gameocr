use anyhow::Result;
use clap::Parser;
use gameocr_core::types::Device;
use gameocr_translation::{catalog, engine::TranslationFactory, types::*};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    model: String,
    #[arg(long, value_enum, default_value = "cpu")]
    device: Device,
    #[arg(long, default_value_t = 4)]
    threads: u32,
    #[arg(long)]
    input: String,
    #[arg(long)]
    output: String,
    #[arg(long, default_value_t = 0.7)]
    temperature: f32,
    #[arg(long, default_value_t = 512)]
    max_tokens: u32,
}
fn main() -> Result<()> {
    let a = Args::parse();
    let input = std::fs::read_to_string(&a.input)?;
    let requests: Vec<serde_json::Value> = serde_json::from_str(&input)?;
    let config = TranslationLoad {
        manifest: catalog::hy_mt2(),
        path: a.model,
        device: a.device,
        threads: a.threads,
        generation: GenerationConfig {
            temperature: a.temperature,
            max_tokens: a.max_tokens,
            ..Default::default()
        },
    };
    let mut engine = TranslationFactory::create(config.clone())?;
    let mut output = Vec::new();
    for item in requests {
        let request: TranslationRequest = serde_json::from_value(item.clone())?;
        eprintln!("Translating {}", request.request_id);
        let r = engine.translate(request.clone())?;
        output.push(serde_json::json!({"request":request,"result":r,"reference":item.get("reference"),"category":item.get("category"),"humanReview":null}));
    }
    std::fs::write(
        a.output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"schemaVersion":1,"model":config.manifest,"generation":config.generation,"results":output}),
        )?,
    )?;
    Ok(())
}
