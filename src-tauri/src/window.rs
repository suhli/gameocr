use anyhow::{Result, ensure};
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        Storage::FileSystem::*,
        System::Threading::*,
        UI::Shell::*,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, PWSTR},
};

static DESTROYED: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
static EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: String,
    pub hwnd: String,
    pub pid: u32,
    pub process_name: String,
    pub title: String,
    pub executable_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
    pub minimized: bool,
    pub capturable: bool,
    pub icon: Option<String>,
    pub created: u64,
    pub epoch: u64,
    pub class_name: String,
}
impl WindowInfo {
    pub fn handle(&self) -> Result<HWND> {
        Ok(HWND(usize::from_str_radix(&self.hwnd, 16)? as *mut _))
    }
    pub fn validate(&self) -> Result<()> {
        let current = inspect(self.handle()?, false)
            .ok_or_else(|| anyhow::anyhow!("目标窗口已关闭或无法访问"))?;
        ensure!(
            same_identity(self, &current),
            "窗口已销毁或重新创建，请重新选择目标"
        );
        ensure!(
            !current.minimized,
            "目标窗口已最小化，请恢复窗口后重新开始捕获"
        );
        ensure!(current.capturable, "窗口当前不可捕获");
        Ok(())
    }
}
fn same_identity(a: &WindowInfo, b: &WindowInfo) -> bool {
    a.hwnd == b.hwnd
        && a.pid == b.pid
        && a.created == b.created
        && a.epoch == b.epoch
        && a.class_name == b.class_name
}

