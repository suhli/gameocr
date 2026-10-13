use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbDevice {
    pub serial: String,
    pub status: String,
    pub description: String,
}
pub fn command(path: &str) -> Command {
    let mut cmd = Command::new(path);
    cmd.creation_flags(0x08000000).kill_on_drop(true);
    cmd
}
pub async fn run(path: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(12),
        command(path).args(args).stdin(Stdio::null()).output(),
    )
    .await
    .context("ADB 操作超时，检查设备连接")??;
    ensure!(
        output.status.success(),
        "ADB: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}
pub async fn devices(path: &str) -> Result<Vec<AdbDevice>> {
    let data = run(path, &["devices", "-l"])
        .await
        .context("无法执行 ADB，请在设置中选择 adb.exe 路径")?;
    Ok(String::from_utf8_lossy(&data)
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut cols = line.split_whitespace();
            let serial = cols.next()?.to_owned();
            let status = cols.next()?.to_owned();
            Some(AdbDevice {
                serial,
                status,
                description: cols.collect::<Vec<_>>().join(" "),
            })
        })
        .collect())
}
pub async fn screenshot(path: &str, serial: &str) -> Result<image::RgbImage> {
    ensure!(!serial.is_empty(), "请选择 ADB 设备");
    let bytes = run(path, &["-s", serial, "exec-out", "screencap", "-p"]).await?;
    ensure!(bytes.len() < 128 * 1024 * 1024, "ADB 截图过大");
    Ok(
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .context("ADB 未返回有效 PNG")?
            .to_rgb8(),
    )
}
