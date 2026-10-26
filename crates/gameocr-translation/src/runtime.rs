//! Model and context live together on a dedicated thread: no self-referential
//! struct, leaked model, or shared OCR context is needed to reuse native state.
use crate::{engine::TranslationEngine, prompt, types::*};
use anyhow::{Context, Result, ensure};
use gameocr_core::types::Device;
use llama_cpp_2::{
    context::{LlamaContext, params::LlamaContextParams},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{LlamaChatMessage, LlamaModel, params::LlamaModelParams},
    sampling::LlamaSampler,
};
use sha2::{Digest, Sha256};
use std::{io::Read, num::NonZeroU32, sync::mpsc, thread, time::Instant};

pub fn verify(path: &str, manifest: &TranslationManifest) -> Result<()> {
    let mut file = std::fs::File::open(path).context("打开翻译模型")?;
    ensure!(file.metadata()?.len() == manifest.size, "翻译模型大小不符");
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    ensure!(
        format!("{:x}", hash.finalize()).eq_ignore_ascii_case(&manifest.sha256),
        "翻译模型 SHA-256 校验失败"
    );
    Ok(())
}
type Job = (
    TranslationRequest,
    mpsc::SyncSender<Result<GenerationResult>>,
);
pub struct HyMtAdapter {
    jobs: Option<mpsc::SyncSender<Job>>,
    join: Option<thread::JoinHandle<()>>,
    device: String,
}
impl HyMtAdapter {
    pub fn load(config: TranslationLoad) -> Result<Self> {
        verify(&config.path, &config.manifest)?;
        let (jobs, rx) = mpsc::sync_channel::<Job>(1);
        let (ready, rr) = mpsc::sync_channel(1);
        let device = match config.device {
            Device::Cuda => "cuda",
            _ => "cpu",
        }
        .to_string();
        let actual = device.clone();
        let join = thread::Builder::new()
            .name("hy-mt2-native".into())
            .spawn(move || {
                let run = || -> Result<()> {
                    let started = Instant::now();
                    let backend = LlamaBackend::init()?;
                    ensure!(
                        config.device != Device::Cuda
                            || (cfg!(feature = "cuda") && backend.supports_gpu_offload()),
                        "翻译 CUDA 后端不可用"
                    );
                    let params = LlamaModelParams::default().with_n_gpu_layers(
                        if config.device == Device::Cuda {
                            u32::MAX
                        } else {
                            0
                        },
                    );
                    let model = LlamaModel::load_from_file(&backend, &config.path, &params)?;
                    ensure!(
                        model.meta_val_str("general.architecture")? == "hunyuan-dense",
                        "Hy-MT2 Adapter 只接受 hunyuan-dense 架构"
                    );
                    let template = model
                        .chat_template(None)
                        .context("模型缺少 Chat Template")?;
                    // Fail loading early if the embedded template is unsupported.
                    model.apply_chat_template(
                        &template,
                        &[LlamaChatMessage::new("user".into(), "test".into())?],
                        true,
                    )?;
                    let cp = LlamaContextParams::default()
                        .with_n_ctx(NonZeroU32::new(config.generation.context_size))
                        .with_n_batch(512)
                        .with_n_ubatch(512)
                        .with_n_threads(config.threads as i32)
                        .with_n_threads_batch(config.threads as i32);
                    let mut ctx = model.new_context(&backend, cp)?;
                    let mut warm = LlamaBatch::new(1, 1);
                    let token = model.vocab().tokenize(b"test", true, false)[0];
                    warm.add(token, 0, &[0], true)?;
                    ctx.decode(&mut warm)?;
                    ctx.clear_kv_cache();
                    let load_ms = started.elapsed().as_secs_f64() * 1000.;
                    let _ = ready.send(Ok(()));
                    while let Ok((request, reply)) = rx.recv() {
                        let result = generate(
                            &model,
                            &mut ctx,
                            &config.generation,
                            &request,
                            &actual,
                            load_ms,
                        );
                        ctx.clear_kv_cache();
                        let _ = reply.send(result);
                    }
                    Ok(())
                };
                if let Err(err) = run() {
                    let _ = ready.send(Err(err));
                }
            })?;
        match rr.recv().context("翻译模型加载线程已退出")? {
            Ok(()) => Ok(Self {
                jobs: Some(jobs),
                join: Some(join),
                device,
            }),
            Err(e) => {
                let _ = join.join();
                Err(e)
            }
        }
    }
}
impl TranslationEngine for HyMtAdapter {
    fn translate(&mut self, request: TranslationRequest) -> Result<GenerationResult> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.jobs
            .as_ref()
            .context("翻译 Worker 已停止")?
            .send((request, reply))
            .context("翻译线程已停止")?;
        rx.recv().context("翻译线程异常退出")?
    }
    fn device(&self) -> &str {
        &self.device
    }
}
impl Drop for HyMtAdapter {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(t) = self.join.take() {
            let _ = t.join();
        }
    }
}

