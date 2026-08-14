use quinn::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

/// 长度前缀（4 字节大端）+ MessagePack 编码的应用层协议
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Message {
    /// 连接后的第一条消息：声明身份，触发挑战应答
    Hello { device_id: String },
    Challenge { nonce: [u8; 32] },
    Auth { device_id: String, signature: Vec<u8> },
    PairingRequest {
        device_id: String,
        device_name: String,
        device_pubkey: Vec<u8>,
        cert_der: Vec<u8>,
        nonce: Vec<u8>,
    },
    PairingAccept {
        device_id: String,
        device_name: String,
        device_pubkey: Vec<u8>,
        cert_der: Vec<u8>,
    },
    PairingReject { device_id: String },
    ClipboardPush {
        msg_id: String,
        sender_id: String,
        content_type: String,
        content: Vec<u8>,
        alt_text: Option<String>,
        content_hash: String,
        timestamp_ms: i64,
    },
}

pub fn encode_frame(msg: &Message) -> Vec<u8> {
    let body = rmp_serde::to_vec(msg).expect("msgpack encode");
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

pub async fn write_frame(s: &mut SendStream, msg: &Message) -> anyhow::Result<()> {
    s.write_all(&encode_frame(msg)).await?;
    Ok(())
}

pub async fn read_frame(r: &mut RecvStream) -> anyhow::Result<Message> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        anyhow::bail!("帧过大: {} 字节", n);
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).await?;
    Ok(rmp_serde::from_slice(&buf)?)
}
