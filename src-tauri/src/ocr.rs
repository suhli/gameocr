use anyhow::{Context, Result, bail};
use gameocr_core::types::*;
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::Manager;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// Each worker owns its native models and sessions on its inference thread.
/// The desktop process has no CUDA-linked imports and can always start on CPU-only PCs.
pub struct Worker {
    mapping: Option<gameocr_core::shared_frame::Mapping>,
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    log: Arc<Mutex<VecDeque<String>>>,
    log_task: tokio::task::JoinHandle<()>,
    closing: Arc<AtomicBool>,
}
impl Worker {
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
    pub async fn spawn(path: &Path, closing: Arc<AtomicBool>) -> Result<Self> {
        Self::spawn_configured(path, Vec::new(), None, closing).await
    }
    pub async fn spawn_engine(
        root: &Path,
        runtime: &str,
        device: Device,
        closing: Arc<AtomicBool>,
    ) -> Result<Self> {
        if device == Device::Cuda {
            let (root, runtime) = (root.to_owned(), runtime.to_owned());
            let launch =
                tokio::task::spawn_blocking(move || crate::cuda_runtime::launch(&root, &runtime))
                    .await??;
            Self::spawn_configured(
                &launch.executable,
                launch.libraries,
                launch.ort_dylib,
                closing,
            )
            .await
        } else {
            let executable = gameocr_core::engine::RuntimeRegistry::default()
                .executable(runtime)?
                .to_owned();
            Self::spawn(
                &root
                    .join("workers")
                    .join(runtime)
                    .join("cpu")
                    .join(executable),
                closing,
            )
            .await
        }
    }
    async fn spawn_configured(
        path: &Path,
        libraries: Vec<PathBuf>,
        ort_dylib: Option<PathBuf>,
        closing: Arc<AtomicBool>,
    ) -> Result<Self> {
        let mut command = Command::new(path);
        // Resolve the complete backend before starting the child. In particular,
        // ORT must load its GPU core/providers together, never the bundled CPU DLL.
        if !libraries.is_empty() {
            let mut paths = vec![path.parent().context("无效 Worker 路径")?.to_owned()];
            paths.extend(libraries);
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            command.env("PATH", std::env::join_paths(paths)?);
        }
        if let Some(dll) = ort_dylib {
            command.env("ORT_DYLIB_PATH", dll);
        }
        let mut child = command
            .current_dir(path.parent().context("工作程序路径错误")?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x08000000)
            .kill_on_drop(true)
            .spawn()
            .with_context(|| {
                format!(
                    "无法启动 {}；请构建/安装对应推理后端和运行库",
                    path.display()
                )
            })?;
        let input = child.stdin.take().context("工作程序 stdin 不可用")?;
        let output = BufReader::new(child.stdout.take().context("工作程序 stdout 不可用")?);
        let stderr = child.stderr.take().context("工作程序 stderr 不可用")?;
        let log = Arc::new(Mutex::new(VecDeque::new()));
        let log_copy = log.clone();
        let log_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            loop {
                // Bound allocation before reading; a worker may log a long line
                // without ever sending a newline. Retain only 128 small chunks.
                let mut bytes = Vec::new();
                match (&mut reader).take(4096).read_until(b'\n', &mut bytes).await {
                    Ok(0) | Err(_) => break,
                    _ => {}
                }
                let line = String::from_utf8_lossy(&bytes).into_owned();
                let mut log = log_copy.lock();
                if log.len() >= 128 {
                    log.pop_front();
                }
                log.push_back(line);
            }
        });
        Ok(Self {
            mapping: None,
            child,
            input,
            output,
            log,
            log_task,
            closing,
        })
    }
    pub async fn request(&mut self, request: WorkerRequest) -> Result<WorkerResponse> {
        let response=tokio::time::timeout(Duration::from_secs(180),async {
            self.input.write_all(&serde_json::to_vec(&request)?).await?;self.input.write_all(b"\n").await?;self.input.flush().await?;
            let read=read_response(&mut self.output);tokio::pin!(read);
            let line=loop{tokio::select!{line=&mut read=>break line?,_=tokio::time::sleep(Duration::from_millis(50))=>{if self.closing.load(Ordering::Acquire){bail!("应用正在退出");}}}};
            let response:WorkerResponse=serde_json::from_str(&line).context("工作程序协议错误")?;
            match response {WorkerResponse::Error {message}=>bail!("{message}"),other=>Ok(other)}
        }).await;
        match response {
            Ok(Ok(r)) => Ok(r),
            error => {
                let _ = self.child.kill().await;
                let logs = self
                    .log
                    .lock()
                    .iter()
                    .rev()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("\n");
                match error {
                    Ok(Err(e)) => bail!("{e:#}\n{logs}"),
                    _ => bail!("推理超过 180 秒，工作程序已终止；请减小图像/输出预算。\n{logs}"),
                }
            }
        }
    }
    pub fn shared_frame(
        &mut self,
        image: &image::RgbImage,
        frame_id: u64,
        context: [u64; 3],
    ) -> Result<gameocr_core::shared_frame::SharedFrame> {
        if self
            .mapping
            .as_ref()
            .is_none_or(|m| m.capacity < image.as_raw().len() + 64)
        {
            self.mapping = Some(gameocr_core::shared_frame::Mapping::create(
                image.as_raw().len(),
            )?);
        }
        self.mapping
            .as_mut()
            .unwrap()
            .write(image, frame_id, context)
    }
    pub async fn shutdown(&mut self) {
        let graceful = tokio::time::timeout(Duration::from_secs(2), async {
            self.input.write_all(b"{\"type\":\"shutdown\"}\n").await?;
            self.input.flush().await?;
            self.child.wait().await
        })
        .await;
        if !matches!(graceful, Ok(Ok(_))) {
            let _ = self.child.kill().await;
        }
        let _ = self.child.wait().await;
        self.log_task.abort();
    }
}
async fn read_response(reader: &mut (impl AsyncBufRead + Unpin)) -> Result<String> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    reader.take(LIMIT + 1).read_until(b'\n', &mut bytes).await?;
    anyhow::ensure!(
        !bytes.is_empty(),
        "推理工作程序退出（可能是 GPU/内存/运行库问题）"
    );
    anyhow::ensure!(
        bytes.len() <= LIMIT as usize,
        "工作程序响应超过 16 MiB 限制"
    );
    anyhow::ensure!(bytes.last() == Some(&b'\n'), "工作程序返回了不完整的响应");
    String::from_utf8(bytes).context("工作程序响应不是 UTF-8")
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.log_task.abort();
    }
}
pub fn worker_path(app: &tauri::AppHandle, runtime: &str, device: Device) -> Result<PathBuf> {
    let kind = if device == Device::Cuda {
        "cuda"
    } else {
        "cpu"
    };
    let root = resource_root(app)?;
    let registry = gameocr_core::engine::RuntimeRegistry::default();
    let executable = registry.executable(runtime)?;
    let path = root
        .join("workers")
        .join(runtime)
        .join(kind)
        .join(executable);
    anyhow::ensure!(
        path.is_file(),
        "未找到 {runtime}/{kind} 工作程序，请运行 scripts/build-workers.ps1 或安装完整发行包"
    );
    Ok(path)
}
pub fn resource_root(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
    } else {
        app.path().resource_dir()?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn responses_are_bounded_and_cannot_be_partial() {
        let mut input = &b"{\"type\":\"unloaded\"}\nnext\n"[..];
        assert_eq!(
            read_response(&mut input).await.unwrap(),
            "{\"type\":\"unloaded\"}\n"
        );
        assert_eq!(input, b"next\n");
        assert!(read_response(&mut &b"partial"[..]).await.is_err());
        let oversized = vec![b'x'; 16 * 1024 * 1024 + 2];
        assert!(read_response(&mut oversized.as_slice()).await.is_err());
    }
}
