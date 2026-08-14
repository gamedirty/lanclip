use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;
use rand::RngCore;
use rustls::pki_types::CertificateDer;

use crate::security::key_store::SecureStore;
use crate::storage::Store;

/// 设备身份：Ed25519 签名密钥（设备 ID 的来源）+ TLS 自签名证书（QUIC 传输层固定用）。
/// 私钥存系统凭据库，证书公钥可存数据库。
pub struct Identity {
    pub device_id: String,
    pub signing: SigningKey,
    pub tls_cert: CertificateDer<'static>,
    pub tls_key_pkcs8: Vec<u8>,
    pub default_name: String,
}

fn blake3_hex8(data: &[u8]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(data);
    let out = h.finalize();
    hex::encode(&out.as_slice()[0..8])
}

impl Identity {
    pub fn load_or_create(secure: &SecureStore, db: &Store) -> Result<Self> {
        // Ed25519 身份密钥
        let seed = secure.get_or_create("identity-signing", || {
            let mut b = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut b);
            Ok(b.to_vec())
        })?;
        let seed: [u8; 32] = seed
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("身份密钥长度异常"))?;
        let signing = SigningKey::from_bytes(&seed);
        let verifying = signing.verifying_key();
        let device_id = blake3_hex8(verifying.as_bytes());

        // TLS 证书（ECDSA P-256 自签名，配对时对证书做公钥固定）
        let tls_key = secure.get_or_create("tls-key", || {
            Ok(rcgen::KeyPair::generate()?.serialize_der().to_vec())
        })?;
        let cert_der: Vec<u8> = match db.kv_get("tls-cert").and_then(|h| hex::decode(h).ok()) {
            Some(der) if !der.is_empty() => der,
            _ => {
                let key_pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
                    &rustls::pki_types::PrivatePkcs8KeyDer::from(tls_key.as_slice()),
                    &rcgen::PKCS_ECDSA_P256_SHA256,
                )
                .context("TLS 私钥解析失败")?;
                let mut params = rcgen::CertificateParams::new(vec![device_id.clone()])?;
                params.distinguished_name = rcgen::DistinguishedName::new();
                params
                    .distinguished_name
                    .push(rcgen::DnType::CommonName, rcgen::DnValue::Utf8String(device_id.clone()));
                params.not_before = rcgen::date_time_ymd(2024, 1, 1);
                params.not_after = rcgen::date_time_ymd(2099, 12, 31);
                let cert = params.self_signed(&key_pair)?;
                let der = cert.der().as_ref().to_vec();
                db.kv_set("tls-cert", &hex::encode(&der));
                der
            }
        };
        let tls_cert = CertificateDer::from(cert_der);

        let host = sanitize_ascii(&whoami::devicename());
        let default_name = format!("{}-{}", host, &device_id[..4]);

        Ok(Self { device_id, signing, tls_cert, tls_key_pkcs8: tls_key, default_name })
    }
}

fn sanitize_ascii(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() { "LanClip".into() } else { trimmed }
}
