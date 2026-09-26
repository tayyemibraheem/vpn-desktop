use crate::split_tunnel::app_tunnel::AppTunnel;
use crate::split_tunnel::destination_routes::{self, PhysicalGateway};
use crate::store::{self, SplitTunnelConfig, WireguardDevice};
use crate::wireguard_nt::{WgAdapter, WireGuardNt};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand_core::OsRng;
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::lookup_host;
use x25519_dalek::{PublicKey, StaticSecret};

pub(crate) const VPN_MANAGER_BASE_URL: &str = "https://api-vpn.tayyem.dev";
const INTERFACE_NAME: &str = "TayyemVPN";

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

/// Holds the loaded driver DLL and the adapter it created — kept alive for the whole time a
/// tunnel is up, since closing/reconfiguring the adapter requires calling back into the DLL.
struct ActiveTunnel {
    nt: WireGuardNt,
    adapter: WgAdapter,
}

pub struct WireguardClient {
    status: Mutex<(String, Option<String>)>,
    physical: Mutex<Option<PhysicalGateway>>,
    applied_routes: Mutex<Vec<String>>,
    generation: Mutex<u64>,
    active: Mutex<Option<ActiveTunnel>>,
}

impl WireguardClient {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(("disconnected".to_string(), None)),
            physical: Mutex::new(None),
            applied_routes: Mutex::new(vec![]),
            generation: Mutex::new(0),
            active: Mutex::new(None),
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

