//! QUIC 传输层（quinn + rustls）：
//! - TLS 自签名证书 + 配对时固定的证书公钥（防止中间人）
//! - 已配对设备连接后走 Hello -> Challenge -> Auth（Ed25519 签名）认证
//! - 未配对设备只允许发起 PairingRequest，收不到任何剪切板内容

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::{Signature, Signer, VerifyingKey};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error as TlsError, SignatureScheme};
use serde::Serialize;

use crate::clipboard;
use crate::network::protocol::{read_frame, write_frame, Message};
use crate::security::pairing::pairing_code;
use crate::state::{now_ms, AppState};
use crate::storage::{ClipboardItem, DeviceRecord, StoredContent};

pub struct PeerConn {
    pub conn: Connection,
    pub send: Arc<tokio::sync::Mutex<Option<quinn::SendStream>>>,
}

pub struct PendingPairing {
    pub device_id: String,
    pub device_name: String,
    pub device_pubkey: Vec<u8>,
    pub cert_der: Vec<u8>,
    pub code: String,
    pub created_ms: i64,
    pub send: Option<quinn::SendStream>,
}

#[derive(Clone, Debug)]
pub struct PairOutcome {
    pub accepted: bool,
    pub message: String,
    pub device_name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PairingEventPayload {
    pub device_id: String,
    pub device_name: String,
    pub code: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PairingResultPayload {
    pub device_id: String,
    pub device_name: String,
    pub ok: bool,
    pub message: String,
}

fn blake3_16(data: &[u8]) -> Vec<u8> {
    let mut h = blake3::Hasher::new();
    h.update(data);
    h.finalize().as_slice()[0..16].to_vec()
}

fn verifying_key_from(public_key: &[u8]) -> Result<VerifyingKey> {
    let pk: [u8; 32] = public_key
        .try_into()
        .map_err(|_| anyhow!("设备公钥长度异常"))?;
    Ok(VerifyingKey::from_bytes(&pk).context("存储的设备公钥无效")?)
}

// ---------- 启动 ----------

pub fn start(state: Arc<AppState>, port_hint: u16) -> Result<u16> {
    let _ = rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());

    // quinn 的 Endpoint 创建必须在 tokio 运行时上下文中执行
    let st = state.clone();
    let (server, client) = tauri::async_runtime::block_on(async move {
        let build = |port: u16| -> Result<Endpoint> {
            let identity = &st.identity;
            let key = rustls::pki_types::PrivatePkcs8KeyDer::from(identity.tls_key_pkcs8.clone());
            let rustls_server = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![identity.tls_cert.clone()], key.into())
                .context("TLS 服务端配置失败")?;
            Endpoint::server(server_config(rustls_server)?, SocketAddr::from(([0, 0, 0, 0], port)))
                .map_err(anyhow::Error::from)
        };
        let server = build(port_hint).or_else(|_| build(0))?;
        let client = Endpoint::client(SocketAddr::from(([0, 0, 0, 0], 0))).map_err(anyhow::Error::from)?;
        anyhow::Ok((server, client))
    })?;
    let port = server.local_addr()?.port();
    let _ = state.server_endpoint.set(server.clone());
    let _ = state.client_endpoint.set(client);

    tauri::async_runtime::spawn(accept_loop(state.clone(), server));
    Ok(port)
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut tc = quinn::TransportConfig::default();
    // 10s 应用层保活；默认 30s 空闲超时足够保持长连接
    tc.keep_alive_interval(Some(Duration::from_secs(10)));
    Arc::new(tc)
}

fn server_config(rustls_cfg: rustls::ServerConfig) -> Result<ServerConfig> {
    let mut cfg = ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(rustls_cfg)?));
    cfg.transport_config(transport_config());
    Ok(cfg)
}

fn client_config(verifier: Arc<dyn ServerCertVerifier>) -> ClientConfig {
    let rustls_cfg = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    let mut cfg = ClientConfig::new(Arc::new(
        QuicClientConfig::try_from(rustls_cfg).expect("quinn client config"),
    ));
    cfg.transport_config(transport_config());
    cfg
}

// ---------- 证书校验器 ----------

