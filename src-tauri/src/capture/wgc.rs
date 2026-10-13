use super::CaptureShared;
use crate::window::WindowInfo;
use anyhow::Result;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows_capture::{
    capture::{Context, GraphicsCaptureApiHandler},
    frame::Frame,
    graphics_capture_api::InternalCaptureControl,
    settings::*,
    window::Window,
};
pub struct Flags {
    shared: Arc<CaptureShared>,
    generation: u64,
    interval: Duration,
}
struct Handler {
    flags: Flags,
    last: Option<Instant>,
    padding: Vec<u8>,
}
impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn new(ctx: Context<Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            flags: ctx.flags,
            last: None,
            padding: Vec::new(),
        })
    }
    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.flags.shared.generation.load(Ordering::Acquire) != self.flags.generation {
            control.stop();
            return Ok(());
        }
        if self.flags.shared.paused.load(Ordering::Acquire)
            || self.last.is_some_and(|t| t.elapsed() < self.flags.interval)
        {
            return Ok(());
        }
        self.last = Some(Instant::now());
        let buffer = frame.buffer()?;
        let (w, h) = (buffer.width(), buffer.height());
        if w == 0 || h == 0 || w as u64 * h as u64 > 64_000_000 {
            return Err("无效的捕获图像尺寸".into());
        }
        let data = buffer.as_nopadding_buffer(&mut self.padding);
        let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
        for px in data.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
        }
        if let Some(image) = image::RgbImage::from_raw(w, h, rgb) {
            self.flags.shared.publish(image, self.flags.generation);
        }
        Ok(())
    }
    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.flags.shared.fail("目标窗口已关闭，请重新选择窗口");
        Ok(())
    }
}
pub fn run(
    target: WindowInfo,
    fps: u32,
    shared: Arc<CaptureShared>,
    stop: Arc<AtomicBool>,
    generation: u64,
) -> Result<()> {
    let interval = Duration::from_secs_f64(1. / fps as f64);
    let settings = Settings::new(
        Window::from_raw_hwnd(target.handle()?.0),
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Exclude,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        Flags {
            shared: shared.clone(),
            generation,
            interval,
        },
    );
    let capture =
        Handler::start_free_threaded(settings).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    shared.set_status("running");
    while !stop.load(Ordering::Acquire) && !capture.is_finished() {
        if let Err(e) = target.validate() {
            let _ = capture.stop();
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    capture.stop().map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
