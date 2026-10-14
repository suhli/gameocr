pub mod pipeline;
use anyhow::{Context, Result, ensure};
use gameocr_core::{
    engine::{EngineConfig, OcrCapabilities, OcrEngine, OnnxConfig, RuntimeConfig},
    types::*,
};
use image::RgbImage;
use ort::{
    session::Session,
    value::{Tensor, TensorElementType, ValueType},
};
use std::{path::Path, time::Instant};

pub struct PaddleOcrAdapter {
    detector: Session,
    recognizer: Session,
    dictionary: Vec<String>,
    config: OnnxConfig,
    model_id: String,
    device: String,
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
    ) -> Result<OcrResult> {
        let source = image.dimensions();
        ensure!(source.0 > 0 && source.1 > 0, "Empty image");
        let ratio = (self.config.detection_resize as f64 / source.0.max(source.1) as f64).min(1.);
        let w = ((source.0 as f64 * ratio / 32.).round() as u32 * 32).max(32);
        let h = ((source.1 as f64 * ratio / 32.).round() as u32 * 32).max(32);
        let resized = image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle);
        let plane = (w * h) as usize;
        let mut input = vec![0.; plane * 3];
        for (i, pixel) in resized.pixels().enumerate() {
            for c in 0..3 {
                input[c * plane + i] = (pixel[2 - c] as f32 / 255. - [0.485, 0.456, 0.406][c])
                    / [0.229, 0.224, 0.225][c];
            }
        }
        let mut preprocessing_ms = ms(total);
        let det_start = Instant::now();
        let outputs = self.detector.run(ort::inputs![Tensor::from_array((
            [1usize, 3, h as usize, w as usize],
            input
        ))?])?;
        let detection_ms = ms(det_start);
        let post = Instant::now();
        let (shape, prob) = outputs[0].try_extract_tensor::<f32>()?;
        ensure!(
            shape.len() == 4 && shape[0] == 1 && shape[1] == 1 && shape[2] > 0 && shape[3] > 0,
            "Invalid DB detector output"
        );
        let boxes =
            pipeline::detect_boxes(prob, shape[3] as u32, shape[2] as u32, source, &self.config)?;
        drop(outputs);
        let mut parsing_ms = ms(post);
        let mut recognition_ms = 0.;
        let mut blocks = Vec::new();
        // Sort crop indices by aspect ratio for bounded, efficient batches. Keep
        // original reading order in the final result.
        let mut order: Vec<_> = (0..boxes.len()).collect();
        order.sort_by(|&a, &b| {
            let ratio = |i: usize| {
                let w = pipeline::distance(boxes[i][0], boxes[i][1]);
                let h = pipeline::distance(boxes[i][0], boxes[i][3]);
                w.max(h) / w.min(h).max(1.)
            };
            ratio(a).total_cmp(&ratio(b))
        });
        for batch in order.chunks(self.config.batch_size) {
            let prep = Instant::now();
            let crops: Vec<_> = batch
                .iter()
                .map(|&i| pipeline::crop(&image, &boxes[i]))
                .collect::<Result<_>>()?;
            let width = crops
                .iter()
                .map(|im| (48. * im.width() as f64 / im.height() as f64).ceil() as usize)
                .max()
                .unwrap_or(320)
                .clamp(320, 3200);
            let plane = 48 * width;
            let mut tensor = vec![0f32; batch.len() * 3 * plane];
            for (n, crop) in crops.iter().enumerate() {
                let rw = ((48. * crop.width() as f64 / crop.height() as f64).ceil() as usize)
                    .min(width)
                    .max(1);
                let resized = image::imageops::resize(
                    crop,
                    rw as u32,
                    48,
                    image::imageops::FilterType::Triangle,
                );
                for y in 0..48 {
                    for x in 0..rw {
                        for c in 0..3 {
                            tensor[n * 3 * plane + c * plane + y * width + x] =
                                resized.get_pixel(x as u32, y as u32)[2 - c] as f32 / 127.5 - 1.;
                        }
                    }
                }
            }
            preprocessing_ms += ms(prep);
            let rec_start = Instant::now();
            let output = self.recognizer.run(ort::inputs![Tensor::from_array((
                [batch.len(), 3, 48, width],
                tensor
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
                if !text.is_empty() && confidence >= self.config.recognition_threshold {
                    blocks.push((
                        index,
                        OcrBlock {
                            id: format!("block-{index}"),
                            text,
                            label: Some("text".into()),
                            bbox: Some(pipeline::bbox(&boxes[index])),
                            polygon: Some(boxes[index].to_vec()),
                            confidence: Some(confidence),
                        },
                    ));
                }
            }
            parsing_ms += ms(post);
        }
        blocks.sort_by_key(|b| b.0);
        Ok(OcrResult {
            frame_id,
            timestamp,
            source_width: source.0,
            source_height: source.1,
            blocks: blocks.into_iter().map(|b| b.1).collect(),
            raw_output: String::new(),
            metrics: Metrics {
                preprocessing_ms,
                inference_ms: detection_ms + recognition_ms,
                parsing_ms,
                total_ms: ms(total),
                detection_ms: Some(detection_ms),
                recognition_ms: Some(recognition_ms),
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
        self.run(reader.decode()?.to_rgb8(), frame_id, timestamp, total)
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
