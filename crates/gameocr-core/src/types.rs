use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OcrBlock {
    pub id: String,
    pub text: String,
    pub label: Option<String>,
    pub bbox: Option<[f64; 4]>,
    #[serde(default)]
    pub confidence: Option<f32>,
    #[serde(default)]
    pub polygon: Option<Vec<Point>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub preprocessing_ms: f64,
    pub inference_ms: f64,
    pub parsing_ms: f64,
    pub total_ms: f64,
    #[serde(default)]
    pub detection_ms: Option<f64>,
    #[serde(default)]
    pub recognition_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub frame_id: u64,
    pub timestamp: u64,
    pub source_width: u32,
    pub source_height: u32,
    pub blocks: Vec<OcrBlock>,
    pub raw_output: String,
    pub metrics: Metrics,
    pub device: String,
    pub truncated: bool,
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub engine_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OcrMode {
    Plain,
    Grounding,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, clap::ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    Auto,
    Cpu,
    Cuda,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceConfig {
    pub model: String,
    pub mmproj: String,
    pub device: Device,
    pub threads: u32,
    pub context_size: u32,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_image_edge: u32,
}

impl InferenceConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!((1..=128).contains(&self.threads), "线程数必须为 1–128");
        anyhow::ensure!(
            (2048..=32768).contains(&self.context_size),
            "上下文大小必须为 2048–32768"
        );
        anyhow::ensure!(
            self.max_tokens > 0 && self.max_tokens < self.context_size,
            "输出 Token 数必须小于上下文大小"
        );
        anyhow::ensure!(
            self.temperature.is_finite() && (0.0..=2.0).contains(&self.temperature),
            "温度必须为 0–2"
        );
        anyhow::ensure!(
            (224..=2048).contains(&self.max_image_edge),
            "图像长边必须为 224–2048"
        );
        Ok(())
    }
}

/// NDJSON over inherited stdio; image bytes stay in an application-owned file.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerRequest {
    LoadEngine {
        protocol_version: u32,
        config: crate::engine::EngineConfig,
    },
    Load {
        config: InferenceConfig,
    },
    Recognize {
        image: String,
        frame_id: u64,
        timestamp: u64,
        mode: OcrMode,
    },
    Unload,
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WorkerResponse {
    Ready { device: String, load_ms: f64 },
    Result { result: OcrResult },
    Unloaded,
    Error { message: String },
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_history_json_remains_readable() {
        let value = serde_json::json!({"frameId":1,"timestamp":2,"sourceWidth":800,"sourceHeight":320,"blocks":[{"id":"1","text":"hello","label":null,"bbox":null}],"rawOutput":"hello","metrics":{"preprocessingMs":1,"inferenceMs":2,"parsingMs":3,"totalMs":6},"device":"CPU","truncated":false});
        let result: OcrResult = serde_json::from_value(value).unwrap();
        assert!(result.runtime.is_none());
        assert!(result.blocks[0].confidence.is_none());
        assert!(result.blocks[0].polygon.is_none());
    }
}
