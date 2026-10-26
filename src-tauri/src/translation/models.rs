use anyhow::{Context, Result, ensure};
use gameocr_translation::{catalog, types::*};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use tauri::Emitter;
pub fn directory() -> PathBuf {
    crate::settings::data_dir().join("translation/models")
}
pub fn list() -> Result<Vec<TranslationManifest>> {
    let dir = directory();
    let mut entries = vec![catalog::hy_mt2()];
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir)?.take(1000) {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") && path.metadata()?.len() <= 65536 {
                if let Ok(m) = serde_json::from_slice::<TranslationManifest>(&std::fs::read(path)?)
                {
                    if m.validate().is_ok() && m.runtime == RUNTIME && m.adapter == ADAPTER {
                        if let Some(old) = entries.iter_mut().find(|e| e.id == m.id) {
                            *old = m;
                        } else if m.imported {
                            entries.push(m);
                        }
                    }
                }
            }
        }
    }
    // Full hash is rechecked before load. Listing only uses size to keep UI responsive.
    for m in &mut entries {
        m.installed = dir.join(format!("{}.json", m.id)).is_file()
            && dir
                .join(&m.file)
                .metadata()
                .is_ok_and(|f| f.len() == m.size);
    }
    Ok(entries)
}
pub fn find(id: &str) -> Result<TranslationManifest> {
    list()?
        .into_iter()
        .find(|m| m.id == id)
        .context("未知翻译模型")
}
pub fn verify(m: &TranslationManifest) -> Result<()> {
    m.validate()?;
    ensure!(m.installed, "翻译模型未安装");
    crate::model::verify_file(&directory().join(&m.file), m.size, &m.sha256)
}
pub async fn download(
    manager: &crate::model::ModelManager,
    mirror: &str,
    app: &tauri::AppHandle,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    ensure!(
        ["https://huggingface.co", "https://hf-mirror.com"].contains(&mirror),
        "模型下载源无效"
    );
    let m = catalog::hy_mt2();
    let dir = directory();
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(&m.file);
    let url = m
        .url
        .as_ref()
        .unwrap()
        .replacen("https://huggingface.co", mirror, 1);
    let progress = |p| {
        let _ = app.emit("translation-download-progress", p);
    };
    let transfer = async {
        let mut error = None;
        for _ in 0..3 {
            match manager
                .download_file(&url, &path, m.size, &m.sha256, &m.id, &progress)
                .await
            {
                Ok(()) => return Ok(()),
                Err(e) => error = Some(e),
            }
        }
        Err(error.unwrap())
    };
    let cancelled = async {
        loop {
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    };
    tokio::select! {r=transfer=>r?,_=cancelled=>anyhow::bail!("已取消，重新下载可续传")};
    crate::settings::atomic_json(&dir.join(format!("{}.json", m.id)), &m)?;
    let _ = app.emit(
        "translation-download-progress",
        serde_json::json!({"id":m.id,"state":"complete"}),
    );
    Ok(())
}
pub fn import(path: &Path) -> Result<TranslationManifest> {
    ensure!(
        path.extension()
            .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("gguf")),
        "请选择 GGUF 文件"
    );
    let mut f = std::fs::File::open(path)?;
    let size = f.metadata()?.len();
    ensure!(
        size > 32 && size < 16 * 1024 * 1024 * 1024,
        "GGUF 文件大小无效"
    );
    let mut magic = [0; 4];
    f.read_exact(&mut magic)?;
    ensure!(&magic == b"GGUF", "不是 GGUF 文件");
    // This first adapter imports the verified Hy-MT2 Q4_K_M release only.
    // Other quantizations need a registered manifest and compatibility evidence.
    let mut hash = Sha256::new();
    hash.update(magic);
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        let n = f.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let sha = format!("{:x}", hash.finalize());
    let mut m = catalog::hy_mt2();
    ensure!(
        size == m.size && sha == m.sha256,
        "当前已验证的本地导入为 Hy-MT2-1.8B Q4_K_M 官方文件；文件损坏或模型/量化不匹配"
    );
    let dir = directory();
    std::fs::create_dir_all(&dir)?;
    let target = dir.join(&m.file);
    if path.canonicalize()? != target.canonicalize().unwrap_or_default() {
        let part = target.with_extension("gguf.import.part");
        std::fs::copy(path, &part)?;
        crate::model::verify_file(&part, m.size, &m.sha256)?;
        std::fs::rename(&part, &target)?;
    }
    m.imported = true;
    m.installed = true;
    crate::settings::atomic_json(&dir.join(format!("{}.json", m.id)), &m)?;
    Ok(m)
}
pub fn delete(id: &str) -> Result<()> {
    let m = find(id)?;
    let others = list()?;
    let dir = directory();
    ensure!(
        !others
            .iter()
            .any(|o| o.id != id && o.file == m.file && o.installed),
        "文件仍被其他翻译模型引用"
    );
    for path in [
        dir.join(&m.file),
        dir.join(format!("{}.json", m.id)),
        dir.join(format!("{}.part", m.file)),
    ] {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}
