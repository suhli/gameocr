use anyhow::{Context, Result, ensure};
use gameocr_core::types::{Device, OcrMode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub capture_fps: u32,
    pub ocr_interval_ms: u64,
    pub change_detection: bool,
    pub change_threshold: f64,
    pub mode: OcrMode,
    pub device: Device,
    pub threads: u32,
    pub context_size: u32,
    pub max_tokens: u32,
    pub temperature: f32,
    pub max_image_edge: u32,
    pub onnx: gameocr_core::engine::OnnxConfig,
    #[serde(flatten)]
    pub parallel: gameocr_core::parallel::ParallelConfig,
    #[serde(default)]
    pub execution_mode: Option<gameocr_core::adaptive::ExecutionMode>,
    pub adaptive: gameocr_core::adaptive::AdaptiveConfig,
    pub model_dir: PathBuf,
    pub adb_path: String,
    pub adb_serial: String,
    pub capture_source: String,
    pub theme: String,
    pub recent_window: Option<String>,
    pub history_limit: usize,
    pub download_mirror: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            capture_fps: 10,
            ocr_interval_ms: 1000,
            change_detection: true,
            change_threshold: 0.03,
            mode: OcrMode::Grounding,
            device: Device::Auto,
            threads: 4,
            context_size: 8192,
            max_tokens: 2048,
            temperature: 0.1,
            max_image_edge: 1280,
            onnx: Default::default(),
            parallel: Default::default(),
            execution_mode: Some(Default::default()),
            adaptive: Default::default(),
            model_dir: data_dir().join("models"),
            adb_path: "adb".into(),
            adb_serial: String::new(),
            capture_source: "window".into(),
            theme: "dark".into(),
            recent_window: None,
            history_limit: 30,
            download_mirror: "https://huggingface.co".into(),
        }
    }
}
pub fn data_dir() -> PathBuf {
    // Also lets integration tests run with isolated user data inside their temporary directory.
    if let Some(path) = std::env::var_os("GAMEOCR_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return path;
    }
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("GameOCR")
}
impl Settings {
    pub fn selected_mode(&self) -> gameocr_core::adaptive::ExecutionMode {
        use gameocr_core::adaptive::ExecutionMode;
        self.execution_mode
            .unwrap_or(if self.parallel.enable_parallel_inference {
                ExecutionMode::Manual
            } else {
                ExecutionMode::Single
            })
    }
    pub fn normalize(&mut self) {
        let mode = self.selected_mode();
        self.execution_mode = Some(mode);
        self.parallel.enable_parallel_inference =
            mode == gameocr_core::adaptive::ExecutionMode::Manual;
    }
    pub fn engine_config(
        &self,
        manifest: gameocr_core::manifest::ModelManifest,
        device: Device,
    ) -> Result<gameocr_core::engine::EngineConfig> {
        use gameocr_core::engine::{EngineConfig, RuntimeConfig};
        // Configuration strategies live at this boundary, not in Service.
        let inference = match manifest.runtime.as_str() {
            "llamacpp" => RuntimeConfig::Llamacpp(gameocr_core::types::InferenceConfig {
                model: String::new(),
                mmproj: String::new(),
                device,
                threads: self.threads,
                context_size: self.context_size,
                max_tokens: self.max_tokens,
                temperature: self.temperature,
                max_image_edge: self.max_image_edge,
            }),
            "onnx" => RuntimeConfig::Onnx(self.onnx.clone()),
            _ => anyhow::bail!("Unregistered runtime configuration"),
        };
        Ok(EngineConfig {
            manifest,
            directory: self.model_dir.to_string_lossy().into(),
            device,
            threads: self.threads,
            inference,
        })
    }

