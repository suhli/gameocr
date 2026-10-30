pub mod state;
use crate::{service::Service, window::OverlayGeometry};
use anyhow::{Context, Result, ensure};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::json;
use state::{LiveState, OverlaySettings, Subtitle, WindowAnchor};
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
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RendererPhase {
    Mounted,
    ViewportWait,
    Error,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererReport {
    phase: RendererPhase,
    serial: Option<u64>,
    width: u32,
    height: u32,
    scale: f64,
    message: Option<String>,
}
pub struct OverlayManager {
    app: tauri::AppHandle,
    pub settings: RwLock<OverlaySettings>,
    live: Mutex<LiveState>,
    snapshot: Mutex<OverlaySnapshot>,
    rendered: Mutex<u64>,
    presented: Mutex<(u64, bool)>,
    display_reason: Mutex<Option<(u64, String)>>,
    last_state_log: Mutex<serde_json::Value>,
    last_native_log: Mutex<serde_json::Value>,
    renderer_report: Mutex<Option<RendererReport>>,
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
        crate::diagnostics::event(
            "info",
            "overlay.settings_loaded",
            json!({"settings":settings,"warning":reason}),
        );
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
            display_reason: Mutex::new(None),
            last_state_log: Mutex::new(serde_json::Value::Null),
            last_native_log: Mutex::new(serde_json::Value::Null),
            renderer_report: Mutex::new(None),
            notify: Default::default(),
        })
    }
    pub fn update(&self, settings: OverlaySettings) -> Result<bool> {
        settings.validate()?;
        crate::settings::atomic_json(
            &crate::settings::data_dir().join("overlay/settings.json"),
            &settings,
        )?;
        let changed_enable = self.settings.read().overlay_enabled != settings.overlay_enabled;
        let just_enabled = changed_enable && settings.overlay_enabled;
        *self.settings.write() = settings.clone();
        crate::diagnostics::event(
            "info",
            "overlay.settings_updated",
            json!({"settings":settings}),
        );
        if changed_enable {
            self.clear();
        }
        let _ = self
            .app
            .emit_to("main", "overlay-settings-changed", &settings);
        self.notify.notify_one();
        Ok(just_enabled)
    }
    pub fn snapshot(&self) -> OverlaySnapshot {
        let mut snapshot = self.snapshot.lock().clone();
        if let Some((serial, reason)) = self.display_reason.lock().as_ref() {
            if *serial == snapshot.serial {
                snapshot.reason = reason.clone();
            }
        }
        snapshot
    }
    fn publish_status(&self, serial: u64, reason: &str) {
        let snapshot = self.snapshot.lock();
        if snapshot.serial != serial {
            return;
        }
        let mut previous = self.display_reason.lock();
        let next = (serial, reason.to_owned());
        if previous.as_ref() != Some(&next) {
            let _ = self.app.emit_to("main", "overlay-status", reason);
            *previous = Some(next);
        }
    }
    pub fn renderer_report(&self, mut report: RendererReport) {
        report.message = report.message.map(crate::diagnostics::short_error);
        let mut previous = self.renderer_report.lock();
        if previous.as_ref() != Some(&report) {
            crate::diagnostics::event(
                if report.phase == RendererPhase::Error {
                    "error"
                } else {
                    "info"
                },
                "overlay.renderer",
                json!(&report),
            );
            *previous = Some(report.clone());
        }
        drop(previous);
        {
            let serial = report.serial.unwrap_or_else(|| self.snapshot.lock().serial);
            match report.phase {
                RendererPhase::ViewportWait => self.publish_status(serial, "等待字幕窗口尺寸同步"),
                RendererPhase::Error => self.publish_status(serial, "字幕绘制失败，请查看本地日志"),
                RendererPhase::Mounted => {}
            }
        }
    }
    pub fn rendered(&self, serial: u64) {
        // A renderer can acknowledge only the newest snapshot. A late response
        // may never resurrect a hidden/invalidated subtitle window.
        if self.snapshot.lock().serial == serial {
            let mut rendered = self.rendered.lock();
            if *rendered != serial {
                crate::diagnostics::event("info", "overlay.render_ack", json!({"serial":serial}));
                *rendered = serial;
            }
            self.notify.notify_one();
        }
    }
    fn hide(&self) {
        if let Some(window) = self.app.get_webview_window("overlay") {
            if let Err(error) = window.hide() {
                crate::diagnostics::event(
                    "error",
                    "overlay.hide_failed",
                    json!({"message":crate::diagnostics::short_error(error)}),
                );
            }
        }
    }
    pub fn clear(&self) {
        self.live.lock().clear();
        self.invalidate_view();
    }
    fn invalidate_view(&self) {
        let mut snapshot = self.snapshot.lock();
        if snapshot.visible || !snapshot.items.is_empty() {
            crate::diagnostics::event(
                "info",
                "overlay.invalidated",
                json!({"serial":snapshot.serial,"identity":snapshot.identity}),
            );
        }
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
        observed: Instant,
    ) {
        if !self.settings.read().overlay_enabled {
            return;
        }
        let target = self
            .app
            .try_state::<Arc<Service>>()
            .and_then(|s| s.selected.lock().as_ref().map(|w| w.id.clone()));
        if !self.live.lock().observe(ocr, identity, target, observed) {
            self.invalidate_view();
        }
        self.notify.notify_one();
    }
    pub fn accept(&self, event: &crate::translation::TranslationEvent) {
        let enabled = self.settings.read().overlay_enabled;
        let mut live = self.live.lock();
        let accepted = enabled && live.accept(event);
        crate::diagnostics::event(
            "info",
            "overlay.translation_received",
            json!({
                "identity":event.identity,"enabled":enabled,"accepted":accepted,
                "results":event.results.len(),"subtitles":live.items.len(),
                "complete":event.results.iter().filter(|r| r.status == "complete").count(),
                "failed":event.results.iter().filter(|r| r.error.is_some()).count(),
                "observedAgeMs":live.observed.map(|t| t.elapsed().as_millis())
            }),
        );
        drop(live);
        if accepted {
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
        if setup.is_ok() {
            crate::diagnostics::event(
                "info",
                "overlay.window_created",
                json!({"captureExcluded":true,"clickThrough":true,"nonActivating":true}),
            );
        }
        setup
    }
    fn sync(self: &Arc<Self>, service: &Service, error: Option<&str>, anchor: &mut WindowAnchor) {
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
        } else if current && !live.fresh(&settings, Instant::now()) {
            "字幕已过期，请重新识别当前画面"
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
                        eligible = anchor.allows(&target.id, &g, settings.overlay_follow_window);
                        reason = if eligible {
                            "等待字幕窗口绘制"
                        } else if !g.visible {
                            "目标窗口不可见或已最小化"
                        } else if !g.foreground {
                            "请切回被捕获的游戏窗口，后台时字幕自动隐藏"
                        } else {
                            "目标窗口已移动，请开启跟随游戏窗口"
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
            anchor.clear();
        }
        let state_log = json!({"reason":reason,"eligible":eligible,"capture":capture,"source":source,
            "current":current,"targetId":live.target_id,"sourceWidth":live.source_width,
            "sourceHeight":live.source_height,"subtitles":live.items.len(),"geometry":geometry});
        if *self.last_state_log.lock() != state_log {
            crate::diagnostics::event(
                "info",
                "overlay.state",
                json!({"state":state_log,"identity":live.identity,
                "observedAgeMs":live.observed.map(|t| t.elapsed().as_millis())}),
            );
            *self.last_state_log.lock() = state_log;
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
        let changed = next != *previous;
        if changed {
            next.serial += 1;
            *previous = next.clone();
            if let Err(error) = self.app.emit_to("overlay", "overlay-state", &next) {
                crate::diagnostics::event(
                    "error",
                    "overlay.emit_failed",
                    json!({"serial":next.serial,"message":crate::diagnostics::short_error(error)}),
                );
            }
        }
        drop(previous);
        drop(live);
        if changed {
            self.publish_status(next.serial, &next.reason);
        }
        let ready = eligible && *self.rendered.lock() == next.serial;
        let stamp = (next.serial, ready);
        if *self.presented.lock() == stamp {
            return;
        }
        *self.presented.lock() = stamp;
        let manager = self.clone();
        // Validate the serial again on the native UI thread. A stop/new-text
        // invalidation can overtake an already queued positioning/show request.
        let serial = next.serial;
        if let Err(error) = self.app.run_on_main_thread(move || {
            if manager.snapshot.lock().serial != next.serial {
                return;
            }
            let Some(window) = manager.app.get_webview_window("overlay") else {
                if eligible { manager.publish_status(next.serial, "字幕窗口尚未创建，请关闭后重新开启游戏字幕"); }
                return;
            };
            let result = (|| -> Result<bool> {
                if !eligible {
                    window.hide()?;
                    return Ok(window.is_visible()?);
                }
                let g = next.geometry.as_ref().context("字幕窗口缺少目标坐标")?;
                let position = tauri::PhysicalPosition::new(g.rect[0], g.rect[1]);
                let size = tauri::PhysicalSize::new(
                    (g.rect[2] - g.rect[0]) as u32,
                    (g.rect[3] - g.rect[1]) as u32,
                );
                if window.outer_position().ok() != Some(position) {
                    window.set_position(position)?;
                }
                if window.inner_size().ok() != Some(size) {
                    window.set_size(size)?;
                }
                if ready && !window.is_visible().unwrap_or(false) {
                    // Show without activating, including when the game is foreground.
                    use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};
                    unsafe {
                        // ShowWindow's return value is the previous visibility, not success.
                        let _ = ShowWindow(HWND(window.hwnd()?.0), SW_SHOWNOACTIVATE);
                    }
                }
                let visible = window.is_visible()?;
                if ready { ensure!(visible, "系统未显示字幕窗口"); }
                Ok(visible)
            })();
            let native_log = json!({"eligible":eligible,"rendered":ready,
                "nativeVisible":result.as_ref().ok(),"position":window.outer_position().ok(),"size":window.inner_size().ok(),
                "error":result.as_ref().err().map(crate::diagnostics::short_error)});
            if *manager.last_native_log.lock() != native_log {
                crate::diagnostics::event(if result.is_err() {"error"} else {"info"}, "overlay.native", json!({"serial":next.serial,"result":native_log}));
                *manager.last_native_log.lock() = native_log;
            }
            match result {
                Err(error) => manager.publish_status(next.serial, &format!("字幕窗口操作失败：{}", crate::diagnostics::short_error(error))),
                Ok(true) if ready => manager.publish_status(next.serial, "游戏字幕窗口已显示"),
                _ => {},
            }
        }) {
            crate::diagnostics::event("error", "overlay.dispatch_failed", json!({"serial":serial,"message":crate::diagnostics::short_error(error)}));
            self.publish_status(serial, "字幕窗口调度失败，请查看本地日志");
        }
    }
    pub fn launch(self: &Arc<Self>, service: Weak<Service>) {
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut attempted = false;
            let mut error = None;
            let mut anchor = WindowAnchor::default();
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
                    if let Some(error) = &error {
                        crate::diagnostics::event(
                            "error",
                            "overlay.create_failed",
                            json!({"message":crate::diagnostics::short_error(error)}),
                        );
                    }
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
