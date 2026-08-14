use serde::Serialize;
use tauri::{AppHandle, Manager, PhysicalPosition};

use crate::commands::HistoryItemView;
use crate::state::AppState;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IncomingPayload {
    pub item: HistoryItemView,
    pub preview_allowed: bool,
}

/// 收到新内容：推送给弹窗窗口 + 刷新主窗口历史 + 显示弹窗 +（主窗口隐藏时）系统通知
pub fn incoming(state: &AppState, item: HistoryItemView) {
    let settings = state.settings_snapshot();
    let payload = IncomingPayload { item: item.clone(), preview_allowed: settings.notify_preview };
    state.emit("lanclip://incoming", payload);
    state.emit("lanclip://history-changed", ());

    let Some(app) = &state.app else { return };
    if let Some(popup) = app.get_webview_window("popup") {
        position_popup(&popup);
        let _ = popup.show();
    }
    if settings.system_notification {
        let main_hidden = app
            .get_webview_window("main")
            .map(|w| !w.is_visible().unwrap_or(true))
            .unwrap_or(true);
        if main_hidden {
            use tauri_plugin_notification::NotificationExt;
            let _ = app
                .notification()
                .builder()
                .title("LanClip · 收到新剪切板内容")
                .body(format!("来自 {}，点击托盘图标查看", item.source_device_name))
                .show();
        }
    }
}

/// 弹窗固定在主显示器右上角
fn position_popup(popup: &tauri::WebviewWindow) {
    let (mx, my, mw, _mh) = popup
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            let pos = m.position();
            let size = m.size();
            (pos.x, pos.y, size.width, size.height)
        })
        .unwrap_or((0, 0, 1920, 1080));
    let size = popup.outer_size().unwrap_or(tauri::PhysicalSize::new(400, 230));
    let x = mx + mw as i32 - size.width as i32 - 16;
    let y = my + 16;
    let _ = popup.set_position(PhysicalPosition::new(x, y));
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(popup) = app.get_webview_window("popup") {
        let _ = popup.hide();
    }
}
