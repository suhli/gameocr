mod batching;
mod cache;
pub mod pipeline;
mod refinement;
pub mod regions;
use anyhow::{Context, Result, ensure};
use gameocr_core::{
    engine::{EngineConfig, OcrCapabilities, OcrEngine, OnnxConfig, RuntimeConfig},
    types::*,
};
use image::RgbImage;
use ort::{
    session::Session,
    value::{Tensor, TensorElementType, TensorRef, ValueType},
};
use std::{path::Path, time::Instant};

pub struct PaddleOcrAdapter {
    detector: Session,
    recognizer: Session,
    dictionary: Vec<String>,
    config: OnnxConfig,
    model_id: String,
    device: String,
    cache: Option<cache::FrameCache>,
    cache_context: Option<[u64; 3]>,
    detection_buffer: Vec<f32>,
    recognition_buffer: Vec<f32>,
}
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.
}
pub fn initialize_runtime() -> Result<()> {
    let path = std::env::var_os("ORT_DYLIB_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_exe()?.with_file_name("onnxruntime.dll"));
    ensure!(
        path.is_file(),
        "ONNX Runtime DLL missing: {}",
        path.display()
    );
    ort::init_from(&path)?.with_name("GameOCR").commit();
    Ok(())
}
fn session(path: &Path, device: Device, threads: u32) -> Result<Session> {
    let builder = Session::builder()?
        .with_intra_threads(threads as usize)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    let mut builder = if device == Device::Cuda {
        #[cfg(feature = "cuda")]
        {
            use ort::ep::ExecutionProvider;
            // cuDNN heuristic B rejects some dynamic PP-OCR widths on Turing.
            // Its fallback algorithm avoids exhaustive tuning spikes per frame.
            let cuda = ort::ep::CUDA::default()
                .with_conv_algorithm_search(ort::ep::cuda::ConvAlgorithmSearch::Default)
                .with_conv_max_workspace(false);
            ensure!(
                cuda.is_available()?,
                "CUDA Execution Provider not available"
            );
            builder
                .with_execution_providers([cuda.build().error_on_failure()])
                .map_err(|e| anyhow::anyhow!(e.to_string()))?
        }
        #[cfg(not(feature = "cuda"))]
        {
            anyhow::bail!("This worker is CPU-only");
        }
    } else {
        builder
            .with_execution_providers([ort::ep::CPU::default().build().error_on_failure()])
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
    };
    if device == Device::Cuda {
        let profile = std::env::temp_dir().join(format!(
            "gameocr-ort-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        builder = builder
            .with_profiling(profile)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    let session = builder
        .commit_from_file(path)
        .with_context(|| format!("ONNX session: {}", path.display()))?;
    ensure!(
        session.inputs().len() == 1 && session.outputs().len() == 1,
        "PP-OCR adapter expects one input and one output"
    );
    match session.inputs()[0].dtype() {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } => ensure!(
            shape.len() == 4 && shape[1] == 3,
            "Expected NCHW RGB/BGR input with three channels"
        ),
        _ => anyhow::bail!("Expected float32 tensor input"),
    }
    Ok(session)
}
fn verify_cuda_execution(session: &mut Session) -> Result<()> {
    let path = session.end_profiling()?;
    let bytes = std::fs::read(&path)?;
    let _ = std::fs::remove_file(&path);
    let events: serde_json::Value = serde_json::from_slice(&bytes)?;
    ensure!(
        events.as_array().is_some_and(|events| events
            .iter()
            .any(|event| event["args"]["provider"] == "CUDAExecutionProvider")),
        "CUDA session did not execute any CUDA kernels"
    );
    Ok(())
}
impl PaddleOcrAdapter {
    pub fn create(config: EngineConfig) -> Result<Box<dyn OcrEngine>> {
        gameocr_core::engine::RuntimeRegistry::default().validate(&config)?;
        ensure!(
            config.device != Device::Auto,
            "Worker requires an explicit CPU/CUDA device; Auto fallback is managed by the desktop WorkerManager"
        );
        initialize_runtime()?;
        let detector_path = config.file("detector")?;
        let recognizer_path = config.file("recognizer")?;
        use sha2::{Digest, Sha256};
        let dictionary_hash = format!(
            "{:x}",
            Sha256::digest(std::fs::read(config.file("dictionary")?)?)
        );
        ensure!(
            dictionary_hash == "b5f2bfe2bdd9448429e3e82b51c789775d9b42f2403d082b00662eb77e401c5d",
            "Dictionary is not the verified PP-OCRv6 character ordering"
        );
        let dictionary = pipeline::read_dictionary(&config.file("dictionary")?)?;
        ensure!(
            dictionary.len() == 18710,
            "PP-OCRv6 requires the complete 18708-character dictionary (+ blank and space)"
        );
        let RuntimeConfig::Onnx(parameters) = config.inference else {
            anyhow::bail!("Expected ONNX configuration")
        };
        let detector = session(&detector_path, config.device, config.threads)?;
        let recognizer = session(&recognizer_path, config.device, config.threads)?;
        if let ValueType::Tensor { shape, .. } = recognizer.outputs()[0].dtype() {
            ensure!(
                shape.len() == 3 && (shape[2] < 0 || shape[2] == dictionary.len() as i64),
                "Recognizer output does not match PP-OCRv6 dictionary"
            );
        }
        let mut loaded = Self {
            detector,
            recognizer,
            dictionary,
            config: parameters,
            model_id: config.manifest.id,
            device: if config.device == Device::Cuda {
                "CUDA"
            } else {
                "CPU"
            }
            .into(),
            cache: None,
            cache_context: None,
            detection_buffer: Vec::new(),
            recognition_buffer: Vec::new(),
        };
        // Run both graphs before Ready. Missing kernels/DLLs surface during load,
        // allowing the parent to discard this process and retry a CPU worker.
        let det = Tensor::from_array(([1usize, 3, 32, 32], vec![0f32; 3 * 32 * 32]))?;
        let output = loaded.detector.run(ort::inputs![det])?;
        let (shape, _) = output[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 4 && shape[0] == 1 && shape[1] == 1,
            "Expected DB probability map [1,1,H,W]"
        );
        drop(output);
        let rec = Tensor::from_array(([1usize, 3, 48, 320], vec![0f32; 3 * 48 * 320]))?;
        let output = loaded.recognizer.run(ort::inputs![rec])?;
        let (shape, _) = output[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 3 && shape[2] == loaded.dictionary.len() as i64,
            "CTC dictionary dimension mismatch"
        );
        drop(output);
        if config.device == Device::Cuda {
            verify_cuda_execution(&mut loaded.detector)?;
            verify_cuda_execution(&mut loaded.recognizer)?;
        }
        Ok(Box::new(loaded))
    }
    fn run(
        &mut self,
        image: RgbImage,
        frame_id: u64,
        timestamp: u64,
        total: Instant,
        detection_resize: u32,
    ) -> Result<OcrResult> {
        let source = image.dimensions();
        ensure!(source.0 > 0 && source.1 > 0, "Empty image");
        let mut regions = regions::plan(source, &self.config, detection_resize)?;
        let refine = self.config.selective_detection && !self.config.preserve_small_text;
        let scope = regions[0];
        let cache_enabled = self.config.incremental
            && self.cache_context.is_some()
            && image.as_raw().len() <= 64 * 1024 * 1024;
        // Taking ownership ensures failures cannot keep partially updated data.
        let previous = self
            .cache
            .take()
            .filter(|c| cache_enabled && c.valid(&image, detection_resize));
        let mut cache_metrics = CacheMetrics {
            refreshed: previous.is_none(),
            ..Default::default()
        };
        let mut next_regions = Vec::new();
        let mut preprocessing_ms = ms(total);
        let mut detection_ms = 0.;
        let mut parsing_ms = 0.;
        let mut candidates = Vec::new();
        for index in 0..65 {
            let Some(region) = regions.get(index).copied() else {
                break;
            };
            let prep = Instant::now();
            if let Some(boxes) = previous.as_ref().and_then(|c| c.region(&image, region)) {
                cache_metrics.reused_regions += 1;
                candidates.extend(boxes.iter().map(|&quad| regions::Candidate {
                    quad,
                    clipped: region.clipped(&quad, scope),
                    region: index,
                }));
                next_regions.push((region, boxes));
                if index == 0 && refine {
                    let overview = candidates.iter().map(|c| c.quad).collect::<Vec<_>>();
                    regions.extend(refinement::select(
                        &image,
                        &self.config,
                        detection_resize,
                        &overview,
                    )?);
                }
                preprocessing_ms += ms(prep);
                continue;
            }
            let ratio = (detection_resize as f64 / region.w.max(region.h) as f64).min(1.);
            let w = ((region.w as f64 * ratio / 32.).round() as u32 * 32).max(32);
            let h = ((region.h as f64 * ratio / 32.).round() as u32 * 32).max(32);
            let view = image::imageops::crop_imm(&image, region.x, region.y, region.w, region.h);
            let resized =
                image::imageops::resize(&*view, w, h, image::imageops::FilterType::Triangle);
            let plane = (w * h) as usize;
            self.detection_buffer.resize(plane * 3, 0.);
            for (i, pixel) in resized.pixels().enumerate() {
                for c in 0..3 {
                    self.detection_buffer[c * plane + i] = (pixel[2 - c] as f32 / 255.
                        - [0.485, 0.456, 0.406][c])
                        / [0.229, 0.224, 0.225][c];
                }
            }
            preprocessing_ms += ms(prep);
            let det_start = Instant::now();
            let outputs = self.detector.run(ort::inputs![TensorRef::from_array_view((
                [1usize, 3, h as usize, w as usize],
                self.detection_buffer.as_slice()
            ))?])?;
            detection_ms += ms(det_start);
            let post = Instant::now();
            let (shape, prob) = outputs[0].try_extract_tensor::<f32>()?;
            ensure!(
                shape.len() == 4 && shape[0] == 1 && shape[1] == 1 && shape[2] > 0 && shape[3] > 0,
                "Invalid DB detector output"
            );
            let detected = pipeline::detect_boxes(
                prob,
                shape[3] as u32,
                shape[2] as u32,
                (region.w, region.h),
                &self.config,
            )?
            .into_iter()
            .map(|q| region.restore(q))
            .collect::<Vec<_>>();
            for &quad in &detected {
                candidates.push(regions::Candidate {
                    quad,
                    clipped: region.clipped(&quad, scope),
                    region: index,
                });
            }
            next_regions.push((region, detected));
            if index == 0 && refine {
                let overview = candidates.iter().map(|c| c.quad).collect::<Vec<_>>();
                regions.extend(refinement::select(
                    &image,
                    &self.config,
                    detection_resize,
                    &overview,
                )?);
            }
            parsing_ms += ms(post);
            ensure!(
                candidates.len() <= 12000,
                "Detection exceeds 12000 boxes; select a smaller ROI"
            );
        }
        let post = Instant::now();
        ensure!(
            candidates.len() <= 12000,
            "Detection exceeds 12000 boxes; select a smaller ROI"
        );
        let boxes = if regions.len() > 1 {
            regions::merge(candidates)
        } else {
            candidates.into_iter().map(|c| c.quad).collect()
        };
        parsing_ms += ms(post);
        let mut recognition_ms = 0.;
        let prep = Instant::now();
        let batches = batching::plan(&boxes, &self.config);
        let mut widths = vec![0; boxes.len()];
        for batch in &batches {
            for &i in &batch.indices {
                widths[i] = batch.width;
            }
        }
        let mut decoded: Vec<Option<(String, f32)>> = boxes
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let value = previous.as_ref().and_then(|c| c.line(&image, q, widths[i]));
                if value.is_some() {
                    cache_metrics.reused_lines += 1;
                }
                value
            })
            .collect();
        preprocessing_ms += ms(prep);
        let mut recognition_batches = 0;
        let mut recognition_pixels = 0;
        for planned in batches {
            let batch: Vec<_> = planned
                .indices
                .into_iter()
                .filter(|&i| decoded[i].is_none())
                .collect();
            if batch.is_empty() {
                continue;
            }
            let width = planned.width;
            recognition_batches += 1;
            recognition_pixels += batch.len() * 48 * width;
            let prep = Instant::now();
            let plane = 48 * width;
            self.recognition_buffer.resize(batch.len() * 3 * plane, 0.);
            self.recognition_buffer.fill(0.); // Padding must not retain an earlier batch.
            for (n, &index) in batch.iter().enumerate() {
                let crop = pipeline::crop(&image, &boxes[index])?;
                let rw = ((48. * crop.width() as f64 / crop.height() as f64).ceil() as usize)
                    .min(width)
                    .max(1);
                let resized = image::imageops::resize(
                    &crop,
                    rw as u32,
                    48,
                    image::imageops::FilterType::Triangle,
                );
                let sample = &mut self.recognition_buffer[n * 3 * plane..(n + 1) * 3 * plane];
                let (blue, rest) = sample.split_at_mut(plane);
                let (green, red) = rest.split_at_mut(plane);
                for (y, row) in resized.as_raw().chunks_exact(rw * 3).enumerate() {
                    for (x, pixel) in row.chunks_exact(3).enumerate() {
                        let i = y * width + x;
                        blue[i] = pixel[2] as f32 / 127.5 - 1.;
                        green[i] = pixel[1] as f32 / 127.5 - 1.;
                        red[i] = pixel[0] as f32 / 127.5 - 1.;
                    }
                }
            }
            preprocessing_ms += ms(prep);
            let rec_start = Instant::now();
            let output = self
                .recognizer
                .run(ort::inputs![TensorRef::from_array_view((
                    [batch.len(), 3, 48, width],
                    self.recognition_buffer.as_slice()
                ))?])?;
            recognition_ms += ms(rec_start);
            let post = Instant::now();
            let (shape, prob) = output[0].try_extract_tensor::<f32>()?;
            ensure!(
                shape.len() == 3
                    && shape[0] == batch.len() as i64
                    && shape[1] > 0
                    && shape[2] == self.dictionary.len() as i64,
                "Invalid CTC output dimensions"
            );
            let steps = shape[1] as usize;
            let classes = shape[2] as usize;
            for (n, &index) in batch.iter().enumerate() {
                let (text, confidence) = pipeline::decode_ctc(
                    &prob[n * steps * classes..(n + 1) * steps * classes],
                    steps,
                    classes,
                    &self.dictionary,
                )?;
                decoded[index] = Some((text, confidence));
            }
            parsing_ms += ms(post);
        }
        let blocks = decoded
            .iter()
            .enumerate()
            .filter_map(|(index, value)| {
                let (text, confidence) = value.as_ref()?;
                (!text.is_empty() && *confidence >= self.config.recognition_threshold).then(|| {
                    OcrBlock {
                        id: format!("block-{index}"),
                        text: text.clone(),
                        label: Some("text".into()),
                        bbox: Some(pipeline::bbox(&boxes[index])),
                        polygon: Some(boxes[index].to_vec()),
                        confidence: Some(*confidence),
                    }
                })
            })
            .collect();
        if cache_enabled && boxes.len() <= 4096 {
            let lines = boxes
                .into_iter()
                .zip(decoded)
                .zip(widths)
                .filter_map(|((q, value), width)| {
                    value.map(|(text, score)| (q, width, text, score))
                })
                .collect();
            self.cache = Some(cache::FrameCache {
                image,
                regions: next_regions,
                lines,
                refreshed: previous.as_ref().map_or_else(Instant::now, |c| c.refreshed),
                frames: previous.as_ref().map_or(0, |c| c.frames + 1),
                edge: detection_resize,
            });
        }
        Ok(OcrResult {
            frame_id,
            timestamp,
            source_width: source.0,
            source_height: source.1,
            blocks,
            raw_output: String::new(),
            metrics: Metrics {
                preprocessing_ms,
                inference_ms: detection_ms + recognition_ms,
                parsing_ms,
                total_ms: ms(total),
                detection_ms: Some(detection_ms),
                recognition_ms: Some(recognition_ms),
                detection_regions: Some(regions.len()),
                cache: cache_enabled.then_some(cache_metrics),
                recognition_batches: Some(recognition_batches),
                recognition_pixels: Some(recognition_pixels),
                execution: None,
            },
            device: self.device.clone(),
            truncated: false,
            runtime: Some("onnx".into()),
            model_id: Some(self.model_id.clone()),
            engine_id: Some("ppocrv6".into()),
        })
    }
}
impl OcrEngine for PaddleOcrAdapter {
    fn set_frame_context(&mut self, context: Option<[u64; 3]>) {
        if context.is_none() || self.cache_context != context {
            self.cache = None;
        }
        self.cache_context = context;
    }
    fn recognize(
        &mut self,
        path: &Path,
        frame_id: u64,
        timestamp: u64,
        _mode: OcrMode,
    ) -> Result<OcrResult> {
        let total = Instant::now();
        let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        self.run(
            reader.decode()?.to_rgb8(),
            frame_id,
            timestamp,
            total,
            self.config.detection_resize,
        )
    }
    fn recognize_tile(
        &mut self,
        path: &Path,
        frame_id: u64,
        timestamp: u64,
        _mode: OcrMode,
        detection_resize: u32,
    ) -> Result<OcrResult> {
        ensure!(
            (32..=self.config.detection_resize).contains(&detection_resize),
            "Tile detector resolution exceeds configured budget"
        );
        let total = Instant::now();
        let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(16384);
        limits.max_image_height = Some(16384);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        self.run(
            reader.decode()?.to_rgb8(),
            frame_id,
            timestamp,
            total,
            detection_resize,
        )
    }
    fn capabilities(&self) -> OcrCapabilities {
        OcrCapabilities::paddle()
    }
    fn identity(&self) -> (&str, &str, &str) {
        ("onnx", "ppocrv6", &self.model_id)
    }
    fn device(&self) -> &str {
        &self.device
    }
    fn shutdown(self: Box<Self>) {
        drop(self);
    }
}
