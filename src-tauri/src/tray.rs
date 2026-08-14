use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// 暂停监听菜单项句柄，用于设置变化时同步勾选状态
static PAUSE_ITEM: std::sync::Mutex<Option<CheckMenuItem<tauri::Wry>>> = std::sync::Mutex::new(None);

pub fn setup_tray(app: &AppHandle, state: Arc<AppState>) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(
        app,
        "pause",
        "暂停剪切板监听",
        true,
        !state.settings_snapshot().watch_enabled,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 LanClip", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &quit])?;
    *PAUSE_ITEM.lock().unwrap() = Some(pause.clone());

    let st = state.clone();
    let _tray = TrayIconBuilder::with_id("lanclip-tray")
        .icon(app.default_window_icon().cloned().expect("窗口图标"))
        .tooltip("LanClip · 局域网剪切板同步")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "pause" => {
                let watch_enabled = {
                    let mut s = st.settings.lock().unwrap();
                    s.watch_enabled = !s.watch_enabled;
                    if let Ok(json) = serde_json::to_string(&*s) {
                        st.store.kv_set("settings", &json);
                    }
                    s.watch_enabled
                };
                if let Some(item) = PAUSE_ITEM.lock().unwrap().as_ref() {
                    let _ = item.set_checked(!watch_enabled);
                }
                let _ = app.emit(
                    "lanclip://settings-changed",
                    serde_json::json!({ "watchEnabled": watch_enabled }),
                );
                tracing::info!(watch_enabled, "剪切板监听状态切换");
            }
            "quit" => {
                tracing::info!("用户退出");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 设置变化后（例如设置页切换了监听）同步托盘勾选状态
pub fn sync_pause_check(settings: &crate::state::Settings) {
    if let Some(item) = PAUSE_ITEM.lock().unwrap().as_ref() {
        let _ = item.set_checked(!settings.watch_enabled);
    }
}
