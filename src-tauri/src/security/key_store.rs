use std::path::PathBuf;

use anyhow::Result;

/// 敏感密钥存储：优先操作系统凭据库
/// （Windows Credential Manager / macOS Keychain，由 keyring crate 提供），
/// 不可用时回退到数据目录下的本地文件并记录警告。
pub struct SecureStore {
    service: String,
    fallback_dir: Option<PathBuf>,
}

impl SecureStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into(), fallback_dir: None }
    }

    pub fn with_fallback_dir(mut self, dir: PathBuf) -> Self {
        self.fallback_dir = Some(dir);
        self
    }

    fn file_path(&self, user: &str) -> Option<PathBuf> {
        self.fallback_dir
            .as_ref()
            .map(|d| d.join(format!("{}.bin", user)))
    }

    pub fn get(&self, user: &str) -> Result<Option<Vec<u8>>> {
        match keyring::Entry::new(&self.service, user) {
            Ok(entry) => match entry.get_secret() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(err) => {
                    tracing::warn!("系统凭据库读取失败（{}），使用本地文件回退", err);
                    self.file_get(user)
                }
            },
            Err(err) => {
                tracing::warn!("系统凭据库不可用（{}），使用本地文件回退", err);
                self.file_get(user)
            }
        }
    }

    pub fn set(&self, user: &str, val: &[u8]) -> Result<()> {
        match keyring::Entry::new(&self.service, user) {
            Ok(entry) => match entry.set_secret(val) {
                Ok(()) => Ok(()),
                Err(err) => {
                    tracing::warn!("系统凭据库写入失败（{}），使用本地文件回退", err);
                    self.file_set(user, val)
                }
            },
            Err(err) => {
                tracing::warn!("系统凭据库不可用（{}），使用本地文件回退", err);
                self.file_set(user, val)
            }
        }
    }

    fn file_get(&self, user: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .file_path(user)
            .and_then(|p| std::fs::read(p).ok()))
    }

    fn file_set(&self, user: &str, val: &[u8]) -> Result<()> {
        let path = self
            .file_path(user)
            .ok_or_else(|| anyhow::anyhow!("未配置回退目录"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, val)?;
        Ok(())
    }

    pub fn get_or_create(
        &self,
        user: &str,
        gen: impl FnOnce() -> Result<Vec<u8>>,
    ) -> Result<Vec<u8>> {
        if let Some(v) = self.get(user)? {
            if !v.is_empty() {
                return Ok(v);
            }
        }
        let v = gen()?;
        self.set(user, &v)?;
        Ok(v)
    }
}
