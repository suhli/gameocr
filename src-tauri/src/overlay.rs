pub mod state;
use crate::{service::Service, window::OverlayGeometry};
use anyhow::{Context, Result, ensure};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use state::{LiveState, OverlaySettings, Subtitle};
use std::{
    sync::{Arc, Weak, atomic::Ordering},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};

#[derive(Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySnapshot {
    pub serial: u64,
    pub settings: OverlaySettings,
    pub identity: Option<gameocr_translation::types::Identity>,
    pub target_id: Option<String>,
    pub source_width: u32,
    pub source_height: u32,
    pub geometry: Option<OverlayGeometry>,
    pub items: Vec<Subtitle>,
    pub visible: bool,
    pub reason: String,
}
pub struct OverlayManager {
    app: tauri::AppHandle,
    pub settings: RwLock<OverlaySettings>,
    live: Mutex<LiveState>,
    snapshot: Mutex<OverlaySnapshot>,
    rendered: Mutex<u64>,
    presented: Mutex<(u64, bool)>,
    notify: tokio::sync::Notify,
}
impl OverlayManager {
    pub fn new(app: tauri::AppHandle) -> Arc<Self> {
        let path = crate::settings::data_dir().join("overlay/settings.json");
        let loaded = std::fs::read(&path)
            .ok()
            .map(|bytes| serde_json::from_slice::<OverlaySettings>(&bytes));
        let (settings, reason) = match loaded {
            Some(Ok(s)) if s.validate().is_ok() => (s, String::new()),
            Some(_) => (
                OverlaySettings::default(),
                "字幕设置无效，已使用安全默认值；原文件保留".into(),
            ),
            None => (OverlaySettings::default(), String::new()),
        };
        Arc::new(Self {
            app,
            snapshot: Mutex::new(OverlaySnapshot {
                serial: 0,
                settings: settings.clone(),
                identity: None,
                target_id: None,
                source_width: 0,
                source_height: 0,
                geometry: None,
                items: vec![],
                visible: false,
                reason,
            }),
            settings: RwLock::new(settings),
            live: Mutex::new(LiveState::default()),
            rendered: Mutex::new(0),
            presented: Mutex::new((0, false)),
            notify: Default::default(),
        })
    }
    pub fn update(&self, settings: OverlaySettings) -> Result<()> {
        settings.validate()?;
        crate::settings::atomic_json(
            &crate::settings::data_dir().join("overlay/settings.json"),
            &settings,
        )?;
        let changed_enable = self.settings.read().overlay_enabled != settings.overlay_enabled;
        *self.settings.write() = settings;
        if changed_enable {
            self.clear();
        }
        self.notify.notify_one();
        Ok(())
    }
    pub fn snapshot(&self) -> OverlaySnapshot {
        self.snapshot.lock().clone()
    }
    pub fn rendered(&self, serial: u64) {
        // A renderer can acknowledge only the newest snapshot. A late response
        // may never resurrect a hidden/invalidated subtitle window.
        if self.snapshot.lock().serial == serial {
            *self.rendered.lock() = serial;
            self.notify.notify_one();
        }
    }
    fn hide(&self) {
        if let Some(window) = self.app.get_webview_window("overlay") {
            let _ = window.hide();
        }
    }
    pub fn clear(&self) {
        self.live.lock().clear();
        self.invalidate_view();
    }
    fn invalidate_view(&self) {
        let mut snapshot = self.snapshot.lock();
        snapshot.serial += 1;
        snapshot.visible = false;
        snapshot.items.clear();
        snapshot.identity = None;
        let _ = self.app.emit_to("overlay", "overlay-state", &*snapshot);
        drop(snapshot);
        self.hide();
        *self.rendered.lock() = 0;
        self.notify.notify_one();
    }
    pub fn observe(
        &self,
        ocr: &gameocr_core::types::OcrResult,
        identity: &gameocr_translation::types::Identity,
    ) {
        if !self.settings.read().overlay_enabled {
            return;
        }
        let target = self
            .app
            .try_state::<Arc<Service>>()
            .and_then(|s| s.selected.lock().as_ref().map(|w| w.id.clone()));
        if !self
            .live
            .lock()
            .observe(ocr, identity, target, Instant::now())
        {
            self.invalidate_view();
        }
        self.notify.notify_one();
    }
    pub fn accept(&self, event: &crate::translation::TranslationEvent) {
        if self.settings.read().overlay_enabled && self.live.lock().accept(event) {
            self.notify.notify_one();
        }
    }
    fn create_window(&self) -> Result<()> {
        if self.app.get_webview_window("overlay").is_some() {
            return Ok(());
        }
        let window = tauri::WebviewWindowBuilder::new(
            &self.app,
            "overlay",
            tauri::WebviewUrl::App("index.html?overlay".into()),
        )
        .title("GameOCR 翻译字幕")
        .decorations(false)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .focusable(false)
        .visible(false)
        .inner_size(800., 600.)
        .build()?;
        let setup = (|| -> Result<()> {
            window.set_ignore_cursor_events(true)?;
            window.set_focusable(false)?;
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::*;
            unsafe {
                let hwnd = HWND(window.hwnd()?.0);
                let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                SetWindowLongPtrW(
                    hwnd,
                    GWL_EXSTYLE,
                    style | WS_EX_NOACTIVATE.0 as isize | WS_EX_TOOLWINDOW.0 as isize,
                );
                let actual = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                ensure!(
                    actual & WS_EX_NOACTIVATE.0 as isize != 0
                        && actual & WS_EX_TRANSPARENT.0 as isize != 0
                        && actual & WS_EX_LAYERED.0 as isize != 0,
                    "无法启用系统级点击穿透"
                );
                // Applied only to the HWND just created by this application.
                SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
                    .context("系统不支持字幕捕获排除；为避免反馈循环，已停用字幕窗口")?;
                let mut affinity = 0;
                GetWindowDisplayAffinity(hwnd, &mut affinity)?;
                ensure!(affinity == WDA_EXCLUDEFROMCAPTURE.0, "字幕捕获排除未生效");
            }
            Ok(())
        })();
        if setup.is_err() {
            let _ = window.destroy();
        }
        setup
    }
    fn sync(
        self: &Arc<Self>,
        service: &Service,
        error: Option<&str>,
        anchor: &mut Option<[i32; 4]>,
    ) {
        let settings = self.settings.read().clone();
        let source = service.settings.read().capture_source.clone();
        let selected = service.selected.lock().clone();
        let capture = service.shared.status.lock().clone();
        let mut live = self.live.lock();
        let current = live.identity.as_ref().is_some_and(|id| {
            id.generation == service.shared.generation.load(Ordering::Acquire)
                && id.epoch == service.ocr_epoch.load(Ordering::Acquire)
        }) && live.target_id.as_deref() == selected.as_ref().map(|w| w.id.as_str());
        if !settings.overlay_enabled || source != "window" || capture != "running" || !current {
            live.clear();
        }
        let mut geometry = None;
        let mut reason = if !settings.overlay_enabled {
            "字幕已关闭"
        } else if source != "window" {
            "字幕仅支持 Windows 窗口捕获"
        } else if capture != "running" {
            "等待捕获运行"
        } else {
            "等待当前完整译文"
        }
        .to_string();
        let mut eligible = settings.overlay_enabled
            && current
            && capture == "running"
            && source == "window"
            && live.fresh(&settings, Instant::now())
            && !live.items.is_empty();
        if let Some(error) = error {
            eligible = false;
            reason = error.into();
        }
        if eligible {
            if let Some(target) = selected {
                match crate::window::overlay_geometry(
                    &target,
                    live.source_width,
                    live.source_height,
                ) {
                    Ok(g) => {
                        if anchor.is_none() {
                            *anchor = Some(g.rect);
                        }
                        eligible = g.foreground
                            && g.visible
                            && (settings.overlay_follow_window || *anchor == Some(g.rect));
                        reason = if eligible {
                            "半透明磨砂视觉；仅 WebView 内部 blur"
                        } else {
                            "目标在后台、不可见或已移动"
                        }
                        .into();
                        geometry = Some(g);
                    }
                    Err(e) => {
                        eligible = false;
                        reason = e.to_string();
                    }
                }
            } else {
                eligible = false;
            }
        }
        if !settings.overlay_enabled || !current {
            *anchor = None;
        }
        let mut next = OverlaySnapshot {
            serial: 0,
            settings,
            identity: live.identity.clone(),
            target_id: live.target_id.clone(),
            source_width: live.source_width,
            source_height: live.source_height,
            geometry,
            items: if eligible { live.items.clone() } else { vec![] },
            visible: eligible,
            reason,
        };
        let mut previous = self.snapshot.lock();
        next.serial = previous.serial;
        if next != *previous {
            next.serial += 1;
            *previous = next.clone();
            let _ = self.app.emit_to("overlay", "overlay-state", &next);
            let _ = self.app.emit_to("main", "overlay-status", &next.reason);
        }
        drop(previous);
        drop(live);
        let ready = eligible && *self.rendered.lock() == next.serial;
        let stamp = (next.serial, ready);
        if *self.presented.lock() == stamp {
            return;
        }
        *self.presented.lock() = stamp;
        let manager = self.clone();
        // Validate the serial again on the native UI thread. A stop/new-text
        // invalidation can overtake an already queued positioning/show request.
        let _ = self.app.run_on_main_thread(move || {
            if manager.snapshot.lock().serial != next.serial {
                return;
            }
            let Some(window) = manager.app.get_webview_window("overlay") else {
                return;
            };
            if !eligible {
                let _ = window.hide();
                return;
            }
            if let Some(g) = &next.geometry {
                let position = tauri::PhysicalPosition::new(g.rect[0], g.rect[1]);
                let size = tauri::PhysicalSize::new(
                    (g.rect[2] - g.rect[0]) as u32,
                    (g.rect[3] - g.rect[1]) as u32,
                );
                if window.outer_position().ok() != Some(position) {
                    let _ = window.set_position(position);
                }
                if window.inner_size().ok() != Some(size) {
                    let _ = window.set_size(size);
                }
                if ready && !window.is_visible().unwrap_or(false) {
                    // Show without activating, including when the game is foreground.
                    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
                    if let Ok(hwnd) = window.hwnd() {
                        unsafe {
                            let _ = ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE);
                        }
                    }
                }
            }
        });
    }
    pub fn launch(self: &Arc<Self>, service: Weak<Service>) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut attempted = false;
            let mut error = None;
            let mut anchor = None;
            loop {
                let Some(service) = service.upgrade() else {
                    break;
                };
                if service.shutdown.load(Ordering::Acquire) {
                    break;
                }
                let enabled = manager.settings.read().overlay_enabled;
                if enabled && !attempted {
                    attempted = true;
                    error = manager.create_window().err().map(|e| format!("{e:#}"));
                }
                if !enabled {
                    attempted = false;
                    error = None;
                }
                manager.sync(&service, error.as_deref(), &mut anchor);
                drop(service);
                tokio::select! { _ = manager.notify.notified() => {}, _ = tokio::time::sleep(Duration::from_millis(if enabled {50} else {500})) => {} }
            }
        });
    }
    pub fn close(&self) {
        self.clear();
        if let Some(window) = self.app.get_webview_window("overlay") {
            let _ = window.destroy();
        }
        self.notify.notify_one();
    }
}
