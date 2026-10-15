//! Visible, deterministic Win32 test target. Only controls its own test window.
#![windows_subsystem = "windows"]
use std::{path::PathBuf, sync::OnceLock};
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
unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
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
                for (y, line) in [
                    (28, "GAME OCR TEST"),
                    (92, "Health: 100 / 100"),
                    (156, "Quest complete!"),
                    (220, "你好 世界 · こんにちは"),
                ] {
                    let text: Vec<u16> = line.encode_utf16().collect();
                    let _ = TextOutW(dc, 32, y, &text);
                }
                let mut rect=RECT::default();
                let _=GetClientRect(hwnd,&mut rect);
                if rect.right>=1280 {
                    for (x,y,line) in [(rect.right-600,70,"Inventory: 12 / 40"),(60,rect.bottom-140,"Press ENTER to continue"),(rect.right-600,rect.bottom-140,"Score: 12345")] {
                        let text:Vec<u16>=line.encode_utf16().collect();
                        let _=TextOutW(dc,x,y,&text);
                    }
                }
                SelectObject(dc, old);
                let _ = DeleteObject(HGDIOBJ(font.0));
                let _ = EndPaint(hwnd, &paint);
                LRESULT(0)
            }
            WM_TIMER => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                if let Some(path) = CONTROL.get() {
                    if let Ok(action) = std::fs::read_to_string(path) {
                        let _ = std::fs::remove_file(path);
                        match action.trim() {
                            "large" => {
                                let _=SetWindowPos(hwnd,None,0,0,1880,1020,SWP_NOMOVE|SWP_NOZORDER);
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
