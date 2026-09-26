use crate::split_tunnel::app_tunnel::AppTunnel;
use crate::split_tunnel::destination_routes::{self, PhysicalGateway};
use crate::store::{self, SplitTunnelConfig, WireguardDevice};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand_core::OsRng;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x25519_dalek::{PublicKey, StaticSecret};

const VPN_MANAGER_BASE_URL: &str = "https://api-vpn.tayyem.dev";
const TUNNEL_NAME: &str = "tayyemvpn";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Deserialize)]
struct DeviceResponse {
    id: i64,
    #[serde(rename = "publicKey")]
    #[allow(dead_code)]
    public_key: String,
    #[serde(rename = "assignedIp")]
    assigned_ip: String,
    #[serde(rename = "serverHostname")]
    server_hostname: String,
    #[serde(rename = "serverPublicKey")]
    server_public_key: Option<String>,
    #[serde(rename = "serverListenPort")]
    server_listen_port: Option<u16>,
    dns: Option<String>,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<String>,
}

pub struct WireguardClient {
    status: Mutex<(String, Option<String>)>,
    physical: Mutex<Option<PhysicalGateway>>,
    applied_routes: Mutex<Vec<String>>,
    generation: Mutex<u64>,
}

impl WireguardClient {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(("disconnected".to_string(), None)),
            physical: Mutex::new(None),
            applied_routes: Mutex::new(vec![]),
            generation: Mutex::new(0),
        }
    }

    pub fn status(&self) -> (String, Option<String>) {
        self.status.lock().unwrap().clone()
    }

    pub fn physical_gateway(&self) -> Option<PhysicalGateway> {
        self.physical.lock().unwrap().clone()
    }

    pub fn applied_destination_routes(&self) -> Vec<String> {
        self.applied_routes.lock().unwrap().clone()
    }

    pub fn set_applied_destination_routes(&self, routes: Vec<String>) {
        *self.applied_routes.lock().unwrap() = routes;
    }

    fn find_binary() -> Option<PathBuf> {
        let candidates = [
            r"C:\Program Files\WireGuard\wireguard.exe",
            r"C:\Program Files (x86)\WireGuard\wireguard.exe",
        ];
        candidates.iter().map(PathBuf::from).find(|p| p.exists())
    }

    fn config_dir() -> PathBuf {
        let mut dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
        dir.push("TayyemVPN");
        fs::create_dir_all(&dir).ok();
        dir
    }

    /// Generates this machine's WireGuard keypair and registers it with vpn_manager. Only ever
    /// called once per install — the private key and the server's response are cached in
    /// store.rs's AppData and reused on every later connect.
    async fn register_device(access_token: &str, emit: &impl Fn(&str, serde_json::Value)) -> Result<WireguardDevice, String> {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        let private_b64 = STANDARD.encode(secret.as_bytes());
        let public_b64 = STANDARD.encode(public.as_bytes());

        emit("vpn:log", serde_json::json!("Registering this device with the VPN..."));

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{VPN_MANAGER_BASE_URL}/api/devices"))
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "deviceName": hostname_label(),
                "platform": "WINDOWS",
                "publicKey": public_b64,
            }))
            .send()
            .await
            .map_err(|e| format!("Could not reach the VPN service: {e}"))?;

        if !resp.status().is_success() {
            let message = resp
                .json::<ApiError>()
                .await
                .ok()
                .and_then(|e| e.error)
                .unwrap_or_else(|| "Could not register this device".to_string());
            return Err(message);
        }

        let body: DeviceResponse = resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))?;
        let server_public_key = body
            .server_public_key
            .ok_or_else(|| "This server has no WireGuard key configured yet".to_string())?;
        let server_listen_port = body
            .server_listen_port
            .ok_or_else(|| "This server has no WireGuard port configured yet".to_string())?;
        let dns = body.dns.unwrap_or_else(|| "1.1.1.1".to_string());

        Ok(WireguardDevice {
            device_id: body.id,
            private_key: private_b64,
            assigned_ip: body.assigned_ip,
            server_hostname: body.server_hostname,
            server_public_key,
            server_listen_port,
            dns,
        })
    }

    fn write_conf(device: &WireguardDevice) -> Result<PathBuf, String> {
        let contents = format!(
            "[Interface]\nPrivateKey = {}\nAddress = {}/32\nDNS = {}\n\n[Peer]\nPublicKey = {}\nEndpoint = {}:{}\nAllowedIPs = 0.0.0.0/0\nPersistentKeepalive = 25\n",
            device.private_key,
            device.assigned_ip,
            device.dns,
            device.server_public_key,
            device.server_hostname,
            device.server_listen_port
        );
        let path = Self::config_dir().join(format!("{TUNNEL_NAME}.conf"));
        fs::write(&path, contents).map_err(|e| format!("Could not write WireGuard config: {e}"))?;
        Ok(path)
    }

    pub async fn connect(
        self: &Arc<Self>,
        access_token: String,
        split_tunnel_config: SplitTunnelConfig,
        app_tunnel: Arc<AppTunnel>,
        emit: impl Fn(&str, serde_json::Value) + Send + Sync + Clone + 'static,
    ) -> Result<(), String> {
        if self.status().0 != "disconnected" {
            return Err("Already connecting or connected".into());
        }

        let binary = Self::find_binary()
            .ok_or_else(|| "WireGuard is not installed. Install it from wireguard.com/install, then try again.".to_string())?;

        let my_generation = {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
            *gen
        };

        *self.status.lock().unwrap() = ("connecting".to_string(), None);
        emit("vpn:status", serde_json::json!({ "state": "connecting", "detail": null }));

        let physical = match destination_routes::capture_original_gateway() {
            Some(p) => p,
            None => {
                return Err(self.fail(my_generation, &emit, "Could not determine your current network gateway".to_string()));
            }
        };
        *self.physical.lock().unwrap() = Some(physical.clone());

        let mut data = store::load();
        let device = match data.wireguard.clone() {
            Some(d) => d,
            None => match Self::register_device(&access_token, &emit).await {
                Ok(d) => {
                    data.wireguard = Some(d.clone());
                    store::save(&data);
                    d
                }
                Err(e) => return Err(self.fail(my_generation, &emit, e)),
            },
        };

        let conf_path = match Self::write_conf(&device) {
            Ok(p) => p,
            Err(e) => return Err(self.fail(my_generation, &emit, e)),
        };

        // A prior crash could have left the service registered without us knowing — clear it
        // first so /installtunnelservice doesn't fail with "service already exists".
        let _ = uninstall_service(&binary);

        emit("vpn:log", serde_json::json!(format!("Starting WireGuard tunnel service ({TUNNEL_NAME})...")));

        let mut cmd = Command::new(&binary);
        cmd.args(["/installtunnelservice", conf_path.to_str().unwrap_or_default()]);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => return Err(self.fail(my_generation, &emit, format!("Failed to start WireGuard: {e}"))),
        };
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let msg = if detail.is_empty() {
                "WireGuard service failed to start".to_string()
            } else {
                format!("WireGuard service failed to start: {detail}")
            };
            return Err(self.fail(my_generation, &emit, msg));
        }

        emit("vpn:log", serde_json::json!("Tunnel service installed, verifying connectivity..."));

        let this = self.clone();
        let dns_ip = device.dns.clone();
        let binary_for_check = binary.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if *this.generation.lock().unwrap() != my_generation {
                return; // superseded by a disconnect in the meantime
            }
            if ping(&dns_ip) {
                *this.status.lock().unwrap() = ("connected".to_string(), None);
                emit("vpn:status", serde_json::json!({ "state": "connected", "detail": null }));
                emit("vpn:log", serde_json::json!("Connected."));

                let applied = destination_routes::apply(&split_tunnel_config.destinations, &physical).await;
                this.set_applied_destination_routes(applied);

                let resources_dir = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("resources")))
                    .unwrap_or_else(|| PathBuf::from("resources"));
                let emit_for_log = emit.clone();
                app_tunnel.start(split_tunnel_config.apps.clone(), physical.clone(), resources_dir, move |msg| {
                    emit_for_log("vpn:log", serde_json::json!(msg));
                });
            } else {
                let _ = uninstall_service(&binary_for_check);
                this.fail(
                    my_generation,
                    &emit,
                    "WireGuard service started but the tunnel isn't passing traffic — check your connection and try again.".to_string(),
                );
            }
        });

        Ok(())
    }

    fn fail(&self, my_generation: u64, emit: &impl Fn(&str, serde_json::Value), message: String) -> String {
        if *self.generation.lock().unwrap() == my_generation {
            *self.status.lock().unwrap() = ("disconnected".to_string(), Some(message.clone()));
            emit("vpn:status", serde_json::json!({ "state": "disconnected", "detail": message }));
        }
        message
    }

    pub async fn disconnect(&self) {
        {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
        }
        destination_routes::clear_all(&self.applied_destination_routes());
        self.set_applied_destination_routes(vec![]);
        if let Some(binary) = Self::find_binary() {
            let _ = uninstall_service(&binary);
        }
        *self.status.lock().unwrap() = ("disconnected".to_string(), None);
    }
}

fn uninstall_service(binary: &PathBuf) -> std::io::Result<std::process::Output> {
    let mut cmd = Command::new(binary);
    cmd.args(["/uninstalltunnelservice", TUNNEL_NAME]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
}

fn ping(ip: &str) -> bool {
    let mut cmd = Command::new("ping");
    #[cfg(target_os = "windows")]
    {
        cmd.args(["-n", "1", "-w", "2000", ip]);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        cmd.args(["-c", "1", "-W", "2", ip]);
    }
    cmd.output().map(|o| o.status.success()).unwrap_or(false)
}

fn hostname_label() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string())
}
