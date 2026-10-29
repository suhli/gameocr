pub mod config;
pub mod models;
mod worker;
use anyhow::{Context, Result, ensure};
use config::TranslationSettings;
use gameocr_core::types::{Device, OcrResult};
use gameocr_translation::{
    cache::{self, CachedText, TranslationCache},
    text,
    types::*,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Emitter;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationStatus {
    pub status: String,
    pub model_id: Option<String>,
    pub device: Option<String>,
    pub message: Option<String>,
    pub model_load_ms: f64,
    pub backend_prepare_ms: f64,
    pub calls: u64,
    pub batch_calls: u64,
    pub batch_fallbacks: u64,
    pub cache_hits: u64,
    pub skipped: u64,
    pub dropped: u64,
    pub submitted: u64,
    pub free_vram_mb: Option<f64>,
    pub estimated_vram_mb: Option<f64>,
    pub last: Option<TranslationMetrics>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslationEvent {
    #[serde(flatten)]
    pub identity: Identity,
    pub results: Vec<TranslationResult>,
}
#[derive(Clone)]
struct Job {
    cancel: Arc<AtomicBool>,
    identity: Identity,
    ocr: OcrResult,
    available: Instant,
    capture: Option<Instant>,
    deadline: Instant,
    signature: String,
}
struct Queue {
    revision: u64,
    latest: Option<Job>,
    pending: Option<Job>,
    context: VecDeque<String>,
    context_version: u64,
    history: VecDeque<TranslationEvent>,
}
impl Queue {
    // Only the current live job may seed a newly enabled overlay. History
    // selection in the UI never participates, and observed time is not renewed.
    fn overlay_seed(
        &self,
        generation: u64,
        epoch: u64,
    ) -> Option<(&Job, Option<&TranslationEvent>)> {
        let job = self.latest.as_ref()?;
        if job.identity.generation != generation
            || job.identity.epoch != epoch
            || job.identity.revision != self.revision
            || job.cancel.load(Ordering::Acquire)
        {
            return None;
        }
        let event = self
            .history
            .iter()
            .find(|event| event.identity == job.identity);
        Some((job, event))
    }
    fn submit(
        &mut self,
        ocr: &OcrResult,
        generation: u64,
        epoch: u64,
        capture: Option<Instant>,
        force: bool,
        debounce_ms: u64,
    ) -> bool {
        let signature = serde_json::to_string(
            &ocr.blocks
                .iter()
                .map(|b| text::normalize(&b.text))
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let now = Instant::now();
        let same = self.latest.as_ref().is_some_and(|j| {
            j.signature == signature
                && j.identity.generation == generation
                && j.identity.epoch == epoch
        });
        if !same {
            if let Some(job) = &self.latest {
                job.cancel.store(true, Ordering::Release);
            }
            self.revision += 1;
        }
        let deadline = if same {
            self.latest.as_ref().unwrap().deadline
        } else {
            now + Duration::from_millis(if force { 0 } else { debounce_ms })
        };
        let job = Job {
            cancel: if same {
                self.latest
                    .as_ref()
                    .map(|j| j.cancel.clone())
                    .unwrap_or_default()
            } else {
                Arc::new(AtomicBool::new(false))
            },
            identity: Identity {
                frame_id: ocr.frame_id,
                generation,
                epoch,
                revision: self.revision,
            },
            ocr: ocr.clone(),
            available: now,
            capture,
            deadline,
            signature,
        };
        self.latest = Some(job.clone());
        // At most one pending OCR snapshot. Identical text does not reset stability.
        self.pending.replace(job).is_some() && !same
    }
    fn invalidate(&mut self, clear_context: bool) -> bool {
        if let Some(job) = &self.latest {
            job.cancel.store(true, Ordering::Release);
        }
        self.revision += 1;
        let dropped = self.pending.take().is_some();
        self.latest = None;
        if clear_context {
            self.context.clear();
            self.context_version += 1;
        }
        dropped
    }
}
struct Session {
    worker: Option<worker::TranslationWorker>,
    config: Option<TranslationLoad>,
    cancel: Arc<AtomicBool>,
    preferred: Device,
}
#[derive(Serialize, Deserialize)]
struct DiskCache {
    version: u32,
    entries: Vec<CachedText>,
}
pub struct TranslationManager {
    app: tauri::AppHandle,
    overlay: Arc<crate::overlay::OverlayManager>,
    pub settings: RwLock<TranslationSettings>,
    pub status: Mutex<TranslationStatus>,
    queue: Mutex<Queue>,
    cache: Mutex<TranslationCache>,
    notify: tokio::sync::Notify,
    session: tokio::sync::Mutex<Session>,
    lifecycle: tokio::sync::Mutex<()>,
    operation: tokio::sync::Mutex<()>,
    pub download_cancel: AtomicBool,
    stopping: AtomicBool,
    active_cancel: Mutex<Arc<AtomicBool>>,
    pub gpu_runtime: crate::cuda_runtime::RuntimeManager,
    cache_epoch: AtomicU64,
}
impl TranslationManager {
    pub fn new(app: tauri::AppHandle, overlay: Arc<crate::overlay::OverlayManager>) -> Arc<Self> {
        let path = crate::settings::data_dir().join("translation/settings.json");
        let settings = std::fs::read(path)
            .ok()
            .and_then(|b| TranslationSettings::from_saved(&b).ok())
            .filter(|s| s.validate().is_ok())
            .unwrap_or_default();
        let mut cache = TranslationCache::new(settings.cache_capacity);
        if settings.persistent_cache {
            if std::fs::metadata(Self::cache_path()).is_ok_and(|m| m.len() <= 5 * 1024 * 1024) {
                if let Ok(bytes) = std::fs::read(Self::cache_path()) {
                    if let Ok(d) = serde_json::from_slice::<DiskCache>(&bytes) {
                        if d.version == 1 {
                            cache.restore(d.entries);
                        }
                    }
                }
            }
        }
        Arc::new(Self {
            app,
            overlay,
            cache: Mutex::new(cache),
            settings: RwLock::new(settings),
            status: Mutex::new(TranslationStatus {
                status: "unloaded".into(),
                ..Default::default()
            }),
            queue: Mutex::new(Queue {
                revision: 1,
                latest: None,
                pending: None,
                context: VecDeque::new(),
                context_version: 1,
                history: VecDeque::new(),
            }),
            session: tokio::sync::Mutex::new(Session {
                worker: None,
                config: None,
                cancel: Arc::new(AtomicBool::new(false)),
                preferred: Device::Auto,
            }),
            lifecycle: tokio::sync::Mutex::new(()),
            operation: tokio::sync::Mutex::new(()),
            notify: tokio::sync::Notify::new(),
            download_cancel: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            active_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            gpu_runtime: Default::default(),
            cache_epoch: AtomicU64::new(0),
        })
    }
    fn cache_path() -> std::path::PathBuf {
        crate::settings::data_dir().join("translation/cache.json")
    }
    fn emit(&self) {
        let _ = self
            .app
            .emit("translation-status", self.status.lock().clone());
    }
    pub fn invalidate(&self, clear_context: bool) {
        let dropped = {
            let mut queue = self.queue.lock();
            let dropped = queue.invalidate(clear_context);
            self.overlay.clear();
            dropped
        };
        if dropped {
            self.status.lock().dropped += 1;
        }
        {
            let mut status = self.status.lock();
            if status.status == "translating" {
                status.status = "discarding".into();
            }
        }
        self.emit();
        let _ = self.app.emit("translation-invalidated", ());
        self.notify.notify_one();
    }
    pub fn submit(
        &self,
        ocr: &OcrResult,
        generation: u64,
        epoch: u64,
        capture: Option<Instant>,
        force: bool,
    ) {
        let settings = self.settings.read();
        if !force && !settings.enabled {
            self.overlay.clear();
            return;
        }
        if self.status.lock().model_id.is_none() {
            self.overlay.clear();
            return;
        }
        let dropped = {
            let mut queue = self.queue.lock();
            let dropped =
                queue.submit(ocr, generation, epoch, capture, force, settings.debounce_ms);
            if let Some(job) = &queue.latest {
                self.overlay.observe(ocr, &job.identity, job.available);
            }
            dropped
        };
        let mut status = self.status.lock();
        status.dropped += u64::from(dropped);
        status.submitted += 1;
        drop(status);
        drop(settings);
        self.notify.notify_one();
    }
    pub fn refresh_overlay(&self, service: &crate::service::Service) {
        let _gate = service.shared.transition.lock();
        if self.stopping.load(Ordering::Acquire)
            || service.shutdown.load(Ordering::Acquire)
            || *service.shared.status.lock() != "running"
        {
            return;
        }
        let queue = self.queue.lock();
        if let Some((job, event)) = queue.overlay_seed(
            service.shared.generation.load(Ordering::Acquire),
            service.ocr_epoch.load(Ordering::Acquire),
        ) {
            self.overlay.observe(&job.ocr, &job.identity, job.available);
            if let Some(event) = event {
                self.overlay.accept(event);
            }
        }
    }
    pub fn launch(self: &Arc<Self>, service: Weak<crate::service::Service>) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                manager.notify.notified().await;
                if manager.stopping.load(Ordering::Acquire) {
                    break;
                }
                loop {
                    let pending = manager.queue.lock().pending.clone();
                    let Some(job) = pending else { break };
                    let delay = job.deadline.saturating_duration_since(Instant::now());
                    if !delay.is_zero() {
                        tokio::select! {_=tokio::time::sleep(delay)=>{},_=manager.notify.notified()=>continue}
                    }
                    let next = {
                        let mut q = manager.queue.lock();
                        if q.pending
                            .as_ref()
                            .is_some_and(|p| p.identity.revision == job.identity.revision)
                        {
                            q.pending.take()
                        } else {
                            None
                        }
                    };
                    if let Some(job) = next {
                        if let Some(s) = service.upgrade() {
                            manager.process(&s, job).await;
                            // Publish idle/error state even when obsolete text was discarded.
                            manager.emit();
                        } else {
                            return;
                        }
                    }
                    if manager.stopping.load(Ordering::Acquire) {
                        return;
                    }
                }
            }
        });
    }
    fn valid(&self, s: &crate::service::Service, job: &Job) -> bool {
        !self.stopping.load(Ordering::Acquire)
            && job.identity.epoch == s.ocr_epoch.load(Ordering::Acquire)
            && job.identity.generation == s.shared.generation.load(Ordering::Acquire)
            && self.queue.lock().revision == job.identity.revision
    }
    async fn process(&self, s: &crate::service::Service, job: Job) {
        if !self.valid(s, &job) {
            self.status.lock().dropped += 1;
            return;
        }
        let settings = self.settings.read().clone();
        let started = Instant::now();
        let groups = text::groups(&job.ocr.blocks, settings.grouping);
        let (context, version) = {
            let q = self.queue.lock();
            (q.context.iter().cloned().collect(), q.context_version)
        };
        let options = settings.options(context, version);
        let text_ms = started.elapsed().as_secs_f64() * 1000.;
        let requests: Vec<_> = groups
            .iter()
            .map(|group| {
                let mut opts = options.clone();
                opts.source_language = text::language(&group.source_text, &options.source_language);
                // Short labels/HUD do not carry narrative context automatically.
                if group.source_text.chars().count() < 20 {
                    opts.context.clear();
                    opts.background.clear();
                    opts.context_version = 0;
                }
                TranslationRequest {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    source_text: group.source_text.clone(),
                    options: opts,
                }
            })
            .collect();
        let mut batch = if settings.batch_translation {
            self.try_batch(&requests, job.cancel.clone()).await
        } else {
            None
        };
        let mut results = Vec::new();
        for (index, (group, request)) in groups.into_iter().zip(requests).enumerate() {
            if !self.valid(s, &job) {
                self.status.lock().dropped += 1;
                return;
            }
            let lang = request.options.source_language.clone();
            let mut result = TranslationResult {
                profile_id: options.profile_id.clone(),
                request_id: request.request_id.clone(),
                identity: job.identity.clone(),
                source_model_id: job.ocr.model_id.clone().unwrap_or_default(),
                translation_model_id: self.status.lock().model_id.clone().unwrap_or_default(),
                source_language: lang.clone(),
                target_language: options.target_language.clone(),
                source_text: group.source_text.clone(),
                translated_text: String::new(),
                source_block_ids: group.blocks.iter().map(|b| b.id.clone()).collect(),
                source_blocks: group.blocks,
                glossary_version: options.glossary_version,
                context_version: request.options.context_version,
                cache_hit: false,
                status: "complete".into(),
                error: None,
                device: String::new(),
                truncated: false,
                stop_reason: String::new(),
                metrics: Default::default(),
            };
            let inferred = if let Some(prepared) = batch.as_mut().and_then(|b| b[index].take()) {
                Ok(prepared)
            } else {
                self.translate_group(
                    request,
                    lang == "none" || lang == options.target_language,
                    job.cancel.clone(),
                )
                .await
            };
            match inferred {
                Ok((generated, hit, id)) => {
                    result.translation_model_id = id;
                    result.translated_text = generated.text;
                    result.device = generated.device;
                    result.truncated = generated.truncated;
                    result.stop_reason = generated.stop_reason;
                    if result.truncated {
                        result.status = "incomplete".into();
                    }
                    result.metrics = generated.metrics;
                    result.cache_hit = hit;
                }
                Err(e) => {
                    result.status = "error".into();
                    result.error = Some(format!("{e:#}"));
                }
            }
            result.metrics.queue_ms = started.duration_since(job.available).as_secs_f64() * 1000.;
            result.metrics.text_processing_ms = text_ms;
            result.metrics.ocr_ms = job.ocr.metrics.total_ms;
            results.push((index, result));
        }
        // Same gate as OCR publication / target changes. No awaited work inside it.
        let _gate = s.shared.transition.lock();
        if !self.valid(s, &job) {
            self.status.lock().dropped += 1;
            return;
        }
        let mut q = self.queue.lock();
        if q.revision != job.identity.revision {
            self.status.lock().dropped += 1;
            return;
        }
        let Some(latest) = q.latest.clone() else {
            return;
        };
        let current_groups = text::groups(&latest.ocr.blocks, settings.grouping);
        let publish = Instant::now();
        let mut final_results = Vec::new();
        for (i, mut result) in results {
            let Some(group) = current_groups.get(i) else {
                continue;
            };
            if group.source_text != result.source_text {
                continue;
            }
            result.identity = latest.identity.clone();
            result.source_blocks = group.blocks.clone();
            result.source_block_ids = group.blocks.iter().map(|b| b.id.clone()).collect();
            result.metrics.ocr_to_translation_ms = latest.available.elapsed().as_secs_f64() * 1000.;
            result.metrics.capture_to_translation_ms =
                latest.capture.map(|t| t.elapsed().as_secs_f64() * 1000.);
            final_results.push(result);
        }
        let event = TranslationEvent {
            identity: latest.identity.clone(),
            results: final_results,
        };
        q.history.retain(|e| {
            e.identity.frame_id != event.identity.frame_id
                || e.identity.generation != event.identity.generation
        });
        q.history.push_front(event.clone());
        while q.history.len() > 100 {
            q.history.pop_back();
        }
        self.overlay.accept(&event);
        let _ = self.app.emit("translation-result", &event);
        if let Some(last) = event.results.last() {
            let mut metrics = last.metrics.clone();
            metrics.publish_ms = publish.elapsed().as_secs_f64() * 1000.;
            self.status.lock().last = Some(metrics);
        }
        drop(q);
        self.emit();
    }
    async fn try_batch(
        &self,
        requests: &[TranslationRequest],
        cancel: Arc<AtomicBool>,
    ) -> Option<Vec<Option<(GenerationResult, bool, String)>>> {
        let config = self.session.lock().await.config.clone()?;
        let plan = gameocr_translation::batch::plan(requests, &config, &mut self.cache.lock());
        let mut ready: Vec<_> = plan
            .ready
            .into_iter()
            .map(|result| {
                result.map(|r| {
                    let hit = r.device == "cache";
                    let mut status = self.status.lock();
                    if hit {
                        status.cache_hits += 1;
                    } else {
                        status.skipped += 1;
                    }
                    (r, hit, config.manifest.id.clone())
                })
            })
            .collect();
        let misses: Vec<_> = plan.missing.iter().map(|&i| requests[i].clone()).collect();
        let Some(packed) = gameocr_translation::batch::pack(&misses, config.generation.max_tokens)
        else {
            return Some(ready);
        };
        if cancel.load(Ordering::Acquire) {
            return Some(ready);
        }
        let cache_epoch = self.cache_epoch.load(Ordering::Acquire);
        self.status.lock().batch_calls += 1;
        let generated = self.translate_group(packed, false, cancel.clone()).await;
        let output = generated.and_then(|(output, _, _)| {
            gameocr_translation::batch::validate_output(&misses, &output)
        });
        let Ok(outputs) = output else {
            if !cancel.load(Ordering::Acquire) {
                self.status.lock().batch_fallbacks += 1;
            }
            return Some(ready);
        };
        let unchanged = self
            .session
            .lock()
            .await
            .config
            .as_ref()
            .is_some_and(|current| {
                current.manifest.sha256 == config.manifest.sha256
                    && current.generation == config.generation
            });
        let mut cache = self.cache.lock();
        let may_cache = unchanged
            && !cancel.load(Ordering::Acquire)
            && self.cache_epoch.load(Ordering::Acquire) == cache_epoch;
        for ((index, request), result) in plan.missing.into_iter().zip(misses).zip(outputs) {
            if may_cache {
                cache.insert(CachedText {
                    key: cache::key(&config.manifest, &config.generation, &request),
                    text: result.text.clone(),
                    truncated: false,
                });
            }
            ready[index] = Some((result, false, config.manifest.id.clone()));
        }
        Some(ready)
    }
    async fn translate_group(
        &self,
        request: TranslationRequest,
        skip: bool,
        cancel: Arc<AtomicBool>,
    ) -> Result<(GenerationResult, bool, String)> {
        let mut session = self.session.lock().await;
        let mut config = session.config.clone().context("请先加载翻译模型")?;
        if cancel.load(Ordering::Acquire) {
            return Ok((
                GenerationResult::cancelled(request.request_id, String::new()),
                false,
                config.manifest.id,
            ));
        }
        let key = cache::key(&config.manifest, &config.generation, &request);
        let cache_epoch = self.cache_epoch.load(Ordering::Acquire);
        // Structured output is cached only after the caller validates every item ID.
        let cacheable = !request.options.batch_format;
        if skip {
            self.status.lock().skipped += 1;
            return Ok((
                GenerationResult {
                    request_id: request.request_id,
                    text: request.source_text,
                    device: "skipped".into(),
                    truncated: false,
                    stop_reason: "sameLanguageOrNonText".into(),
                    metrics: Default::default(),
                },
                false,
                config.manifest.id,
            ));
        }
        if let Some(hit) = self.cache.lock().get(&key) {
            self.status.lock().cache_hits += 1;
            return Ok((
                GenerationResult {
                    request_id: request.request_id,
                    text: hit.text,
                    device: "cache".into(),
                    truncated: hit.truncated,
                    stop_reason: "cache".into(),
                    metrics: Default::default(),
                },
                true,
                config.manifest.id,
            ));
        }
        if session.cancel.load(Ordering::Acquire) {
            if let Some(mut old) = session.worker.take() {
                old.shutdown().await;
            }
        }
        if session.worker.is_none() {
            session.cancel = Arc::new(AtomicBool::new(false));
            *self.active_cancel.lock() = session.cancel.clone();
            let root = crate::ocr::resource_root(&self.app)?;
            let loaded = worker::load_with_fallback(
                session.preferred,
                &mut config,
                session.cancel.clone(),
                |config, cancel| worker::TranslationWorker::load(&root, config, cancel),
            )
            .await;
            // Retain CPU selection even if its first recovery load fails.
            session.config = Some(config.clone());
            match loaded {
                Ok((worker, fallback)) => {
                    let mut st = self.status.lock();
                    st.device = Some(worker.device.clone());
                    st.model_load_ms = worker.load_ms;
                    st.message = fallback.or_else(|| {
                        (session.preferred == Device::Auto && config.device == Device::Cpu)
                            .then(|| "Auto 已使用 CPU 恢复翻译 Worker".into())
                    });
                    session.worker = Some(worker);
                }
                Err(error) => {
                    let mut st = self.status.lock();
                    st.status = "error".into();
                    st.device = None;
                    st.message = Some(format!("翻译 Worker 恢复失败：{error:#}"));
                    return Err(error);
                }
            }
        }
        {
            let mut status = self.status.lock();
            status.status = "translating".into();
            status.calls += 1;
        }
        self.emit();
        let worker = session.worker.as_mut().context("翻译 Worker 未加载")?;
        let output = worker.translate(request, cancel.clone()).await;
        match output {
            Ok(mut result) => {
                result.metrics.process_memory_mb = worker.memory_mb();
                result.metrics.backend_prepare_ms = self.status.lock().backend_prepare_ms;
                let mut cache = self.cache.lock();
                if cacheable
                    && result.cacheable()
                    && !cancel.load(Ordering::Acquire)
                    && self.cache_epoch.load(Ordering::Acquire) == cache_epoch
                {
                    cache.insert(CachedText {
                        key,
                        text: result.text.clone(),
                        truncated: result.truncated,
                    });
                }
                drop(cache);
                {
                    let mut status = self.status.lock();
                    status.status = "ready".into();
                }
                Ok((result, false, config.manifest.id))
            }
            Err(e) => {
                if let Some(mut w) = session.worker.take() {
                    w.shutdown().await;
                }
                let recovery = worker::recovery_device(session.preferred, config.device);
                if let Some(config) = &mut session.config {
                    config.device = recovery;
                }
                let mut status = self.status.lock();
                status.status = "error".into();
                status.device = None;
                status.message = Some(if recovery != config.device {
                    format!("{e:#}；Auto 下一次请求将使用 CPU 恢复")
                } else {
                    format!("{e:#}；下一次请求可重建 Worker")
                });
                Err(e)
            }
        }
    }
    pub async fn load(&self, id: String) -> Result<()> {
        let _lifecycle = self.lifecycle.try_lock().context("翻译模型正在切换")?;
        let _operation = self
            .operation
            .try_lock()
            .context("翻译模型文件操作正在进行")?;
        self.invalidate(true);
        self.unload_inner().await;
        {
            let mut st = self.status.lock();
            st.status = "loading".into();
            st.message = None;
        }
        self.emit();
        let outcome = self.load_inner(id).await;
        if let Err(e) = &outcome {
            let mut st = self.status.lock();
            st.status = "error".into();
            st.message = Some(format!("{e:#}"));
        }
        self.emit();
        outcome
    }
    async fn load_inner(&self, id: String) -> Result<()> {
        let manifest = tokio::task::spawn_blocking(move || {
            let m = models::find(&id)?;
            models::verify(&m)?;
            Ok::<_, anyhow::Error>(m)
        })
        .await??;
        let settings = self.settings.read().clone();
        let root = crate::ocr::resource_root(&self.app)?;
        let preferred = if settings.resource_policy == "cpuOnly" {
            Device::Cpu
        } else {
            settings.device
        };
        let free = worker::free_vram().await.ok();
        // Weights + bounded KV/scratch + policy reserve; actual allocation can differ.
        let estimate = manifest.size as f64 / 1048576.
            + settings.generation.context_size as f64 * 0.0625
            + 512.;
        let reserve = if settings.resource_policy == "balanced" {
            settings.gpu_reserve_mb.max(2048)
        } else {
            settings.gpu_reserve_mb
        } as f64;
        let cuda_allowed = free.is_some_and(|v| v > estimate + reserve);
        {
            let mut st = self.status.lock();
            st.free_vram_mb = free;
            st.estimated_vram_mb = Some(estimate);
        }
        if preferred == Device::Cuda {
            ensure!(
                cuda_allowed,
                "显存不足或无法读取 NVIDIA 显存：当前可用 {} MiB，预计翻译需约 {estimate:.0} MiB，另保留 {reserve:.0} MiB。可选择 CPU/Auto",
                free.map_or("未知".into(), |v| format!("{v:.0}"))
            );
        }
        let device = if preferred == Device::Cpu || (preferred == Device::Auto && !cuda_allowed) {
            Device::Cpu
        } else {
            Device::Cuda
        };
        let mut config = TranslationLoad {
            manifest: manifest.clone(),
            path: models::directory()
                .join(&manifest.file)
                .to_string_lossy()
                .into(),
            device,
            threads: settings.threads,
            generation: settings.generation,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        *self.active_cancel.lock() = cancel.clone();
        let preparation = Instant::now();
        let gpu = if device == Device::Cuda && preferred == Device::Cuda {
            self.gpu_runtime
                .ensure(&root, "translation", cancel.clone(), |p| {
                    let _ = self.app.emit("cuda-runtime-progress", p);
                })
                .await
        } else {
            Ok(())
        };
        let prepare_ms = preparation.elapsed().as_secs_f64() * 1000.;
        gpu?;
        let (worker, recovered) =
            worker::load_with_fallback(preferred, &mut config, cancel.clone(), |config, cancel| {
                worker::TranslationWorker::load(&root, config, cancel)
            })
            .await?;
        let mut fallback = if preferred == Device::Auto && !cuda_allowed {
            Some("当前可用显存未达到翻译准入要求，Auto 已使用 CPU".into())
        } else {
            None
        };
        if recovered.is_some() {
            fallback = recovered;
        }
        ensure!(!self.stopping.load(Ordering::Acquire), "应用正在退出");
        {
            let mut st = self.status.lock();
            st.status = "ready".into();
            st.model_id = Some(id_of(&manifest));
            st.device = Some(worker.device.clone());
            st.model_load_ms = worker.load_ms;
            st.backend_prepare_ms = prepare_ms;
            st.message = fallback;
        }
        let mut session = self.session.lock().await;
        session.worker = Some(worker);
        session.config = Some(config);
        session.cancel = cancel;
        session.preferred = preferred;
        Ok(())
    }
    async fn unload_inner(&self) {
        self.active_cancel.lock().store(true, Ordering::Release);
        let mut s = self.session.lock().await;
        if let Some(mut w) = s.worker.take() {
            w.shutdown().await;
        }
        s.config = None;
        let mut st = self.status.lock();
        st.status = "unloaded".into();
        st.model_id = None;
        st.device = None;
    }
    pub async fn unload(&self) -> Result<()> {
        let _g = self.lifecycle.try_lock().context("翻译模型正在切换")?;
        self.invalidate(true);
        self.unload_inner().await;
        self.emit();
        Ok(())
    }
    pub async fn update(&self, mut settings: TranslationSettings) -> Result<()> {
        settings.validate()?;
        let _g = self.lifecycle.try_lock().context("翻译模型正在切换")?;
        let old = self.settings.read().clone();
        settings.update_profile_versions(&old)?;
        self.invalidate(true);
        if old.device != settings.device
            || old.resource_policy != settings.resource_policy
            || old.threads != settings.threads
            || old.generation != settings.generation
        {
            self.unload_inner().await;
        }
        crate::settings::atomic_json(
            &crate::settings::data_dir().join("translation/settings.json"),
            &settings,
        )?;
        if old.cache_capacity != settings.cache_capacity {
            let previous = self.cache.lock().export();
            let mut c = TranslationCache::new(settings.cache_capacity);
            c.restore(previous);
            *self.cache.lock() = c;
        }
        if !settings.persistent_cache && Self::cache_path().exists() {
            std::fs::remove_file(Self::cache_path())?;
        }
        *self.settings.write() = settings;
        self.invalidate(true);
        self.emit();
        Ok(())
    }
    pub fn history(&self) -> Vec<TranslationEvent> {
        self.queue.lock().history.iter().cloned().collect()
    }
    pub fn confirm_context(&self, frame_id: u64) -> Result<()> {
        let settings = self.settings.read();
        ensure!(
            settings.context_enabled && settings.profile_id.is_some(),
            "请先选择游戏 Profile 并启用上下文"
        );
        let mut q = self.queue.lock();
        let event = q
            .history
            .iter()
            .find(|e| e.identity.frame_id == frame_id)
            .context("译文已不在历史中")?
            .clone();
        for result in event.results {
            ensure!(
                Some(&result.profile_id) == settings.profile_id.as_ref(),
                "该对白属于其他游戏 Profile"
            );
            if result.status == "complete" && result.source_text.len() < 2048 {
                q.context.push_back(format!(
                    "{} → {}",
                    result.source_text, result.translated_text
                ));
            }
        }
        while q.context.len() > settings.context_lines
            || q.context.iter().map(String::len).sum::<usize>() > 4096
        {
            q.context.pop_front();
        }
        q.context_version += 1;
        Ok(())
    }
    pub fn clear_cache(&self) -> Result<()> {
        self.invalidate(false);
        let mut cache = self.cache.lock();
        self.cache_epoch.fetch_add(1, Ordering::AcqRel);
        cache.clear();
        drop(cache);
        if Self::cache_path().exists() {
            std::fs::remove_file(Self::cache_path())?;
        }
        Ok(())
    }
    pub async fn download(
        &self,
        mirror: String,
        manager: Arc<crate::model::ModelManager>,
    ) -> Result<()> {
        let _g = self
            .operation
            .try_lock()
            .context("翻译模型文件操作正在进行")?;
        self.download_cancel.store(false, Ordering::Release);
        models::download(&manager, &mirror, &self.app, &self.download_cancel).await
    }
    pub async fn import(&self, path: std::path::PathBuf) -> Result<TranslationManifest> {
        let _g = self
            .operation
            .try_lock()
            .context("翻译模型文件操作正在进行")?;
        ensure!(self.status.lock().model_id.is_none(), "请先卸载翻译模型");
        tokio::task::spawn_blocking(move || models::import(&path)).await?
    }
    pub async fn delete(&self, id: String) -> Result<()> {
        let _g = self
            .operation
            .try_lock()
            .context("翻译模型文件操作正在进行")?;
        ensure!(
            self.status.lock().model_id.as_ref() != Some(&id),
            "请先卸载翻译模型"
        );
        tokio::task::spawn_blocking(move || models::delete(&id)).await?
    }
    pub async fn close(&self) {
        self.stopping.store(true, Ordering::Release);
        self.download_cancel.store(true, Ordering::Release);
        self.invalidate(true);
        self.unload_inner().await;
        if self.settings.read().persistent_cache {
            let disk = DiskCache {
                version: 1,
                entries: self.cache.lock().export(),
            };
            let _ = crate::settings::atomic_json(&Self::cache_path(), &disk);
        }
    }
}
fn id_of(m: &TranslationManifest) -> String {
    m.id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queue() -> Queue {
        Queue {
            revision: 1,
            latest: None,
            pending: None,
            context: VecDeque::new(),
            context_version: 1,
            history: VecDeque::new(),
        }
    }
    fn ocr(frame: u64, text: &str) -> OcrResult {
        serde_json::from_value(serde_json::json!({"frameId":frame,"timestamp":0,"sourceWidth":800,"sourceHeight":600,"blocks":[{"id":"original","text":text,"bbox":[1,2,30,40],"confidence":0.9,"polygon":[{"x":1,"y":2}]}],"rawOutput":"raw","metrics":gameocr_core::types::Metrics::default(),"device":"cpu","truncated":false})).unwrap()
    }
    #[test]
    fn enabling_overlay_seeds_only_current_result_without_refreshing_its_age() {
        let mut q = queue();
        q.submit(&ocr(1, "New Game"), 2, 3, None, false, 0);
        let observed = Instant::now() - Duration::from_secs(31);
        q.latest.as_mut().unwrap().available = observed;
        let event = TranslationEvent {
            identity: q.latest.as_ref().unwrap().identity.clone(),
            results: vec![],
        };
        q.history.push_front(event);
        let (job, result) = q.overlay_seed(2, 3).unwrap();
        assert!(result.is_some());
        assert_eq!(job.available, observed);
        let mut live = crate::overlay::state::LiveState::default();
        live.observe(
            &job.ocr,
            &job.identity,
            Some("target".into()),
            job.available,
        );
        assert!(!live.fresh(&Default::default(), Instant::now()));
        assert!(q.overlay_seed(9, 3).is_none());
        assert!(q.overlay_seed(2, 9).is_none());
        q.submit(&ocr(2, "Changed text"), 2, 3, None, false, 0);
        assert!(q.overlay_seed(2, 3).unwrap().1.is_none());
        q.invalidate(true);
        assert!(q.overlay_seed(2, 3).is_none());
        assert!(!q.history.is_empty()); // retained history must not drive live subtitles
    }
    #[test]
    fn latest_only_cancels_running_a_and_pending_b_but_identical_frames_keep_deadline() {
        let mut q = queue();
        q.submit(&ocr(1, "A"), 1, 1, None, false, 350);
        let running = q.pending.take().unwrap();
        q.submit(&ocr(2, "A"), 1, 1, None, false, 350);
        assert!(!running.cancel.load(Ordering::Acquire));
        assert_eq!(q.latest.as_ref().unwrap().deadline, running.deadline);
        assert_eq!(q.revision, running.identity.revision);
        q.submit(&ocr(3, "B"), 1, 1, None, false, 350);
        let b = q.pending.clone().unwrap();
        assert!(running.cancel.load(Ordering::Acquire));
        q.submit(&ocr(4, "C"), 1, 1, None, false, 350);
        assert!(b.cancel.load(Ordering::Acquire));
        assert_ne!(q.revision, running.identity.revision);
        let c = q.pending.take().unwrap();
        assert_eq!(c.identity.frame_id, 4);
        assert_eq!(c.ocr.blocks[0].bbox, Some([1., 2., 30., 40.]));
        assert_eq!(c.ocr.blocks[0].id, "original");
        assert!(q.pending.is_none());
        q.invalidate(true); // pause, target/model change and shutdown share this path
        assert!(c.cancel.load(Ordering::Acquire));
        assert!(q.latest.is_none());
        q.submit(&ocr(5, "C"), 2, 2, None, true, 350);
        assert!(!q.latest.as_ref().unwrap().cancel.load(Ordering::Acquire));
        assert!(q.latest.as_ref().unwrap().deadline <= Instant::now());
    }
    #[test]
    fn same_text_with_new_epoch_or_generation_cancels_old_work() {
        let mut q = queue();
        for (generation, epoch) in [(1, 1), (2, 1), (2, 2)] {
            let previous = q.latest.clone();
            q.submit(&ocr(1, "same"), generation, epoch, None, false, 100);
            if let Some(previous) = previous {
                assert!(previous.cancel.load(Ordering::Acquire));
            }
        }
    }
}
