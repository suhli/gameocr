//! Bounded, CPU-only scene analysis and online policy. No OCR or image persistence.
pub mod features;
pub mod profiles;
pub mod scheduler;
use crate::types::Device;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
pub const FEATURE_VERSION: u32 = 1;
pub const ALGORITHM_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    #[default]
    Single,
    Manual,
    Adaptive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AdaptiveConfig {
    pub learning: bool,
    pub exploration: bool,
    pub max_cpu_workers: usize,
    pub max_gpu_workers: usize,
}
impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            learning: true,
            exploration: true,
            max_cpu_workers: 2,
            max_gpu_workers: 0,
        }
    }
}
impl AdaptiveConfig {
    pub fn validate(&self, device: Device) -> Result<()> {
        ensure!(
            device != Device::Cuda,
            "自适应目前仅支持 CPU；请选择 CPU 或 Auto，显式 CUDA 不会静默回退"
        );
        ensure!(
            (1..=2).contains(&self.max_cpu_workers),
            "自适应 CPU Worker 上限必须为 1–2"
        );
        ensure!(
            self.max_gpu_workers == 0,
            "GPU 自适应策略尚未验证，当前上限必须为 0"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrategyId {
    CpuSingle,
    #[serde(rename = "cpu-parallel-2")]
    CpuParallel2,
}
impl StrategyId {
    pub fn workers(self) -> usize {
        if self == Self::CpuSingle { 1 } else { 2 }
    }
    pub fn other(self) -> Self {
        if self == Self::CpuSingle {
            Self::CpuParallel2
        } else {
            Self::CpuSingle
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AdaptiveMetrics {
    pub strategy: Option<StrategyId>,
    pub analysis_ms: f64,
    pub decision_ms: f64,
    pub switch_ms: f64,
    pub ocr_ms: f64,
    pub similar_samples: u64,
    pub predicted_ms: Option<f64>,
    pub single_median_ms: Option<f64>,
    pub parallel_median_ms: Option<f64>,
    pub exploring: bool,
    pub reliable: bool,
    pub fallback_risk: f64,
    pub reason: String,
    pub match_kind: String,
    pub quality_risk: bool,
    pub cpu_load: Option<f64>,
    pub available_memory_mb: Option<u64>,
}
