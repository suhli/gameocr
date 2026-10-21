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
    pub temporal: TemporalConfig,
    pub experimental_gpu_ctc: bool,
    pub experimental_shared_memory: bool,
    pub detection_threshold: f32,
    pub box_threshold: f32,
    pub unclip_ratio: f32,
    pub recognition_threshold: f32,
    pub batch_size: usize,
    pub detection_resize: u32,
    /// Detect overlapping regions at source resolution, then recognize once.
    pub preserve_small_text: bool,
    /// Normalized [left, top, width, height] in the original captured frame.
    pub detection_region: Option<[f64; 4]>,
    pub incremental: bool,
    pub width_buckets: bool,
    pub recognition_pixel_budget: usize,
    pub selective_detection: bool,
    pub refinement_regions: usize,
}
impl Default for OnnxConfig {
    fn default() -> Self {
        Self {
            temporal: TemporalConfig::default(),
            experimental_gpu_ctc: false,
            experimental_shared_memory: false,
            detection_threshold: 0.2,
            box_threshold: 0.45,
            unclip_ratio: 1.4,
            recognition_threshold: 0.5,
            batch_size: 6,
            detection_resize: 960,
            preserve_small_text: false,
            detection_region: None,
            incremental: false,
            width_buckets: false,
            recognition_pixel_budget: 196608,
            selective_detection: false,
            refinement_regions: 4,
        }
    }
}
impl OnnxConfig {
    pub fn requires_whole_frame(&self) -> bool {
        self.preserve_small_text
            || self.detection_region.is_some()
            || self.incremental
            || self.selective_detection
            || self.temporal.enabled
    }
    pub fn validate(&self) -> Result<()> {
        self.temporal.validate()?;
        ensure!(
            (1..=8).contains(&self.refinement_regions),
            "Refinement regions must be in 1..8"
        );
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
            (153600..=786432).contains(&self.recognition_pixel_budget),
            "Recognition pixel budget must be in 153600..786432"
        );
        ensure!(
            (32..=2048).contains(&self.detection_resize),
            "Detection resize must be in 32..2048"
        );
        ensure!(
            !(self.preserve_small_text || self.selective_detection) || self.detection_resize >= 256,
            "Small-text detection requires a detection edge of at least 256 px"
        );
        if let Some([x, y, w, h]) = self.detection_region {
            ensure!(
                [x, y, w, h].iter().all(|v| v.is_finite())
                    && x >= 0.
                    && y >= 0.
                    && w > 0.
                    && h > 0.
                    && x + w <= 1.00000001
                    && y + h <= 1.00000001,
                "Detection region must be [left, top, width, height] inside 0..1"
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TemporalStrategy {
    #[default]
    Auto,
    Full,
    RecOnly,
    CacheOnly,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TemporalConfig {
    pub enabled: bool,
    pub strategy: TemporalStrategy,
    pub refresh_ms: u64,
    pub max_reuse: u32,
    pub min_confidence: f32,
    pub edge_delta: u8,
}
impl Default for TemporalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strategy: TemporalStrategy::Auto,
            refresh_ms: 1000,
            max_reuse: 30,
            min_confidence: 0.85,
            edge_delta: 12,
        }
    }
}
impl TemporalConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (100..=10000).contains(&self.refresh_ms),
            "Temporal refresh must be 100..10000 ms"
        );
        ensure!(
            (1..=60).contains(&self.max_reuse),
            "Temporal reuse must be 1..60 frames"
        );
        ensure!(
            self.min_confidence.is_finite() && (0.5..=1.).contains(&self.min_confidence),
            "Temporal confidence must be 0.5..1"
        );
        ensure!(
            (1..=32).contains(&self.edge_delta),
            "Temporal edge threshold must be 1..32"
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
    /// Capture generation, cancellation epoch and executor cache revision.
    fn set_frame_context(&mut self, _context: Option<[u64; 3]>) {}
    fn recognize(
        &mut self,
        image: &Path,
        frame_id: u64,
        timestamp: u64,
        mode: OcrMode,
    ) -> Result<OcrResult>;
    fn recognize_tile(
        &mut self,
        _image: &Path,
        _frame_id: u64,
        _timestamp: u64,
        _mode: OcrMode,
        _detection_resize: u32,
    ) -> Result<OcrResult> {
        anyhow::bail!("This adapter does not support independent tile inference")
    }
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
    registry: RuntimeRegistry,
    constructors: BTreeMap<&'static str, EngineConstructor>,
}
impl EngineFactory {
    pub fn with_registry(registry: RuntimeRegistry) -> Self {
        Self {
            registry,
            constructors: BTreeMap::new(),
        }
    }
    pub fn register(&mut self, adapter: &'static str, constructor: EngineConstructor) {
        self.constructors.insert(adapter, constructor);
    }
}
impl OcrEngineFactory for EngineFactory {
    fn create(&self, config: EngineConfig) -> Result<Box<dyn OcrEngine>> {
        self.registry.validate(&config)?;
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
    fn config(
        runtime: &str,
        adapter: &str,
        roles: &[&str],
        capabilities: OcrCapabilities,
        inference: RuntimeConfig,
    ) -> EngineConfig {
        EngineConfig {
            manifest: ModelManifest {
                schema_version: 2,
                id: "factory-test".into(),
                name: "Factory dispatch".into(),
                runtime: runtime.into(),
                adapter: adapter.into(),
                version: "1".into(),
                quantization: "F32".into(),
                capabilities,
                files: roles
                    .iter()
                    .map(|role| crate::manifest::ModelFile {
                        role: (*role).into(),
                        path: format!("{role}.bin"),
                        size: 1,
                        sha256: "a".repeat(64),
                        url: None,
                    })
                    .collect(),
                source: "unit-test".into(),
                parameters: serde_json::Value::Null,
                installed: false,
                imported: false,
            },
            directory: ".".into(),
            device: Device::Cpu,
            threads: 4,
            inference,
        }
    }
    #[test]
    fn factory_validates_then_dispatches_only_the_registered_constructor() {
        // Sentinels test dispatch without claiming a loaded model or OCR success.
        fn onnx(_: EngineConfig) -> Result<Box<dyn OcrEngine>> {
            anyhow::bail!("selected ONNX")
        }
        fn llama(_: EngineConfig) -> Result<Box<dyn OcrEngine>> {
            anyhow::bail!("selected llama.cpp")
        }
        let mut factory = EngineFactory::default();
        factory.register("ppocrv6", onnx);
        factory.register("lightonocr", llama);
        let mut c = config(
            "onnx",
            "ppocrv6",
            &["detector", "recognizer", "dictionary"],
            OcrCapabilities::paddle(),
            RuntimeConfig::Onnx(OnnxConfig::default()),
        );
        assert_eq!(
            factory.create(c.clone()).err().unwrap().to_string(),
            "selected ONNX"
        );
        c.manifest.capabilities.generation = true;
        assert!(
            factory
                .create(c)
                .err()
                .unwrap()
                .to_string()
                .contains("capabilities")
        );
        let llama_config = InferenceConfig {
            model: String::new(),
            mmproj: String::new(),
            device: Device::Cpu,
            threads: 4,
            context_size: 8192,
            max_tokens: 2048,
            temperature: 0.1,
            max_image_edge: 1280,
        };
        let c = config(
            "llamacpp",
            "lightonocr",
            &["main", "mmproj"],
            OcrCapabilities::lighton(),
            RuntimeConfig::Llamacpp(llama_config),
        );
        assert_eq!(
            factory.create(c).err().unwrap().to_string(),
            "selected llama.cpp"
        );
    }
    #[test]
    fn custom_adapter_registry_can_be_used_by_factory() {
        let mut registry = RuntimeRegistry::default();
        registry.register_adapter(AdapterRegistration {
            id: "future-onnx",
            runtime: "onnx",
            roles: &["recognizer"],
            capabilities: OcrCapabilities::paddle(),
        });
        let mut factory = EngineFactory::with_registry(registry);
        factory.register("future-onnx", |_| anyhow::bail!("selected custom adapter"));
        let c = config(
            "onnx",
            "future-onnx",
            &["recognizer"],
            OcrCapabilities::paddle(),
            RuntimeConfig::Onnx(OnnxConfig::default()),
        );
        assert_eq!(
            factory.create(c).err().unwrap().to_string(),
            "selected custom adapter"
        );
    }
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
    #[test]
    fn roi_validation_and_legacy_defaults() {
        let mut c: OnnxConfig = serde_json::from_str("{}").unwrap();
        assert!(!c.requires_whole_frame());
        for roi in [
            [0.9, 0., 0.2, 1.],
            [-0.1, 0., 1., 1.],
            [0., 0., 0., 1.],
            [0., f64::NAN, 1., 1.],
        ] {
            c.detection_region = Some(roi);
            assert!(c.validate().is_err());
        }
        c.detection_region = Some([0.5, 0.7, 0.5, 0.3]);
        assert!(c.validate().is_ok());
        assert!(c.requires_whole_frame());
        c.preserve_small_text = true;
        c.detection_resize = 32;
        assert!(c.validate().is_err());
    }
}
