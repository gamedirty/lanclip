//! 本机回环自测：--selftest
//! 验证密钥/加密/配对码、QUIC 服务端启动、证书固定连接、挑战应答认证、
//! 剪切板消息收发与历史落库解密。全部通过输出 SELFTEST OK。

use std::net::SocketAddr;
use std::sync::Arc;

use crate::clipboard;
use crate::network::discovery::{self, Peer};
use crate::network::protocol::Message;
use crate::network::transport;
use crate::security::pairing::pairing_code;
use crate::state::{now_ms, AppState};
use crate::storage::{DeviceRecord, StoredContent};

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .try_init();
    println!("== LanClip selftest ==");

    // 独立进程没有 Tauri 应用初始化异步运行时，这里手动设置（Runtime 泄漏以保活）
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("创建 tokio 运行时失败");
    let handle = rt.handle().clone();
    std::mem::forget(rt);
    tauri::async_runtime::set(handle);

    let tmp = std::env::temp_dir().join(format!("lanclip-selftest-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp).expect("创建临时目录失败");
    println!("[1] 数据目录: {}", tmp.display());

    let state = Arc::new(AppState::initialize(None, Some(tmp.clone())).expect("初始化状态失败"));
    println!("[2] 设备身份 OK  device_id={}", state.identity.device_id);

    // 本地加密往返
    let secret = "hello, lanclip 中文内容".to_string();
    let enc = state.cipher.encrypt(secret.as_bytes());
    let dec = state.cipher.decrypt(&enc).expect("解密失败");
    assert_eq!(dec, secret.as_bytes());
    println!("[3] 本地内容加密往返 OK");

    // 配对码：与参数顺序无关
    let c1 = pairing_code(b"pub-aaaa", b"pub-bbbb", b"nonce-1");
    let c2 = pairing_code(b"pub-bbbb", b"pub-aaaa", b"nonce-1");
    assert_eq!(c1, c2);
    assert_eq!(c1.len(), 6);
    assert!(c1.chars().all(|c| c.is_ascii_digit()));
    assert_ne!(c1, pairing_code(b"pub-aaaa", b"pub-bbbb", b"nonce-2"));
    println!("[4] 配对验证码计算 OK  code={}", c1);

    // Ed25519 签名/验签
    use ed25519_dalek::Signer;
    let nonce = rand::random::<[u8; 32]>();
    let sig = state.identity.signing.sign(&nonce);
    state
        .identity
        .signing
        .verifying_key()
        .verify_strict(&nonce, &sig)
        .expect("验签失败");
    println!("[5] Ed25519 签名验签 OK");

    // QUIC 传输
    let port = transport::start(state.clone(), 48654).expect("QUIC 传输启动失败");
    println!("[6] QUIC 服务端已启动 udp/{port}");

    // mDNS（注册成功即算通过；回环发现受环境防火墙影响，只记录）
    match discovery::start(state.clone(), port) {
        Ok(()) => println!("[7] mDNS 服务注册 OK（服务类型 {}）", discovery::SERVICE_TYPE),
        Err(e) => println!("[7] mDNS 注册失败（不影响核心链路）: {e:#}"),
    }

    // 模拟一台已配对设备：直接把「本机自己」注册为可信设备，
    // 走真实的证书固定 + 挑战应答链路（Hello 的 device_id 必须在信任列表中）
    let self_id = state.identity.device_id.clone();
    let rec = DeviceRecord {
        id: self_id.clone(),
        name: "SelfTest Peer".into(),
        public_key: state.identity.signing.verifying_key().as_bytes().to_vec(),
        cert_der: state.identity.tls_cert.as_ref().to_vec(),
        send_enabled: true,
        receive_enabled: true,
        last_seen_at: None,
        created_at: now_ms(),
    };
    state.store.upsert_device(&rec).expect("写入设备失败");
    state.peers.lock().unwrap().insert(
        self_id.clone(),
        Peer {
            device_id: self_id.clone(),
            name: "SelfTest Peer".into(),
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            last_seen_ms: now_ms(),
        },
    );

    // 通过 QUIC 回环发送一条剪切板消息
    let text = "lanclip-selftest-内容测试123".to_string();
    let hash = clipboard::content_hash("text", &clipboard::normalized_text(&text));
    let msg = Message::ClipboardPush {
        msg_id: uuid::Uuid::new_v4().to_string(),
        sender_id: self_id.clone(),
        content_type: "text".into(),
        content: text.clone().into_bytes(),
        alt_text: None,
        content_hash: hash.clone(),
        timestamp_ms: now_ms(),
    };
    tauri::async_runtime::block_on(async {
        transport::send_to_peer(
            state.clone(),
            self_id.clone(),
            SocketAddr::from(([127, 0, 0, 1], port)),
            &msg,
        )
        .await
        .expect("QUIC 发送失败");
    });
    println!("[8] QUIC 回环发送 OK（证书固定 + 挑战应答认证通过）");

    // 等服务端异步落库
    let mut found = None;
    for _ in 0..50 {
        if let Some(item) = state
            .store
            .list_items(None, None, 10)
            .into_iter()
            .find(|i| i.content_hash == hash)
        {
            found = Some(item);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let item = found.expect("未在历史中找到接收到的条目");
    assert_eq!(item.source_device_name, "SelfTest Peer");
    assert_eq!(item.status, "pending");
    assert!(item.preview.contains("内容测试"));
    println!("[9] 接收入库 OK  preview={:?}", item.preview);

    // 解密还原
    let plain = state.cipher.decrypt(&item.encrypted_content).expect("条目解密失败");
    let stored: StoredContent = rmp_serde::from_slice(&plain).expect("条目反序列化失败");
    assert_eq!(stored.text.as_deref(), Some(text.as_str()));
    println!("[10] 历史条目解密还原 OK");

    // 清理
    let _ = state.store.remove_device(&self_id);
    let _ = std::fs::remove_dir_all(&tmp);

    println!("SELFTEST OK");
    std::process::exit(0);
}