fn verify_schemes() -> Vec<SignatureScheme> {
    rustls::crypto::ring::default_provider()
        .signature_verification_algorithms
        .supported_schemes()
}

fn verify_tls12(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> std::result::Result<HandshakeSignatureValid, TlsError> {
    rustls::crypto::verify_tls12_signature(
        message,
        cert,
        dss,
        &rustls::crypto::ring::default_provider().signature_verification_algorithms,
    )
}

fn verify_tls13(
    message: &[u8],
    cert: &CertificateDer<'_>,
    dss: &DigitallySignedStruct,
) -> std::result::Result<HandshakeSignatureValid, TlsError> {
    rustls::crypto::verify_tls13_signature(
        message,
        cert,
        dss,
        &rustls::crypto::ring::default_provider().signature_verification_algorithms,
    )
}

/// 已配对设备：只接受与配对时固定的证书 DER 完全一致的对端证书
#[derive(Debug)]
struct PinnedVerifier {
    expected: Vec<u8>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        if end_entity.as_ref() == self.expected.as_slice() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::General("证书与配对时不匹配".into()))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls12(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls13(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        verify_schemes()
    }
}

/// 配对阶段：TOFU 捕获对端证书（握手签名仍严格校验），
/// 之后通过 6 位验证码人工比对绑定密钥。
#[derive(Debug, Clone)]
struct PairingVerifier {
    captured: Arc<Mutex<Option<Vec<u8>>>>,
}

impl ServerCertVerifier for PairingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        *self.captured.lock().unwrap() = Some(end_entity.as_ref().to_vec());
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls12(message, cert, dss)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls13(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        verify_schemes()
    }
}

// ---------- 服务端 ----------

async fn accept_loop(state: Arc<AppState>, server: Endpoint) {
    while let Some(incoming) = server.accept().await {
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            match incoming.await {
                Ok(conn) => {
                    if let Err(e) = handle_connection(state, conn).await {
                        tracing::debug!("连接结束: {e:#}");
                    }
                }
                Err(e) => tracing::warn!("入站连接失败: {e}"),
            }
        });
    }
}

async fn handle_connection(state: Arc<AppState>, conn: Connection) -> Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;
    let first = read_frame(&mut recv).await.context("读取首帧")?;
    match first {
        Message::Hello { device_id } => {
            let dev = state
                .store
                .get_device(&device_id)
                .ok_or_else(|| anyhow!("设备 {device_id} 未配对，拒绝连接"))?;
            let nonce = rand::random::<[u8; 32]>();
            write_frame(&mut send, &Message::Challenge { nonce }).await?;
            match read_frame(&mut recv).await? {
                Message::Auth { signature, .. } => {
                    let vk = verifying_key_from(&dev.public_key)?;
                    let sig = Signature::from_slice(&signature).context("签名编码无效")?;
                    vk.verify_strict(&nonce, &sig).context("设备认证失败")?;
                }
                _ => bail!("期望 Auth 消息"),
            }
            tracing::info!(device = %dev.id, "已配对设备认证成功");
            let dev = Arc::new(dev);
            loop {
                let msg = read_frame(&mut recv).await.context("读取消息")?;
                handle_message(state.clone(), dev.clone(), msg).await;
            }
        }
        Message::PairingRequest { .. } => handle_pairing_server(state, send, first).await,
        other => bail!("非法首帧: {other:?}"),
    }
}

