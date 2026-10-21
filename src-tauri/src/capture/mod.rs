pub mod adb;
mod wgc;
use crate::{settings::Settings, window::WindowInfo};
use anyhow::{Result, ensure};
use base64::Engine;
use gameocr_core::{latest::Latest, types::now_ms};
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Emitter;

pub use gameocr_core::parallel::FrameInput as Frame;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub frame_id: u64,
    pub generation: u64,
    pub width: u32,
    pub height: u32,
    pub data_url: String,
}
pub fn preview(frame: &Frame) -> Result<Preview> {
    let img = image::imageops::resize(
        &frame.image,
        frame.image.width().min(1280),
        ((frame.image.height() as f64 * 1280.0 / frame.image.width() as f64)
            .min(frame.image.height() as f64))
        .max(1.) as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 78).encode_image(&img)?;
    Ok(Preview {
        frame_id: frame.id,
        generation: frame.generation,
        width: frame.image.width(),
        height: frame.image.height(),
        data_url: format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ),
    })
}
/// Reuse an already published JPEG only when the complete source frame is
/// identical. OCR-region similarity is not sufficient for preview equivalence.
pub fn reuse_preview(previous: &Frame, current: &Frame, encoded: &Preview) -> Option<Preview> {
    if previous.generation != current.generation
        || encoded.generation != previous.generation
        || encoded.frame_id != previous.id
        || previous.image.dimensions() != current.image.dimensions()
        || (encoded.width, encoded.height) != current.image.dimensions()
        || previous.image.as_raw() != current.image.as_raw()
    {
        return None;
    }
    let mut result = encoded.clone();
    result.frame_id = current.id;
    Some(result)
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    fn reuse_keeps_current_identity_and_rejects_one_pixel_or_generation_change() {
        let previous = Frame {
            id: 1,
            timestamp: 1,
            generation: 1,
            image: image::RgbImage::new(40, 20),
            capture: Default::default(),
        };
        let mut current = previous.clone();
        current.id = 2;
        let encoded = preview(&previous).unwrap();
        let reused = reuse_preview(&previous, &current, &encoded).unwrap();
        assert_eq!(reused.frame_id, 2);
        assert_eq!(reused.data_url, encoded.data_url);
        current.image.put_pixel(20, 10, image::Rgb([1, 0, 0]));
        assert!(reuse_preview(&previous, &current, &encoded).is_none());
        current = previous.clone();
        current.generation = 2;
        assert!(reuse_preview(&previous, &current, &encoded).is_none());
    }
}
pub struct CaptureShared {
    pub latest: Mutex<Latest<Arc<Frame>>>,
    pub preview_frame: Mutex<Option<Arc<Frame>>>,
    pub status: Mutex<String>,
    pub generation: AtomicU64,
    pub sequence: AtomicU64,
    pub captured: AtomicU64,
    pub paused: AtomicBool,
    pub app: tauri::AppHandle,
    pub transition: Mutex<()>,
}
impl CaptureShared {
    pub fn set_status(&self, status: &str) {
        *self.status.lock() = status.into();
        let _ = self.app.emit("capture-status", status);
    }
    pub fn invalidate(&self) {
        let _gate = self.transition.lock();
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.latest.lock().clear();
        *self.preview_frame.lock() = None;
    }
    pub fn fail(&self, error: &str) {
        self.invalidate();
        self.set_status("error");
        let _ = self.app.emit("capture-error", error);
    }
    pub fn publish(&self, image: image::RgbImage, generation: u64) {
        self.publish_timed(image, generation, Default::default());
    }
    pub fn publish_timed(
        &self,
        image: image::RgbImage,
        generation: u64,
        mut capture: gameocr_core::parallel::CaptureTiming,
    ) {
        let _gate = self.transition.lock();
        capture.ready = Some(Instant::now());
        if self.paused.load(Ordering::Acquire)
            || self.generation.load(Ordering::Acquire) != generation
        {
            return;
        }
        let frame = Arc::new(Frame {
            id: self.sequence.fetch_add(1, Ordering::Relaxed),
            timestamp: now_ms(),
            generation,
            image,
            capture,
        });
        self.latest.lock().replace(frame.clone());
        *self.preview_frame.lock() = Some(frame);
        self.captured.fetch_add(1, Ordering::Relaxed);
    }
}
pub struct CaptureManager {
    pub shared: Arc<CaptureShared>,
    session: Option<(Arc<AtomicBool>, std::thread::JoinHandle<()>)>,
}
impl CaptureManager {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            shared: Arc::new(CaptureShared {
                latest: Mutex::new(Latest::default()),
                preview_frame: Mutex::new(None),
                status: Mutex::new("idle".into()),
                generation: AtomicU64::new(1),
                sequence: AtomicU64::new(1),
                captured: AtomicU64::new(0),
                paused: AtomicBool::new(false),
                app,
                transition: Mutex::new(()),
            }),
            session: None,
        }
    }
    pub fn start(&mut self, settings: Settings, window: Option<WindowInfo>) -> Result<()> {
        self.stop();
        self.shared.set_status("starting");
        self.shared.paused.store(false, Ordering::Release);
        let generation = self.shared.generation.load(Ordering::Acquire);
        let shared = self.shared.clone();
        let quit = Arc::new(AtomicBool::new(false));
        let stop = quit.clone();
        if settings.capture_source == "window" {
            let target = window.ok_or_else(|| anyhow::anyhow!("请先选择窗口"))?;
            target.validate()?;
            self.session = Some((
                quit,
                std::thread::Builder::new()
                    .name("wgc-controller".into())
                    .spawn(move || {
                        if let Err(e) = wgc::run(
                            target,
                            settings.capture_fps,
                            shared.clone(),
                            stop,
                            generation,
                        ) {
                            shared.fail(&format!("WGC 捕获失败：{e:#}"));
                        }
                    })?,
            ));
        } else {
            ensure!(!settings.adb_serial.is_empty(), "请选择 ADB 设备");
            self.session=Some((quit,std::thread::Builder::new().name("adb-capture".into()).spawn(move||{
                let runtime=match tokio::runtime::Builder::new_current_thread().enable_all().build() {Ok(r)=>r,Err(e)=>{shared.fail(&e.to_string());return;}};
                runtime.block_on(async move {
                    shared.set_status("running"); let interval=Duration::from_secs_f64(1./settings.capture_fps as f64);
                    while !stop.load(Ordering::Acquire) {
                        let start=Instant::now();
                        if !shared.paused.load(Ordering::Acquire) {
                            let capture=adb::screenshot(&settings.adb_path,&settings.adb_serial);tokio::pin!(capture);
                            let response=loop {tokio::select! {r=&mut capture=>break Some(r),_=tokio::time::sleep(Duration::from_millis(50))=>{if stop.load(Ordering::Acquire){break None;}}}};
                            match response {Some(Ok(image))=>shared.publish(image,generation),Some(Err(e))=>{shared.fail(&format!("ADB 设备断开或截图失败：{e:#}；连接恢复后点击开始"));break;},None=>break}
                        }
                        // Sleep the entire remaining interval, checking cancellation in bounded steps.
                        while start.elapsed()<interval&&!stop.load(Ordering::Acquire) {
                            tokio::time::sleep(interval.saturating_sub(start.elapsed()).min(Duration::from_millis(50))).await;
                        }
                    }
                });
            })?));
        }
        Ok(())
    }
    pub fn stop(&mut self) {
        self.shared.invalidate();
        if let Some((stop, join)) = self.session.take() {
            self.shared.set_status("stopping");
            stop.store(true, Ordering::Release);
            let _ = join.join();
        }
        self.shared.latest.lock().clear();
        *self.shared.preview_frame.lock() = None;
        self.shared.set_status("idle");
    }
    pub fn pause(&self) -> Result<()> {
        let _gate = self.shared.transition.lock();
        ensure!(*self.shared.status.lock() == "running", "捕获未运行");
        self.shared.paused.store(true, Ordering::Release);
        self.shared.latest.lock().clear();
        self.shared.set_status("paused");
        Ok(())
    }
    pub fn resume(&self) -> Result<()> {
        ensure!(*self.shared.status.lock() == "paused", "捕获未暂停");
        self.shared.paused.store(false, Ordering::Release);
        self.shared.set_status("running");
        Ok(())
    }
}
impl Drop for CaptureManager {
    fn drop(&mut self) {
        self.stop();
    }
}
