use anyhow::{anyhow, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

use crate::security::key_store::SecureStore;

/// 历史内容本地静态加密（ChaCha20-Poly1305，密钥存系统凭据库）。
pub struct LocalCipher {
    cipher: ChaCha20Poly1305,
}

impl LocalCipher {
    pub fn load_or_create(store: &SecureStore) -> Result<Self> {
        let key = store.get_or_create("local-content-key", || {
            let mut b = [0u8; 32];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut b);
            Ok(b.to_vec())
        })?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| anyhow!("内容加密密钥长度异常"))?;
        Ok(Self { cipher })
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce_bytes = rand::random::<[u8; 12]>();
        let ct = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), Payload::from(plaintext))
            .expect("chacha20poly1305 encrypt");
        let mut out = nonce_bytes.to_vec();
        out.extend_from_slice(&ct);
        out
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 12 + 16 {
            return Err(anyhow!("密文长度异常"));
        }
        let (nonce, ct) = data.split_at(12);
        self.cipher
            .decrypt(Nonce::from_slice(nonce), Payload::from(ct))
            .map_err(|_| anyhow!("内容解密失败"))
    }
}