async fn handle_message(state: Arc<AppState>, dev: Arc<DeviceRecord>, msg: Message) {
    let Message::ClipboardPush {
        msg_id,
        sender_id,
        content_type,
        content,
        alt_text,
        content_hash: _,
        timestamp_ms: _,
    } = msg
    else {
        return;
    };
    if sender_id != dev.id {
        tracing::warn!("发送者身份不匹配，丢弃");
        return;
    }
    if !dev.receive_enabled {
        return;
    }
    let now = now_ms();
    // 同一消息 ID 只处理一次
    if AppState::recent_contains(&state.processed_msgs, &msg_id, now, 600_000) {
        return;
    }
    AppState::recent_put(&state.processed_msgs, &msg_id, now);
    // 同内容 30s 内不重复入库/弹通知
    let text = alt_text
        .clone()
        .or_else(|| String::from_utf8(content.clone()).ok())
        .unwrap_or_default();
    let normalized = clipboard::normalized_text(&text);
    if normalized.trim().is_empty() {
        return;
    }
    let hash = clipboard::content_hash("text", &normalized);
    if state.receive_seen_recently(&hash) {
        return;
    }

    let is_html = content_type == "html";
    let trimmed = normalized.trim_start();
    let final_type = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        "url"
    } else if is_html {
        "html"
    } else {
        "text"
    };

    let stored = StoredContent {
        content_type: final_type.to_string(),
        text: Some(text.clone()),
        html: is_html.then(|| String::from_utf8_lossy(&content).into_owned()),
        alt_text: alt_text.clone(),
    };
    let plain = rmp_serde::to_vec(&stored).expect("encode stored content");
    let encrypted = state.cipher.encrypt(&plain);
    let preview: String = normalized
        .chars()
        .take(200)
        .collect::<String>()
        .replace(['\n', '\r', '\t'], " ");

    let settings = state.settings_snapshot();
    let item = ClipboardItem {
        id: msg_id,
        source_device_id: dev.id.clone(),
        source_device_name: dev.name.clone(),
        content_type: final_type.to_string(),
        preview,
        content_hash: hash,
        encrypted_content: encrypted,
        content_size: plain.len() as i64,
        status: "pending".into(),
        created_at: now,
        expires_at: Some(now + settings.retention_days.max(1) * 86_400_000),
    };
    if settings.save_history {
        if let Err(e) = state.store.insert_item(&item) {
            tracing::warn!("历史入库失败: {e:#}");
            return;
        }
    } else {
        let mut mem = state.mem_items.lock().unwrap();
        mem.insert(0, item.clone());
        mem.truncate(100);
    }
    tracing::info!(from = %dev.name, kind = final_type, "收到剪切板内容");
    crate::notification::incoming(&state, crate::commands::item_view(&item));
}

// ---------- 配对（服务端 = 被请求方） ----------

async fn handle_pairing_server(state: Arc<AppState>, mut send: quinn::SendStream, req: Message) -> Result<()> {
    let Message::PairingRequest { device_id, device_name, device_pubkey, cert_der, nonce } = req
    else {
        bail!("非配对请求");
    };
    // 已配对：幂等地直接重新接受（换机重装场景）
    if state.store.get_device(&device_id).is_some() {
        let i = &state.identity;
        write_frame(
            &mut send,
            &Message::PairingAccept {
                device_id: i.device_id.clone(),
                device_name: state.settings_snapshot().device_name,
                device_pubkey: i.signing.verifying_key().as_bytes().to_vec(),
                cert_der: i.tls_cert.as_ref().to_vec(),
            },
        )
        .await?;
        send.finish()?;
        return Ok(());
    }

    let cert_binding = blake3_16(state.identity.tls_cert.as_ref());
    let code = pairing_code(&device_pubkey, &cert_binding, &nonce);
    let pending = PendingPairing {
        device_id: device_id.clone(),
        device_name: device_name.clone(),
        device_pubkey,
        cert_der,
        code: code.clone(),
        created_ms: now_ms(),
        send: Some(send),
    };
    state
        .pending_pairings
        .lock()
        .unwrap()
        .insert(device_id.clone(), pending);
    tracing::info!(device = %device_name, "收到配对请求");
    state.emit(
        "lanclip://pairing-incoming",
        PairingEventPayload { device_id, device_name, code },
    );
    Ok(())
}

