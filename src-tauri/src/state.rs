use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::network::discovery::Peer;
use crate::network::transport::{PairOutcome, PeerConn, PendingPairing};
use crate::security::cipher::LocalCipher;
use crate::security::identity::Identity;
use crate::security::key_store::SecureStore;
use crate::storage::{ClipboardItem, Store};

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub device_name: String,
    pub watch_enabled: bool,
    pub notify_preview: bool,
    pub system_notification: bool,
    pub save_history: bool,
    pub retention_days: i64,
    pub max_items: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            watch_enabled: true,
            notify_preview: true,
            system_notification: true,
            save_history: true,
            retention_days: 7,
            max_items: 1000,
        }
    }
}

pub struct AppState {
    /// None 表示 selftest 模式（无 Tauri 窗口，事件为空操作）
    pub app: Option<AppHandle>,
    pub selftest: AtomicBool,
    pub identity: Identity,
    pub store: Store,
    pub cipher: LocalCipher,
    pub settings: Arc<Mutex<Settings>>,
    pub data_dir: PathBuf,

    /// mDNS 发现的设备（含未配对），key = device_id
    pub peers: Arc<Mutex<HashMap<String, Peer>>>,
    /// 已建立的 QUIC 客户端连接缓存
    pub conns: Arc<Mutex<HashMap<String, Arc<PeerConn>>>>,
    pub server_endpoint: OnceLock<quinn::Endpoint>,
    pub client_endpoint: OnceLock<quinn::Endpoint>,
    pub discovery: OnceLock<mdns_sd::ServiceDaemon>,
    pub discovery_name: Mutex<String>,

    // ---- 防死循环与去重 ----
    /// 远程内容写入本机剪切板后的哈希抑制（hash -> 失效时间 ms）
    pub suppress: Mutex<HashMap<String, i64>>,
    /// 最近本地复制过的内容哈希
    pub recent_local: Mutex<HashMap<String, i64>>,
    /// 最近发送过的内容哈希（30s 内不重发）
    pub recent_sent: Mutex<HashMap<String, i64>>,
    /// 已处理过的消息 ID
    pub processed_msgs: Mutex<HashMap<String, i64>>,

    /// 收到的配对请求（本机是被请求方）
    pub pending_pairings: Mutex<HashMap<String, PendingPairing>>,
    /// 发出的配对请求等待回应（本机是请求方）
    pub pairing_waits: Mutex<HashMap<String, tokio::sync::oneshot::Sender<PairOutcome>>>,

    /// 「不保存历史」模式下在内存里保留最近条目
    pub mem_items: Mutex<Vec<ClipboardItem>>,
}

impl AppState {
    pub fn initialize(
        app: Option<AppHandle>,
        data_dir_override: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let data_dir = match data_dir_override {
            Some(d) => d,
            None => app
                .as_ref()
                .map(|a| a.path().app_data_dir())
                .transpose()?
                .expect("app handle required"),
        };
        std::fs::create_dir_all(&data_dir)?;

        let secure = SecureStore::new("lanclip").with_fallback_dir(data_dir.join("secure"));
        let store = Store::open(&data_dir.join("lanclip.db"))?;
        let identity = Identity::load_or_create(&secure, &store)?;
        let cipher = LocalCipher::load_or_create(&secure)?;

        let settings = store
            .kv_get("settings")
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .unwrap_or_default();
        let mut settings = settings;
        if settings.device_name.trim().is_empty() {
            settings.device_name = identity.default_name.clone();
        }
        store.kv_set("settings", &serde_json::to_string(&settings)?);

        Ok(Self {
            app,
            selftest: AtomicBool::new(false),
            identity,
            store,
            cipher,
            settings: Arc::new(Mutex::new(settings)),
            data_dir,
            peers: Arc::new(Mutex::new(HashMap::new())),
            conns: Arc::new(Mutex::new(HashMap::new())),
            server_endpoint: OnceLock::new(),
            client_endpoint: OnceLock::new(),
            discovery: OnceLock::new(),
            discovery_name: Mutex::new(String::new()),
            suppress: Mutex::new(HashMap::new()),
            recent_local: Mutex::new(HashMap::new()),
            recent_sent: Mutex::new(HashMap::new()),
            processed_msgs: Mutex::new(HashMap::new()),
            pending_pairings: Mutex::new(HashMap::new()),
            pairing_waits: Mutex::new(HashMap::new()),
            mem_items: Mutex::new(Vec::new()),
        })
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn emit<S: Serialize + Clone>(&self, event: &str, payload: S) {
        if let Some(app) = &self.app {
            let _ = app.emit(event, payload);
        }
    }

    // ---- 去重/抑制工具 ----

    pub fn recent_contains(map: &Mutex<HashMap<String, i64>>, key: &str, now: i64, window_ms: i64) -> bool {
        let mut m = map.lock().unwrap();
        m.retain(|_, t| now - *t < 600_000);
        m.get(key).map(|t| now - *t < window_ms).unwrap_or(false)
    }

    pub fn recent_put(map: &Mutex<HashMap<String, i64>>, key: &str, now: i64) {
        let mut m = map.lock().unwrap();
        m.retain(|_, t| now - *t < 600_000);
        m.insert(key.to_string(), now);
    }

    pub fn is_suppressed(&self, hash: &str, now: i64) -> bool {
        let mut m = self.suppress.lock().unwrap();
        m.retain(|_, u| *u > now);
        m.contains_key(hash)
    }

    pub fn suppress_insert(&self, hash: &str, until_ms: i64) {
        self.suppress.lock().unwrap().insert(hash.to_string(), until_ms);
    }

    /// 接收侧：同一内容 30 秒内不重复入库/弹通知
    pub fn receive_seen_recently(&self, hash: &str) -> bool {
        let now = now_ms();
        if Self::recent_contains(&self.recent_sent, hash, now, 30_000) {
            return true;
        }
        if let Some(t) = self.store.last_hash_time(hash) {
            if now - t < 30_000 {
                return true;
            }
        }
        let mem = self.mem_items.lock().unwrap();
        mem.iter()
            .any(|i| i.content_hash == hash && now - i.created_at < 30_000)
    }
}
