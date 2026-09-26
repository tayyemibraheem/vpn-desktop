use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub username: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SplitTunnelConfig {
    pub apps: Vec<String>,
    pub destinations: Vec<String>,
}

/// This machine's WireGuard identity — generated once on first connect and reused on every
/// connect after that, the same way a real WireGuard client treats a device's keypair as a
/// persistent identity rather than something to re-provision every session. Survives logout;
/// only cleared if the user explicitly forgets this device.
#[derive(Serialize, Deserialize, Clone)]
pub struct WireguardDevice {
    pub device_id: i64,
    pub private_key: String,
    pub assigned_ip: String,
    pub server_hostname: String,
    pub server_public_key: String,
    pub server_listen_port: u16,
    pub dns: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppData {
    pub session: Option<Session>,
    pub split_tunnel: SplitTunnelConfig,
    pub wireguard: Option<WireguardDevice>,
}

fn config_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or(std::env::temp_dir());
    dir.push("TayyemVPN");
    fs::create_dir_all(&dir).ok();
    dir.push("settings.json");
    dir
}

pub fn load() -> AppData {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => AppData::default(),
    }
}

pub fn save(data: &AppData) {
    let path = config_path();
    if let Ok(json) = serde_json::to_string_pretty(data) {
        let _ = fs::write(path, json);
    }
}
