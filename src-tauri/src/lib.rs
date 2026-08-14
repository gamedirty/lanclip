pub mod clipboard;
pub mod commands;
pub mod network;
pub mod notification;
pub mod security;
pub mod selftest;
pub mod state;
pub mod storage;
pub mod tray;

use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

use crate::state::AppState;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let handle = app.handle().clone();
            init_logger(&handle);

            // macOS: 只驻留菜单栏，不显示 Dock 图标
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let state = AppState::initialize(Some(handle.clone()), None)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            let state = Arc::new(state);
            app.manage(state.clone());

            let port = network::transport::start(state.clone(), 47654)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            network::discovery::start(state.clone(), port)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            clipboard::start_watching(state.clone());
            tray::setup_tray(&handle, state.clone())?;

            // 历史保留策略 + 过期配对请求清理
            {
                let s = state.settings_snapshot();
                let _ = state.store.prune(s.retention_days, s.max_items);
            }
            let st = state.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                    let s = st.settings_snapshot();
                    let _ = st.store.prune(s.retention_days, s.max_items);
                    let now = state::now_ms();
                    st.pending_pairings
                        .lock()
                        .unwrap()
                        .retain(|_, p| now - p.created_ms < 120_000);
                    st.emit("lanclip://history-changed", ());
                }
            });

            tracing::info!(
                device_id = %state.identity.device_id,
                port,
                "LanClip 已启动"
            );
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭窗口 = 隐藏到托盘，真正退出走托盘菜单
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::get_history,
            commands::get_item_content,
            commands::accept_item,
            commands::ignore_item,
            commands::delete_item,
            commands::clear_history,
            commands::update_settings,
            commands::pair_request,
            commands::respond_pairing,
            commands::cancel_pair_wait,
            commands::set_device_flags,
            commands::remove_device,
            commands::hide_popup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_logger(app: &tauri::AppHandle) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let file_layer = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("logs"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())
        .map(|dir| {
            let appender = tracing_appender::rolling::daily(dir, "lanclip.log");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            std::mem::forget(guard);
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(writer)
        });

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info".into());
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr),
        )
        .with(file_layer)
        .with(filter)
        .try_init();
}
