#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(target_os = "windows"))]
mod macos;

use std::sync::Arc;

use anyhow::{anyhow, Result};

use crate::network::protocol::Message;
use crate::state::{now_ms, AppState};

#[derive(Clone, Debug, Default)]
pub struct ClipContent {
    pub text: Option<String>,
    pub html: Option<String>,
}

/// 归一化：跨平台统一换行符后再参与哈希，保证 A/B 两端算出相同 content_hash
pub fn normalized_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub fn content_hash(kind: &str, normalized: &str) -> String {
    let mut h = blake3::Hasher::new();
    h.update(kind.as_bytes());
    h.update(&[0]);
    h.update(normalized.as_bytes());
    hex::encode(h.finalize().as_slice())
}

/// 极简 HTML 转纯文本（仅用于没有纯文本版本时的哈希/预览）
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn read_clipboard() -> Option<ClipContent> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let text_opt = cb.get().text().ok().filter(|t| !t.trim().is_empty());
    let html_opt = cb.get().html().ok().filter(|h| !h.trim().is_empty());
    match (html_opt, text_opt) {
        (Some(h), text) => Some(ClipContent {
            text: text.or_else(|| Some(strip_html(&h))),
            html: Some(h),
        }),
        (None, Some(t)) => Some(ClipContent { text: Some(t), html: None }),
        (None, None) => None,
    }
}

pub fn write_clipboard(content: &ClipContent) -> Result<()> {
    let mut cb = arboard::Clipboard::new()?;
    let text = content.text.as_deref().unwrap_or("");
    if let Some(html) = &content.html {
        let alt = (!text.trim().is_empty()).then(|| text.to_string());
        cb.set()
            .html(html.to_string(), alt)
            .map_err(|e| anyhow!("写入 HTML 失败: {e}"))?;
    } else if !text.trim().is_empty() {
        cb.set()
            .text(text.to_string())
            .map_err(|e| anyhow!("写入文本失败: {e}"))?;
    } else {
        return Err(anyhow!("内容为空"));
    }
    Ok(())
}

/// 启动平台剪切板监听（Windows: 系统事件；macOS/其他: 500ms 轮询）
pub fn start_watching(state: Arc<AppState>) {
    #[cfg(target_os = "windows")]
    windows::spawn(state);
    #[cfg(not(target_os = "windows"))]
    macos::spawn(state);
}

/// 本机用户复制了新内容：去重后广播给所有已配对在线设备
pub fn handle_local_clipboard(state: &Arc<AppState>, content: &ClipContent) {
    let text = match &content.text {
        Some(t) if !t.trim().is_empty() => t,
        _ => return,
    };
    if !state.settings_snapshot().watch_enabled {
        return;
    }
    let now = now_ms();
    let hash = content_hash("text", &normalized_text(text));
    // 防回环：自己刚从远程接收写入的内容不再发送
    if state.is_suppressed(&hash, now) {
        return;
    }
    // 同内容 30s 内不重复处理/发送
    if AppState::recent_contains(&state.recent_local, &hash, now, 30_000) {
        return;
    }
    AppState::recent_put(&state.recent_local, &hash, now);
    AppState::recent_put(&state.recent_sent, &hash, now);

    let msg = Message::ClipboardPush {
        msg_id: uuid::Uuid::new_v4().to_string(),
        sender_id: state.identity.device_id.clone(),
        content_type: if content.html.is_some() { "html".into() } else { "text".into() },
        content: content
            .html
            .clone()
            .map(|h| h.into_bytes())
            .unwrap_or_else(|| text.clone().into_bytes()),
        alt_text: content.html.is_some().then(|| text.clone()),
        content_hash: hash,
        timestamp_ms: now,
    };
    let st = state.clone();
    tauri::async_runtime::spawn(async move {
        crate::network::transport::send_clipboard_to_all(st, msg).await;
    });
}

/// 接收方点击「接收」：写入本机剪切板并抑制随后的变化事件
pub fn write_remote_clipboard(state: Arc<AppState>, content: ClipContent, hash: String) {
    let now = now_ms();
    state.suppress_insert(&hash, now + 3_500);
    AppState::recent_put(&state.recent_local, &hash, now);
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = write_clipboard(&content) {
            tracing::warn!("写入本机剪切板失败: {e:#}");
        }
    });
}
