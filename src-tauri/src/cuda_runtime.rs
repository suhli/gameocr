//! Pinned GPU backends and NVIDIA redistributables installed in user data.
//! No driver, system PATH, registry, or administrator installation is involved.
use anyhow::{Context, Result, bail, ensure};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    url: String,
    sha256: String,
    size: u64,
    runtimes: Vec<String>,
    required: Vec<String>,
    #[serde(default)]
    kind: PackageKind,
}
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum PackageKind {
    #[default]
    Nvidia,
    Onnx,
    Llamacpp,
    Translation,
}
#[derive(Deserialize)]
struct Catalog {
    packages: Vec<Package>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub state: String,
    pub component: String,
    pub received: u64,
    pub total: u64,
    pub message: String,
}
#[derive(Serialize, Deserialize)]
struct InstalledFile {
    name: String,
    size: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct Receipt {
    archive_sha256: String,
    files: Vec<InstalledFile>,
}

pub struct RuntimeManager {
    cancelled: Arc<AtomicBool>,
    progress: Mutex<Progress>,
}
// OCR and translation share verified NVIDIA files, so preparation must not
// write the same partial archive or receipt concurrently.
static PREPARE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
impl Default for RuntimeManager {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Mutex::new(Progress {
                state: "idle".into(),
                ..Default::default()
            }),
        }
    }
}
pub fn official_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("developer.download.nvidia.com" | "developer.download.nvidia.cn")
        )
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && (url.path().starts_with("/compute/cuda/redist/")
            || url.path().starts_with("/compute/cudnn/redist/"))
}
pub fn runtime_download_url(url: &reqwest::Url) -> bool {
    if official_url(url) {
        return true;
    }
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return false;
    }
    match url.host_str() {
        Some("github.com") => {
            url.path()
                .starts_with("/microsoft/onnxruntime/releases/download/")
                || url.path().starts_with("/suhli/gameocr/releases/download/")
        }
        // GitHub's official release-asset redirect destination. The archive hash
        // still has to match the application-embedded catalog before extraction.
        Some("release-assets.githubusercontent.com") => true,
        _ => false,
    }
}
fn validate_package(p: &Package) -> Result<()> {
    ensure!(
        safe_name(&p.id)
            && safe_name(&p.version)
            && !p.required.is_empty()
            && p.required.iter().all(|n| safe_name(n)),
        "无效的运行库目录"
    );
    let url = reqwest::Url::parse(&p.url)?;
    let source_ok = match p.kind {
        PackageKind::Nvidia => official_url(&url),
        PackageKind::Onnx => {
            runtime_download_url(&url)
                && url.host_str() == Some("github.com")
                && url
                    .path()
                    .starts_with("/microsoft/onnxruntime/releases/download/")
        }
        PackageKind::Llamacpp | PackageKind::Translation => {
            runtime_download_url(&url)
                && url.host_str() == Some("github.com")
                && url.path().starts_with("/suhli/gameocr/releases/download/")
        }
    };
    ensure!(
        source_ok
            && p.size > 0
            && p.sha256.len() == 64
            && p.sha256.bytes().all(|c| c.is_ascii_hexdigit()),
        "无效的官方运行库校验信息"
    );
    Ok(())
}
fn packages(runtime: &str) -> Result<Vec<Package>> {
    ensure!(
        matches!(runtime, "onnx" | "llamacpp" | "translation"),
        "不支持的 CUDA Runtime"
    );
    let catalog: Catalog = serde_json::from_str(include_str!("../catalog/cuda-runtime.json"))?;
    let packages: Vec<_> = catalog
        .packages
        .into_iter()
        .filter(|p| {
            p.runtimes.iter().any(|r| {
                r == if runtime == "translation" {
                    "llamacpp"
                } else {
                    runtime
                }
            })
        })
        .collect();
    for p in &packages {
        validate_package(p)?;
    }
    Ok(packages)
}
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn cache_root() -> PathBuf {
    crate::settings::data_dir().join("runtimes").join("nvidia")
}
fn package_cache(p: &Package) -> PathBuf {
    if p.kind == PackageKind::Nvidia {
        cache_root()
    } else {
        crate::settings::data_dir().join("runtimes/backends")
    }
}
fn backend_package(runtime: &str) -> Result<Package> {
    let p: Package = if runtime == "onnx" {
        serde_json::from_str(include_str!("../catalog/onnx-cuda-runtime.json"))?
    } else {
        let catalog: Catalog = serde_json::from_str(backend_catalog())?;
        catalog
            .packages
            .into_iter()
            .find(|p| {
                p.kind
                    == if runtime == "translation" {
                        PackageKind::Translation
                    } else {
                        PackageKind::Llamacpp
                    }
            })
            .with_context(|| {
                format!("此构建未配置 {runtime} GPU 下载包；请使用包含配套 GPU 资源的发行版本")
            })?
    };
    validate_package(&p)?;
    ensure!(p.runtimes.iter().any(|r| r == runtime), "GPU 后端不匹配");
    Ok(p)
}
pub fn backend_catalog() -> &'static str {
    include_str!(concat!(env!("OUT_DIR"), "/gpu-backends.json"))
}
fn binary_name(runtime: &str) -> Result<&'static str> {
    match runtime {
        "onnx" => Ok("onnx-worker.exe"),
        "llamacpp" => Ok("ocr-worker.exe"),
        "translation" => Ok("translation-worker.exe"),
        _ => bail!("不支持的 CUDA Runtime"),
    }
}
fn bundled_worker(root: &Path, runtime: &str) -> Result<PathBuf> {
    Ok(root
        .join("workers")
        .join(runtime)
        .join("cuda")
        .join(binary_name(runtime)?))
}
fn needs_backend(root: &Path, runtime: &str) -> Result<bool> {
    let worker = bundled_worker(root, runtime)?;
    let complete = worker.is_file()
        && (runtime != "onnx"
            || [
                "onnxruntime.dll",
                "onnxruntime_providers_shared.dll",
                "onnxruntime_providers_cuda.dll",
            ]
            .iter()
            .all(|n| worker.with_file_name(n).is_file()));
    if !complete {
        ensure!(
            root.join("gpu-downloads.json").is_file(),
            "此 CPU 专用包未包含 CUDA 后端，请使用支持按需下载的标准包"
        );
        ensure!(
            runtime != "onnx" || worker.is_file(),
            "标准包缺少 ONNX 工作程序，请重新安装"
        );
    }
    Ok(!complete)
}
/// Resolved only after all components have passed integrity checks; never downloads.
pub struct GpuLaunch {
    pub executable: PathBuf,
    pub libraries: Vec<PathBuf>,
    pub ort_dylib: Option<PathBuf>,
}
pub fn launch(root: &Path, runtime: &str) -> Result<GpuLaunch> {
    let remote_backend = needs_backend(root, runtime)?;
    let mut executable = bundled_worker(root, runtime)?;
    let mut libraries = vec![root.to_owned(), root.join("runtime")];
    let backend = if remote_backend {
        let p = backend_package(runtime)?;
        let directory = verify_installed(&package_cache(&p), &p, &AtomicBool::new(false))
            .context("GPU 后端尚未准备好；明确选择 CUDA 并加载模型可下载")?;
        if runtime != "onnx" {
            executable = directory.join(binary_name(runtime)?);
        }
        directory
    } else {
        executable.parent().context("无效工作程序目录")?.to_owned()
    };
    libraries.push(backend.clone());
    libraries.extend(paths(root, runtime)?);
    Ok(GpuLaunch {
        executable,
        libraries,
        ort_dylib: (runtime == "onnx").then(|| backend.join("onnxruntime.dll")),
    })
}
fn installed_path(root: &Path, p: &Package) -> PathBuf {
    root.join(format!("{}-{}-{}", p.id, p.version, &p.sha256[..12]))
}
fn bundled(root: &Path, packages: &[Package]) -> Option<PathBuf> {
    let dir = root.join("workers/cuda-runtime");
    packages
        .iter()
        .all(|p| {
            p.required.iter().all(|name| {
                if name.starts_with("nvrtc-builtins64_") {
                    std::fs::read_dir(&dir).is_ok_and(|entries| {
                        entries.flatten().any(|e| {
                            e.file_name()
                                .to_string_lossy()
                                .starts_with("nvrtc-builtins64_")
                                && e.path().extension().is_some_and(|x| x == "dll")
                        })
                    })
                } else {
                    dir.join(name).is_file()
                }
            })
        })
        .then_some(dir)
}
fn file_info(path: &Path, cancelled: &AtomicBool) -> Result<InstalledFile> {
    ensure!(
        !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
        "运行库不能是符号链接"
    );
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    let mut size = 0;
    loop {
        ensure!(!cancelled.load(Ordering::Acquire), "已取消 CUDA 运行库准备");
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        size += count as u64;
    }
    Ok(InstalledFile {
        name: path
            .file_name()
            .context("无效文件名")?
            .to_string_lossy()
            .into(),
        size,
        sha256: hex::encode(hash.finalize()),
    })
}
fn verify_installed(root: &Path, p: &Package, cancelled: &AtomicBool) -> Result<PathBuf> {
    let dir = installed_path(root, p);
    let receipt: Receipt = serde_json::from_slice(&std::fs::read(dir.join("installed.json"))?)?;
    ensure!(
        receipt.archive_sha256 == p.sha256 && receipt.files.len() <= 64,
        "运行库安装记录不匹配"
    );
    let mut names = HashSet::new();
    for f in receipt.files {
        ensure!(
            safe_name(&f.name) && names.insert(f.name.to_ascii_lowercase()),
            "无效的运行库文件记录"
        );
        let actual = file_info(&dir.join(&f.name), cancelled)?;
        ensure!(
            actual.size == f.size && actual.sha256 == f.sha256,
            "运行库文件损坏：{}",
            f.name
        );
    }
    ensure!(
        p.required
            .iter()
            .all(|n| names.contains(&n.to_ascii_lowercase()))
            && names.iter().any(|n| n.starts_with("license-")),
        "运行库组件不完整"
    );
    Ok(dir)
}
/// Called only for GPU workers. Auto may use an existing installation, never download.
pub fn paths(root: &Path, runtime: &str) -> Result<Vec<PathBuf>> {
    let packages = packages(runtime)?;
    if let Some(dir) = bundled(root, &packages) {
        return Ok(vec![dir]);
    }
    packages.iter().map(|p| verify_installed(&cache_root(), p, &AtomicBool::new(false)))
        .collect::<Result<Vec<_>>>()
        .context("CUDA 运行库尚未准备好；选择 NVIDIA CUDA 并加载模型可从 NVIDIA 官方下载。CPU 可直接使用")
}

