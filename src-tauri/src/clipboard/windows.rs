//! Windows：AddClipboardFormatListener + 消息专用窗口监听 WM_CLIPBOARDUPDATE。
//! 这是 Windows 官方推荐的事件式剪切板监听方式，无需轮询。

use std::sync::{Arc, Mutex};

use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::AddClipboardFormatListener;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
};

use crate::state::AppState;

const WM_CLIPBOARDUPDATE: u32 = 0x031D;

static STATE: Mutex<Option<Arc<AppState>>> = Mutex::new(None);

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        let guard = STATE.lock().unwrap();
        if let Some(state) = guard.as_ref() {
            if let Some(content) = crate::clipboard::read_clipboard() {
                crate::clipboard::handle_local_clipboard(state, &content);
            }
        }
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

pub fn spawn(state: Arc<AppState>) {
    *STATE.lock().unwrap() = Some(state);
    std::thread::Builder::new()
        .name("clipboard-listener".into())
        .spawn(|| unsafe {
            let Ok(hinstance) = GetModuleHandleW(None) else {
                tracing::error!("GetModuleHandleW 失败，剪切板监听未启动");
                return;
            };
            let class_name = w!("LanClipClipboardListener");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: HINSTANCE(hinstance.0),
                lpszClassName: class_name,
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                tracing::error!("RegisterClassW 失败，剪切板监听未启动");
                return;
            }
            let hwnd = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("listener"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(HINSTANCE(hinstance.0)),
                None,
            ) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("创建监听窗口失败: {e}");
                    return;
                }
            };
            if let Err(e) = AddClipboardFormatListener(hwnd) {
                tracing::error!("AddClipboardFormatListener 失败: {e}");
                return;
            }
            tracing::debug!("剪切板监听线程已启动");
            let mut msg = MSG::default();
            loop {
                // BOOL: >0 有消息, 0 WM_QUIT, -1 错误
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 == 0 || r.0 == -1 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("spawn clipboard listener thread");
}