    pub fn validate(&self) -> Result<()> {
        self.onnx.validate()?;
        let mut parallel = self.parallel.clone();
        parallel.enable_parallel_inference =
            self.selected_mode() == gameocr_core::adaptive::ExecutionMode::Manual;
        parallel.validate(
            self.device,
            std::thread::available_parallelism().map_or(1, |n| n.get()),
        )?;
        // Resource settings are global; enforce CPU/Auto only when loading ONNX
        // Adaptive so a previous ONNX choice never changes GGUF's CUDA behavior.
        if self.selected_mode() == gameocr_core::adaptive::ExecutionMode::Adaptive {
            self.adaptive.validate(Device::Cpu)?;
        }
        ensure!(
            (1..=60).contains(&self.capture_fps),
            "捕获频率必须为 1–60 FPS"
        );
        ensure!(
            (10..=60000).contains(&self.ocr_interval_ms),
            "OCR 间隔必须为 10–60000 ms"
        );
        ensure!(
            self.change_threshold.is_finite() && (0.0..=1.0).contains(&self.change_threshold),
            "变化阈值必须为 0–1"
        );
        ensure!(
            (1..=100).contains(&self.history_limit),
            "历史上限必须为 1–100"
        );
        ensure!(
            self.capture_source == "window" || self.capture_source == "adb",
            "未知捕获方式"
        );
        ensure!(self.theme == "dark" || self.theme == "light", "未知主题");
        ensure!(self.model_dir.is_absolute(), "模型目录必须是绝对路径");
        ensure!(
            ["https://huggingface.co", "https://hf-mirror.com"]
                .contains(&self.download_mirror.as_str()),
            "仅支持 Hugging Face 或 HF-Mirror 下载源"
        );
        gameocr_core::types::InferenceConfig {
            model: String::new(),
            mmproj: String::new(),
            device: self.device,
            threads: self.threads,
            context_size: self.context_size,
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            max_image_edge: self.max_image_edge,
        }
        .validate()
    }
    pub fn read(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut value: Self = serde_json::from_slice(&std::fs::read(path)?)
            .context("设置文件损坏，请恢复或重命名 settings.json")?;
        value.normalize();
        value.validate()?;
        Ok(value)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        atomic_json(path, self)
    }
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_mode_migration_is_opt_in() {
        use gameocr_core::adaptive::ExecutionMode;
        for (json, expected) in [
            (r#"{}"#, ExecutionMode::Single),
            (r#"{"enableParallelInference":true}"#, ExecutionMode::Manual),
            (
                r#"{"enableParallelInference":true,"executionMode":"single"}"#,
                ExecutionMode::Single,
            ),
        ] {
            let mut s: Settings = serde_json::from_str(json).unwrap();
            s.normalize();
            assert_eq!(s.selected_mode(), expected);
            assert_eq!(
                s.parallel.enable_parallel_inference,
                expected == ExecutionMode::Manual
            );
            let saved = serde_json::to_value(&s).unwrap();
            assert_eq!(saved["adaptive"]["maxCpuWorkers"], 2);
            assert_eq!(saved["adaptive"]["maxGpuWorkers"], 0);
        }
    }
    #[test]
    fn persist_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings::read(&path).unwrap();
        s.capture_fps = 25;
        s.save(&path).unwrap();
        assert_eq!(Settings::read(&path).unwrap().capture_fps, 25);
        s.capture_fps = 0;
        assert!(s.save(&path).is_err());
        assert_eq!(Settings::read(&path).unwrap().capture_fps, 25);
        std::fs::write(&path, "broken").unwrap();
        assert!(Settings::read(&path).is_err());
    }
    #[test]
    fn old_settings_default_to_sequential_and_parallel_fields_persist() {
        let old: Settings = serde_json::from_str(r#"{"captureFps":12,"device":"cpu"}"#).unwrap();
        assert!(!old.parallel.enable_parallel_inference);
        assert!(!old.parallel.enable_cpu_workers);
        let mut next = old;
        next.parallel.enable_parallel_inference = true;
        let value = serde_json::to_value(&next).unwrap();
        assert_eq!(value["enableParallelInference"], true);
        assert_eq!(value["cpuWorkerCount"], 1);
        assert!(
            serde_json::from_value::<Settings>(value)
                .unwrap()
                .parallel
                .enable_parallel_inference
        );
    }
}
