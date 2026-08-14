//! mDNS/DNS-SD 设备发现（mdns-sd crate）。
//! 只广播：设备 ID、设备名、服务端口、协议版本 —— 不广播任何剪切板内容。
//! 服务类型：_lanclip._udp.local.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

use crate::state::{now_ms, AppState};

pub const SERVICE_TYPE: &str = "_lanclip._udp.local.";
pub const PROTO_VERSION: &str = "1";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub device_id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub last_seen_ms: i64,
}

fn sanitize_label(s: &str) -> String {
    let filtered: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let trimmed = filtered.trim_matches('-').to_string();
    let v = if trimmed.is_empty() { "lanclip".to_string() } else { trimmed };
    v.chars().take(40).collect()
}

fn pick_ip() -> IpAddr {
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        let mut candidates: Vec<IpAddr> = addrs
            .iter()
            .map(|i| i.ip())
            .filter(|ip| ip.is_ipv4() && !ip.is_loopback())
            .collect();
        // 优先常见内网网段
        candidates.sort_by_key(|ip| {
            let o = match ip {
                IpAddr::V4(v4) => v4.octets(),
                IpAddr::V6(_) => [0, 0, 0, 0],
            };
            match o {
                [192, 168, ..] => 0,
                [10, ..] => 1,
                [172, b, ..] if (16..=31).contains(&b) => 2,
                _ => 3,
            }
        });
        if let Some(ip) = candidates.first() {
            return *ip;
        }
    }
    IpAddr::from([127, 0, 0, 1])
}

fn register(state: &AppState, daemon: &ServiceDaemon, name: &str, port: u16) -> Result<()> {
    let short = state.identity.device_id[..6].to_lowercase();
    let instance = format!("{}-{}", sanitize_label(name), short);
    let host = format!("lanclip-{short}.local.");
    let mut props = HashMap::new();
    props.insert("id".to_string(), state.identity.device_id.clone());
    props.insert("v".to_string(), PROTO_VERSION.to_string());
    props.insert("n".to_string(), name.to_string());
    let info = ServiceInfo::new(SERVICE_TYPE, &instance, &host, pick_ip(), port, props)?;
    daemon.register(info)?;
    *state.discovery_name.lock().unwrap() = format!("{}.{}", instance, SERVICE_TYPE);
    tracing::info!(instance = %instance, port, "mDNS 服务已注册");
    Ok(())
}

pub fn start(state: std::sync::Arc<AppState>, port: u16) -> Result<()> {
    let daemon = ServiceDaemon::new()?;
    let name = state.settings_snapshot().device_name;
    register(&state, &daemon, &name, port)?;
    let _ = state.discovery.set(daemon.clone());

    let receiver = daemon.browse(SERVICE_TYPE)?;
    std::thread::Builder::new()
        .name("mdns-browse".into())
        .spawn(move || {
            let mut fullname_to_id: HashMap<String, String> = HashMap::new();
            loop {
                match receiver.recv() {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        let Some(id) = info.get_property_val_str("id").map(|s| s.to_string()) else {
                            continue;
                        };
                        if id == state.identity.device_id {
                            continue;
                        }
                        let Some(ip) = info.get_addresses().iter().find(|a| a.is_ipv4()) else {
                            continue;
                        };
                        let name = info
                            .get_property_val_str("n")
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "未知设备".into());
                        let peer = Peer {
                            device_id: id.clone(),
                            name,
                            addr: SocketAddr::new(*ip, info.get_port()),
                            last_seen_ms: now_ms(),
                        };
                        fullname_to_id.insert(info.get_fullname().to_string(), id.clone());
                        state.peers.lock().unwrap().insert(id.clone(), peer);
                        state.store.touch_device(&id, now_ms());
                        state.emit("lanclip://devices-changed", ());
                    }
                    Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                        if let Some(id) = fullname_to_id.remove(fullname.as_str()) {
                            state.peers.lock().unwrap().remove(&id);
                            state.emit("lanclip://devices-changed", ());
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("mDNS 浏览异常: {e}");
                        std::thread::sleep(std::time::Duration::from_secs(2));
                    }
                }
            }
        })?;
    Ok(())
}

/// 设备改名后重新注册 mDNS 服务
pub fn set_device_name(state: std::sync::Arc<AppState>, name: &str, port: u16) {
    if let Some(daemon) = state.discovery.get() {
        let full = state.discovery_name.lock().unwrap().clone();
        if !full.is_empty() {
            let _ = daemon.unregister(&full);
        }
        if let Err(e) = register(&state, daemon, name, port) {
            tracing::warn!("重新注册 mDNS 服务失败: {e:#}");
        }
    }
}
