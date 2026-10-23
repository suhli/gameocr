use crate::{
    adaptive::{manager::ExecutionStrategyManager, persistence::ProfileRepository},
    capture::{CaptureManager, CaptureShared, Frame, Preview, preview},
    executor::{Cancellation, ExecutorStatus, SharedStatus},
    model::{ModelEntry, ModelManager},
    ocr::resource_root,
    settings::{Settings, data_dir},
    window::WindowInfo,
};
use anyhow::{Result, ensure};
use gameocr_core::{change::ChangeDetector, engine::RuntimeRegistry, types::*};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Emitter;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub status: String,
    pub id: Option<String>,
    pub device: Option<String>,
    pub load_ms: Option<f64>,
    pub message: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub result: OcrResult,
    pub preview: Preview,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    pub capture_fps: f64,
    pub ocr_per_minute: usize,
    pub last_ocr_ms: Option<f64>,
    pub average_ocr_ms: Option<f64>,
    pub preprocessing_ms: Option<f64>,
    pub inference_ms: Option<f64>,
    pub skipped_frames: u64,
    pub dropped_frames: u64,
    pub queue: String,
    pub model_load_ms: Option<f64>,
    pub device: Option<String>,
    pub vram_mb: Option<f64>,
    pub execution: Option<ExecutorStatus>,
}
pub struct Service {
    pub app: tauri::AppHandle,
    pub settings: RwLock<Settings>,
    pub settings_error: Option<String>,
    pub capture: Mutex<CaptureManager>,
    pub shared: Arc<CaptureShared>,
    pub selected: Mutex<Option<WindowInfo>>,
    pub models: Arc<ModelManager>,
    pub cuda_runtime: crate::cuda_runtime::RuntimeManager,
    pub translation: Arc<crate::translation::TranslationManager>,
    pub model_status: Mutex<ModelStatus>,
    executor: tokio::sync::Mutex<Option<ExecutionStrategyManager>>,
    pub adaptive_profiles: Arc<ProfileRepository>,
    executor_status: RwLock<Option<SharedStatus>>,
    pub lifecycle: tokio::sync::Mutex<()>,
    pub continuous: AtomicBool,
    pub processing: AtomicBool,
    pub ocr_epoch: AtomicU64,
    pub ocr_status: Mutex<String>,
    detector: Mutex<ChangeDetector>,
    pub history: Mutex<VecDeque<Snapshot>>,
    pub recognized_frame: Mutex<Option<Arc<Frame>>>,
    pub performance: Mutex<Performance>,
    times: Mutex<VecDeque<Instant>>,
    durations: Mutex<VecDeque<f64>>,
    pub shutdown: Arc<AtomicBool>,
    pub cache_dir: PathBuf,
}
struct ProcessingGuard<'a>(&'a AtomicBool);
impl Drop for ProcessingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Service {
    pub fn new(app: tauri::AppHandle) -> Result<Arc<Self>> {
        let (settings, settings_error) = match Settings::read(&data_dir().join("settings.json")) {
            Ok(s) => (s, None),
            Err(e) => (Settings::default(), Some(format!("{e:#}"))),
        };
        let cache_dir = data_dir()
            .join("sessions")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&cache_dir)?;
        let capture = CaptureManager::new(app.clone());
        let shared = capture.shared.clone();
        let translation = crate::translation::TranslationManager::new(app.clone());
        Ok(Arc::new(Self {
            app,
            settings: RwLock::new(settings),
            settings_error,
            capture: Mutex::new(capture),
            shared,
            selected: Mutex::new(None),
            models: Arc::new(ModelManager::new()?),
            cuda_runtime: Default::default(),
            translation,
            model_status: Mutex::new(ModelStatus {
                status: "unloaded".into(),
                id: None,
                device: None,
                load_ms: None,
                message: None,
            }),
            executor: tokio::sync::Mutex::new(None),
            adaptive_profiles: ProfileRepository::open(&data_dir().join("adaptive-profiles.json")),
            executor_status: RwLock::new(None),
            lifecycle: tokio::sync::Mutex::new(()),
            continuous: AtomicBool::new(false),
            processing: AtomicBool::new(false),
            ocr_epoch: AtomicU64::new(1),
            ocr_status: Mutex::new("idle".into()),
            detector: Mutex::new(ChangeDetector::default()),
            history: Mutex::new(VecDeque::new()),
            recognized_frame: Mutex::new(None),
            performance: Mutex::new(Performance::default()),
            times: Mutex::new(VecDeque::new()),
            durations: Mutex::new(VecDeque::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
            cache_dir,
        }))
    }
    pub fn emit_model(&self) {
        let status = self.model_status.lock().clone();
        let _ = self.app.emit("model-status", status);
    }
    pub fn stop_ocr(&self) {
        let _gate = self.shared.transition.lock();
        self.continuous.store(false, Ordering::Release);
        self.ocr_epoch.fetch_add(1, Ordering::AcqRel);
        self.translation.invalidate(true);
        self.set_ocr_status("idle");
    }
    fn set_ocr_status(&self, status: &str) {
        *self.ocr_status.lock() = status.into();
        let _ = self.app.emit("ocr-status", status);
    }
    pub fn reset_target(&self) {
        self.stop_ocr();
        self.detector.lock().reset();
        self.history.lock().clear();
        *self.recognized_frame.lock() = None;
    }
    pub fn start_capture(&self) -> Result<()> {
        self.reset_target();
        let settings = self.settings.read().clone();
        let window = self.selected.lock().clone();
        if let Err(e) = self.capture.lock().start(settings, window) {
            self.shared.fail(&format!("{e:#}"));
            return Err(e);
        }
        Ok(())
    }
    pub fn update_settings(&self, mut settings: Settings) -> Result<()> {
        settings.normalize();
        let _transition = self
            .lifecycle
            .try_lock()
            .map_err(|_| anyhow::anyhow!("模型操作期间不能修改设置"))?;
        settings.validate()?;
        ensure!(
            !self.models.busy.load(Ordering::Acquire),
            "模型文件操作期间不能修改设置"
        );
        let old = self.settings.read().clone();
        ensure!(
            old.model_dir == settings.model_dir || self.model_status.lock().status == "unloaded",
            "更改模型目录前请卸载模型"
        );
        let model_active = self.model_status.lock().status != "unloaded";
        ensure!(
            !model_active
                || (old.device == settings.device
                    && old.threads == settings.threads
                    && old.context_size == settings.context_size
                    && old.max_tokens == settings.max_tokens
                    && old.temperature == settings.temperature
                    && old.max_image_edge == settings.max_image_edge
                    && old.onnx == settings.onnx
                    && old.parallel == settings.parallel
                    && old.selected_mode() == settings.selected_mode()
                    && old.adaptive == settings.adaptive),
            "更改推理参数前请卸载模型"
        );
        let capture_active =
            ["running", "paused", "starting"].contains(&self.shared.status.lock().as_str());
        ensure!(
            !capture_active
                || (old.capture_fps == settings.capture_fps
                    && old.capture_source == settings.capture_source
                    && old.adb_path == settings.adb_path
                    && old.adb_serial == settings.adb_serial),
            "修改捕获来源或频率前请停止捕获"
        );
        settings.save(&data_dir().join("settings.json"))?;
        *self.settings.write() = settings;
        self.detector.lock().reset();
        Ok(())
    }
    pub async fn load_model(self: &Arc<Self>, id: String) -> Result<()> {
        let _transition = self
            .lifecycle
            .try_lock()
            .map_err(|_| anyhow::anyhow!("模型正在切换"))?;
        ensure!(
            !self.models.busy.load(Ordering::Acquire),
            "请等待模型文件操作完成"
        );
        self.stop_ocr();
        let settings = self.settings.read().clone();
        {
            let mut status = self.model_status.lock();
            status.status = "loading".into();
            status.id = Some(id.clone());
            status.message = None;
        }
        self.emit_model();
        let result = self.load_inner(&id, &settings).await;
        if let Err(e) = &result {
            let mut status = self.model_status.lock();
            status.status = "error".into();
            status.device = None;
            status.message = Some(format!("{e:#}"));
        }
        self.emit_model();
        result
    }
    async fn load_inner(&self, id: &str, settings: &Settings) -> Result<()> {
        let mut executor = self.executor.lock().await;
        if let Some(mut old) = executor.take() {
            old.shutdown().await;
        }
        *self.executor_status.write() = None;
        let dir = settings.model_dir.clone();
        let model_id = id.to_owned();
        let entry = tokio::task::spawn_blocking(move || {
            let entry = ModelManager::find(&dir, &model_id)?;
            ModelManager::verify(&dir, &entry)?;
            Ok::<_, anyhow::Error>(entry)
        })
        .await??;
        let registry = RuntimeRegistry::default();
        registry.validate_manifest(&entry)?;
        ensure!(
            entry.capabilities.supports(settings.mode),
            "模型不支持当前识别模式"
        );
        let root = resource_root(&self.app)?;
        if entry.runtime == "onnx"
            && settings.selected_mode() == gameocr_core::adaptive::ExecutionMode::Adaptive
        {
            // Reject unsupported strategies before starting a large runtime download.
            settings.adaptive.validate(settings.device)?;
        }
        if settings.device == Device::Cuda {
            self.cuda_runtime
                .ensure(&root, &entry.runtime, self.shutdown.clone(), |progress| {
                    let _ = self.app.emit("cuda-runtime-progress", progress);
                })
                .await?;
        }
        let loaded = ExecutionStrategyManager::load(
            &root,
            settings.engine_config(entry, settings.device)?,
            &settings.parallel,
            settings.selected_mode(),
            settings.adaptive.clone(),
            self.adaptive_profiles.clone(),
            self.shutdown.clone(),
        )
        .await?;
        *self.model_status.lock() = ModelStatus {
            status: "ready".into(),
            id: Some(id.into()),
            device: Some(loaded.device.clone()),
            load_ms: Some(loaded.load_ms),
            message: loaded.message.clone(),
        };
        *self.executor_status.write() = Some(loaded.status.clone());
        *executor = Some(loaded);
        self.detector.lock().reset();
        Ok(())
    }
    pub async fn unload_model(&self) -> Result<()> {
        let _transition = self
            .lifecycle
            .try_lock()
            .map_err(|_| anyhow::anyhow!("模型正在切换"))?;
        self.stop_ocr();
        self.model_status.lock().status = "unloading".into();
        self.emit_model();
        if let Some(mut worker) = self.executor.lock().await.take() {
            worker.shutdown().await;
        }
        *self.executor_status.write() = None;
        *self.model_status.lock() = ModelStatus {
            status: "unloaded".into(),
            id: None,
            device: None,
            load_ms: None,
            message: None,
        };
        self.emit_model();
        Ok(())
    }
    pub fn start_ocr(&self) -> Result<()> {
        ensure!(self.model_status.lock().status == "ready", "请先加载模型");
        ensure!(*self.shared.status.lock() == "running", "请先开始捕获");
        self.continuous.store(true, Ordering::Release);
        self.set_ocr_status(if self.processing.load(Ordering::Acquire) {
            "processing"
        } else {
            "waiting"
        });
        Ok(())
    }
    pub async fn recognize(self: &Arc<Self>, force: bool) -> Result<()> {
        ensure!(self.model_status.lock().status == "ready", "模型未就绪");
        ensure!(
            ["running", "paused"].contains(&self.shared.status.lock().as_str()),
            "捕获未就绪"
        );
        ensure!(
            !self.processing.swap(true, Ordering::AcqRel),
            "OCR 正在处理，未添加重复任务"
        );
        let _busy = ProcessingGuard(&self.processing);
        let frame = if force {
            self.shared
                .latest
                .lock()
                .take()
                .or_else(|| self.shared.preview_frame.lock().clone())
        } else {
            self.shared.latest.lock().take()
        };
        let Some(frame) = frame else {
            return if force {
                Err(anyhow::anyhow!("尚未收到截图，请稍后再试"))
            } else {
                Ok(())
            };
        };
        let gate_start = Instant::now();
        let frame_wait_ms = frame
            .capture
            .ready
            .map(|t| t.elapsed().as_secs_f64() * 1000.);
        let settings = self.settings.read().clone();
        // The thumbnail gate can hide a one-character update. Incremental OCR
        // performs its own exact, region-level comparison inside the worker.
        let incremental = (settings.onnx.incremental || settings.onnx.temporal.enabled)
            && self
                .executor
                .lock()
                .await
                .as_ref()
                .is_some_and(|e| e.incremental());
        if !force
            && !incremental
            && settings.change_detection
            && !self
                .detector
                .lock()
                .changed(&frame.image, settings.change_threshold)
        {
            self.performance.lock().skipped_frames += 1;
            return Ok(());
        }
        let gate_ms = gate_start.elapsed().as_secs_f64() * 1000.;
        let epoch = self.ocr_epoch.load(Ordering::Acquire);
        let generation = frame.generation;
        self.set_ocr_status("processing");
        let end_to_end = Instant::now();
        let service = self.clone();
        let cancel = Cancellation::new(epoch, move || {
            !service.shutdown.load(Ordering::Acquire)
                && epoch == service.ocr_epoch.load(Ordering::Acquire)
                && generation == service.shared.generation.load(Ordering::Acquire)
        });
        let mut learning = None;
        let outcome: Result<OcrResult> = async {
            let mut executor = self.executor.lock().await;
            let result = executor
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("模型工作程序不可用"))?
                .recognize(frame.clone(), settings.mode, &self.cache_dir, cancel)
                .await;
            learning = executor.as_mut().and_then(|e| e.take_learning());
            if executor.as_ref().is_some_and(|e| !e.available()) {
                *executor = None;
            }
            result
        }
        .await;
        let current = epoch == self.ocr_epoch.load(Ordering::Acquire)
            && generation == self.shared.generation.load(Ordering::Acquire);
        match outcome {
            Ok(mut result) if current => {
                let preview_start = Instant::now();
                let f = frame.clone();
                let previous = settings
                    .onnx
                    .temporal
                    .enabled
                    .then(|| self.recognized_frame.lock().clone())
                    .flatten();
                let encoded = previous.as_ref().and_then(|p| {
                    self.history
                        .lock()
                        .iter()
                        .find(|s| {
                            s.preview.frame_id == p.id && s.preview.generation == p.generation
                        })
                        .map(|s| s.preview.clone())
                });
                let (preview, preview_reused) =
                    tokio::task::spawn_blocking(move || -> Result<_> {
                        if let (Some(previous), Some(encoded)) = (previous, encoded) {
                            if let Some(reused) =
                                crate::capture::reuse_preview(&previous, &f, &encoded)
                            {
                                return Ok((reused, true));
                            }
                        }
                        Ok((preview(&f)?, false))
                    })
                    .await??;
                if let Some(metrics) = &mut result.metrics.execution {
                    metrics.app_latency = Some(gameocr_core::parallel::AppLatency {
                        capture_readback_ms: frame.capture.readback_ms,
                        conversion_ms: frame.capture.conversion_ms,
                        frame_wait_ms,
                        gate_ms,
                        preview_ms: preview_start.elapsed().as_secs_f64() * 1000.,
                        preview_reused,
                        ..Default::default()
                    });
                    metrics.end_to_end_ms = end_to_end.elapsed().as_secs_f64() * 1000.;
                    if let Some(status) = self.executor_status.read().as_ref() {
                        status.lock().last = Some(metrics.clone());
                    }
                }
                // Cancellation may happen while JPEG encoding is off-thread. Serialize the final
                // commit against stop/target changes, then check BOTH generations again.
                let _gate = self.shared.transition.lock();
                if epoch != self.ocr_epoch.load(Ordering::Acquire)
                    || generation != self.shared.generation.load(Ordering::Acquire)
                {
                    return Ok(());
                }
                if let Some(event) = learning.take() {
                    self.adaptive_profiles.commit(event);
                }
                self.detector.lock().commit(&frame.image);
                *self.recognized_frame.lock() = Some(frame.clone());
                {
                    let mut perf = self.performance.lock();
                    perf.last_ocr_ms = Some(result.metrics.total_ms);
                    perf.preprocessing_ms = Some(result.metrics.preprocessing_ms);
                    perf.inference_ms = Some(result.metrics.inference_ms);
                }
                let mut ds = self.durations.lock();
                ds.push_back(result.metrics.total_ms);
                while ds.len() > 100 {
                    ds.pop_front();
                }
                self.performance.lock().average_ocr_ms =
                    Some(ds.iter().sum::<f64>() / ds.len() as f64);
                drop(ds);
                self.times.lock().push_back(Instant::now());
                if let Some(m) = result
                    .metrics
                    .execution
                    .as_mut()
                    .and_then(|m| m.app_latency.as_mut())
                {
                    m.capture_to_publish_ms = frame
                        .capture
                        .arrived
                        .map(|t| t.elapsed().as_secs_f64() * 1000.);
                }
                let snapshot = Snapshot { result, preview };
                {
                    let mut history = self.history.lock();
                    // A forced rerun of a paused frame supersedes its previous result.
                    // frame_id remains a unique export key, independent of inference count.
                    history.retain(|old| old.result.frame_id != snapshot.result.frame_id);
                    history.push_front(snapshot.clone());
                    while history.len() > settings.history_limit {
                        history.pop_back();
                    }
                }
                let publish = Instant::now();
                let _ = self.app.emit("ocr-result", &snapshot);
                self.translation.submit(&snapshot.result, generation, epoch, frame.capture.arrived, false);
                // Emission duration is available after emitting: update telemetry, not the past event.
                if let Some(status) = self.executor_status.read().as_ref() {
                    let mut metrics = snapshot.result.metrics.execution.clone();
                    if let Some(a) = metrics.as_mut().and_then(|m| m.app_latency.as_mut()) {
                        a.publish_ms = Some(publish.elapsed().as_secs_f64() * 1000.);
                    }
                    status.lock().last = metrics;
                }
            }
            Err(e) => {
                // A failed worker is never still advertised as ready, even when its request became stale.
                if self.executor.lock().await.is_none()
                    && self.model_status.lock().status == "ready"
                {
                    self.model_status.lock().status = "error".into();
                    self.emit_model();
                    self.continuous.store(false, Ordering::Release);
                }
                if current {
                    {
                        let _gate = self.shared.transition.lock();
                        if epoch == self.ocr_epoch.load(Ordering::Acquire)
                            && generation == self.shared.generation.load(Ordering::Acquire)
                        {
                            if let Some(event) = learning.take() {
                                self.adaptive_profiles.commit(event);
                            }
                        }
                    }
                    let _ = self.app.emit("ocr-error", format!("{e:#}"));
                    self.set_ocr_status("error");
                    return Err(e);
                }
            }
            _ => {}
        }
        self.set_ocr_status(if self.continuous.load(Ordering::Acquire) {
            "waiting"
        } else {
            "idle"
        });
        Ok(())
    }
    pub fn launch(self: &Arc<Self>) {
        self.translation.launch(Arc::downgrade(self));
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last = Instant::now() - Duration::from_secs(60);
            while !service.shutdown.load(Ordering::Acquire) {
                if service.continuous.load(Ordering::Acquire)
                    && *service.shared.status.lock() == "error"
                {
                    service.stop_ocr();
                }
                let interval = service.settings.read().ocr_interval_ms;
                if service.continuous.load(Ordering::Acquire)
                    && *service.shared.status.lock() == "running"
                    && last.elapsed() >= Duration::from_millis(interval)
                {
                    last = Instant::now();
                    let _ = service.recognize(false).await;
                }
                let poll_ms = if service.settings.read().onnx.temporal.enabled && interval < 100 {
                    5
                } else {
                    40
                };
                tokio::time::sleep(Duration::from_millis(poll_ms)).await;
            }
        });
        let service = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_frame = 0;
            let mut last_count = 0;
            let mut sample = Instant::now();
            while !service.shutdown.load(Ordering::Acquire) {
                let frame = service.shared.preview_frame.lock().clone();
                if let Some(f) = frame.filter(|f| f.id != last_frame) {
                    last_frame = f.id;
                    if let Ok(Ok(p)) = tokio::task::spawn_blocking(move || preview(&f)).await {
                        let _gate = service.shared.transition.lock();
                        if p.generation == service.shared.generation.load(Ordering::Acquire) {
                            let _ = service.app.emit("capture-preview", p);
                        }
                    }
                }
                if sample.elapsed() >= Duration::from_secs(1) {
                    let count = service.shared.captured.load(Ordering::Relaxed);
                    let status = service.model_status.lock().clone();
                    let mut times = service.times.lock();
                    while times
                        .front()
                        .is_some_and(|t| t.elapsed() > Duration::from_secs(60))
                    {
                        times.pop_front();
                    }
                    let mut perf = service.performance.lock();
                    perf.capture_fps = (count - last_count) as f64 / sample.elapsed().as_secs_f64();
                    perf.ocr_per_minute = times.len();
                    perf.dropped_frames = service.shared.latest.lock().dropped;
                    perf.queue = if service.processing.load(Ordering::Acquire) {
                        "处理中"
                    } else if service.shared.latest.lock().peek().is_some() {
                        "最新帧就绪"
                    } else {
                        "空闲"
                    }
                    .into();
                    perf.model_load_ms = status.load_ms;
                    perf.device = status.device;
                    perf.execution = service
                        .executor_status
                        .read()
                        .as_ref()
                        .map(|s| s.lock().clone());
                    let _ = service.app.emit("performance-update", perf.clone());
                    last_count = count;
                    sample = Instant::now();
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });
    }
    pub async fn close(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.stop_ocr();
        self.capture.lock().stop();
        self.translation.close().await;
        if let Some(mut worker) = self.executor.lock().await.take() {
            worker.shutdown().await;
        }
        // Only this process's application-owned session directory is removed.
        let profiles = self.adaptive_profiles.clone();
        let _ = tokio::task::spawn_blocking(move || profiles.flush()).await;
        let _ = std::fs::remove_dir_all(&self.cache_dir);
    }
    pub fn list_models(&self) -> Result<Vec<ModelEntry>> {
        ModelManager::list(&self.settings.read().model_dir)
    }
}
