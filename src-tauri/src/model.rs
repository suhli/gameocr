use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
pub use gameocr_core::manifest::ModelManifest as ModelEntry;
use gameocr_core::{
    engine::{OcrCapabilities, RuntimeRegistry},
    manifest::{ModelFile, safe_id},
};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

pub fn catalog() -> Vec<ModelEntry> {
    [
        (
            "Q8_0",
            811843648,
            "b5c262bc905015e78002ea0e360bcbc7e6b3edd71d29a80857f677392f48eb15",
        ),
        (
            "Q4_K_M",
            529297472,
            "32eed216ffdd38254ecf6f1b7bc3360e74dae5b65873b7f1ce3590043315a658",
        ),
        (
            "Q5_K_M",
            577998912,
            "60051f855c0225642654eba710cc95767c9215ba7f77c981c54caba1e2de1b73",
        ),
        (
            "f16",
            1516744768,
            "a583ba92bdf6a62566101b098842840b7de84a800f767a1032a3b687d24f561f",
        ),
    ]
    .into_iter()
    .map(|(q, size, hash)| {
        let main = format!("LightOnOCR-3-0.8B-{q}.gguf");
        let mmproj = "LightOnOCR-3-0.8B-mmproj-f16.gguf".to_string();
        let file = |role: &str, path: String, size, hash: &str| ModelFile { role:role.into(), url:Some(format!("https://huggingface.co/Prathat/LightOnOCR-3-0.8B-GGUF/resolve/main/{path}?download=true")),path,size,sha256:hash.into() };
        ModelEntry { schema_version:2,id:q.into(),name:format!("LightOnOCR-3 · {q}"),quantization:q.into(),runtime:"llamacpp".into(),adapter:"lightonocr".into(),version:"3".into(),capabilities:OcrCapabilities::lighton(),files:vec![file("main",main,size,hash),file("mmproj",mmproj,204987136,"e77f2ebb738f5c7852e955f09440c407d9c54b56c6f6098900380706302e8742")],source:"Prathat / Hugging Face".into(),parameters:serde_json::Value::Null,installed:false,imported:false }
    })
    .chain(onnx_catalog())
    .collect()
}
pub fn onnx_catalog() -> Vec<ModelEntry> {
    serde_json::from_str(include_str!("../catalog/ppocrv6.json")).expect("checked built-in catalog")
}
pub struct ModelManager {
    pub busy: Arc<AtomicBool>,
    client: reqwest::Client,
}
struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl ModelManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            busy: Arc::new(AtomicBool::new(false)),
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(20))
                .read_timeout(std::time::Duration::from_secs(60))
                .user_agent("GameOCR/0.1.0")
                .build()?,
        })
    }
    fn guard(&self) -> Result<BusyGuard> {
        ensure!(
            !self.busy.swap(true, Ordering::AcqRel),
            "模型文件操作正在进行"
        );
        Ok(BusyGuard(self.busy.clone()))
    }
    pub fn list(dir: &Path) -> Result<Vec<ModelEntry>> {
        let mut entries = catalog();
        if dir.exists() {
            for e in std::fs::read_dir(dir)? {
                let path = e?.path();
                if path.extension().is_some_and(|s| s == "json") {
                    if let Ok(m) = ModelEntry::from_slice(&std::fs::read(&path)?) {
                        if RuntimeRegistry::default().validate_manifest(&m).is_ok() {
                            // Publish v2 metadata atomically, retaining original file names.
                            if !serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path)?)?
                                .get("schemaVersion")
                                .is_some()
                            {
                                crate::settings::atomic_json(&path, &m)?;
                            }
                            if let Some(old) = entries.iter_mut().find(|x| x.id == m.id) {
                                *old = m;
                            } else if m.imported {
                                entries.push(m);
                            }
                        }
                    }
                }
            }
        }
        for m in &mut entries {
            m.installed = dir.join(format!("{}.json", m.id)).exists()
                && m.files
                    .iter()
                    .all(|f| verify_file(&dir.join(&f.path), f.size, &f.sha256).is_ok());
        }
        Ok(entries)
    }
    pub fn find(dir: &Path, id: &str) -> Result<ModelEntry> {
        Self::list(dir)?
            .into_iter()
            .find(|m| m.id == id)
            .context("未知模型")
    }
    pub fn verify(dir: &Path, m: &ModelEntry) -> Result<()> {
        ensure!(m.installed, "模型尚未完成下载和校验");
        RuntimeRegistry::default().validate_manifest(m)?;
        for f in &m.files {
            verify_file(&dir.join(&f.path), f.size, &f.sha256)?;
        }
        Ok(())
    }
    pub async fn download(
        &self,
        dir: PathBuf,
        id: String,
        mirror: String,
        app: tauri::AppHandle,
    ) -> Result<()> {
        let _guard = self.guard()?;
        let mut entry = catalog()
            .into_iter()
            .find(|m| m.id == id)
            .context("此模型不在下载目录中")?;
        tokio::fs::create_dir_all(&dir).await?;
        for item in &entry.files {
            let (file, size, hash) = (&item.path, item.size, &item.sha256);
            let path = dir.join(file);
            let verify_path = path.clone();
            let check_hash = hash.clone();
            if tokio::task::spawn_blocking(move || verify_file(&verify_path, size, &check_hash))
                .await?
                .is_ok()
            {
                continue;
            }
            let url = item
                .url
                .as_ref()
                .context("No verified download URL")?
                .replacen("https://huggingface.co", &mirror, 1);
            let mut last_error = None;
            for attempt in 1..=3 {
                let progress = |value| {
                    let _ = app.emit("model-download-progress", value);
                };
                match self
                    .download_file(&url, &path, size, hash, &id, &progress)
                    .await
                {
                    Ok(()) => {
                        last_error = None;
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e);
                        let _=app.emit("model-download-progress",serde_json::json!({"id":id,"file":file,"state":"retrying","attempt":attempt}));
                        if attempt < 3 {
                            tokio::time::sleep(std::time::Duration::from_secs(attempt * 2)).await;
                        }
                    }
                }
            }
            if let Some(e) = last_error {
                bail!("下载失败，可重试并续传：{e:#}");
            }
        }
        entry.installed = true;
        crate::settings::atomic_json(&dir.join(format!("{}.json", entry.id)), &entry)?;
        let _ = app.emit(
            "model-download-progress",
            serde_json::json!({"id":id,"state":"complete"}),
        );
        Ok(())
    }
    pub(crate) async fn download_file(
        &self,
        url: &str,
        path: &Path,
        size: u64,
        hash: &str,
        id: &str,
        progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<()> {
        let part = part_path(&path);
        let mut offset = tokio::fs::metadata(&part).await.map_or(0, |v| v.len());
        if offset > size {
            tokio::fs::remove_file(&part).await?;
            offset = 0;
        }
        if offset < size {
            let mut req = self.client.get(url);
            if offset > 0 {
                req = req.header(reqwest::header::RANGE, format!("bytes={offset}-"));
            }
            let response = req.send().await?.error_for_status()?;
            let append = resume_response(
                offset,
                response.status().as_u16(),
                response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok()),
                size,
            )?;
            if !append {
                offset = 0;
            }
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(append)
                .truncate(!append)
                .open(&part)
                .await?;
            let mut stream = response.bytes_stream();
            let mut last = std::time::Instant::now();
            while let Some(bytes) = stream.next().await {
                let bytes = bytes?;
                ensure!(
                    offset + bytes.len() as u64 <= size,
                    "服务器文件大小与目录不符"
                );
                file.write_all(&bytes).await?;
                offset += bytes.len() as u64;
                if last.elapsed().as_millis() > 200 {
                    progress(
                        serde_json::json!({"id":id,"file":path.file_name(),"received":offset,"total":size,"state":"downloading"}),
                    );
                    last = std::time::Instant::now();
                }
            }
            file.sync_all().await?;
        }
        let p = part.clone();
        let h = hash.to_string();
        progress(serde_json::json!({"id":id,"state":"verifying"}));
        let check = tokio::task::spawn_blocking(move || verify_file(&p, size, &h)).await?;
        if let Err(e) = check {
            if offset == size {
                tokio::fs::remove_file(&part).await?;
            }
            return Err(e);
        }
        tokio::fs::rename(part, path).await?;
        Ok(())
    }
    pub(crate) fn official_runtime_downloader() -> Result<Self> {
        Ok(Self {
            busy: Arc::new(AtomicBool::new(false)),
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(20))
                .read_timeout(std::time::Duration::from_secs(60))
                .user_agent("GameOCR/0.1.0")
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() < 5
                        && crate::cuda_runtime::official_url(attempt.url())
                    {
                        attempt.follow()
                    } else {
                        attempt.error("CUDA 运行库只允许 NVIDIA 官方 HTTPS 下载地址")
                    }
                }))
                .build()?,
        })
    }
    pub fn import(&self, dir: &Path, model: &Path, mmproj: &Path) -> Result<ModelEntry> {
        let _guard = self.guard()?;
        std::fs::create_dir_all(dir)?;
        let id = format!("import-{}", uuid::Uuid::new_v4());
        // Validate both inputs before copying either, and roll back this import on every error.
        for source in [model, mmproj] {
            ensure!(
                source
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("gguf")),
                "导入必须选择 .gguf 文件"
            );
        }
        let metadata = [
            (hash_gguf(model)?, std::fs::metadata(model)?.len()),
            (hash_gguf(mmproj)?, std::fs::metadata(mmproj)?.len()),
        ];
        let mut owned = Vec::new();
        let result = (|| -> Result<ModelEntry> {
            let mut files = Vec::new();
            for (i, (source, name)) in [
                (model, format!("{id}.gguf")),
                (mmproj, format!("{id}-mmproj.gguf")),
            ]
            .into_iter()
            .enumerate()
            {
                let (source_hash, size) = metadata[i].clone();
                let dest = dir.join(&name);
                let part = dest.with_extension("gguf.part");
                owned.extend([dest.clone(), part.clone()]);
                std::fs::copy(source, &part)?;
                verify_file(&part, size, &source_hash)?;
                std::fs::rename(part, dest)?;
                files.push((name, size, source_hash));
            }
            let entry = ModelEntry {
                schema_version: 2,
                id: id.clone(),
                name: model
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                quantization: "本地导入".into(),
                runtime: "llamacpp".into(),
                adapter: "lightonocr".into(),
                version: "3".into(),
                capabilities: OcrCapabilities::lighton(),
                files: files
                    .into_iter()
                    .zip(["main", "mmproj"])
                    .map(|((path, size, sha256), role)| ModelFile {
                        role: role.into(),
                        path,
                        size,
                        sha256,
                        url: None,
                    })
                    .collect(),
                source: "local".into(),
                parameters: serde_json::Value::Null,
                installed: true,
                imported: true,
            };
            crate::settings::atomic_json(&dir.join(format!("{id}.json")), &entry)?;
            Ok(entry)
        })();
        if result.is_err() {
            for path in owned {
                let _ = std::fs::remove_file(path);
            }
        }
        result
    }
    pub fn delete(&self, dir: &Path, id: &str) -> Result<()> {
        let _guard = self.guard()?;
        let entry = Self::find(dir, id)?;
        ensure!(safe_id(id), "非法模型 ID");
        // Shared catalog projector remains while another installed model references it.
        let others = Self::list(dir)?;
        for file in &entry.files {
            let name = &file.path;
            if others.iter().any(|m| {
                m.id != id
                    && dir.join(format!("{}.json", m.id)).exists()
                    && m.files.iter().any(|f| &f.path == name)
            }) {
                continue;
            }
            for path in [dir.join(name), part_path(&dir.join(name))] {
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
            }
        }
        let manifest = dir.join(format!("{id}.json"));
        if manifest.exists() {
            std::fs::remove_file(manifest)?;
        }
        Ok(())
    }
    pub async fn import_onnx(
        &self,
        dir: PathBuf,
        variant: String,
        paths: [PathBuf; 3],
        app: tauri::AppHandle,
    ) -> Result<ModelEntry> {
        let _guard = self.guard()?;
        ensure!(
            ["small", "medium"].contains(&variant.as_str()),
            "请选择 PP-OCRv6 small 或 medium"
        );
        let id = format!("import-{}", uuid::Uuid::new_v4());
        let roles = ["detector", "recognizer", "dictionary"];
        let mut owned = Vec::new();
        let result: Result<ModelEntry> = async {
            tokio::fs::create_dir_all(&dir).await?;
            let mut files = Vec::new();
            for (index, source) in paths.into_iter().enumerate() {
                let ext = if index == 2 { "txt" } else { "onnx" };
                ensure!(
                    source
                        .extension()
                        .is_some_and(|s| s.eq_ignore_ascii_case(ext)),
                    "错误的模型文件扩展名"
                );
                let name = format!("{id}-{}.{ext}", roles[index]);
                let dest = dir.join(&name);
                let part = part_path(&dest);
                owned.extend([dest.clone(), part.clone()]);
                let (size, sha256) = tokio::task::spawn_blocking(move || -> Result<_> {
                    let size = std::fs::metadata(&source)?.len();
                    ensure!(size > 0, "模型文件为空");
                    let hash = hash_file(&source)?;
                    std::fs::copy(source, &part)?;
                    verify_file(&part, size, &hash)?;
                    std::fs::rename(part, dest)?;
                    Ok((size, hash))
                })
                .await??;
                files.push(ModelFile {
                    role: roles[index].into(),
                    path: name,
                    size,
                    sha256,
                    url: None,
                });
            }
            let entry = ModelEntry {
                schema_version: 2,
                id: id.clone(),
                name: format!("PP-OCRv6 {variant} · 本地导入"),
                runtime: "onnx".into(),
                adapter: "ppocrv6".into(),
                version: format!("6-{variant}"),
                quantization: "FP32".into(),
                capabilities: OcrCapabilities::paddle(),
                files,
                source: "local".into(),
                parameters: serde_json::json!({"variant":variant}),
                installed: true,
                imported: true,
            };
            let config = gameocr_core::engine::EngineConfig {
                manifest: entry.clone(),
                directory: dir.to_string_lossy().into(),
                device: gameocr_core::types::Device::Cpu,
                threads: 4,
                inference: gameocr_core::engine::RuntimeConfig::Onnx(Default::default()),
            };
            RuntimeRegistry::default().validate(&config)?;
            let path = crate::ocr::worker_path(&app, "onnx", gameocr_core::types::Device::Cpu)?;
            let mut worker =
                crate::ocr::Worker::spawn(&path, Arc::new(AtomicBool::new(false))).await?;
            let response = worker
                .request(gameocr_core::types::WorkerRequest::LoadEngine {
                    protocol_version: 2,
                    config,
                })
                .await;
            worker.shutdown().await;
            ensure!(
                matches!(response?, gameocr_core::types::WorkerResponse::Ready { .. }),
                "ONNX 验证未完成"
            );
            crate::settings::atomic_json(&dir.join(format!("{id}.json")), &entry)?;
            Ok(entry)
        }
        .await;
        if result.is_err() {
            for path in owned {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        result
    }
}
pub fn hash_gguf(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    ensure!(&magic == b"GGUF", "不是 GGUF 文件");
    let mut hash = Sha256::new();
    hash.update(magic);
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    Ok(hex::encode(hash.finalize()))
}
fn part_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", path.to_string_lossy()))
}
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let len = file.read(&mut buffer)?;
        if len == 0 {
            break;
        }
        hash.update(&buffer[..len]);
    }
    Ok(hex::encode(hash.finalize()))
}
fn verify_file(path: &Path, size: u64, hash: &str) -> Result<()> {
    ensure!(std::fs::metadata(path)?.len() == size, "文件大小不完整");
    ensure!(hash_file(path)? == hash, "SHA-256 校验失败");
    Ok(())
}
fn resume_response(offset: u64, status: u16, range: Option<&str>, size: u64) -> Result<bool> {
    if status == 200 {
        return Ok(false);
    }
    if status == 206 {
        let range = range.context("续传响应缺少 Content-Range")?;
        let expected = format!("bytes {offset}-");
        ensure!(
            range.starts_with(&expected) && range.ends_with(&format!("/{size}")),
            "续传响应范围不匹配"
        );
        return Ok(true);
    }
    bail!("不支持的下载响应 {status}")
}
#[cfg(test)]
mod tests {
    use super::*;
    use gameocr_core::manifest::safe_file;
    #[test]
    fn resume_safety() {
        assert!(!resume_response(100, 200, None, 500).unwrap());
        assert!(resume_response(100, 206, Some("bytes 100-499/500"), 500).unwrap());
        assert!(resume_response(100, 206, Some("bytes 0-499/500"), 500).is_err());
    }
    #[test]
    fn unfinished_is_never_ready() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("LightOnOCR-3-0.8B-Q8_0.gguf.part"), b"GGUF").unwrap();
        assert!(
            ModelManager::list(d.path())
                .unwrap()
                .iter()
                .all(|m| !m.installed)
        );
    }
    #[test]
    fn path_safety() {
        for p in ["../x.gguf", "C:\\x.gguf", "a/b.gguf"] {
            assert!(!safe_file(p));
        }
        assert!(safe_file("model.gguf"));
    }
    #[test]
    fn multi_file_integrity_and_shared_file_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = onnx_catalog().remove(0);
        a.id = "import-a".into();
        a.imported = true;
        a.installed = true;
        for file in &mut a.files {
            let bytes = format!("test artifact for {}", file.role).into_bytes();
            file.size = bytes.len() as u64;
            file.sha256 = hex::encode(Sha256::digest(&bytes));
            std::fs::write(dir.path().join(&file.path), bytes).unwrap();
        }
        let mut b = a.clone();
        b.id = "import-b".into();
        crate::settings::atomic_json(&dir.path().join("import-a.json"), &a).unwrap();
        crate::settings::atomic_json(&dir.path().join("import-b.json"), &b).unwrap();
        assert!(
            ModelManager::find(dir.path(), "import-a")
                .unwrap()
                .installed
        );
        let manager = ModelManager::new().unwrap();
        manager.delete(dir.path(), "import-a").unwrap();
        assert!(b.files.iter().all(|f| dir.path().join(&f.path).exists()));
        let file = &b.files[0];
        std::fs::write(dir.path().join(&file.path), vec![b'x'; file.size as usize]).unwrap();
        assert!(
            !ModelManager::find(dir.path(), "import-b")
                .unwrap()
                .installed
        );
        manager.delete(dir.path(), "import-b").unwrap();
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn catalog_is_valid_and_has_real_download_metadata() {
        for model in catalog() {
            RuntimeRegistry::default()
                .validate_manifest(&model)
                .unwrap();
            assert!(
                model
                    .files
                    .iter()
                    .all(|f| f.url.as_ref().is_some_and(|s| s.starts_with("https://")))
            );
        }
    }
    #[test]
    fn import_failure_does_not_leave_copies() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("models");
        let a = d.path().join("a.gguf");
        let b = d.path().join("b.gguf");
        std::fs::write(&a, b"GGUFgood").unwrap();
        std::fs::write(&b, b"bad").unwrap();
        assert!(ModelManager::new().unwrap().import(&out, &a, &b).is_err());
        assert_eq!(std::fs::read_dir(out).unwrap().count(), 0);
    }
    #[test]
    fn import_verify_and_delete_lifecycle() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("models");
        let a = d.path().join("a.gguf");
        let b = d.path().join("b.gguf");
        std::fs::write(&a, b"GGUF-model").unwrap();
        std::fs::write(&b, b"GGUF-projector").unwrap();
        let manager = ModelManager::new().unwrap();
        let m = manager.import(&out, &a, &b).unwrap();
        ModelManager::verify(&out, &m).unwrap();
        std::fs::write(out.join(&m.file("main").unwrap().path), b"GGUF-other").unwrap();
        assert!(ModelManager::verify(&out, &m).is_err());
        manager.delete(&out, &m.id).unwrap();
        assert_eq!(std::fs::read_dir(out).unwrap().count(), 0);
        assert!(a.exists() && b.exists());
    }
    async fn http_response(response: String, expected_range: Option<&'static str>) -> String {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let count = socket.read(&mut request).await.unwrap();
            if let Some(range) = expected_range {
                assert!(
                    String::from_utf8_lossy(&request[..count])
                        .to_lowercase()
                        .contains(range)
                );
            }
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        format!("http://{address}/model.gguf")
    }
    #[tokio::test]
    async fn interrupted_download_resumes_and_verifies_before_publish() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("model.gguf");
        let bytes = b"GGUF-resumable-file";
        let size = bytes.len() as u64;
        let hash = hex::encode(Sha256::digest(bytes));
        let manager = ModelManager::new().unwrap();
        let url = http_response(
            format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\nGGUF"),
            None,
        )
        .await;
        assert!(
            manager
                .download_file(&url, &path, size, &hash, "test", &|_| {})
                .await
                .is_err()
        );
        assert!(!path.exists());
        assert_eq!(std::fs::read(part_path(&path)).unwrap(), b"GGUF");
        let url = http_response(format!("HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 4-{}/{size}\r\nConnection: close\r\n\r\n{}", size-4, size-1, String::from_utf8_lossy(&bytes[4..])), Some("range: bytes=4-")).await;
        manager
            .download_file(&url, &path, size, &hash, "test", &|_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!part_path(&path).exists());
    }
    #[tokio::test]
    async fn ignored_range_restarts_and_corruption_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("model.gguf");
        std::fs::write(part_path(&path), b"GGUF").unwrap();
        let bytes = b"GGUF-complete";
        let size = bytes.len() as u64;
        let manager = ModelManager::new().unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n{}",
            String::from_utf8_lossy(bytes)
        );
        let url = http_response(response.clone(), Some("range: bytes=4-")).await;
        manager
            .download_file(
                &url,
                &path,
                size,
                &hex::encode(Sha256::digest(bytes)),
                "test",
                &|_| {},
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_file(&path).unwrap();
        let url = http_response(response, None).await;
        assert!(
            manager
                .download_file(&url, &path, size, "wrong-hash", "test", &|_| {})
                .await
                .is_err()
        );
        assert!(!path.exists() && !part_path(&path).exists());
    }
}
