//! Only one strategy pool is resident. Switching shuts down, loads and warms the
//! replacement before accepting work, preserving each strategy's true thread budget.
use super::persistence::{LearningEvent, ProfileRepository};
use crate::executor::{Cancellation, Executor, SharedStatus};
use anyhow::{Result, ensure};
use gameocr_core::{
    adaptive::{
        features::FrameAnalyzer,
        profiles::Observation,
        scheduler::{AdaptiveScheduler, ResourceState},
        *,
    },
    engine::EngineConfig,
    parallel::{FrameInput, ParallelConfig},
    types::{Device, OcrMode, OcrResult, now_ms},
};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

pub struct ExecutionStrategyManager {
    executor: Executor,
    pub status: SharedStatus,
    pub device: String,
    pub load_ms: f64,
    pub message: Option<String>,
    mode: ExecutionMode,
    config: EngineConfig,
    root: PathBuf,
    options: AdaptiveConfig,
    closing: Arc<AtomicBool>,
    pub profiles: Arc<ProfileRepository>,
    pub fingerprint: String,
    analyzer: FrameAnalyzer,
    scheduler: AdaptiveScheduler,
    current: StrategyId,
    can_parallel: bool,
    switch_estimate: f64,
    previous_frame_ms: f64,
    resource: ResourceMonitor,
    generation: Option<u64>,
    pending: Option<LearningEvent>,
    return_after_probe: Option<StrategyId>,
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn parameters(id: StrategyId) -> ParallelConfig {
    ParallelConfig {
        enable_parallel_inference: id == StrategyId::CpuParallel2,
        enable_cpu_workers: false,
        cpu_worker_count: id.workers(),
        gpu_worker_count: 0,
    }
}
pub fn fingerprint(root: &Path, config: &EngineConfig) -> Result<String> {
    use std::io::Read;
    let mut hash = Sha256::new();
    hash.update(serde_json::to_vec(&serde_json::json!({"model":config.manifest.id,"version":config.manifest.version,
        "files":config.manifest.files.iter().map(|f|(&f.role,&f.sha256,f.size)).collect::<Vec<_>>(),
        "runtime":config.manifest.runtime,"adapter":config.manifest.adapter,"provider":"CPU","parameters":config.inference,
        "threads":config.threads,"featureSchema":FEATURE_VERSION,"algorithm":ALGORITHM_VERSION,
        "cpu":std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_|std::env::consts::ARCH.into()),
        "logicalCores":std::thread::available_parallelism().map_or(1,|n|n.get()),"os":std::env::consts::OS,
        "gpu":"disabled","app":env!("CARGO_PKG_VERSION")
    }))?);
    for name in ["onnx-worker.exe", "onnxruntime.dll"] {
        let mut file = std::fs::File::open(root.join("workers/onnx/cpu").join(name))?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
    }
    Ok(hex::encode(hash.finalize()))
}
impl ExecutionStrategyManager {
    #[allow(clippy::too_many_arguments)]
    pub async fn load(
        root: &Path,
        mut config: EngineConfig,
        parallel: &ParallelConfig,
        mode: ExecutionMode,
        options: AdaptiveConfig,
        profiles: Arc<ProfileRepository>,
        closing: Arc<AtomicBool>,
    ) -> Result<Self> {
        let started = Instant::now();
        let mode = if config.manifest.runtime == "onnx" && config.manifest.adapter == "ppocrv6" {
            mode
        } else {
            ExecutionMode::Single
        };
        let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
        if mode == ExecutionMode::Adaptive {
            options.validate(config.device)?;
            config.device = Device::Cpu;
            config.threads = config.threads.min(logical as u32).max(1);
        }
        let mut plan = parallel.clone();
        plan.enable_parallel_inference = mode == ExecutionMode::Manual;
        if mode == ExecutionMode::Adaptive {
            plan = parameters(StrategyId::CpuSingle);
        }
        let fingerprint = if config.manifest.runtime == "onnx"
            && (mode == ExecutionMode::Adaptive || config.device == Device::Cpu)
        {
            let r = root.to_owned();
            let c = config.clone();
            tokio::task::spawn_blocking(move || fingerprint(&r, &c)).await??
        } else {
            String::new()
        };
        let executor = Executor::load(root, config.clone(), &plan, closing.clone()).await?;
        let status = executor.status.clone();
        status.lock().execution_mode = mode;
        let message = if mode == ExecutionMode::Adaptive {
            Some("自适应优化：CPU 候选，按需切换；GPU 探索未启用".into())
        } else {
            executor.message.clone()
        };
        let can_parallel = options.max_cpu_workers >= 2 && config.threads >= 2 && logical >= 2;
        Ok(Self {
            device: executor.device.clone(),
            switch_estimate: executor.load_ms.max(100.),
            executor,
            status,
            load_ms: ms(started),
            message,
            mode,
            config,
            root: root.into(),
            options,
            closing,
            profiles,
            fingerprint,
            analyzer: Default::default(),
            scheduler: Default::default(),
            current: StrategyId::CpuSingle,
            can_parallel,
            previous_frame_ms: 0.,
            resource: Default::default(),
            generation: None,
            pending: None,
            return_after_probe: None,
        })
    }
    pub fn available(&self) -> bool {
        self.executor.available()
    }
    pub fn take_learning(&mut self) -> Option<LearningEvent> {
        self.pending.take()
    }
    pub async fn shutdown(&mut self) {
        self.pending = None;
        self.executor.shutdown().await;
    }
    async fn switch(&mut self, id: StrategyId, cancel: &Cancellation) -> Result<f64> {
        if self.current == id && self.executor.available() {
            return Ok(0.);
        }
        ensure!(!cancel.cancelled(), "任务已取消");
        let start = Instant::now();
        self.executor.shutdown().await;
        // Once the old pool is gone, finish initializing its replacement even if
        // capture is paused. Keep a reusable loaded model, but reject this frame.
        let mut executor = Executor::load(
            &self.root,
            self.config.clone(),
            &parameters(id),
            self.closing.clone(),
        )
        .await?;
        let new_status = executor.status.lock().clone();
        *self.status.lock() = new_status;
        executor.status = self.status.clone();
        self.status.lock().execution_mode = self.mode;
        self.executor = executor;
        self.current = id;
        let time = ms(start);
        self.switch_estimate = self.switch_estimate * 0.7 + time * 0.3;
        self.scheduler.switched(now_ms());
        ensure!(!cancel.cancelled(), "任务已取消");
        Ok(time)
    }
    pub async fn recognize(
        &mut self,
        frame: Arc<FrameInput>,
        mode: OcrMode,
        cache: &Path,
        cancel: Cancellation,
    ) -> Result<OcrResult> {
        self.pending = None;
        if self.mode != ExecutionMode::Adaptive {
            return self.executor.recognize(frame, mode, cache, cancel).await;
        }
        let start = Instant::now();
        // Clearing history invalidates the entire in-flight frame, including analysis.
        let revision = self.profiles.generation();
        ensure!(!cancel.cancelled(), "任务已取消");
        if self.generation != Some(frame.generation) {
            self.analyzer.reset();
            self.generation = Some(frame.generation);
        }
        let analysis_start = Instant::now();
        let mut analyzer = std::mem::take(&mut self.analyzer);
        let input = frame.clone();
        let (analyzer, features) = tokio::task::spawn_blocking(move || {
            let features = analyzer.analyze(&input.image);
            (analyzer, features)
        })
        .await?;
        self.analyzer = analyzer;
        let analysis_ms = ms(analysis_start);
        ensure!(!cancel.cancelled(), "任务已取消");
        let decision_start = Instant::now();
        let model_bytes = self
            .config
            .manifest
            .files
            .iter()
            .map(|f| f.size)
            .sum::<u64>();
        let resource = self.resource.sample(
            self.previous_frame_ms,
            (model_bytes / 1024 / 1024 * 6).max(512),
        );
        let mut choice = self.profiles.read(|store| {
            self.scheduler.select(
                store,
                &self.fingerprint,
                &features,
                &self.options,
                self.current,
                self.can_parallel,
                self.switch_estimate,
                resource,
                now_ms(),
            )
        });
        if let Some(id) = self.return_after_probe.take() {
            choice.strategy = Some(id);
            choice.exploring = false;
            choice.reason = "探索预算结束，恢复原策略；切换成本计入端到端".into();
        }
        if !self.can_parallel
            || resource
                .available_memory_mb
                .is_some_and(|m| m < resource.minimum_memory_mb)
        {
            choice.strategy = Some(StrategyId::CpuSingle);
            choice.exploring = false;
        }
        let id = choice.strategy.unwrap_or(StrategyId::CpuSingle);
        let previous = self.current;
        choice.analysis_ms = analysis_ms;
        choice.decision_ms = ms(decision_start);
        let switched = id != self.current || !self.executor.available();
        let switch_result = self.switch(id, &cancel).await;
        if let Err(error) = switch_result {
            // Failed initialization is a failed observation, never a successful fast sample.
            if !cancel.cancelled() && self.options.learning {
                self.pending = Some(LearningEvent {
                    generation: revision,
                    key: self.fingerprint.clone(),
                    features,
                    strategy: id,
                    observation: Observation {
                        latency_ms: ms(start),
                        success: false,
                        fallback: false,
                        retries: 0,
                        blocks: 0,
                        quality_risk: true,
                    },
                    now: now_ms(),
                    exploring: choice.exploring,
                    switched,
                    total_ms: ms(start),
                });
            }
            return Err(error);
        }
        choice.switch_ms = switch_result.unwrap();
        if choice.exploring {
            self.return_after_probe = Some(previous);
        }
        let ocr_start = Instant::now();
        let response = self
            .executor
            .recognize(frame, mode, cache, cancel.clone())
            .await;
        choice.ocr_ms = ms(ocr_start);
        if cancel.cancelled() {
            self.pending = None;
            anyhow::bail!("任务已取消");
        }
        self.previous_frame_ms = ms(start);
        let mut observation = Observation {
            latency_ms: choice.ocr_ms + choice.analysis_ms + choice.decision_ms,
            success: response.is_ok(),
            fallback: false,
            retries: 0,
            blocks: 0,
            quality_risk: response.is_err(),
        };
        if let Ok(result) = &response {
            observation.blocks = result.blocks.len();
            if let Some(metrics) = &result.metrics.execution {
                observation.fallback = metrics.whole_frame_fallback;
                observation.retries = metrics.retried_tasks;
                observation.quality_risk |= metrics.failed_tasks > 0;
            }
            // Count jumps are uncertainty signals; confidence does not prove recall.
            let typical = self.profiles.read(|s| {
                let m = s.matching(&self.fingerprint, &features, now_ms());
                s.evidence(&m, id)
            });
            if typical.reliable()
                && typical.block_mean >= 3.
                && ((observation.blocks as f64) < typical.block_mean * 0.5
                    || (observation.blocks as f64) > typical.block_mean * 2.5)
            {
                observation.quality_risk = true;
            }
        }
        choice.quality_risk = observation.quality_risk;
        self.scheduler
            .observed(observation.success, choice.exploring, now_ms());
        if self.options.learning {
            self.pending = Some(LearningEvent {
                generation: revision,
                key: self.fingerprint.clone(),
                features,
                strategy: id,
                observation,
                now: now_ms(),
                exploring: choice.exploring,
                switched,
                total_ms: ms(start),
            });
        }
        let mut result = response?;
        if let Some(metrics) = &mut result.metrics.execution {
            metrics.adaptive = Some(choice);
            metrics.end_to_end_ms = ms(start);
            result.metrics.total_ms = metrics.end_to_end_ms;
            self.status.lock().last = Some(metrics.clone());
        }
        self.status.lock().learning = Some(self.profiles.summary());
        Ok(result)
    }
}

