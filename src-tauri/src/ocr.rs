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
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// Each worker owns all llama/mtmd objects on its main inference thread.
/// The desktop process has no CUDA-linked imports and can always start on CPU-only PCs.
pub struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    log: Arc<Mutex<VecDeque<String>>>,
    log_task: tokio::task::JoinHandle<()>,
    closing: Arc<AtomicBool>,
}
impl Worker {
    pub async fn spawn(path: &Path, closing: Arc<AtomicBool>) -> Result<Self> {
        let mut command = Command::new(path);
        // CUDA libraries are shared on disk but visible only to GPU children.
        // CPU children never add this directory to their loader search path.
        if path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "cuda")
        {
            if let Some(root) = path.parent().and_then(Path::parent).and_then(Path::parent) {
                let mut paths = vec![
                    path.parent().unwrap().to_path_buf(),
                    root.join("cuda-runtime"),
                ];
                paths.extend(std::env::split_paths(
                    &std::env::var_os("PATH").unwrap_or_default(),
                ));
                command.env("PATH", std::env::join_paths(paths)?);
            }
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
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(mut line)) = lines.next_line().await {
                if line.len() > 4096 {
                    line.truncate(line.floor_char_boundary(4096));
                }
                let mut log = log_copy.lock();
                if log.len() >= 128 {
                    log.pop_front();
                }
                log.push_back(line);
            }
        });
        Ok(Self {
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
            let mut line=String::new();
            let read=self.output.read_line(&mut line);tokio::pin!(read);
            let len=loop{tokio::select!{len=&mut read=>break len?,_=tokio::time::sleep(Duration::from_millis(50))=>{if self.closing.load(Ordering::Acquire){bail!("应用正在退出");}}}};
            if len==0 {bail!("推理工作程序退出（可能是 GPU/内存/运行库问题）");}
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
    pub async fn shutdown(&mut self) {
        let graceful=tokio::time::timeout(Duration::from_secs(2),async {
            self.input.write_all(b"{\"type\":\"shutdown\"}\n").await?;
            self.input.flush().await?;
            self.child.wait().await
        }).await;
        if !matches!(graceful,Ok(Ok(_))) {let _ = self.child.kill().await;}
        let _ = self.child.wait().await;
        self.log_task.abort();
    }
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
    let root = if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
    } else {
        app.path().resource_dir()?
    };
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
