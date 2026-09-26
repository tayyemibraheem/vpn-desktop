use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub username: String,
    pub email: Option<String>,
    /// Unix ms when access_token expires. 0 for a session saved before this field existed —
    /// treated as already-expired so it refreshes on first use instead of assuming it's valid.
    pub expires_at: i64,
    /// Only a session with this set true is restored on the next launch; unchecking "keep me
    /// logged in" still lets the app work for the current run, it just won't survive a restart.
    pub remember: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SplitTunnelConfig {
    pub apps: Vec<String>,
    pub destinations: Vec<String>,
}

/// This machine's AmneziaWG identity — generated once on first connect and reused on every
/// connect after that, the same way a real WireGuard client treats a device's keypair as a
/// persistent identity rather than something to re-provision every session. Survives logout;
/// only cleared if the user explicitly forgets this device. The jc/jmin/jmax/s1/s2/h1-h4
/// obfuscation parameters and the preshared key come from the server at registration time rather
/// than being hardcoded here, so they can be rotated without an app update.
#[derive(Serialize, Deserialize, Clone)]
pub struct AmneziaWgDevice {
    pub device_id: i64,
    pub private_key: String,
    pub preshared_key: String,
    pub assigned_ip: String,
    pub server_hostname: String,
    pub server_public_key: String,
    pub server_listen_port: u16,
    pub dns: String,
    pub jc: u16,
    pub jmin: u16,
    pub jmax: u16,
    pub s1: u16,
    pub s2: u16,
    pub h1: u32,
    pub h2: u32,
    pub h3: u32,
    pub h4: u32,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppData {
    pub session: Option<Session>,
    pub split_tunnel: SplitTunnelConfig,
    pub amneziawg: Option<AmneziaWgDevice>,
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