#[derive(Default)]
struct ResourceMonitor {
    last: Option<(u64, u64)>,
}
impl ResourceMonitor {
    fn sample(&mut self, previous_frame_ms: f64, minimum_memory_mb: u64) -> ResourceState {
        let mut state = ResourceState {
            previous_frame_ms,
            minimum_memory_mb,
            ..Default::default()
        };
        #[cfg(windows)]
        unsafe {
            use windows::Win32::{
                Foundation::FILETIME,
                System::{
                    SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
                    Threading::GetSystemTimes,
                },
            };
            let mut memory = MEMORYSTATUSEX {
                dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
                ..Default::default()
            };
            if GlobalMemoryStatusEx(&mut memory).is_ok() {
                state.available_memory_mb = Some(memory.ullAvailPhys / 1024 / 1024);
            }
            let (mut idle, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            if GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).is_ok() {
                let value =
                    |t: FILETIME| ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64;
                let total = value(kernel) + value(user);
                let idle = value(idle);
                if let Some((old_total, old_idle)) = self.last {
                    let delta = total.saturating_sub(old_total);
                    if delta > 0 {
                        state.cpu_load = Some(
                            (1. - idle.saturating_sub(old_idle) as f64 / delta as f64)
                                .clamp(0., 1.),
                        );
                    }
                }
                self.last = Some((total, idle));
            }
        }
        state
    }
}
