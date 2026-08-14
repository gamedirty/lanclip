//! macOS（及未实现原生事件的其他平台）：轮询剪切板。
//! macOS 没有全局剪切板变化事件，官方通行做法就是定时检查
//! NSPasteboard.changeCount；这里以「读取文本并计算哈希」的等效方式轮询，
//! 只有内容变化时才做后续处理。首版简化实现，后续可切换到原生 changeCount。

use std::sync::Arc;
use std::time::Duration;

use crate::clipboard::{content_hash, normalized_text, read_clipboard, handle_local_clipboard};
use crate::state::AppState;

pub fn spawn(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("clipboard-poller".into())
        .spawn(move || {
            let mut last_hash = String::new();
            loop {
                if let Some(content) = read_clipboard() {
                    if let Some(text) = &content.text {
                        if !text.trim().is_empty() {
                            let h = content_hash("text", &normalized_text(text));
                            if h != last_hash {
                                last_hash = h;
                                handle_local_clipboard(&state, &content);
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            }
        })
        .expect("spawn clipboard poller thread");
}
