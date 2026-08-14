use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::clipboard::{self, ClipContent};
use crate::network::{discovery, transport};
use crate::notification;
use crate::state::AppState;
use crate::storage::{ClipboardItem, StoredContent};

// ---------- 视图类型 ----------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItemView {
    pub id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub content_type: String,
    pub preview: String,
    pub content_size: i64,
    pub status: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    /// true = 落库保存；false = 仅内存（「不保存历史」模式）
    pub persistent: bool,
}

pub fn item_view(item: &ClipboardItem) -> HistoryItemView {
    HistoryItemView {
        id: item.id.clone(),
        source_device_id: item.source_device_id.clone(),
        source_device_name: item.source_device_name.clone(),
        content_type: item.content_type.clone(),
        preview: item.preview.clone(),
        content_size: item.content_size,
        status: item.status.clone(),
        created_at: item.created_at,
        expires_at: item.expires_at,
        persistent: true,
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    pub device_id: String,
    pub name: String,
    pub online: bool,
    pub paired: bool,
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub last_seen_at: Option<i64>,
    pub address: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub device_name: String,
    pub watch_enabled: bool,
    pub notify_preview: bool,
    pub system_notification: bool,
    pub save_history: bool,
    pub retention_days: i64,
    pub max_items: i64,
    pub autostart: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OwnInfo {
    pub device_id: String,
    pub device_name: String,
    pub version: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    pub own: OwnInfo,
    pub settings: SettingsView,
    pub devices: Vec<DeviceView>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContentView {
    pub content_type: String,
    pub text: String,
    pub html: Option<String>,
}

// ---------- 命令 ----------

fn settings_view(state: &AppState) -> SettingsView {
    let s = state.settings_snapshot();
    let autostart = state
        .app
        .as_ref()
        .map(|app| {
            use tauri_plugin_autostart::ManagerExt;
            app.autolaunch().is_enabled().unwrap_or(false)
        })
        .unwrap_or(false);
    SettingsView {
        device_name: s.device_name,
        watch_enabled: s.watch_enabled,
        notify_preview: s.notify_preview,
        system_notification: s.system_notification,
        save_history: s.save_history,
        retention_days: s.retention_days,
        max_items: s.max_items,
        autostart,
    }
}

#[tauri::command]
pub fn get_state(state: State<'_, Arc<AppState>>) -> Result<StateView, String> {
    let devices = state.store.list_devices().map_err(|e| e.to_string())?;
    let peers = state.peers.lock().unwrap().clone();
    let mut views: Vec<DeviceView> = devices
        .into_iter()
        .map(|d| {
            let peer = peers.get(&d.id);
            DeviceView {
                device_id: d.id.clone(),
                name: d.name.clone(),
                online: peer.is_some(),
                paired: true,
                send_enabled: d.send_enabled,
                receive_enabled: d.receive_enabled,
                last_seen_at: d.last_seen_at,
                address: peer.map(|p| p.addr.to_string()),
            }
        })
        .collect();
    let paired: std::collections::HashSet<String> = views.iter().map(|v| v.device_id.clone()).collect();
    for (id, p) in &peers {
        if !paired.contains(id) {
            views.push(DeviceView {
                device_id: id.clone(),
                name: p.name.clone(),
                online: true,
                paired: false,
                send_enabled: false,
                receive_enabled: false,
                last_seen_at: Some(p.last_seen_ms),
                address: Some(p.addr.to_string()),
            });
        }
    }
    Ok(StateView {
        own: OwnInfo {
            device_id: state.identity.device_id.clone(),
            device_name: state.settings_snapshot().device_name,
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        settings: settings_view(&state),
        devices: views,
    })
}

#[tauri::command]
pub fn get_history(
    state: State<'_, Arc<AppState>>,
    search: Option<String>,
    status: Option<String>,
) -> Result<Vec<HistoryItemView>, String> {
    let mut items: Vec<HistoryItemView> = state
        .store
        .list_items(search.as_deref(), status.as_deref(), 500)
        .iter()
        .map(item_view)
        .collect();
    if !state.settings_snapshot().save_history {
        let mem = state.mem_items.lock().unwrap();
        let mem_views: Vec<HistoryItemView> = mem
            .iter()
            .map(|i| {
                let mut v = item_view(i);
                v.persistent = false;
                v
            })
            .collect();
        items.extend(mem_views);
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        items.truncate(500);
    }
    Ok(items)
}

fn find_item_anywhere(state: &AppState, id: &str) -> Option<ClipboardItem> {
    state
        .store
        .get_item(id)
        .or_else(|| state.mem_items.lock().unwrap().iter().find(|i| i.id == id).cloned())
}

fn decrypt_item(state: &AppState, id: &str) -> Result<StoredContent, String> {
    let item = find_item_anywhere(state, id).ok_or_else(|| "条目不存在".to_string())?;
    let plain = state.cipher.decrypt(&item.encrypted_content).map_err(|e| e.to_string())?;
    rmp_serde::from_slice(&plain).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_content(state: State<'_, Arc<AppState>>, id: String) -> Result<ContentView, String> {
    let stored = decrypt_item(&state, &id)?;
    Ok(ContentView {
        content_type: stored.content_type,
        text: stored.text.or(stored.alt_text).unwrap_or_default(),
        html: stored.html,
    })
}

#[tauri::command]
pub async fn accept_item(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    let stored = decrypt_item(&state, &id)?;
    let content = ClipContent {
        text: stored.text.clone().or(stored.alt_text.clone()),
        html: stored.html.clone(),
    };
    let text = content.text.clone().unwrap_or_default();
    let hash = clipboard::content_hash("text", &clipboard::normalized_text(&text));
    clipboard::write_remote_clipboard(state.inner().clone(), content, hash);

    // 更新状态
    {
        let mut mem = state.mem_items.lock().unwrap();
        if let Some(item) = mem.iter_mut().find(|i| i.id == id) {
            item.status = "accepted".into();
        }
    }
    state.store.update_status(&id, "accepted");
    state.emit("lanclip://history-changed", ());
    Ok(())
}

#[tauri::command]
pub fn ignore_item(state: State<'_, Arc<AppState>>, id: String) {
    {
        let mut mem = state.mem_items.lock().unwrap();
        if let Some(item) = mem.iter_mut().find(|i| i.id == id) {
            item.status = "ignored".into();
        }
    }
    state.store.update_status(&id, "ignored");
    state.emit("lanclip://history-changed", ());
}

#[tauri::command]
pub fn delete_item(state: State<'_, Arc<AppState>>, id: String) {
    state.mem_items.lock().unwrap().retain(|i| i.id != id);
    state.store.delete_item(&id);
    state.emit("lanclip://history-changed", ());
}

#[tauri::command]
pub fn clear_history(state: State<'_, Arc<AppState>>) {
    state.store.clear_history();
    state.mem_items.lock().unwrap().clear();
    state.emit("lanclip://history-changed", ());
}

#[tauri::command]
pub fn update_settings(
    state: State<'_, Arc<AppState>>,
    settings: SettingsView,
) -> Result<SettingsView, String> {
    let name = settings.device_name.trim().to_string();
    if name.is_empty() {
        return Err("设备名称不能为空".into());
    }
    let name_changed = name != state.settings_snapshot().device_name;
    let watch_changed = settings.watch_enabled != state.settings_snapshot().watch_enabled;

    {
        let mut s = state.settings.lock().unwrap();
        s.device_name = name;
        s.watch_enabled = settings.watch_enabled;
        s.notify_preview = settings.notify_preview;
        s.system_notification = settings.system_notification;
        s.save_history = settings.save_history;
        s.retention_days = settings.retention_days.clamp(1, 90);
        s.max_items = settings.max_items.clamp(10, 10_000);
    }
    let snapshot = state.settings_snapshot();
    let json = serde_json::to_string(&snapshot).map_err(|e| e.to_string())?;
    state.store.kv_set("settings", &json);

    if let Some(app_handle) = &state.app {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app_handle.autolaunch();
        let current = launcher.is_enabled().unwrap_or(false);
        if current != settings.autostart {
            let r = if settings.autostart {
                launcher.enable()
            } else {
                launcher.disable()
            };
            if let Err(e) = r {
                tracing::warn!("切换开机自启失败: {e}");
            }
        }
    }

    if name_changed {
        if let Some(ep) = state.server_endpoint.get() {
            if let Ok(addr) = ep.local_addr() {
                discovery::set_device_name(state.inner().clone(), &snapshot.device_name, addr.port());
            }
        }
    }
    if watch_changed {
        crate::tray::sync_pause_check(&snapshot);
    }
    state.emit("lanclip://settings-changed", ());
    state.emit("lanclip://devices-changed", ());
    Ok(settings_view(&state))
}

#[tauri::command]
pub async fn pair_request(state: State<'_, Arc<AppState>>, deviceId: String) -> Result<(), String> {
    transport::pair_request(state.inner().clone(), deviceId)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn respond_pairing(
    state: State<'_, Arc<AppState>>,
    deviceId: String,
    accept: bool,
) -> Result<(), String> {
    transport::respond_pairing(state.inner().clone(), deviceId, accept)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_pair_wait(state: State<'_, Arc<AppState>>, deviceId: String) {
    // 丢弃等待通道；后台读任务发现后不会再发结果事件
    state.pairing_waits.lock().unwrap().remove(&deviceId);
}

#[tauri::command]
pub fn set_device_flags(
    state: State<'_, Arc<AppState>>,
    deviceId: String,
    sendEnabled: Option<bool>,
    receiveEnabled: Option<bool>,
) -> Result<(), String> {
    state
        .store
        .set_device_flags(&deviceId, sendEnabled, receiveEnabled)
        .map_err(|e| e.to_string())?;
    state.emit("lanclip://devices-changed", ());
    Ok(())
}

#[tauri::command]
pub fn remove_device(state: State<'_, Arc<AppState>>, deviceId: String) -> Result<(), String> {
    state.store.remove_device(&deviceId).map_err(|e| e.to_string())?;
    state.conns.lock().unwrap().remove(&deviceId);
    state.emit("lanclip://devices-changed", ());
    Ok(())
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) {
    notification::hide_popup(&app);
}
