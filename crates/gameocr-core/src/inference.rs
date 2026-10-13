use crate::{grounding, types::*};
use anyhow::{Context, Result, ensure};
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaModel, params::LlamaModelParams},
    mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText, mtmd_default_marker},
    sampling::LlamaSampler,
};
use std::{num::NonZeroU32, path::Path, time::Instant};

// Order matters: vision/model are destroyed before the global backend.
pub struct OcrEngine {
    vision: MtmdContext,
    model: LlamaModel,
    backend: LlamaBackend,
    config: InferenceConfig,
    pub device: String,
}

/// Exact single-user, thinking-disabled form of the official Qwen3.5 template.
/// MTMD expands its media marker, including the architecture's vision delimiters.
pub fn prompt(mode: OcrMode) -> String {
    let instruction = if mode == OcrMode::Grounding {
        "grounding"
    } else {
        ""
    };
    format!(
        "<|im_start|>user\n{}{instruction}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
        mtmd_default_marker()
    )
}

impl OcrEngine {
    pub fn load(config: InferenceConfig) -> Result<Self> {
        config.validate()?;
        for path in [&config.model, &config.mmproj] {
            ensure!(
                Path::new(path)
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gguf")),
                "必须使用已完成下载的 .gguf 文件"
            );
            ensure!(
                std::fs::metadata(path)
                    .with_context(|| format!("无法读取 {path}"))?
                    .len()
                    > 32,
                "模型文件为空"
            );
        }
        let backend = LlamaBackend::init()?;
        let gpu = cfg!(feature = "cuda")
            && backend.supports_gpu_offload()
            && config.device != Device::Cpu;
        ensure!(
            config.device != Device::Cuda || gpu,
            "CUDA 不可用：需要 CUDA 工作程序、兼容的 NVIDIA GPU 和驱动；可选择 CPU"
        );
        let params = LlamaModelParams::default().with_n_gpu_layers(if gpu { u32::MAX } else { 0 });
        let model = LlamaModel::load_from_file(&backend, &config.model, &params)
            .context("GGUF 加载失败（架构、量化或内存不足），详见工作程序日志")?;
        let arch = model.meta_val_str("general.architecture")?;
        ensure!(
            arch == "qwen35",
            "此版本仅适配 LightOnOCR-3-0.8B (qwen35)，文件架构为 {arch}"
        );
        let vision = MtmdContext::init_from_file(
            &config.mmproj,
            &model,
            &MtmdContextParams {
                use_gpu: gpu,
                n_threads: config.threads as i32,
                ..Default::default()
            },
        )
        .context("mmproj 加载失败：请使用与 LightOnOCR-3-0.8B 匹配的视觉投影文件")?;
        ensure!(vision.support_vision(), "mmproj 不支持图像输入");
        Ok(Self {
            vision,
            model,
            backend,
            config,
            device: if gpu { "CUDA" } else { "CPU" }.into(),
        })
    }

    pub fn recognize(
        &mut self,
        path: &Path,
        frame_id: u64,
        timestamp: u64,
        mode: OcrMode,
    ) -> Result<OcrResult> {
        let total = Instant::now();
        let image = image::ImageReader::open(path)?
            .with_guessed_format()?
            .decode()
            .context("图像解码失败")?;
        let (width, height) = (image.width(), image.height());
        let image = if image.width().max(image.height()) > self.config.max_image_edge {
            image.thumbnail(self.config.max_image_edge, self.config.max_image_edge)
        } else {
            image
        }
        .to_rgb8();
        let bitmap = MtmdBitmap::from_image_data(image.width(), image.height(), image.as_raw())?;
        let chunks = self.vision.tokenize(
            MtmdInputText {
                text: prompt(mode),
                add_special: true,
                parse_special: true,
            },
            &[&bitmap],
        )?;
        ensure!(
            chunks.total_tokens() + 32 < self.config.context_size as usize,
            "图像超出上下文预算，请减小图像长边或增大上下文"
        );
        // A fresh context per frame intentionally clears recurrent state AND KV cache.
        // The expensive model and vision encoder remain resident across requests.
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.config.context_size))
            .with_n_batch(512)
            .with_n_ubatch(512)
            .with_n_threads(self.config.threads as i32)
            .with_n_threads_batch(self.config.threads as i32)
            .with_offload_kqv(self.device == "CUDA");
        let mut ctx = self.model.new_context(&self.backend, ctx_params)?;
        let preprocessing_ms = total.elapsed().as_secs_f64() * 1000.;
        let infer = Instant::now();
        let mut pos = chunks.eval_chunks(&mut self.vision, &mut ctx, 0, 0, 512, true)?;
        let mut sampler = if self.config.temperature == 0. {
            LlamaSampler::greedy()
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::top_k(20),
                LlamaSampler::top_p(0.9, 1),
                LlamaSampler::temp(self.config.temperature),
                LlamaSampler::dist(42),
            ])
        };
        let mut bytes = Vec::new();
        let mut batch = LlamaBatch::new(1, 1);
        let mut truncated = true;
        for _ in 0..self.config.max_tokens {
            let token = sampler.sample(&ctx, -1);
            if self.model.vocab().is_eog(token) {
                truncated = false;
                break;
            }
            sampler.accept(token);
            bytes.extend(self.model.vocab().token_to_piece(token, false, None));
            if pos as u32 >= self.config.context_size - 1 {
                break;
            }
            batch.clear();
            batch.add(token, pos, &[0], true)?;
            ctx.decode(&mut batch)?;
            pos += 1;
        }
        let inference_ms = infer.elapsed().as_secs_f64() * 1000.;
        let parsing = Instant::now();
        let raw_output = String::from_utf8_lossy(&bytes).into_owned();
        let blocks = grounding::parse(&raw_output, width, height);
        let parsing_ms = parsing.elapsed().as_secs_f64() * 1000.;
        Ok(OcrResult {
            frame_id,
            timestamp,
            source_width: width,
            source_height: height,
            blocks,
            raw_output,
            metrics: Metrics {
                preprocessing_ms,
                inference_ms,
                parsing_ms,
                total_ms: total.elapsed().as_secs_f64() * 1000.,
            },
            device: self.device.clone(),
            truncated,
        })
    }
}