impl RuntimeManager {
    pub fn status(&self) -> Progress {
        self.progress.lock().clone()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn publish(&self, value: Progress, emit: &impl Fn(Progress)) {
        *self.progress.lock() = value.clone();
        emit(value);
    }
    pub async fn ensure(
        &self,
        root: &Path,
        runtime: &str,
        shutdown: Arc<AtomicBool>,
        emit: impl Fn(Progress) + Send + Sync,
    ) -> Result<()> {
        self.cancelled.store(false, Ordering::Release);
        let _installation = PREPARE_LOCK.lock().await;
        ensure!(
            !self.cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
            "已取消运行库准备"
        );
        let result = self.prepare(root, runtime, shutdown, &emit).await;
        let mut progress = self.status();
        match &result {
            Ok(_) => {
                progress.state = "complete".into();
                progress.received = progress.total;
                progress.message = "CUDA 运行库已就绪".into();
            }
            Err(e) => {
                progress.state = if self.cancelled.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                progress.message = format!("{e:#}");
            }
        }
        self.publish(progress, &emit);
        result
    }
    async fn prepare(
        &self,
        root: &Path,
        runtime: &str,
        shutdown: Arc<AtomicBool>,
        emit: &(impl Fn(Progress) + Send + Sync),
    ) -> Result<()> {
        let nvidia = packages(runtime)?;
        let mut packages = Vec::new();
        if needs_backend(root, runtime)? {
            packages.push(backend_package(runtime)?);
        }
        if bundled(root, &nvidia).is_none() {
            packages.extend(nvidia);
        }
        self.publish(
            Progress {
                state: "checking".into(),
                message: "正在检查本地 CUDA 运行库".into(),
                ..Default::default()
            },
            emit,
        );
        let cache = crate::settings::data_dir().join("runtimes");
        std::fs::create_dir_all(&cache)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(cache.join("install.lock"))?;
        lock.try_lock()
            .context("另一个 GameOCR 实例正在准备 CUDA 运行库，请稍后重试")?;
        let mut missing = Vec::new();
        for p in packages {
            let (r, c, package) = (package_cache(&p), self.cancelled.clone(), p.clone());
            if tokio::task::spawn_blocking(move || verify_installed(&r, &package, &c))
                .await?
                .is_err()
            {
                missing.push(p);
            }
            ensure!(
                !self.cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
                "已取消 CUDA 运行库准备"
            );
        }
        let total = missing.iter().map(|p| p.size).sum();
        if !missing.is_empty() {
            let driver = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .context("无法定位 Windows 系统目录")?
                .join("System32/nvcuda.dll");
            ensure!(
                driver.is_file(),
                "请先安装兼容的 NVIDIA 显卡驱动，再使用 CUDA；CPU 可直接使用"
            );
        }
        let downloader = crate::model::ModelManager::official_runtime_downloader()?;
        let mut finished = 0;
        for p in missing {
            let cache = package_cache(&p);
            std::fs::create_dir_all(&cache)?;
            let url = reqwest::Url::parse(&p.url)?;
            let filename = url
                .path_segments()
                .and_then(|mut s| s.next_back())
                .context("无效归档地址")?;
            ensure!(safe_name(filename), "无效归档名称");
            // Optional offline companion assets remain outside the small base ZIP.
            // They are subject to the exact same pinned size/hash verification.
            let local = root.join("runtime-downloads").join(filename);
            let from_local = local.is_file();
            let archive = if from_local {
                local
            } else {
                cache.join(format!("{}.zip", p.sha256))
            };
            let mut progress = Progress {
                state: "downloading".into(),
                component: p.name.clone(),
                total,
                received: finished,
                message: if from_local {
                    "正在校验本地配套 GPU 组件包"
                } else {
                    match p.kind {
                        PackageKind::Nvidia => "正在从 NVIDIA 官方下载；可取消后续传",
                        PackageKind::Onnx => {
                            "正在从微软官方 Release 下载 ONNX CUDA 后端；可取消后续传"
                        }
                        PackageKind::Llamacpp | PackageKind::Translation => {
                            "正在从 GameOCR Release 下载配套 GPU Worker；可取消后续传"
                        }
                    }
                }
                .into(),
            };
            self.publish(progress.clone(), emit);
            let (a, c) = (archive.clone(), self.cancelled.clone());
            let expected = p.sha256.clone();
            let valid_archive = tokio::task::spawn_blocking(move || {
                file_info(&a, &c).is_ok_and(|f| f.sha256 == expected)
            })
            .await?;
            if !valid_archive {
                ensure!(
                    !from_local,
                    "离线 GPU 组件包校验失败，请使用与此版本匹配的资源"
                );
                // A completed but invalid archive must not shadow the new atomic rename.
                if archive.exists() {
                    std::fs::remove_file(&archive)?;
                }
                let mut success = false;
                for attempt in 1..=3 {
                    let callback = |value: serde_json::Value| {
                        let mut update = progress.clone();
                        update.state = value["state"].as_str().unwrap_or("downloading").into();
                        update.received = finished
                            + value["received"].as_u64().unwrap_or(
                                if update.state == "verifying" {
                                    p.size
                                } else {
                                    0
                                },
                            );
                        self.publish(update, emit);
                    };
                    let transfer = downloader
                        .download_file(&p.url, &archive, p.size, &p.sha256, &p.id, &callback);
                    let cancelled = async {
                        loop {
                            if self.cancelled.load(Ordering::Acquire)
                                || shutdown.load(Ordering::Acquire)
                            {
                                break;
                            }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    };
                    let result = tokio::select! { result = transfer => result, _ = cancelled => bail!("已取消下载；再次加载模型可继续下载") };
                    match result {
                        Ok(()) => {
                            success = true;
                            break;
                        }
                        Err(e) if attempt == 3 => {
                            let hint = if matches!(
                                p.kind,
                                PackageKind::Llamacpp | PackageKind::Translation
                            ) {
                                "请确认配套 Release 资源已公开发布"
                            } else {
                                "请检查网络能否访问组件官方源"
                            };
                            return Err(e).with_context(|| {
                                format!("{} 下载失败；{hint}，再次加载可续传，或切换 CPU", p.name)
                            });
                        }
                        Err(_) => {
                            progress.state = "retrying".into();
                            progress.message = format!("下载中断，正在重试 ({attempt}/3)");
                            self.publish(progress.clone(), emit);
                        }
                    }
                }
                ensure!(success, "CUDA 运行库未完成下载");
            }
            ensure!(
                !self.cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
                "已取消 CUDA 运行库准备"
            );
            progress.state = "extracting".into();
            progress.received = finished + p.size;
            progress.message = "正在校验并准备运行库".into();
            self.publish(progress.clone(), emit);
            let (r, a, c, s, package) = (
                cache.clone(),
                archive.clone(),
                self.cancelled.clone(),
                shutdown.clone(),
                p.clone(),
            );
            tokio::task::spawn_blocking(move || install_archive(&r, &a, &package, &c, &s))
                .await??;
            // Keep interrupted .part downloads; remove successful archives to bound disk use.
            if !from_local {
                std::fs::remove_file(&archive)?;
            }
            finished += p.size;
        }
        Ok(())
    }
}

struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn install_archive(
    root: &Path,
    archive: &Path,
    p: &Package,
    cancelled: &AtomicBool,
    shutdown: &AtomicBool,
) -> Result<()> {
    let actual = file_info(archive, cancelled)?;
    ensure!(
        actual.sha256 == p.sha256 && actual.size == p.size,
        "运行库 ZIP 校验失败"
    );
    let stage = Staging(root.join(format!(".install-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir(&stage.0)?;
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    ensure!(zip.len() <= 10000, "运行库 ZIP 文件数量异常");
    let mut names = HashSet::new();
    let mut files = Vec::new();
    let mut expanded = 0u64;
    for i in 0..zip.len() {
        ensure!(
            !cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
            "已取消 CUDA 运行库准备"
        );
        let mut entry = zip.by_index(i)?;
        let safe = entry.enclosed_name().context("运行库 ZIP 包含越界路径")?;
        ensure!(
            entry
                .unix_mode()
                .is_none_or(|mode| mode & 0o170000 != 0o120000),
            "运行库 ZIP 包含符号链接"
        );
        if entry.is_dir() {
            continue;
        }
        let name = safe
            .file_name()
            .context("无效的运行库 ZIP 路径")?
            .to_string_lossy()
            .to_string();
        let dll = p.required.iter().any(|n| n.eq_ignore_ascii_case(&name));
        let license = name.to_ascii_lowercase().contains("license")
            || name.to_ascii_lowercase().contains("notices")
            || name.to_ascii_lowercase().ends_with("eula.txt");
        if !dll && !license {
            continue;
        }
        ensure!(safe_name(&name), "无效运行库文件名");
        let name = if license {
            format!("license-{i}.txt")
        } else {
            name
        };
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "运行库 ZIP 包含重复文件"
        );
        expanded = expanded
            .checked_add(entry.size())
            .context("运行库 ZIP 过大")?;
        ensure!(
            expanded <= 4 * 1024 * 1024 * 1024 && entry.size() <= 2 * 1024 * 1024 * 1024,
            "运行库解压大小超限"
        );
        let path = stage.0.join(&name);
        let mut output = File::create(&path)?;
        let mut buffer = vec![0u8; 256 * 1024];
        let mut written = 0;
        loop {
            ensure!(
                !cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
                "已取消 CUDA 运行库准备"
            );
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            written += count as u64;
            ensure!(written <= entry.size(), "ZIP 解压大小不符");
            output.write_all(&buffer[..count])?;
        }
        ensure!(written == entry.size(), "ZIP 文件不完整");
        output.sync_all()?;
        drop(output);
        files.push(file_info(&path, cancelled)?);
    }
    ensure!(
        p.required
            .iter()
            .all(|n| names.contains(&n.to_ascii_lowercase()))
            && names.iter().any(|n| n.starts_with("license-")),
        "运行库缺少所需文件或许可证"
    );
    crate::settings::atomic_json(
        &stage.0.join("installed.json"),
        &Receipt {
            archive_sha256: p.sha256.clone(),
            files,
        },
    )?;
    ensure!(
        !cancelled.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire),
        "已取消 CUDA 运行库准备"
    );
    let destination = installed_path(root, p);
    if destination.exists() {
        std::fs::remove_dir_all(&destination)
            .context("无法修复旧的运行库缓存；请关闭其他使用该缓存的实例")?;
    }
    std::fs::rename(&stage.0, destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(root: &Path, entries: &[(&str, &[u8])]) -> (PathBuf, Package) {
        let path = root.join("test.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
        let info = file_info(&path, &AtomicBool::new(false)).unwrap();
        let p = Package {
            id: "test".into(),
            name: "Test".into(),
            version: "1".into(),
            url: String::new(),
            size: info.size,
            sha256: info.sha256,
            runtimes: vec!["onnx".into()],
            required: vec!["test.dll".into()],
            kind: PackageKind::Nvidia,
        };
        (path, p)
    }
    #[test]
    fn official_catalog_is_pinned_and_runtime_specific() {
        let gguf = packages("llamacpp").unwrap();
        let onnx = packages("onnx").unwrap();
        assert_eq!(gguf.len(), 2);
        assert!(official_url(
            &reqwest::Url::parse(
                "https://developer.download.nvidia.cn/compute/cuda/redist/test.zip"
            )
            .unwrap()
        ));
        assert_eq!(onnx.len(), 6);
        assert_eq!(onnx.iter().map(|p| p.size).sum::<u64>(), 1_995_551_251);
        assert!(packages("unknown").is_err());
        for bad in [
            "http://developer.download.nvidia.com/compute/cuda/redist/test",
            "https://evil.example/compute/cuda/redist/test",
            "https://developer.download.nvidia.com.evil.example/compute/cuda/redist/test",
            "https://developer.download.nvidia.com:444/compute/cuda/redist/test",
            "https://developer.download.nvidia.com/other",
        ] {
            assert!(!official_url(&reqwest::Url::parse(bad).unwrap()));
        }
    }
    #[test]
    fn install_is_complete_and_detects_corruption() {
        let tmp = tempfile::tempdir().unwrap();
        let (zip, p) = archive(
            tmp.path(),
            &[
                ("pkg/bin/test.dll", b"library"),
                ("pkg/LICENSE.txt", b"license"),
                ("pkg/lib/unused.lib", b"ignored"),
            ],
        );
        let flag = AtomicBool::new(false);
        install_archive(tmp.path(), &zip, &p, &flag, &flag).unwrap();
        let installed = verify_installed(tmp.path(), &p, &flag).unwrap();
        assert!(!installed.join("unused.lib").exists());
        std::fs::write(installed.join("test.dll"), b"corrupt").unwrap();
        assert!(verify_installed(tmp.path(), &p, &flag).is_err());
        install_archive(tmp.path(), &zip, &p, &flag, &flag).unwrap();
        assert!(verify_installed(tmp.path(), &p, &flag).is_ok());
    }
    #[test]
    fn invalid_archive_never_publishes_partial_installation() {
        for entries in [
            vec![("../outside.dll", b"bad".as_slice())],
            vec![("pkg/bin/test.dll", b"library".as_slice())],
            vec![("pkg/LICENSE.txt", b"license".as_slice())],
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let (zip, p) = archive(tmp.path(), &entries);
            let flag = AtomicBool::new(false);
            assert!(install_archive(tmp.path(), &zip, &p, &flag, &flag).is_err());
            assert!(!installed_path(tmp.path(), &p).exists());
            assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
        }
    }
    #[test]
    fn checksum_cancellation_and_shutdown_leave_no_installation() {
        let tmp = tempfile::tempdir().unwrap();
        let (zip, mut p) = archive(
            tmp.path(),
            &[
                ("pkg/bin/test.dll", b"library"),
                ("pkg/LICENSE.txt", b"license"),
            ],
        );
        let yes = AtomicBool::new(true);
        let no = AtomicBool::new(false);
        assert!(install_archive(tmp.path(), &zip, &p, &yes, &no).is_err());
        assert!(install_archive(tmp.path(), &zip, &p, &no, &yes).is_err());
        p.sha256 = "0".repeat(64);
        assert!(install_archive(tmp.path(), &zip, &p, &no, &no).is_err());
        assert!(!installed_path(tmp.path(), &p).exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
    #[test]
    fn corrupt_zip_with_matching_digest_never_becomes_ready() {
        let tmp = tempfile::tempdir().unwrap();
        let (zip, mut p) = archive(
            tmp.path(),
            &[("test.dll", b"library"), ("LICENSE.txt", b"license")],
        );
        std::fs::write(&zip, b"corrupt zip, even if its catalog digest matches").unwrap();
        let flag = AtomicBool::new(false);
        let info = file_info(&zip, &flag).unwrap();
        p.size = info.size;
        p.sha256 = info.sha256;
        assert!(install_archive(tmp.path(), &zip, &p, &flag, &flag).is_err());
        assert!(verify_installed(tmp.path(), &p, &flag).is_err());
        assert!(!installed_path(tmp.path(), &p).exists());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
    #[tokio::test]
    async fn cpu_only_distribution_fails_before_any_download() {
        let tmp = tempfile::tempdir().unwrap();
        let manager = RuntimeManager::default();
        assert!(
            manager
                .ensure(tmp.path(), "onnx", Arc::new(AtomicBool::new(false)), |_| {})
                .await
                .is_err()
        );
        assert_eq!(manager.status().state, "failed");
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }
    #[test]
    fn backend_sources_are_pinned_and_redirects_are_restricted() {
        let onnx = backend_package("onnx").unwrap();
        assert_eq!(onnx.size, 311_219_580);
        assert_eq!(onnx.required.len(), 3);
        assert!(validate_package(&onnx).is_ok());
        for good in [
            "https://github.com/microsoft/onnxruntime/releases/download/v1.24.3/ort.zip",
            "https://github.com/suhli/gameocr/releases/download/v1/worker.zip",
            "https://release-assets.githubusercontent.com/github-production-release-asset/a?sig=b",
        ] {
            assert!(runtime_download_url(&reqwest::Url::parse(good).unwrap()));
        }
        for bad in [
            "http://github.com/suhli/gameocr/releases/download/v1/test.zip",
            "https://github.com/untrusted/project/releases/download/v1/test.zip",
            "https://github.com/suhli/gameocr/raw/main/test.zip",
            "https://release-assets.githubusercontent.com.evil.example/test.zip",
        ] {
            assert!(!runtime_download_url(&reqwest::Url::parse(bad).unwrap()));
        }
        let mut wrong = onnx;
        wrong.url = "https://github.com/suhli/gameocr/releases/download/v1/test.zip".into();
        assert!(validate_package(&wrong).is_err());
    }
    #[test]
    fn lean_layout_requires_opt_in_and_complete_bundled_ort_still_works() {
        let tmp = tempfile::tempdir().unwrap();
        let worker = bundled_worker(tmp.path(), "onnx").unwrap();
        std::fs::create_dir_all(worker.parent().unwrap()).unwrap();
        std::fs::write(&worker, b"worker").unwrap();
        assert!(needs_backend(tmp.path(), "onnx").is_err());
        std::fs::write(tmp.path().join("gpu-downloads.json"), b"{}").unwrap();
        assert!(needs_backend(tmp.path(), "onnx").unwrap());
        for name in backend_package("onnx").unwrap().required {
            std::fs::write(worker.with_file_name(name), b"library").unwrap();
        }
        std::fs::remove_file(tmp.path().join("gpu-downloads.json")).unwrap();
        assert!(!needs_backend(tmp.path(), "onnx").unwrap());
        assert!(needs_backend(tmp.path(), "llamacpp").is_err());
    }
    #[test]
    fn backend_archive_requires_matching_core_and_provider_and_keeps_notices() {
        let tmp = tempfile::tempdir().unwrap();
        let (zip, mut p) = archive(
            tmp.path(),
            &[
                ("ort/lib/onnxruntime.dll", b"core"),
                ("ort/lib/onnxruntime_providers_cuda.dll", b"provider"),
                ("ort/lib/onnxruntime_providers_shared.dll", b"shared"),
                ("ort/ThirdPartyNotices.txt", b"notices"),
                ("ort/CUDA-EULA.txt", b"toolchain license"),
            ],
        );
        p.kind = PackageKind::Onnx;
        p.required = backend_package("onnx").unwrap().required;
        let no = AtomicBool::new(false);
        install_archive(tmp.path(), &zip, &p, &no, &no).unwrap();
        let installed = verify_installed(tmp.path(), &p, &no).unwrap();
        assert!(installed.join("license-3.txt").is_file());
        assert_eq!(
            std::fs::read(installed.join("license-4.txt")).unwrap(),
            b"toolchain license"
        );
        std::fs::remove_file(installed.join("onnxruntime.dll")).unwrap();
        assert!(verify_installed(tmp.path(), &p, &no).is_err());
    }
    #[test]
    fn installation_lock_is_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("lock");
        let first = File::create(&path).unwrap();
        first.try_lock().unwrap();
        let second = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        assert!(second.try_lock().is_err());
        drop(first);
        assert!(second.try_lock().is_ok());
    }
}
