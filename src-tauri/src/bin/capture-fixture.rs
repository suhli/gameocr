//! Visible, deterministic Win32 test target. Only controls its own test window.
#![windows_subsystem = "windows"]
use std::{
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::w,
};
static CONTROL: OnceLock<PathBuf> = OnceLock::new();
static NUMBER_CHANGED: AtomicBool = AtomicBool::new(false);
static TRANSLATION_TEXT: Mutex<Option<String>> = Mutex::new(None);
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let window_dc = BeginPaint(hwnd, &mut paint);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let dc = CreateCompatibleDC(Some(window_dc));
                let bitmap = CreateCompatibleBitmap(window_dc, rect.right, rect.bottom);
                let old_bitmap = SelectObject(dc, HGDIOBJ(bitmap.0));
                // Start each frame clean and publish it in one blit: WGC must not
                // observe accumulated antialiasing or a partially painted frame.
                let _ = FillRect(dc, &rect, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                let font = CreateFontW(
                    38,
                    0,
                    0,
                    0,
                    500,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    ANTIALIASED_QUALITY,
                    0,
                    w!("Segoe UI"),
                );
                let old = SelectObject(dc, HGDIOBJ(font.0));
                SetBkMode(dc, TRANSPARENT);
                let custom = TRANSLATION_TEXT.lock().unwrap().clone();
                let regular = [
                    (28, "GAME OCR TEST"),
                    (
                        92,
                        if NUMBER_CHANGED.load(Ordering::Relaxed) {
                            "Health: 101 / 100"
                        } else {
                            "Health: 100 / 100"
                        },
                    ),
                    (156, "Quest complete!"),
                    (220, "你好 世界 · こんにちは"),
                ];
                let custom_lines: Vec<_> = custom
                    .as_deref()
                    .unwrap_or("")
                    .lines()
                    .enumerate()
                    .map(|(i, line)| (28 + i as i32 * 64, line))
                    .collect();
                for &(y, line) in if custom.is_some() {
                    custom_lines.as_slice()
                } else {
                    regular.as_slice()
                } {
                    let text: Vec<u16> = line.encode_utf16().collect();
                    let _ = TextOutW(dc, 32, y, &text);
                }
                if rect.right >= 1280 {
                    for (x, y, line) in [
                        (rect.right - 600, 70, "Inventory: 12 / 40"),
                        (60, rect.bottom - 140, "Press ENTER to continue"),
                        (rect.right - 600, rect.bottom - 140, "Score: 12345"),
                    ] {
                        let text: Vec<u16> = line.encode_utf16().collect();
                        let _ = TextOutW(dc, x, y, &text);
                    }
                }
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(font.0));
                let _ = BitBlt(
                    window_dc,
                    0,
                    0,
                    rect.right,
                    rect.bottom,
                    Some(dc),
                    0,
                    0,
                    SRCCOPY,
                );
                SelectObject(dc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_TIMER => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                if let Some(path) = CONTROL.get() {
                    if let Ok(action) = std::fs::read_to_string(path) {
                        let _ = std::fs::remove_file(path);
                        if let Some(text) = action.strip_prefix("text:") {
                            *TRANSLATION_TEXT.lock().unwrap() = Some(text.to_string());
                            let _ = InvalidateRect(Some(hwnd), None, true);
                        }
                        match action.trim() {
                            "number" => {
                                NUMBER_CHANGED.store(true, Ordering::Relaxed);
                                let _ = InvalidateRect(Some(hwnd), None, true);
                            }
                            "large" => {
                                let _ = SetWindowPos(
                                    hwnd,
                                    None,
                                    0,
                                    0,
                                    1880,
                                    1020,
                                    SWP_NOMOVE | SWP_NOZORDER,
                                );
                            }
                            "resize" => {
                                let _ = SetWindowPos(
                                    hwnd,
                                    None,
                                    0,
                                    0,
                                    960,
                                    520,
                                    SWP_NOMOVE | SWP_NOZORDER,
                                );
                            }
                            "minimize" => {
                                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                            }
                            "restore" => {
                                let _ = ShowWindow(hwnd, SW_RESTORE);
                            }
                            "close" => {
                                let _ = DestroyWindow(hwnd);
                            }
                            _ => {}
                        }
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
fn main() -> anyhow::Result<()> {
    unsafe {
        if let Some(path) = std::env::args().nth(1) {
            let _ = CONTROL.set(PathBuf::from(path));
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = w!("GameOCRFixture");
        let wc = WNDCLASSW {
            hInstance: HINSTANCE(instance.0),
            lpszClassName: class,
            lpfnWndProc: Some(procedure),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        anyhow::ensure!(RegisterClassW(&wc) != 0, "RegisterClassW failed");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("GameOCR Capture Fixture"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            80,
            90,
            840,
            400,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )?;
        // CREATE_NO_WINDOW / STARTUPINFO from the test runner can hide the first show.
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);
        SetTimer(Some(hwnd), 1, 100, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        Ok(())
    }
}