pub fn start_identity_monitor() -> Result<()> {
    if DESTROYED.get().is_some() {
        return Ok(());
    }
    let _ = DESTROYED.set(Mutex::new(HashMap::new()));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("window-identity".into())
        .spawn(move || unsafe {
            let hook = SetWinEventHook(
                EVENT_OBJECT_DESTROY,
                EVENT_OBJECT_DESTROY,
                None,
                Some(on_destroy),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            let ok = !hook.0.is_null();
            let _ = tx.send(ok);
            if !ok {
                return;
            }
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            let _ = UnhookWinEvent(hook);
        })?;
    ensure!(
        rx.recv_timeout(std::time::Duration::from_secs(5))?,
        "无法监听窗口生命周期"
    );
    Ok(())
}
unsafe extern "system" fn on_destroy(
    _: HWINEVENTHOOK,
    _: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if object == 0 && child == 0 {
        if let Some(map) = DESTROYED.get() {
            let mut map = map.lock();
            if let Some(epoch) = map.get_mut(&(hwnd.0 as usize)) {
                *epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}
pub fn list(show_all: bool) -> Result<Vec<WindowInfo>> {
    let mut handles = Vec::<usize>::new();
    unsafe {
        EnumWindows(Some(enum_callback), LPARAM(&mut handles as *mut _ as isize))?;
    }
    let mut windows: Vec<_> = handles
        .into_iter()
        .filter_map(|h| inspect(HWND(h as *mut _), true))
        .filter(|w| {
            show_all
                || (!w.title.is_empty()
                    && w.visible
                    && w.pid != std::process::id()
                    && w.width > 80
                    && w.height > 40)
        })
        .collect();
    windows.sort_by_key(|w| (!w.capturable, w.process_name.clone(), w.title.clone()));
    // Retain only observed/selected HWNDs; bound the lifecycle registry to desktop window count.
    if let Some(map) = DESTROYED.get() {
        map.lock()
            .retain(|h, _| unsafe { IsWindow(Some(HWND(*h as *mut _))).as_bool() });
    }
    Ok(windows)
}
unsafe extern "system" fn enum_callback(hwnd: HWND, data: LPARAM) -> windows::core::BOOL {
    unsafe {
        (&mut *(data.0 as *mut Vec<usize>)).push(hwnd.0 as usize);
    }
    TRUE
}
fn inspect(hwnd: HWND, with_icon: bool) -> Option<WindowInfo> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let mut title = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let len = GetWindowTextW(hwnd, &mut title);
        let title = String::from_utf16_lossy(&title[..len.max(0) as usize]);
        let mut class = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class);
        let class_name = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        if ["Progman", "WorkerW", "Shell_TrayWnd"].contains(&class_name.as_str()) {
            return None;
        }
        let mut rect = RECT::default();
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .or_else(|_| GetWindowRect(hwnd, &mut rect));
        let visible = IsWindowVisible(hwnd).as_bool();
        let minimized = IsIconic(hwnd).as_bool();
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        let mut path = None;
        let mut created = 0;
        if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buffer = vec![0u16; 32768];
            let mut size = buffer.len() as u32;
            if QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(buffer.as_mut_ptr()),
                &mut size,
            )
            .is_ok()
            {
                path = Some(String::from_utf16_lossy(&buffer[..size as usize]));
            }
            let (mut c, mut e, mut k, mut u) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            if GetProcessTimes(process, &mut c, &mut e, &mut k, &mut u).is_ok() {
                created = (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64;
            }
            let _ = CloseHandle(process);
        }
        let process_name = path
            .as_ref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("PID {pid}"));
        let epoch = DESTROYED
            .get()
            .map(|m| {
                *m.lock()
                    .entry(hwnd.0 as usize)
                    .or_insert_with(|| EPOCH.fetch_add(1, Ordering::Relaxed))
            })
            .unwrap_or(0);
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        let icon = if with_icon {
            path.as_deref().and_then(|p| file_icon(p).ok())
        } else {
            None
        };
        Some(WindowInfo {
            id: format!("{:x}:{pid}:{created}:{epoch}", hwnd.0 as usize),
            hwnd: format!("{:x}", hwnd.0 as usize),
            pid,
            process_name,
            title,
            executable_path: path,
            width,
            height,
            visible,
            minimized,
            capturable: visible
                && !minimized
                && cloaked == 0
                && created > 0
                && width > 0
                && height > 0,
            icon,
            created,
            epoch,
            class_name,
        })
    }
}
fn file_icon(path: &str) -> Result<String> {
    unsafe {
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let mut info = SHFILEINFOW::default();
        ensure!(
            SHGetFileInfoW(
                PCWSTR(wide.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut info),
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON | SHGFI_SMALLICON
            ) != 0,
            "无图标"
        );
        let result = (|| -> Result<String> {
            let dc = CreateCompatibleDC(None);
            ensure!(!dc.0.is_null(), "创建图标 DC 失败");
            let mut bi = BITMAPINFO::default();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: 32,
                biHeight: -32,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(e.into());
                }
            };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            std::ptr::write_bytes(bits as *mut u8, 0, 4096);
            let draw = DrawIconEx(dc, 0, 0, info.hIcon, 32, 32, 0, None, DI_NORMAL);
            let data = std::slice::from_raw_parts(bits as *const u8, 4096).to_vec();
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            draw?;
            let mut rgba = data;
            for px in rgba.chunks_exact_mut(4) {
                px.swap(0, 2);
                if px[3] == 0 && px[..3].iter().any(|v| *v != 0) {
                    px[3] = 255;
                }
            }
            let image = image::RgbaImage::from_raw(32, 32, rgba).unwrap();
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png)?;
            Ok(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
            ))
        })();
        let _ = DestroyIcon(info.hIcon);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_rejects_reused_handle() {
        let mut a = WindowInfo {
            id: "x".into(),
            hwnd: "42".into(),
            pid: 10,
            process_name: "test".into(),
            title: "test".into(),
            executable_path: None,
            width: 100,
            height: 100,
            visible: true,
            minimized: false,
            capturable: true,
            icon: None,
            created: 50,
            epoch: 1,
            class_name: "class".into(),
        };
        let b = a.clone();
        assert!(same_identity(&a, &b));
        a.epoch += 1;
        assert!(!same_identity(&a, &b));
        a.epoch = b.epoch;
        a.created += 1;
        assert!(!same_identity(&a, &b));
    }
}