    /// Tears down whatever adapter is currently tracked, if any — used both for a normal
    /// disconnect and to clean up a leftover adapter from a prior crash before reconnecting.
    fn teardown_active(&self) {
        if let Some(active) = self.active.lock().unwrap().take() {
            active.nt.set_adapter_down(&active.adapter);
            active.nt.close_adapter(active.adapter);
        }
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

    pub async fn connect(
        self: &Arc<Self>,
        access_token: String,
        resources_dir: PathBuf,
        split_tunnel_config: SplitTunnelConfig,
        app_tunnel: Arc<AppTunnel>,
        emit: impl Fn(&str, serde_json::Value) + Send + Sync + Clone + 'static,
    ) -> Result<(), String> {
        if self.status().0 != "disconnected" {
            return Err("Already connecting or connected".into());
        }

        let my_generation = {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
            *gen
        };

        *self.status.lock().unwrap() = ("connecting".to_string(), None);
        emit("vpn:status", serde_json::json!({ "state": "connecting", "detail": null }));

        // A prior crash could have left an adapter registered without us knowing about it.
        self.teardown_active();

        let physical = match destination_routes::capture_original_gateway() {
            Some(p) => p,
            None => return Err(self.fail(my_generation, &emit, "Could not determine your current network gateway".to_string())),
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

        let private_key = match decode_key(&device.private_key) {
            Ok(k) => k,
            Err(e) => return Err(self.fail(my_generation, &emit, e)),
        };
        let peer_public_key = match decode_key(&device.server_public_key) {
            Ok(k) => k,
            Err(e) => return Err(self.fail(my_generation, &emit, e)),
        };
        let endpoint_ip = match resolve_ipv4(&device.server_hostname).await {
            Some(ip) => ip,
            None => return Err(self.fail(my_generation, &emit, format!("Could not resolve {}", device.server_hostname))),
        };

        // Must exist before the broad tunnel routes go in below — otherwise the encrypted
        // handshake/data packets addressed to the server itself get swallowed by the tunnel's
        // own 0.0.0.0/1 + 128.0.0.0/1 override and routed back into the tunnel that's still
        // trying to establish, instead of out the real network card.
        let endpoint_route = destination_routes::apply(&[endpoint_ip.to_string()], &physical).await;
        if endpoint_route.is_empty() {
            return Err(self.fail(my_generation, &emit, format!("Could not add a route to {endpoint_ip} via your physical network")));
        }
        self.set_applied_destination_routes(endpoint_route);

        let dll_path = resources_dir.join("wireguard-nt").join("amd64").join("wireguard.dll");
        if !dll_path.exists() {
            return Err(self.fail_and_clear_routes(my_generation, &emit, "Missing bundled WireGuard driver — try reinstalling the app.".to_string()));
        }
        let nt = match WireGuardNt::load(&dll_path) {
            Ok(n) => n,
            Err(e) => return Err(self.fail_and_clear_routes(my_generation, &emit, e)),
        };
        if let Some(version) = nt.driver_version() {
            emit("vpn:log", serde_json::json!(format!("WireGuard driver loaded (version 0x{version:x})")));
        }

        emit("vpn:log", serde_json::json!("Creating WireGuard adapter..."));
        let adapter = match nt.create_adapter(INTERFACE_NAME) {
            Ok(a) => a,
            Err(e) => return Err(self.fail_and_clear_routes(my_generation, &emit, e)),
        };

        if let Err(e) = nt.set_configuration(&adapter, private_key, peer_public_key, endpoint_ip.octets(), device.server_listen_port) {
            nt.close_adapter(adapter);
            return Err(self.fail_and_clear_routes(my_generation, &emit, e));
        }
        if let Err(e) = nt.set_adapter_up(&adapter) {
            nt.close_adapter(adapter);
            return Err(self.fail_and_clear_routes(my_generation, &emit, e));
        }

        emit("vpn:log", serde_json::json!("Configuring interface address, DNS, and routes..."));
        if let Err(e) = configure_interface(INTERFACE_NAME, &device.assigned_ip, &device.dns) {
            nt.set_adapter_down(&adapter);
            nt.close_adapter(adapter);
            return Err(self.fail_and_clear_routes(my_generation, &emit, e));
        }

        *self.active.lock().unwrap() = Some(ActiveTunnel { nt, adapter });

        emit("vpn:log", serde_json::json!("Adapter up, verifying connectivity..."));

        let this = self.clone();
        let dns_ip = device.dns.clone();
        let resources_dir_for_apps = resources_dir.clone();
        tokio::spawn(async move {
            // A few retries over several seconds rather than one fast check — a first-ever
            // handshake can take a moment, and this is the app's only signal of success.
            let mut handshake_seen = false;
            let mut last_stats_log = String::new();
            for attempt in 0..5 {
                tokio::time::sleep(Duration::from_secs(if attempt == 0 { 2 } else { 1 })).await;
                if *this.generation.lock().unwrap() != my_generation {
                    return; // superseded by a disconnect in the meantime
                }
                let stats = this.active.lock().unwrap().as_ref().and_then(|a| a.nt.get_peer_stats(&a.adapter));
                if let Some(ref s) = stats {
                    last_stats_log = format!("tx={} rx={} handshake_ns={}", s.tx_bytes, s.rx_bytes, s.last_handshake);
                    if s.last_handshake != 0 {
                        handshake_seen = true;
                        break;
                    }
                }
                if ping(&dns_ip) {
                    handshake_seen = true;
                    break;
                }
            }

            if *this.generation.lock().unwrap() != my_generation {
                return;
            }

            if handshake_seen {
                *this.status.lock().unwrap() = ("connected".to_string(), None);
                emit("vpn:status", serde_json::json!({ "state": "connected", "detail": null }));
                emit("vpn:log", serde_json::json!("Connected."));

                let mut applied = this.applied_destination_routes(); // keeps the mandatory server-endpoint route
                applied.extend(destination_routes::apply(&split_tunnel_config.destinations, &physical).await);
                this.set_applied_destination_routes(applied);

                let emit_for_log = emit.clone();
                app_tunnel.start(split_tunnel_config.apps.clone(), physical.clone(), resources_dir_for_apps, move |msg| {
                    emit_for_log("vpn:log", serde_json::json!(msg));
                });
            } else {
                emit("vpn:log", serde_json::json!(format!("No handshake after retries ({last_stats_log}).")));
                this.teardown_active();
                this.fail_and_clear_routes(
                    my_generation,
                    &emit,
                    "WireGuard adapter came up but never completed a handshake — check your connection and try again."
                        .to_string(),
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

    /// Same as `fail`, but also removes whatever destination routes (at minimum the mandatory
    /// server-endpoint exception route) were already applied before the failure — used by every
    /// error path that runs after that route goes in, so a failed connect never leaves it behind.
    fn fail_and_clear_routes(&self, my_generation: u64, emit: &impl Fn(&str, serde_json::Value), message: String) -> String {
        destination_routes::clear_all(&self.applied_destination_routes());
        self.set_applied_destination_routes(vec![]);
        self.fail(my_generation, emit, message)
    }

    pub async fn disconnect(&self) {
        {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
        }
        destination_routes::clear_all(&self.applied_destination_routes());
        self.set_applied_destination_routes(vec![]);
        self.teardown_active();
        *self.status.lock().unwrap() = ("disconnected".to_string(), None);
    }
}

pub(crate) fn decode_key(b64: &str) -> Result<[u8; 32], String> {
    let bytes = STANDARD.decode(b64).map_err(|e| format!("Invalid key: {e}"))?;
    bytes.try_into().map_err(|_| "Invalid key length".to_string())
}

pub(crate) async fn resolve_ipv4(host: &str) -> Option<Ipv4Addr> {
    if let Ok(addr) = host.parse::<Ipv4Addr>() {
        return Some(addr);
    }
    let addrs = lookup_host((host, 0)).await.ok()?;
    for addr in addrs {
        if let std::net::IpAddr::V4(v4) = addr.ip() {
            return Some(v4);
        }
    }
    None
}

fn run_powershell_checked(script: &str) -> Result<(), String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output().map_err(|e| format!("Failed to run PowerShell: {e}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("PowerShell command failed: {}", if detail.is_empty() { "unknown error".to_string() } else { detail }));
    }
    Ok(())
}

/// Assigns the tunnel IP, DNS, and a full-tunnel default route to the adapter. The default route
/// is split into two /1 routes rather than one literal 0.0.0.0/0 — the same trick the official
/// WireGuard client uses — so it always wins over the existing physical default route regardless
/// of that route's metric, without creating an ambiguous duplicate 0.0.0.0/0 entry.
pub(crate) fn configure_interface(name: &str, assigned_ip: &str, dns: &str) -> Result<(), String> {
    run_powershell_checked(&format!(
        "New-NetIPAddress -InterfaceAlias '{name}' -IPAddress {assigned_ip} -PrefixLength 32 -ErrorAction Stop | Out-Null"
    ))?;
    run_powershell_checked(&format!(
        "Set-DnsClientServerAddress -InterfaceAlias '{name}' -ServerAddresses ('{dns}') -ErrorAction Stop | Out-Null"
    ))?;
    run_powershell_checked(&format!(
        "New-NetRoute -DestinationPrefix 0.0.0.0/1 -InterfaceAlias '{name}' -NextHop 0.0.0.0 -ErrorAction Stop | Out-Null"
    ))?;
    run_powershell_checked(&format!(
        "New-NetRoute -DestinationPrefix 128.0.0.0/1 -InterfaceAlias '{name}' -NextHop 0.0.0.0 -ErrorAction Stop | Out-Null"
    ))?;
    Ok(())
}

pub(crate) fn ping(ip: &str) -> bool {
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
