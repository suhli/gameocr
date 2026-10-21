//! Process-only executors. ONNX/CUDA remain exclusively inside worker children.
use crate::ocr::Worker;
use anyhow::{Context, Result, bail, ensure};
use futures_util::{StreamExt, stream::FuturesUnordered};
use gameocr_core::{
    engine::{EngineConfig, RuntimeConfig, RuntimeRegistry, device_candidates},
    parallel::*,
    types::*,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatus {
    pub id: usize,
    pub pid: Option<u32>,
    pub device: String,
    pub state: String,
    pub threads: u32,
    pub completed_tasks: u64,
    pub failed_tasks: u64,
    pub average_ms: f64,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutorStatus {
    #[serde(default)]
    pub execution_mode: gameocr_core::adaptive::ExecutionMode,
    pub learning: Option<gameocr_core::adaptive::profiles::LearningSummary>,
    pub parallel: bool,
    pub queued_tasks: usize,
    pub frame_id: Option<u64>,
    pub workers: Vec<WorkerStatus>,
    pub last: Option<ExecutionMetrics>,
}
pub type SharedStatus = Arc<Mutex<ExecutorStatus>>;

#[derive(Clone)]
pub struct Cancellation {
    pub epoch: u64,
    valid: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl Cancellation {
    pub fn new(epoch: u64, valid: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        Self {
            epoch,
            valid: Arc::new(valid),
        }
    }
    pub fn never() -> Self {
        Self::new(0, || true)
    }
    pub fn cancelled(&self) -> bool {
        !(self.valid)()
    }
}
struct Slot {
    id: usize,
    worker: Worker,
    device: Device,
    estimate: f64,
}
struct Task {
    tile: Tile,
    path: PathBuf,
    identity: TaskIdentity,
    tried: Vec<usize>,
    queued: Instant,
}
struct TaskDirectory(PathBuf);
struct ExecutionTimer {
    status: SharedStatus,
    started: Instant,
}
impl Drop for ExecutionTimer {
    fn drop(&mut self) {
        let mut status = self.status.lock();
        status.queued_tasks = 0;
        if let Some(metrics) = &mut status.last {
            metrics.end_to_end_ms = elapsed(self.started);
        }
    }
}
impl TaskDirectory {
    fn new(root: &Path) -> Result<Self> {
        let path = root.join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TaskDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
enum Strategy {
    Sequential(Option<Slot>),
    Parallel(WorkerPool),
}
pub struct WorkerPool {
    slots: Vec<Option<Slot>>,
}
pub struct Executor {
    strategy: Strategy,
    pub status: SharedStatus,
    model_id: String,
    runtime: String,
    shared_memory: bool,
    resize: u32,
    batch: u64,
    cache_revision: u64,
    pub device: String,
    pub load_ms: f64,
    pub message: Option<String>,
}
fn elapsed(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
fn vram_reserve(manifest: &gameocr_core::manifest::ModelManifest) -> u64 {
    // Imported manifests have UUID IDs. Estimate from verified graph sizes,
    // so an imported Medium model receives the same guard as the built-in one.
    let bytes = manifest
        .files
        .iter()
        .filter(|f| matches!(f.role.as_str(), "detector" | "recognizer"))
        .fold(0u64, |total, file| total.saturating_add(file.size));
    if bytes > 64 * 1024 * 1024 { 1024 } else { 512 }
}
fn explicit_config(mut config: EngineConfig, device: Device, threads: u32) -> EngineConfig {
    config.device = device;
    config.threads = threads;
    if let RuntimeConfig::Llamacpp(p) = &mut config.inference {
        p.device = device;
        p.threads = threads;
    }
    config
}
async fn load_worker(
    root: &Path,
    config: EngineConfig,
    closing: Arc<AtomicBool>,
) -> Result<Worker> {
    RuntimeRegistry::default().validate(&config)?;
    let kind = if config.device == Device::Cuda {
        "cuda"
    } else {
        "cpu"
    };
    let executable = RuntimeRegistry::default()
        .executable(&config.manifest.runtime)?
        .to_owned();
    let path = root
        .join("workers")
        .join(&config.manifest.runtime)
        .join(kind)
        .join(executable);
    ensure!(
        path.is_file(),
        "未找到 {}/{kind} 工作程序，请运行 scripts/build-workers.ps1 或安装完整发行包",
        config.manifest.runtime
    );
    let expected = if config.device == Device::Cuda {
        "CUDA"
    } else {
        "CPU"
    };
    let mut worker = Worker::spawn(&path, closing).await?;
    match worker
        .request(WorkerRequest::LoadEngine {
            protocol_version: 2,
            config,
        })
        .await?
    {
        WorkerResponse::Ready { device, .. } => ensure!(
            device == expected,
            "Worker reported unexpected execution device"
        ),
        _ => bail!("工作程序返回了意外状态"),
    }
    Ok(worker)
}
async fn free_vram() -> Option<u64> {
    let output = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("nvidia-smi")
            .args([
                "--id=0",
                "--query-gpu=memory.free",
                "--format=csv,noheader,nounits",
            ])
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}
impl Executor {
    pub async fn load(
        root: &Path,
        config: EngineConfig,
        parallel: &ParallelConfig,
        closing: Arc<AtomicBool>,
    ) -> Result<Self> {
        let started = Instant::now();
        let enabled = parallel.enable_parallel_inference
            && config.manifest.runtime == "onnx"
            && config.manifest.adapter == "ppocrv6"
            && !matches!(&config.inference, RuntimeConfig::Onnx(p) if p.requires_whole_frame());
        let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
        if enabled {
            parallel.validate(config.device, logical)?;
        }
        let resize = match &config.inference {
            RuntimeConfig::Onnx(p) => p.detection_resize,
            _ => 0,
        };
        let status = Arc::new(Mutex::new(ExecutorStatus {
            parallel: enabled,
            ..Default::default()
        }));
        let mut slots = Vec::new();
        let mut errors = Vec::new();
        let mut fallback = false;
        if !enabled {
            for device in device_candidates(config.device) {
                match load_worker(
                    root,
                    explicit_config(config.clone(), device, config.threads),
                    closing.clone(),
                )
                .await
                {
                    Ok(worker) => {
                        slots.push(Some(Slot {
                            id: 0,
                            worker,
                            device,
                            estimate: 0.,
                        }));
                        fallback = !errors.is_empty();
                        break;
                    }
                    Err(e) => errors.push(format!("{e:#}")),
                }
            }
        } else {
            let (gpu, mut cpu) = parallel.requested(config.device);
            for _ in 0..gpu {
                let attempt = async {
                    if let Some(free) = free_vram().await {
                        let reserve = vram_reserve(&config.manifest);
                        ensure!(
                            free >= reserve,
                            "可用显存 {free} MiB，低于新 GPU Worker 的 {reserve} MiB 安全余量"
                        );
                    }
                    load_worker(
                        root,
                        explicit_config(config.clone(), Device::Cuda, 1),
                        closing.clone(),
                    )
                    .await
                }
                .await;
                match attempt {
                    Ok(worker) => {
                        let id = slots.len();
                        slots.push(Some(Slot {
                            id,
                            worker,
                            device: Device::Cuda,
                            estimate: 0.,
                        }));
                    }
                    Err(e) => {
                        errors.push(format!("{e:#}"));
                        if config.device == Device::Cuda {
                            bail!("CUDA Worker 初始化失败：{}", errors.join("; "));
                        }
                        break;
                    }
                }
            }
            if slots.is_empty() && config.device == Device::Auto && gpu > 0 {
                cpu = parallel.cpu_worker_count.max(1);
                fallback = true;
            }
            let budget = (config.threads as usize).min(logical.saturating_sub(slots.len()).max(1));
            ensure!(
                cpu <= budget,
                "CPU Worker 数量 {cpu} 超过可用线程预算 {budget}，请减少 Worker 或增加 CPU 线程数"
            );
            let cpu_threads = (budget / cpu.max(1)).max(1) as u32;
            for _ in 0..cpu {
                let worker = load_worker(
                    root,
                    explicit_config(config.clone(), Device::Cpu, cpu_threads),
                    closing.clone(),
                )
                .await?;
                let id = slots.len();
                slots.push(Some(Slot {
                    id,
                    worker,
                    device: Device::Cpu,
                    estimate: 0.,
                }));
            }
        }
        ensure!(
            !slots.is_empty(),
            "没有可执行 Worker：{}",
            errors.join("; ")
        );
        let cpu = slots
            .iter()
            .flatten()
            .filter(|s| s.device == Device::Cpu)
            .count();
        let gpu = slots.len() - cpu;
        let device = if cpu > 0 && gpu > 0 {
            "CPU + CUDA"
        } else if gpu > 0 {
            "CUDA"
        } else {
            "CPU"
        }
        .to_owned();
        let threads = if enabled {
            (config.threads as usize).min(logical.saturating_sub(gpu).max(1)) / cpu.max(1)
        } else {
            config.threads as usize
        };
        status.lock().workers = slots
            .iter()
            .flatten()
            .map(|s| WorkerStatus {
                id: s.id,
                pid: s.worker.pid(),
                device: if s.device == Device::Cuda {
                    "CUDA"
                } else {
                    "CPU"
                }
                .into(),
                state: "idle".into(),
                threads: if enabled && s.device == Device::Cuda {
                    1
                } else {
                    threads.max(1) as u32
                },
                completed_tasks: 0,
                failed_tasks: 0,
                average_ms: 0.,
            })
            .collect();
        let message = if fallback {
            Some(format!("CUDA 不可用，已回退 CPU：{}", errors.join("; ")))
        } else if !errors.is_empty() {
            Some(format!(
                "部分 GPU Worker 不可用；实际 CUDA × {gpu} / CPU × {cpu}：{}",
                errors.join("; ")
            ))
        } else if enabled {
            Some(format!(
                "并行已启用：CUDA × {gpu} / CPU × {cpu}；根据实测耗时动态分配"
            ))
        } else if parallel.enable_parallel_inference
            && matches!(&config.inference, RuntimeConfig::Onnx(p) if p.requires_whole_frame())
        {
            Some("局部、小字、补检、增量或时序识别启用时使用单 Worker，保留完整线程预算".into())
        } else {
            None
        };
        let strategy = if enabled {
            Strategy::Parallel(WorkerPool { slots })
        } else {
            Strategy::Sequential(slots.pop().unwrap())
        };
        Ok(Self {
            strategy,
            status,
            shared_memory: matches!(&config.inference, RuntimeConfig::Onnx(p) if p.experimental_shared_memory),
            model_id: config.manifest.id,
            runtime: config.manifest.runtime,
            resize,
            batch: 0,
            cache_revision: 0,
            device,
            load_ms: elapsed(started),
            message,
        })
    }
    pub fn available(&self) -> bool {
        match &self.strategy {
            Strategy::Sequential(s) => s.is_some(),
            Strategy::Parallel(p) => p.slots.iter().any(Option::is_some),
        }
    }
    pub async fn shutdown(&mut self) {
        match &mut self.strategy {
            Strategy::Sequential(slot) => {
                if let Some(mut s) = slot.take() {
                    s.worker.shutdown().await;
                }
            }
            Strategy::Parallel(pool) => {
                let mut pending = FuturesUnordered::new();
                for s in &mut pool.slots {
                    if let Some(mut s) = s.take() {
                        pending.push(async move {
                            s.worker.shutdown().await;
                        });
                    }
                }
                while pending.next().await.is_some() {}
            }
        }
        let mut status = self.status.lock();
        status.queued_tasks = 0;
        for w in &mut status.workers {
            w.state = "stopped".into();
            w.pid = None;
        }
    }
    pub async fn recognize(
        &mut self,
        frame: Arc<FrameInput>,
        mode: OcrMode,
        cache: &Path,
        cancel: Cancellation,
    ) -> Result<OcrResult> {
        let started = Instant::now();
        self.batch += 1;
        let mut metrics = ExecutionMetrics {
            batch_id: self.batch,
            ..Default::default()
        };
        let directory = TaskDirectory::new(cache)?;
        {
            let mut status = self.status.lock();
            status.frame_id = Some(frame.id);
            status.last = None;
        }
        let _timer = ExecutionTimer {
            status: self.status.clone(),
            started,
        };
        let mut result = match &mut self.strategy {
            Strategy::Sequential(slot) => {
                let native = self.runtime == "onnx";
                let path = directory.0.join(if native {
                    "inference.ppm"
                } else {
                    "inference.png"
                });
                let start = Instant::now();
                let s = slot.as_mut().context("模型工作程序不可用")?;
                let context = [frame.generation, cancel.epoch, self.cache_revision];
                let shared = if native && self.shared_memory {
                    match s.worker.shared_frame(&frame.image, frame.id, context) {
                        Ok(frame) => Some(frame),
                        Err(e) => {
                            metrics.transport_fallback = Some(e.to_string());
                            None
                        }
                    }
                } else {
                    None
                };
                let request = if let Some(descriptor) = shared {
                    metrics.transport = "shared-memory".into();
                    WorkerRequest::RecognizeShared {
                        frame: descriptor,
                        timestamp: frame.timestamp,
                        mode,
                    }
                } else {
                    metrics.transport = if native { "ppm" } else { "png" }.into();
                    let write_path = path.clone();
                    let input = frame.clone();
                    tokio::task::spawn_blocking(move || -> Result<()> {
                        if native {
                            write_tile(
                                &write_path,
                                &input.image,
                                &Tile::whole(input.image.width(), input.image.height(), 960),
                            )
                        } else {
                            input.image.save(write_path).map_err(Into::into)
                        }
                    })
                    .await??;
                    WorkerRequest::Recognize {
                        image: path.to_string_lossy().into(),
                        frame_id: frame.id,
                        timestamp: frame.timestamp,
                        mode,
                        cache_context: Some(context),
                    }
                };
                metrics.transfer_ms = elapsed(start);
                ensure!(!cancel.cancelled(), "任务已取消");
                self.status.lock().workers[s.id].state = "busy".into();
                let start = Instant::now();
                let response = s.worker.request(request).await;
                metrics.worker_wall_ms = elapsed(start);
                metrics.tile_count = 1;
                let success = matches!(&response, Ok(WorkerResponse::Result { .. }));
                record(&self.status, s.id, metrics.worker_wall_ms, success);
                if s.device == Device::Cpu {
                    metrics.cpu_tasks = 1;
                } else {
                    metrics.gpu_tasks = 1;
                }
                if !success {
                    metrics.failed_tasks = 1;
                }
                self.status.lock().last = Some(metrics.clone());
                match response {
                    Ok(WorkerResponse::Result { result }) => {
                        metrics.ipc_overhead_ms =
                            Some((metrics.worker_wall_ms - result.metrics.total_ms).max(0.));
                        metrics.pipeline = result.metrics.pipeline.clone();
                        result
                    }
                    other => {
                        *slot = None;
                        return Err(other
                            .err()
                            .unwrap_or_else(|| anyhow::anyhow!("意外的 Worker 响应")));
                    }
                }
            }
            Strategy::Parallel(pool) => {
                metrics.transport = "ppm-tiles".into();
                let start = Instant::now();
                let input = frame.clone();
                let count = pool.slots.iter().flatten().count();
                let resize = self.resize;
                let tiles = tokio::task::spawn_blocking(move || {
                    FrameSplitter::plan(&input.image, count, resize)
                })
                .await?;
                metrics.split_ms = elapsed(start);
                metrics.tile_count = tiles.len();
                let results = pool
                    .execute(
                        tiles,
                        &directory,
                        &frame,
                        mode,
                        &cancel,
                        &self.status,
                        &mut metrics,
                    )
                    .await?;
                let start = Instant::now();
                let source = frame.image.dimensions();
                let mut blocks = Vec::new();
                let mut uncertain = false;
                let mut aggregate = Metrics::default();
                for (tile, result) in &results {
                    ensure!(
                        result.model_id.as_deref() == Some(&self.model_id)
                            && result.runtime.as_deref() == Some("onnx"),
                        "错误的模型响应"
                    );
                    if let Some(d) = &result.metrics.pipeline {
                        aggregate
                            .pipeline
                            .get_or_insert_with(Default::default)
                            .accumulate(d);
                    }
                    aggregate.preprocessing_ms += result.metrics.preprocessing_ms;
                    aggregate.inference_ms += result.metrics.inference_ms;
                    aggregate.parsing_ms += result.metrics.parsing_ms;
                    *aggregate.detection_ms.get_or_insert(0.) +=
                        result.metrics.detection_ms.unwrap_or(0.);
                    *aggregate.recognition_ms.get_or_insert(0.) +=
                        result.metrics.recognition_ms.unwrap_or(0.);
                    *aggregate.detection_regions.get_or_insert(0) +=
                        result.metrics.detection_regions.unwrap_or(0);
                    *aggregate.recognition_batches.get_or_insert(0) +=
                        result.metrics.recognition_batches.unwrap_or(0);
                    *aggregate.recognition_pixels.get_or_insert(0) +=
                        result.metrics.recognition_pixels.unwrap_or(0);
                    for block in &result.blocks {
                        uncertain |= touches_internal_edge(block, tile, source);
                        blocks.push(map_block(block.clone(), tile, source)?);
                    }
                }
                // A low-recall or border-truncated tiled result is never published.
                uncertain |= metrics.tile_count > 1 && blocks.len() < 2;
                let blocks = ResultMerger::merge(blocks);
                metrics.merge_ms += elapsed(start);
                if metrics.tile_count > 1 && uncertain {
                    metrics.whole_frame_fallback = true;
                    let whole = Tile {
                        id: 4,
                        ..Tile::whole(source.0, source.1, self.resize)
                    };
                    let mut full = pool
                        .execute(
                            vec![whole],
                            &directory,
                            &frame,
                            mode,
                            &cancel,
                            &self.status,
                            &mut metrics,
                        )
                        .await?;
                    let mut full = full.pop().context("完整性重试没有结果")?.1;
                    // Retrying the whole image must retain the cost of discarded tiles.
                    full.metrics.preprocessing_ms += aggregate.preprocessing_ms;
                    full.metrics.inference_ms += aggregate.inference_ms;
                    full.metrics.parsing_ms += aggregate.parsing_ms;
                    *full.metrics.detection_ms.get_or_insert(0.) +=
                        aggregate.detection_ms.unwrap_or(0.);
                    *full.metrics.recognition_ms.get_or_insert(0.) +=
                        aggregate.recognition_ms.unwrap_or(0.);
                    *full.metrics.recognition_batches.get_or_insert(0) +=
                        aggregate.recognition_batches.unwrap_or(0);
                    *full.metrics.recognition_pixels.get_or_insert(0) +=
                        aggregate.recognition_pixels.unwrap_or(0);
                    if let Some(p) = aggregate.pipeline {
                        full.metrics
                            .pipeline
                            .get_or_insert_with(Default::default)
                            .accumulate(&p);
                    }
                    full
                } else {
                    let cpu = results.iter().any(|(_, r)| r.device == "CPU");
                    let gpu = results.iter().any(|(_, r)| r.device == "CUDA");
                    let actual = if cpu && gpu {
                        "CPU + CUDA"
                    } else if gpu {
                        "CUDA"
                    } else {
                        "CPU"
                    };
                    OcrResult {
                        frame_id: frame.id,
                        timestamp: frame.timestamp,
                        source_width: source.0,
                        source_height: source.1,
                        blocks,
                        raw_output: String::new(),
                        metrics: aggregate,
                        device: actual.into(),
                        truncated: false,
                        runtime: Some(self.runtime.clone()),
                        model_id: Some(self.model_id.clone()),
                        engine_id: Some("ppocrv6".into()),
                    }
                }
            }
        };
        if cancel.cancelled() {
            self.cache_revision = self.cache_revision.wrapping_add(1);
            bail!("任务已取消");
        }
        ensure!(
            result.model_id.as_deref() == Some(&self.model_id)
                && result.runtime.as_deref() == Some(&self.runtime),
            "错误的模型响应"
        );
        ensure!(
            result.frame_id == frame.id
                && result.timestamp == frame.timestamp
                && result.source_width == frame.image.width()
                && result.source_height == frame.image.height(),
            "Worker 返回了过期帧或错误坐标尺寸"
        );
        metrics.end_to_end_ms = elapsed(started);
        metrics.cache = result.metrics.cache.clone();
        metrics.temporal = result.metrics.temporal.clone();
        metrics.pipeline = result.metrics.pipeline.clone();
        metrics.detection_regions = result.metrics.detection_regions;
        if self.status.lock().parallel {
            result.metrics.total_ms = metrics.end_to_end_ms;
        }
        result.metrics.execution = Some(metrics.clone());
        self.status.lock().last = Some(metrics);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_models_keep_their_memory_guard() {
        let mut models: Vec<gameocr_core::manifest::ModelManifest> =
            serde_json::from_str(include_str!("../catalog/ppocrv6.json")).unwrap();
        for (model, expected) in models.iter_mut().zip([512, 1024]) {
            assert_eq!(vram_reserve(model), expected);
            model.id = "import-opaque-id".into();
            assert_eq!(vram_reserve(model), expected);
        }
    }
}

fn record(status: &SharedStatus, id: usize, ms: f64, success: bool) {
    let mut status = status.lock();
    let w = &mut status.workers[id];
    if success {
        w.completed_tasks += 1;
        w.average_ms += (ms - w.average_ms) / w.completed_tasks as f64;
        w.state = "idle".into();
    } else {
        w.failed_tasks += 1;
        w.state = "failed".into();
        w.pid = None;
    }
}
fn write_tile(path: &Path, image: &image::RgbImage, tile: &Tile) -> Result<()> {
    let mut file = BufWriter::with_capacity(256 * 1024, std::fs::File::create(path)?);
    write!(file, "P6\n{} {}\n255\n", tile.width, tile.height)?;
    // Write source rows directly: no RGB crop allocation or PNG compression.
    for y in tile.y..tile.y + tile.height {
        let start = ((y as usize * image.width() as usize) + tile.x as usize) * 3;
        file.write_all(&image.as_raw()[start..start + tile.width as usize * 3])?;
    }
    file.flush()?;
    Ok(())
}
async fn run_task(
    mut slot: Slot,
    task: Task,
    frame_id: u64,
    timestamp: u64,
    mode: OcrMode,
) -> (Slot, Task, Result<OcrResult>, f64) {
    let start = Instant::now();
    let result = async {
        match slot
            .worker
            .request(WorkerRequest::RecognizeTile {
                image: task.path.to_string_lossy().into(),
                frame_id,
                timestamp,
                mode,
                task: task.identity.clone(),
                detection_resize: task.tile.detection_resize,
            })
            .await?
        {
            WorkerResponse::TileResult {
                task: identity,
                result,
            } => {
                ensure!(
                    identity == task.identity
                        && result.frame_id == frame_id
                        && result.timestamp == timestamp,
                    "过期或不匹配的切片响应"
                );
                ensure!(
                    result.source_width == task.tile.width
                        && result.source_height == task.tile.height,
                    "切片尺寸与响应不匹配"
                );
                Ok(result)
            }
            _ => bail!("Worker 不支持切片协议，请重新构建 ONNX Worker"),
        }
    }
    .await;
    let ms = elapsed(start);
    (slot, task, result, ms)
}
impl WorkerPool {
    async fn execute(
        &mut self,
        tiles: Vec<Tile>,
        directory: &TaskDirectory,
        frame: &Arc<FrameInput>,
        mode: OcrMode,
        cancel: &Cancellation,
        status: &SharedStatus,
        metrics: &mut ExecutionMetrics,
    ) -> Result<Vec<(Tile, OcrResult)>> {
        let expected = tiles.len();
        ensure!(tiles.len() <= 4, "切片队列超过上限");
        ensure!(!cancel.cancelled(), "任务已取消");
        let start = Instant::now();
        let input = frame.clone();
        let root = directory.0.clone();
        let copy = tiles.clone();
        let paths = tokio::task::spawn_blocking(move || -> Result<Vec<PathBuf>> {
            copy.iter()
                .map(|tile| {
                    let path = root.join(format!("tile-{}.ppm", tile.id));
                    write_tile(&path, &input.image, tile)?;
                    Ok(path)
                })
                .collect()
        })
        .await??;
        metrics.transfer_ms += elapsed(start);
        let mut pending: VecDeque<_> = tiles
            .into_iter()
            .zip(paths)
            .map(|(tile, path)| Task {
                identity: TaskIdentity {
                    batch_id: metrics.batch_id,
                    task_id: tile.id,
                    epoch: cancel.epoch,
                    generation: frame.generation,
                },
                tile,
                path,
                tried: Vec::new(),
                queued: Instant::now(),
            })
            .collect();
        let mut running = FuturesUnordered::new();
        let mut results = Vec::new();
        let mut errors = Vec::new();
        let start = Instant::now();
        let mut due = vec![None; self.slots.len()];
        let mut rates: Vec<_> = self
            .slots
            .iter()
            .map(|s| s.as_ref().map_or(0., |s| s.estimate))
            .collect();
        while !pending.is_empty() || !running.is_empty() {
            if cancel.cancelled() {
                pending.clear();
            }
            let mut idle: Vec<_> = self
                .slots
                .iter()
                .flatten()
                .map(|s| (s.id, s.estimate))
                .collect();
            idle.sort_by(|a, b| a.1.total_cmp(&b.1));
            for (id, estimate) in idle {
                let Some(index) = pending.iter().position(|t| !t.tried.contains(&id)) else {
                    continue;
                };
                let area = pending[index].tile.area() / 1_000_000.;
                // Unknown workers get a real calibration task. Once measured, a
                // slow device waits if faster workers can finish the bounded queue sooner.
                let fastest = rates
                    .iter()
                    .enumerate()
                    .filter(|(i, r)| **r > 0. && !pending[index].tried.contains(i))
                    .map(|(_, r)| *r)
                    .fold(estimate, f64::min);
                let active_soon = due
                    .iter()
                    .flatten()
                    .map(|d: &Instant| {
                        d.saturating_duration_since(Instant::now()).as_secs_f64() * 1000.
                    })
                    .min_by(f64::total_cmp)
                    .unwrap_or(0.);
                if estimate > 0.
                    && fastest > 0.
                    && estimate * area > active_soon + fastest * area * pending.len() as f64
                    && !running.is_empty()
                {
                    continue;
                }
                let mut task = pending.remove(index).unwrap();
                task.tried.push(id);
                metrics.queue_wait_ms = metrics.queue_wait_ms.max(elapsed(task.queued));
                let slot = self.slots[id].take().unwrap();
                status.lock().workers[id].state = "busy".into();
                due[id] = Some(
                    Instant::now() + Duration::from_secs_f64((estimate * area / 1000.).max(0.001)),
                );
                running.push(run_task(slot, task, frame.id, frame.timestamp, mode));
            }
            status.lock().queued_tasks = pending.len();
            let Some((mut slot, mut task, result, ms)) = running.next().await else {
                break;
            };
            let id = slot.id;
            due[id] = None;
            if slot.device == Device::Cpu {
                metrics.cpu_tasks += 1;
            } else {
                metrics.gpu_tasks += 1;
            }
            record(status, id, ms, result.is_ok());
            match result {
                Ok(result) => {
                    let observed = ms / (task.tile.area() / 1_000_000.).max(0.001);
                    slot.estimate = if slot.estimate == 0. {
                        observed
                    } else {
                        0.7 * slot.estimate + 0.3 * observed
                    };
                    rates[id] = slot.estimate;
                    results.push((task.tile, result));
                    self.slots[id] = Some(slot);
                }
                Err(error) => {
                    rates[id] = 0.;
                    metrics.failed_tasks += 1;
                    errors.push(format!("Worker {id}: {error:#}"));
                    slot.worker.shutdown().await;
                    if task.tried.len() < 2 && !cancel.cancelled() {
                        metrics.retried_tasks += 1;
                        task.queued = Instant::now();
                        pending.push_back(task);
                    }
                }
            }
        }
        metrics.worker_wall_ms += elapsed(start);
        {
            let mut status = status.lock();
            status.queued_tasks = 0;
            status.last = Some(metrics.clone());
        }
        ensure!(!cancel.cancelled(), "任务已取消");
        // An exhausted retry may leave no pending task; the exact task count is
        // checked as well, so partial batches can never escape this executor.
        ensure!(
            pending.is_empty() && results.len() == expected,
            "切片不完整：{}",
            errors.join("; ")
        );
        Ok(results)
    }
}
