use anyhow::{Context, Result, bail};
use gameocr_core::types::Device;
use gameocr_translation::{
    protocol::{WorkerRequest, WorkerResponse},
    types::*,
};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
// No runtime preparation here: Auto only uses an existing GPU installation and
// makes at most one CPU fallback attempt. A recovered CPU session stays on CPU.
pub(super) async fn load_with_fallback<T, F, Fut>(
    preferred: Device,
    config: &mut TranslationLoad,
    cancel: Arc<AtomicBool>,
    mut load: F,
) -> Result<(T, Option<String>)>
where
    F: FnMut(TranslationLoad, Arc<AtomicBool>) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    match load(config.clone(), cancel.clone()).await {
        Ok(worker) => Ok((worker, None)),
        Err(error)
            if preferred == Device::Auto
                && config.device == Device::Cuda
                && !cancel.load(std::sync::atomic::Ordering::Acquire) =>
        {
            config.device = Device::Cpu;
            let worker = load(config.clone(), cancel)
                .await
                .context("Auto 的 CPU 回退也失败")?;
            Ok((worker, Some(format!("CUDA 未就绪，已回退 CPU：{error:#}"))))
        }
        Err(error) => Err(error),
    }
}
pub(super) fn recovery_device(preferred: Device, actual: Device) -> Device {
    if preferred == Device::Auto && actual == Device::Cuda {
        Device::Cpu
    } else {
        actual
    }
}
pub struct TranslationWorker {
    inner: crate::ocr::Worker,
    pub device: String,
    pub load_ms: f64,
    supports_cancellation: bool,
}
impl TranslationWorker {
    pub async fn load(
        root: &Path,
        config: TranslationLoad,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let mut inner = if config.device == Device::Cuda {
            let root = root.to_owned();
            let launch = tokio::task::spawn_blocking(move || {
                crate::cuda_runtime::launch(&root, "translation")
            })
            .await??;
            crate::ocr::Worker::spawn_configured(&launch.executable, launch.libraries, None, cancel)
                .await?
        } else {
            crate::ocr::Worker::spawn(
                &root.join("workers/translation/cpu/translation-worker.exe"),
                cancel,
            )
            .await?
        };
        let response: WorkerResponse = inner
            .request_typed(WorkerRequest::Load {
                protocol_version: PROTOCOL_VERSION,
                config,
            })
            .await?;
        match response {
            WorkerResponse::Ready {
                device,
                load_ms,
                supports_cancellation,
            } => Ok(Self {
                inner,
                device,
                load_ms,
                supports_cancellation,
            }),
            _ => bail!("翻译 Worker 未就绪"),
        }
    }
    pub async fn translate(
        &mut self,
        request: TranslationRequest,
        cancel: Arc<AtomicBool>,
    ) -> Result<GenerationResult> {
        let id = request.request_id.clone();
        let cancellation = if self.supports_cancellation {
            Some((
                cancel,
                serde_json::to_value(WorkerRequest::Cancel {
                    request_id: id.clone(),
                })?,
            ))
        } else {
            None
        };
        let response: WorkerResponse = self
            .inner
            .request_cancellable(WorkerRequest::Translate { request }, cancellation)
            .await?;
        match response {
            WorkerResponse::Result { result } if result.request_id == id => Ok(result),
            _ => bail!("翻译 Worker 请求身份不匹配"),
        }
    }
    pub async fn shutdown(&mut self) {
        self.inner.shutdown().await;
    }
    pub fn memory_mb(&self) -> Option<f64> {
        use windows::Win32::System::{
            ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        };
        unsafe {
            let h = OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                false,
                self.inner.pid()?,
            )
            .ok()?;
            let mut counters = PROCESS_MEMORY_COUNTERS::default();
            counters.cb = std::mem::size_of_val(&counters) as u32;
            let ok = GetProcessMemoryInfo(h, &mut counters, counters.cb).is_ok();
            let _ = windows::Win32::Foundation::CloseHandle(h);
            ok.then_some(counters.WorkingSetSize as f64 / 1048576.)
        }
    }
}
pub async fn free_vram() -> Result<f64> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
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
    .await??;
    anyhow::ensure!(output.status.success(), "NVIDIA 驱动不可用");
    String::from_utf8(output.stdout)?
        .trim()
        .parse()
        .context("无法读取可用显存")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn recovery_is_bounded_sticky_and_respects_explicit_device() {
        for (preferred, actual, fail, expected) in [
            (Device::Auto, Device::Cuda, false, vec![Device::Cuda]),
            (
                Device::Auto,
                Device::Cuda,
                true,
                vec![Device::Cuda, Device::Cpu],
            ),
            (Device::Cpu, Device::Cpu, true, vec![Device::Cpu]),
            (Device::Cuda, Device::Cuda, true, vec![Device::Cuda]),
        ] {
            let mut config = TranslationLoad {
                manifest: gameocr_translation::catalog::hy_mt2(),
                path: "unused".into(),
                device: actual,
                threads: 1,
                generation: Default::default(),
            };
            let mut calls = Vec::new();
            let result = load_with_fallback(
                preferred,
                &mut config,
                Arc::new(AtomicBool::new(false)),
                |c, _| {
                    calls.push(c.device);
                    std::future::ready(if fail && calls.len() == 1 {
                        Err(anyhow::anyhow!("injected load failure"))
                    } else {
                        Ok(c.device)
                    })
                },
            )
            .await;
            assert_eq!(calls, expected);
            assert_eq!(result.is_ok(), !fail || expected.len() == 2);
            if let Ok((device, _)) = result {
                assert_eq!(device, config.device);
            }
            // A running GPU crash chooses CPU only for Auto. Once CPU, no GPU retry.
            let recovered = recovery_device(preferred, actual);
            assert_eq!(
                recovered,
                if preferred == Device::Auto {
                    Device::Cpu
                } else {
                    actual
                }
            );
            assert_eq!(recovery_device(preferred, recovered), recovered);
        }
    }
    #[tokio::test]
    async fn cancelled_load_and_double_failure_never_retry_indefinitely() {
        let mut config = TranslationLoad {
            manifest: gameocr_translation::catalog::hy_mt2(),
            path: "unused".into(),
            device: Device::Cuda,
            threads: 1,
            generation: Default::default(),
        };
        for cancelled in [true, false] {
            config.device = Device::Cuda;
            let mut calls = 0;
            let result: Result<((), _)> = load_with_fallback(
                Device::Auto,
                &mut config,
                Arc::new(AtomicBool::new(cancelled)),
                |_, _| {
                    calls += 1;
                    std::future::ready(Err(anyhow::anyhow!("injected failure")))
                },
            )
            .await;
            assert!(result.is_err());
            assert_eq!(calls, if cancelled { 1 } else { 2 });
        }
    }
}