/// 被请求方在 UI 上点击「确认配对 / 拒绝」
pub async fn respond_pairing(state: Arc<AppState>, device_id: String, accept: bool) -> Result<()> {
    let mut pending = state
        .pending_pairings
        .lock()
        .unwrap()
        .remove(&device_id)
        .ok_or_else(|| anyhow!("配对请求不存在或已过期"))?;
    let mut send = pending.send.take().ok_or_else(|| anyhow!("配对通道已关闭"))?;
    let i = &state.identity;
    if accept {
        write_frame(
            &mut send,
            &Message::PairingAccept {
                device_id: i.device_id.clone(),
                device_name: state.settings_snapshot().device_name,
                device_pubkey: i.signing.verifying_key().as_bytes().to_vec(),
                cert_der: i.tls_cert.as_ref().to_vec(),
            },
        )
        .await?;
        send.finish()?;
        let rec = DeviceRecord {
            id: pending.device_id.clone(),
            name: pending.device_name.clone(),
            public_key: pending.device_pubkey.clone(),
            cert_der: pending.cert_der.clone(),
            send_enabled: true,
            receive_enabled: true,
            last_seen_at: Some(now_ms()),
            created_at: now_ms(),
        };
        state.store.upsert_device(&rec)?;
        state.emit("lanclip://devices-changed", ());
        tracing::info!(device = %pending.device_name, "配对成功");
    } else {
        let _ = write_frame(&mut send, &Message::PairingReject { device_id: i.device_id.clone() }).await;
        let _ = send.finish();
        tracing::info!(device = %pending.device_name, "已拒绝配对");
    }
    Ok(())
}

// ---------- 配对（客户端 = 请求方） ----------

pub async fn pair_request(state: Arc<AppState>, target_id: String) -> Result<PairOutcome> {
    let peer = state
        .peers
        .lock()
        .unwrap()
        .get(&target_id)
        .cloned()
        .ok_or_else(|| anyhow!("设备不在线或尚未发现"))?;
    let client = state
        .client_endpoint
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("传输层未启动"))?;
    let verifier = PairingVerifier { captured: Arc::new(Mutex::new(None)) };
    let i = &state.identity;
    let nonce = rand::random::<[u8; 16]>();

    let conn = client
        .connect_with(
            client_config(Arc::new(verifier.clone())),
            peer.addr,
            "lanclip",
        )
        .context("连接对方设备失败")?
        .await
        .context("QUIC 握手失败")?;
    let (mut send, mut recv) = conn.open_bi().await.context("打开配对通道失败")?;
    write_frame(
        &mut send,
        &Message::PairingRequest {
            device_id: i.device_id.clone(),
            device_name: state.settings_snapshot().device_name,
            device_pubkey: i.signing.verifying_key().as_bytes().to_vec(),
            cert_der: i.tls_cert.as_ref().to_vec(),
            nonce: nonce.to_vec(),
        },
    )
    .await?;

    let captured = verifier
        .captured
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow!("未捕获对方证书"))?;
    let cert_binding = blake3_16(&captured);
    let code = pairing_code(i.signing.verifying_key().as_bytes(), &cert_binding, &nonce);
    state.emit(
        "lanclip://pairing-waiting",
        PairingEventPayload { device_id: target_id.clone(), device_name: peer.name.clone(), code },
    );

    let (tx, rx) = tokio::sync::oneshot::channel::<PairOutcome>();
    state.pairing_waits.lock().unwrap().insert(target_id.clone(), tx);

    // 后台读对方响应；若请求方已取消/超时（tx 被移除），静默丢弃
    let st = state.clone();
    let tid = target_id.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = match read_frame(&mut recv).await {
            Ok(Message::PairingAccept { device_id, device_name, device_pubkey, cert_der }) => {
                if captured != cert_der {
                    PairOutcome { accepted: false, message: "证书不匹配，疑似中间人攻击".into(), device_name }
                } else {
                    let rec = DeviceRecord {
                        id: device_id,
                        name: device_name.clone(),
                        public_key: device_pubkey,
                        cert_der,
                        send_enabled: true,
                        receive_enabled: true,
                        last_seen_at: Some(now_ms()),
                        created_at: now_ms(),
                    };
                    let _ = st.store.upsert_device(&rec);
                    st.emit("lanclip://devices-changed", ());
                    PairOutcome { accepted: true, message: "配对成功".into(), device_name }
                }
            }
            Ok(Message::PairingReject { .. }) => {
                PairOutcome { accepted: false, message: "对方拒绝了配对".into(), device_name: String::new() }
            }
            Ok(_) => PairOutcome { accepted: false, message: "收到意外响应".into(), device_name: String::new() },
            Err(e) => PairOutcome { accepted: false, message: format!("配对通道断开: {e}"), device_name: String::new() },
        };
        let mut waits = st.pairing_waits.lock().unwrap();
        if let Some(tx) = waits.remove(&tid) {
            let name = outcome.device_name.clone();
            let ok = outcome.accepted;
            let msg = outcome.message.clone();
            let _ = tx.send(outcome);
            drop(waits);
            st.emit(
                "lanclip://pairing-result",
                PairingResultPayload { device_id: tid, device_name: name, ok, message: msg },
            );
        }
    });

    let result = match tokio::time::timeout(Duration::from_secs(90), rx).await {
        Ok(Ok(outcome)) => Ok(outcome),
        Ok(Err(_)) => Err(anyhow!("配对任务异常终止")),
        Err(_) => {
            state.pairing_waits.lock().unwrap().remove(&target_id);
            Err(anyhow!("等待对方确认超时"))
        }
    };
    result
}

