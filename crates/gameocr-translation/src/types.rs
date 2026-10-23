use anyhow::{Result, ensure};
use gameocr_core::types::{Device, OcrBlock};
use serde::{Deserialize, Serialize};

pub const RUNTIME: &str = "llamacpp-translation";
pub const ADAPTER: &str = "hy-mt2";
pub const PROMPT_VERSION: u32 = 1;
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslationManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub runtime: String,
    pub adapter: String,
    pub revision: String,
    pub quantization: String,
    pub license: String,
    pub source: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub url: Option<String>,
    pub languages: Vec<String>,
    #[serde(default)]
    pub imported: bool,
    #[serde(default)]
    pub installed: bool,
}
pub fn safe_name(value: &str) -> bool {
    !value.is_empty() && value != "." && value != ".."
        && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
impl TranslationManifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema_version == 1, "不支持的翻译清单版本");
        ensure!(safe_name(&self.id) && safe_name(&self.file) && self.file.ends_with(".gguf"), "无效翻译模型路径");
        ensure!(self.size > 32 && self.sha256.len() == 64 && self.sha256.bytes().all(|c| c.is_ascii_hexdigit()), "翻译模型缺少完整校验信息");
        ensure!(!self.revision.is_empty() && !self.license.is_empty(), "翻译模型缺少版本或许可证");
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GenerationConfig {
    pub context_size: u32,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: i32,
    pub repetition_penalty: f32,
    pub seed: u32,
}
impl Default for GenerationConfig {
    fn default() -> Self { Self { context_size: 4096, max_tokens: 512, temperature: 0.7, top_p: 0.6, top_k: 20, repetition_penalty: 1.05, seed: 42 } }
}
impl GenerationConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!((512..=16384).contains(&self.context_size) && (1..=4096).contains(&self.max_tokens) && self.max_tokens < self.context_size, "翻译 Token 预算无效");
        ensure!(self.temperature.is_finite() && (0.0..=2.0).contains(&self.temperature), "翻译温度无效");
        ensure!(self.top_p.is_finite() && (0.01..=1.0).contains(&self.top_p) && (1..=200).contains(&self.top_k), "翻译采样参数无效");
        ensure!(self.repetition_penalty.is_finite() && (1.0..=2.0).contains(&self.repetition_penalty), "翻译重复惩罚无效");
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationLoad {
    pub manifest: TranslationManifest,
    pub path: String,
    pub device: Device,
    pub threads: u32,
    pub generation: GenerationConfig,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Term { pub source: String, pub target: String, #[serde(default)] pub category: String }
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct GameProfile {
    pub id: String,
    pub name: String,
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub background: String,
    pub glossary: Vec<Term>,
    pub version: u64,
    // User-confirmed executable name, independent of PID/window handle.
    pub window_process: String,
}
impl GameProfile {
    pub fn validate(&self) -> Result<()> {
        ensure!(safe_name(&self.id) && self.name.len() <= 256, "游戏 Profile 标识无效");
        ensure!(self.background.len() <= 8192 && self.style.len() <= 1024 && self.glossary.len() <= 200, "Profile 内容超出上限");
        ensure!(self.glossary.iter().all(|t| !t.source.trim().is_empty() && !t.target.trim().is_empty() && t.source.len() <= 256 && t.target.len() <= 256), "术语必须包含原文和译文，且不超过 256 字节");
        for l in [&self.source_language, &self.target_language] { ensure!(l.is_empty() || l == "auto" || crate::prompt::language_name(l).is_some(), "Profile 语言不受支持"); }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PromptOptions {
    pub source_language: String,
    pub target_language: String,
    pub style: String,
    pub glossary: Vec<Term>,
    pub background: String,
    pub context: Vec<String>,
    pub preserve_format: bool,
    pub glossary_version: u64,
    pub context_version: u64,
}
impl Default for PromptOptions {
    fn default() -> Self { Self { source_language:"auto".into(), target_language:"zh".into(), style:String::new(), glossary:Vec::new(), background:String::new(), context:Vec::new(), preserve_format:true, glossary_version:0, context_version:0 } }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationRequest { pub request_id: String, pub source_text: String, pub options: PromptOptions }
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TranslationMetrics {
    pub model_load_ms: f64,
    pub backend_prepare_ms: f64,
    pub prompt_ms: f64,
    pub queue_ms: f64,
    pub ttft_ms: Option<f64>,
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub tokens_per_second: Option<f64>,
    pub inference_ms: f64,
    pub total_ms: f64,
    pub text_processing_ms: f64,
    pub publish_ms: f64,
    pub ocr_ms: f64,
    pub ocr_to_translation_ms: f64,
    pub capture_to_translation_ms: Option<f64>,
    pub process_memory_mb: Option<f64>,
    pub free_vram_mb: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationResult {
    pub request_id: String,
    pub text: String,
    pub device: String,
    pub truncated: bool,
    pub stop_reason: String,
    pub metrics: TranslationMetrics,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Identity { pub frame_id: u64, pub generation: u64, pub epoch: u64, pub revision: u64 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextGroup { pub source_text: String, pub blocks: Vec<OcrBlock> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationResult {
    pub request_id: String,
    #[serde(flatten)] pub identity: Identity,
    pub source_model_id: String,
    pub translation_model_id: String,
    pub source_language: String,
    pub target_language: String,
    pub source_text: String,
    pub translated_text: String,
    pub source_block_ids: Vec<String>,
    pub source_blocks: Vec<OcrBlock>,
    pub glossary_version: u64,
    pub context_version: u64,
    pub cache_hit: bool,
    pub status: String,
    pub error: Option<String>,
    pub device: String,
    pub truncated: bool,
    pub metrics: TranslationMetrics,
}