fn generate(
    model: &LlamaModel,
    ctx: &mut LlamaContext<'_>,
    g: &GenerationConfig,
    r: &TranslationRequest,
    device: &str,
    load_ms: f64,
) -> Result<GenerationResult> {
    let total = Instant::now();
    ctx.clear_kv_cache();
    let content = prompt::build(r)?;
    let template = model.chat_template(None)?;
    let formatted = model.apply_chat_template(
        &template,
        &[LlamaChatMessage::new("user".into(), content)?],
        true,
    )?;
    let vocab = model.vocab();
    let tokens = vocab.tokenize(formatted.as_bytes(), true, true);
    ensure!(
        tokens.len() + g.max_tokens as usize <= g.context_size as usize,
        "正文与上下文超出翻译 Token 预算，请缩短文本或上下文"
    );
    let prompt_ms = total.elapsed().as_secs_f64() * 1000.;
    let inference = Instant::now();
    let mut batch = LlamaBatch::new(512, 1);
    for (offset, chunk) in tokens.chunks(512).enumerate() {
        batch.clear();
        for (i, &token) in chunk.iter().enumerate() {
            let pos = offset * 512 + i;
            batch.add(token, pos as i32, &[0], pos == tokens.len() - 1)?;
        }
        ctx.decode(&mut batch)?;
    }
    let mut chain = vec![LlamaSampler::penalties(
        vocab.n_tokens(),
        64,
        g.repetition_penalty,
        0.,
        0.,
    )];
    if g.temperature == 0. {
        chain.push(LlamaSampler::greedy());
    } else {
        chain.extend([
            LlamaSampler::top_k(g.top_k),
            LlamaSampler::top_p(g.top_p, 1),
            LlamaSampler::temp(g.temperature),
            LlamaSampler::dist(g.seed),
        ]);
    }
    let mut sampler = LlamaSampler::chain_simple(chain);
    let mut bytes = Vec::new();
    let mut count = 0;
    let mut ttft = None;
    let mut stop = "maxTokens";
    for i in 0..g.max_tokens as usize {
        let token = sampler.sample(ctx, -1);
        sampler.accept(token);
        if vocab.is_eog(token) {
            stop = "eos";
            break;
        }
        if ttft.is_none() {
            ttft = Some(inference.elapsed().as_secs_f64() * 1000.);
        }
        count += 1;
        bytes.extend(vocab.token_to_piece(token, false, None));
        if i + 1 < g.max_tokens as usize {
            batch.clear();
            batch.add(token, (tokens.len() + i) as i32, &[0], true)?;
            ctx.decode(&mut batch)?;
        }
    }
    let inference_ms = inference.elapsed().as_secs_f64() * 1000.;
    let text =
        crate::text::decode_output(bytes, stop != "eos").context("翻译结果包含无效 UTF-8")?;
    Ok(GenerationResult {
        request_id: r.request_id.clone(),
        text: text.trim().into(),
        device: device.into(),
        truncated: stop != "eos",
        stop_reason: stop.into(),
        metrics: TranslationMetrics {
            model_load_ms: load_ms,
            prompt_ms,
            ttft_ms: ttft,
            prompt_tokens: tokens.len(),
            generated_tokens: count,
            tokens_per_second: ttft
                .filter(|t| inference_ms > *t && count > 1)
                .map(|t| (count - 1) as f64 * 1000. / (inference_ms - t)),
            inference_ms,
            total_ms: total.elapsed().as_secs_f64() * 1000.,
            ..Default::default()
        },
    })
}