// ---------- 客户端发送 ----------

pub async fn send_clipboard_to_all(state: Arc<AppState>, msg: Message) {
    let peers = state.peers.lock().unwrap().clone();
    let devices = state.store.list_devices().unwrap_or_default();
    for dev in devices {
        if !dev.send_enabled {
            continue;
        }
        let Some(peer) = peers.get(&dev.id) else { continue };
        let st = state.clone();
        let msg = msg.clone();
        let (id, addr) = (dev.id.clone(), peer.addr);
        tauri::async_runtime::spawn(async move {
            if let Err(e) = send_to_peer(st.clone(), id.clone(), addr, &msg).await {
                tracing::warn!(device = %id, "发送失败: {e:#}");
            }
        });
    }
}

pub async fn send_to_peer(state: Arc<AppState>, device_id: String, addr: SocketAddr, msg: &Message) -> Result<()> {
    let attempt = |state: Arc<AppState>, device_id: String| async move {
        let pc = get_or_dial(state.clone(), &device_id, addr).await?;
        let mut guard = pc.send.lock().await;
        let Some(s) = guard.as_mut() else {
            bail!("连接尚未就绪");
        };
        write_frame(s, msg).await
    };
    if attempt(state.clone(), device_id.clone()).await.is_err() {
        // 连接可能已坏：丢弃缓存后重拨一次
        state.conns.lock().unwrap().remove(&device_id);
        attempt(state, device_id).await?;
    }
    Ok(())
}

async fn get_or_dial(state: Arc<AppState>, device_id: &str, addr: SocketAddr) -> Result<Arc<PeerConn>> {
    {
        let conns = state.conns.lock().unwrap();
        if let Some(pc) = conns.get(device_id) {
            if pc.conn.close_reason().is_none() {
                return Ok(pc.clone());
            }
        }
    }
    state.conns.lock().unwrap().remove(device_id);

    let dev = state
        .store
        .get_device(device_id)
        .ok_or_else(|| anyhow!("设备未配对"))?;
    let client = state
        .client_endpoint
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("传输层未启动"))?;
    let conn = client
        .connect_with(
            client_config(Arc::new(PinnedVerifier { expected: dev.cert_der.clone() })),
            addr,
            "lanclip",
        )
        .context("建立 QUIC 连接失败")?
        .await
        .context("QUIC 握手失败")?;

    let (mut send, mut recv) = conn.open_bi().await?;
    let self_id = state.identity.device_id.clone();
    write_frame(&mut send, &Message::Hello { device_id: self_id.clone() }).await?;
    match read_frame(&mut recv).await? {
        Message::Challenge { nonce } => {
            let sig = state.identity.signing.sign(&nonce);
            write_frame(
                &mut send,
                &Message::Auth { device_id: self_id, signature: sig.to_bytes().to_vec() },
            ).await?;
        }
        _ => bail!("期望 Challenge 消息"),
    }

    // 后台持续读取，检测连接关闭并清理缓存
    {
        let st = state.clone();
        let did = device_id.to_string();
        tauri::async_runtime::spawn(async move {
            let mut recv = recv;
            loop {
                if read_frame(&mut recv).await.is_err() {
                    st.conns.lock().unwrap().remove(&did);
                    break;
                }
            }
        });
    }

    let pc = Arc::new(PeerConn {
        conn: conn.clone(),
        send: Arc::new(tokio::sync::Mutex::new(Some(send))),
    });
    state.conns.lock().unwrap().insert(device_id.to_string(), pc.clone());
    Ok(pc)
}
