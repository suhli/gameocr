//! Model contracts and process-independent factory. Native implementations are
//! registered only inside their worker, never linked into the desktop process.
use crate::{manifest::ModelManifest, types::*};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrCapabilities {
    pub plain: bool,
    pub grounding: bool,
    pub native_coordinates: bool,
    pub confidence: bool,
    pub generation: bool,
}
impl OcrCapabilities {
    pub fn lighton() -> Self {
        Self {
            plain: true,
            grounding: true,
            native_coordinates: false,
            confidence: false,
            generation: true,
        }
    }
    pub fn paddle() -> Self {
        Self {
            plain: true,
            grounding: true,
            native_coordinates: true,
            confidence: true,
            generation: false,
        }
    }
    pub fn supports(&self, mode: OcrMode) -> bool {
        match mode {
            OcrMode::Plain => self.plain,
            OcrMode::Grounding => self.grounding,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OnnxConfig {
    pub detection_threshold: f32,
    pub box_threshold: f32,
    pub unclip_ratio: f32,
    pub recognition_threshold: f32,
    pub batch_size: usize,
    pub detection_resize: u32,
}
impl Default for OnnxConfig {
    fn default() -> Self {
        Self {
            detection_threshold: 0.2,
            box_threshold: 0.45,
            unclip_ratio: 1.4,
            recognition_threshold: 0.5,
            batch_size: 6,
            detection_resize: 960,
        }
    }
}
impl OnnxConfig {
    pub fn validate(&self) -> Result<()> {
        for value in [
            self.detection_threshold,
            self.box_threshold,
            self.recognition_threshold,
        ] {
            ensure!(
                value.is_finite() && (0.0..=1.0).contains(&value),
                "ONNX thresholds must be finite and in 0..1"
            );
        }
        ensure!(
            self.unclip_ratio.is_finite() && (0.1..=5.0).contains(&self.unclip_ratio),
            "Unclip ratio must be in 0.1..5"
        );
        ensure!(
            (1..=32).contains(&self.batch_size),
            "Batch size must be in 1..32"
        );
        ensure!(
            (32..=2048).contains(&self.detection_resize),
            "Detection resize must be in 32..2048"
        );
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "runtime", content = "parameters", rename_all = "lowercase")]
pub enum RuntimeConfig {
    Llamacpp(InferenceConfig),
    Onnx(OnnxConfig),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    pub manifest: ModelManifest,
    pub directory: String,
    pub device: Device,
    pub threads: u32,
    pub inference: RuntimeConfig,
}
impl EngineConfig {
    pub fn file(&self, role: &str) -> Result<std::path::PathBuf> {
        Ok(Path::new(&self.directory).join(&self.manifest.file(role)?.path))
    }
}
pub trait OcrEngine {
    fn recognize(
        &mut self,
        image: &Path,
        frame_id: u64,
        timestamp: u64,
        mode: OcrMode,
    ) -> Result<OcrResult>;
    fn capabilities(&self) -> OcrCapabilities;
    fn identity(&self) -> (&str, &str, &str);
    fn device(&self) -> &str;
    /// Consuming shutdown releases sessions/model tensors before the next load.
    fn shutdown(self: Box<Self>);
}

pub struct AdapterRegistration {
    pub id: &'static str,
    pub runtime: &'static str,
    pub roles: &'static [&'static str],
    pub capabilities: OcrCapabilities,
}
pub struct RuntimeRegistry {
    runtimes: BTreeMap<&'static str, &'static str>,
    adapters: BTreeMap<&'static str, AdapterRegistration>,
}
impl Default for RuntimeRegistry {
    fn default() -> Self {
        let mut registry = Self {
            runtimes: BTreeMap::new(),
            adapters: BTreeMap::new(),
        };
        registry.register_runtime("llamacpp", "ocr-worker.exe");
        registry.register_runtime("onnx", "onnx-worker.exe");
        registry.register_adapter(AdapterRegistration {
            id: "lightonocr",
            runtime: "llamacpp",
            roles: &["main", "mmproj"],
            capabilities: OcrCapabilities::lighton(),
        });
        registry.register_adapter(AdapterRegistration {
            id: "ppocrv6",
            runtime: "onnx",
            roles: &["detector", "recognizer", "dictionary"],
            capabilities: OcrCapabilities::paddle(),
        });
        registry
    }
}
impl RuntimeRegistry {
    pub fn register_runtime(&mut self, id: &'static str, executable: &'static str) {
        self.runtimes.insert(id, executable);
    }
    pub fn register_adapter(&mut self, adapter: AdapterRegistration) {
        self.adapters.insert(adapter.id, adapter);
    }
    pub fn executable(&self, runtime: &str) -> Result<&str> {
        self.runtimes
            .get(runtime)
            .copied()
            .context("Unregistered runtime")
    }
    pub fn adapter(&self, id: &str) -> Result<&AdapterRegistration> {
        self.adapters.get(id).context("Unregistered model adapter")
    }
    pub fn validate_manifest(&self, model: &ModelManifest) -> Result<()> {
        model.validate()?;
        self.executable(&model.runtime)?;
        let adapter = self.adapter(&model.adapter)?;
        ensure!(adapter.runtime == model.runtime, "Adapter/runtime mismatch");
        ensure!(
            model.capabilities == adapter.capabilities,
            "Manifest capabilities disagree with adapter"
        );
        for role in adapter.roles {
            model.file(role)?;
        }
        Ok(())
    }
    pub fn validate(&self, config: &EngineConfig) -> Result<()> {
        self.validate_manifest(&config.manifest)?;
        ensure!(
            (1..=128).contains(&config.threads),
            "Threads must be in 1..128"
        );
        match &config.inference {
            RuntimeConfig::Llamacpp(c) => {
                ensure!(
                    config.manifest.runtime == "llamacpp",
                    "Wrong runtime configuration"
                );
                c.validate()?;
            }
            RuntimeConfig::Onnx(c) => {
                ensure!(
                    config.manifest.runtime == "onnx",
                    "Wrong runtime configuration"
                );
                c.validate()?;
            }
        }
        Ok(())
    }
}
pub type EngineConstructor = fn(EngineConfig) -> Result<Box<dyn OcrEngine>>;
pub trait OcrEngineFactory {
    fn create(&self, config: EngineConfig) -> Result<Box<dyn OcrEngine>>;
}
#[derive(Default)]
pub struct EngineFactory {
    constructors: BTreeMap<&'static str, EngineConstructor>,
}
impl EngineFactory {
    pub fn register(&mut self, adapter: &'static str, constructor: EngineConstructor) {
        self.constructors.insert(adapter, constructor);
    }
}
impl OcrEngineFactory for EngineFactory {
    fn create(&self, config: EngineConfig) -> Result<Box<dyn OcrEngine>> {
        RuntimeRegistry::default().validate(&config)?;
        self.constructors
            .get(config.manifest.adapter.as_str())
            .context("This worker does not implement the requested adapter")?(config)
    }
}
pub fn device_candidates(device: Device) -> Vec<Device> {
    match device {
        Device::Auto => vec![Device::Cuda, Device::Cpu],
        other => vec![other],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_cuda_never_silently_falls_back() {
        assert_eq!(device_candidates(Device::Cuda), [Device::Cuda]);
        assert_eq!(device_candidates(Device::Auto), [Device::Cuda, Device::Cpu]);
    }
    #[test]
    fn onnx_coordinates_do_not_require_generation() {
        let c = OcrCapabilities::paddle();
        assert!(c.supports(OcrMode::Grounding));
        assert!(!c.generation);
    }
    #[test]
    fn invalid_thresholds_and_memory_limits_rejected() {
        let mut c = OnnxConfig::default();
        c.detection_threshold = f32::NAN;
        assert!(c.validate().is_err());
        c.detection_threshold = 0.3;
        c.batch_size = usize::MAX;
        assert!(c.validate().is_err());
    }
}
