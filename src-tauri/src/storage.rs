use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::state::now_ms;

#[derive(Clone, Debug)]
pub struct DeviceRecord {
    pub id: String,
    pub name: String,
    pub public_key: Vec<u8>,
    pub cert_der: Vec<u8>,
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub last_seen_at: Option<i64>,
    pub created_at: i64,
}

/// 加密存储前先序列化成这个结构，再整体加密落库
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StoredContent {
    pub content_type: String,
    pub text: Option<String>,
    pub html: Option<String>,
    pub alt_text: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ClipboardItem {
    pub id: String,
    pub source_device_id: String,
    pub source_device_name: String,
    pub content_type: String,
    pub preview: String,
    pub content_hash: String,
    pub encrypted_content: Vec<u8>,
    pub content_size: i64,
    pub status: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
}

pub struct Store {
    conn: Mutex<Connection>,
}

const MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS devices (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    public_key BLOB NOT NULL,
    cert_der BLOB NOT NULL,
    send_enabled INTEGER NOT NULL DEFAULT 1,
    receive_enabled INTEGER NOT NULL DEFAULT 1,
    last_seen_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS clipboard_items (
    id TEXT PRIMARY KEY,
    source_device_id TEXT NOT NULL,
    source_device_name TEXT NOT NULL,
    content_type TEXT NOT NULL,
    preview TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    encrypted_content BLOB NOT NULL,
    content_size INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_items_created ON clipboard_items(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_items_hash ON clipboard_items(content_hash);
CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("打开数据库失败: {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(MIGRATIONS)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    pub fn kv_get(&self, key: &str) -> Option<String> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn kv_set(&self, key: &str, value: &str) {
        let _ = self.conn.lock().unwrap().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        );
    }

    // ---- devices ----

    pub fn upsert_device(&self, d: &DeviceRecord) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO devices (id, name, public_key, cert_der, send_enabled, receive_enabled, last_seen_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                public_key = excluded.public_key,
                cert_der = excluded.cert_der,
                send_enabled = excluded.send_enabled,
                receive_enabled = excluded.receive_enabled,
                last_seen_at = excluded.last_seen_at",
            params![d.id, d.name, d.public_key, d.cert_der, d.send_enabled, d.receive_enabled, d.last_seen_at, d.created_at],
        )?;
        Ok(())
    }

    pub fn get_device(&self, id: &str) -> Option<DeviceRecord> {
        self.conn
            .lock()
            .unwrap()
            .query_row("SELECT id, name, public_key, cert_der, send_enabled, receive_enabled, last_seen_at, created_at FROM devices WHERE id = ?1", params![id], |r| {
                Ok(DeviceRecord {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    public_key: r.get(2)?,
                    cert_der: r.get(3)?,
                    send_enabled: r.get::<_, i32>(4)? != 0,
                    receive_enabled: r.get::<_, i32>(5)? != 0,
                    last_seen_at: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })
            .optional()
            .ok()
            .flatten()
    }

    pub fn list_devices(&self) -> Result<Vec<DeviceRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, public_key, cert_der, send_enabled, receive_enabled, last_seen_at, created_at FROM devices ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(DeviceRecord {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    public_key: r.get(2)?,
                    cert_der: r.get(3)?,
                    send_enabled: r.get::<_, i32>(4)? != 0,
                    receive_enabled: r.get::<_, i32>(5)? != 0,
                    last_seen_at: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn remove_device(&self, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM devices WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_device_flags(&self, id: &str, send: Option<bool>, recv: Option<bool>) -> Result<()> {
        if let Some(s) = send {
            self.conn.lock().unwrap().execute(
                "UPDATE devices SET send_enabled = ?2 WHERE id = ?1",
                params![id, s as i32],
            )?;
        }
        if let Some(r) = recv {
            self.conn.lock().unwrap().execute(
                "UPDATE devices SET receive_enabled = ?2 WHERE id = ?1",
                params![id, r as i32],
            )?;
        }
        Ok(())
    }

    pub fn touch_device(&self, id: &str, ts: i64) {
        let _ = self.conn.lock().unwrap().execute(
            "UPDATE devices SET last_seen_at = ?2 WHERE id = ?1",
            params![id, ts],
        );
    }

    // ---- clipboard history ----

    pub fn insert_item(&self, item: &ClipboardItem) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT OR REPLACE INTO clipboard_items
             (id, source_device_id, source_device_name, content_type, preview, content_hash, encrypted_content, content_size, status, created_at, expires_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                item.id,
                item.source_device_id,
                item.source_device_name,
                item.content_type,
                item.preview,
                item.content_hash,
                item.encrypted_content,
                item.content_size,
                item.status,
                item.created_at,
                item.expires_at,
            ],
        )?;
        Ok(())
    }

    fn row_to_item(r: &rusqlite::Row) -> rusqlite::Result<ClipboardItem> {
        Ok(ClipboardItem {
            id: r.get(0)?,
            source_device_id: r.get(1)?,
            source_device_name: r.get(2)?,
            content_type: r.get(3)?,
            preview: r.get(4)?,
            content_hash: r.get(5)?,
            encrypted_content: r.get(6)?,
            content_size: r.get(7)?,
            status: r.get(8)?,
            created_at: r.get(9)?,
            expires_at: r.get(10)?,
        })
    }

    const ITEM_COLS: &str = "id, source_device_id, source_device_name, content_type, preview, content_hash, encrypted_content, content_size, status, created_at, expires_at";

    pub fn get_item(&self, id: &str) -> Option<ClipboardItem> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                &format!("SELECT {} FROM clipboard_items WHERE id = ?1", Self::ITEM_COLS),
                params![id],
                |r| Self::row_to_item(r),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn list_items(&self, search: Option<&str>, status: Option<&str>, limit: i64) -> Vec<ClipboardItem> {
        let conn = self.conn.lock().unwrap();
        let mut sql = format!("SELECT {} FROM clipboard_items WHERE 1=1", Self::ITEM_COLS);
        let mut bind_search: Option<String> = None;
        let mut bind_status: Option<String> = None;
        if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
            sql.push_str(" AND (preview LIKE '%' || ?1 || '%' OR source_device_name LIKE '%' || ?1 || '%')");
            bind_search = Some(s.to_string());
        }
        if let Some(st) = status.filter(|s| *s != "all" && !s.is_empty()) {
            sql.push_str(" AND status = ?2");
            bind_status = Some(st.to_string());
        }
        sql.push_str(" ORDER BY created_at DESC LIMIT ?3");
        let Ok(mut stmt) = conn.prepare(&sql) else { return vec![] };
        let rows = stmt
            .query_map(params![bind_search, bind_status, limit], |r| Self::row_to_item(r))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();
        rows
    }

    pub fn update_status(&self, id: &str, status: &str) {
        let _ = self.conn.lock().unwrap().execute(
            "UPDATE clipboard_items SET status = ?2 WHERE id = ?1",
            params![id, status],
        );
    }

    pub fn delete_item(&self, id: &str) {
        let _ = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM clipboard_items WHERE id = ?1", params![id]);
    }

    pub fn clear_history(&self) {
        let _ = self.conn.lock().unwrap().execute("DELETE FROM clipboard_items", []);
    }

    pub fn last_hash_time(&self, hash: &str) -> Option<i64> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT created_at FROM clipboard_items WHERE content_hash = ?1 ORDER BY created_at DESC LIMIT 1",
                params![hash],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// 过期置位 + 按保留天数删除 + 超出上限删旧
    pub fn prune(&self, retention_days: i64, max_items: i64) -> Result<()> {
        let now = now_ms();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clipboard_items SET status = 'expired'
             WHERE status = 'pending' AND expires_at IS NOT NULL AND expires_at < ?1",
            params![now],
        )?;
        let cutoff = now - retention_days.max(1) * 86_400_000;
        conn.execute(
            "DELETE FROM clipboard_items WHERE created_at < ?1 OR status = 'deleted'",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM clipboard_items WHERE id NOT IN (
                SELECT id FROM clipboard_items ORDER BY created_at DESC LIMIT ?1
            )",
            params![max_items.max(1)],
        )?;
        Ok(())
    }
}
