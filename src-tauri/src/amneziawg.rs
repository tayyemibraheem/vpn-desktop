//! Drives the embedded AmneziaWG engine (`awgbridge.dll` + bundled `wintun.dll`) — the AmneziaWG
//! counterpart to `wireguard.rs`'s `WireguardClient`, reusing its network-setup helpers
//! (`configure_interface`, the mandatory host-exception route, the retry-loop connectivity check)
//! since those operate on the adapter by name/IP and don't care which engine created it.

use crate::amneziawg_bridge::{base64_key_to_hex, AmneziaWgBridge, AwgTunnel};
use crate::split_tunnel::app_tunnel::AppTunnel;
use crate::split_tunnel::destination_routes::{self, PhysicalGateway};
use crate::store::{self, AmneziaWgDevice, SplitTunnelConfig};
use crate::wireguard::{configure_interface, ping, resolve_ipv4, VPN_MANAGER_BASE_URL};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand_core::OsRng;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x25519_dalek::{PublicKey, StaticSecret};

const INTERFACE_NAME: &str = "TayyemVPN-AWG";
const MTU: i32 = 1420;

fn hostname_label() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".to_string())
}

#[derive(Deserialize)]
struct AmneziaWgDeviceResponse {
    id: i64,
    #[serde(rename = "assignedIp")]
    assigned_ip: String,
    #[serde(rename = "presharedKey")]
    preshared_key: String,
    #[serde(rename = "serverHostname")]
    server_hostname: String,
    #[serde(rename = "serverPublicKey")]
    server_public_key: Option<String>,
    #[serde(rename = "serverListenPort")]
    server_listen_port: Option<u16>,
    dns: Option<String>,
    jc: Option<u16>,
    jmin: Option<u16>,
    jmax: Option<u16>,
    s1: Option<u16>,
    s2: Option<u16>,
    h1: Option<u32>,
    h2: Option<u32>,
    h3: Option<u32>,
    h4: Option<u32>,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<String>,
}

/// Generates this machine's AmneziaWG keypair and registers it with vpn_manager, mirroring
/// `wireguard::WireguardClient::register_device` exactly — a separate identity from the
/// plain-WireGuard one, since the two protocols aren't interchangeable.
async fn register_device(access_token: &str, emit: &impl Fn(&str, serde_json::Value)) -> Result<AmneziaWgDevice, String> {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let private_b64 = STANDARD.encode(secret.as_bytes());
    let public_b64 = STANDARD.encode(public.as_bytes());

    emit("vpn:log", serde_json::json!("Registering this device with the VPN..."));

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{VPN_MANAGER_BASE_URL}/api/devices/register-awg"))
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

    let body: AmneziaWgDeviceResponse = resp.json().await.map_err(|e| format!("Unexpected response from the VPN service: {e}"))?;
    let server_public_key = body
        .server_public_key
        .ok_or_else(|| "This server has no AmneziaWG key configured yet".to_string())?;
    let server_listen_port = body
        .server_listen_port
        .ok_or_else(|| "This server has no AmneziaWG port configured yet".to_string())?;

    Ok(AmneziaWgDevice {
        device_id: body.id,
        private_key: private_b64,
        preshared_key: body.preshared_key,
        assigned_ip: body.assigned_ip,
        server_hostname: body.server_hostname,
        server_public_key,
        server_listen_port,
        dns: body.dns.unwrap_or_else(|| "1.1.1.1,8.8.8.8".to_string()),
        jc: body.jc.ok_or_else(|| "Server did not return Jc".to_string())?,
        jmin: body.jmin.ok_or_else(|| "Server did not return Jmin".to_string())?,
        jmax: body.jmax.ok_or_else(|| "Server did not return Jmax".to_string())?,
        s1: body.s1.ok_or_else(|| "Server did not return S1".to_string())?,
        s2: body.s2.ok_or_else(|| "Server did not return S2".to_string())?,
        h1: body.h1.ok_or_else(|| "Server did not return H1".to_string())?,
        h2: body.h2.ok_or_else(|| "Server did not return H2".to_string())?,
        h3: body.h3.ok_or_else(|| "Server did not return H3".to_string())?,
        h4: body.h4.ok_or_else(|| "Server did not return H4".to_string())?,
    })
}

struct ActiveTunnel {
    bridge: Arc<AmneziaWgBridge>,
    tunnel: AwgTunnel,
}

pub struct AmneziaWgClient {
    status: Mutex<(String, Option<String>)>,
    physical: Mutex<Option<PhysicalGateway>>,
    applied_routes: Mutex<Vec<String>>,
    generation: Mutex<u64>,
    active: Mutex<Option<ActiveTunnel>>,
}

impl AmneziaWgClient {
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

    fn teardown_active(&self) {
        if let Some(active) = self.active.lock().unwrap().take() {
            active.bridge.stop(active.tunnel);
        }
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

        self.teardown_active();

        let physical = match destination_routes::capture_original_gateway() {
            Some(p) => p,
            None => return Err(self.fail(my_generation, &emit, "Could not determine your current network gateway".to_string())),
        };
        *self.physical.lock().unwrap() = Some(physical.clone());

        let mut data = store::load();
        let device = match data.amneziawg.clone() {
            Some(d) => d,
            None => match register_device(&access_token, &emit).await {
                Ok(d) => {
                    data.amneziawg = Some(d.clone());
                    store::save(&data);
                    d
                }
                Err(e) => return Err(self.fail(my_generation, &emit, e)),
            },
        };

        let endpoint_ip = match resolve_ipv4(&device.server_hostname).await {
            Some(ip) => ip,
            None => return Err(self.fail(my_generation, &emit, format!("Could not resolve {}", device.server_hostname))),
        };

        // Same mandatory exception as the plain-WireGuard path: the tunnel's own broad routes
        // would otherwise swallow the encrypted packets addressed to the server itself.
        let endpoint_route = destination_routes::apply(&[endpoint_ip.to_string()], &physical).await;
        if endpoint_route.is_empty() {
            return Err(self.fail(my_generation, &emit, format!("Could not add a route to {endpoint_ip} via your physical network")));
        }
        self.set_applied_destination_routes(endpoint_route);

        let dll_path = resources_dir.join("amneziawg").join("amd64").join("awgbridge.dll");
        if !dll_path.exists() {
            return Err(self.fail_and_clear_routes(my_generation, &emit, "Missing bundled AmneziaWG driver — try reinstalling the app.".to_string()));
        }
        let bridge = match AmneziaWgBridge::load(&dll_path) {
            Ok(b) => Arc::new(b),
            Err(e) => return Err(self.fail_and_clear_routes(my_generation, &emit, e)),
        };

        emit("vpn:log", serde_json::json!("Creating AmneziaWG adapter..."));
        let tunnel = match bridge.start(INTERFACE_NAME, MTU) {
            Ok(t) => t,
            Err(e) => return Err(self.fail_and_clear_routes(my_generation, &emit, e)),
        };

        let uapi_config = match build_uapi_config(&device, endpoint_ip) {
            Ok(c) => c,
            Err(e) => return Err(self.fail_and_clear_routes(my_generation, &emit, e)),
        };
        if let Err(e) = bridge.configure(&tunnel, &uapi_config) {
            bridge.stop(tunnel);
            return Err(self.fail_and_clear_routes(my_generation, &emit, e));
        }

        emit("vpn:log", serde_json::json!("Configuring interface address, DNS, and routes..."));
        if let Err(e) = configure_interface(INTERFACE_NAME, &device.assigned_ip, &device.dns) {
            bridge.stop(tunnel);
            return Err(self.fail_and_clear_routes(my_generation, &emit, e));
        }

        *self.active.lock().unwrap() = Some(ActiveTunnel { bridge: bridge.clone(), tunnel });

        emit("vpn:log", serde_json::json!("Adapter up, verifying connectivity..."));

        let this = self.clone();
        let dns_ip = device.dns.clone();
        let resources_dir_for_apps = resources_dir.clone();
        tokio::spawn(async move {
            let mut handshake_seen = false;
            let mut last_stats_log = String::new();
            for attempt in 0..5 {
                tokio::time::sleep(Duration::from_secs(if attempt == 0 { 2 } else { 1 })).await;
                if *this.generation.lock().unwrap() != my_generation {
                    return;
                }
                let stats = this.active.lock().unwrap().as_ref().and_then(|a| a.bridge.peer_stats(&a.tunnel));
                if let Some(ref s) = stats {
                    last_stats_log = format!("tx={} rx={} handshake_secs={}", s.tx_bytes, s.rx_bytes, s.last_handshake_secs);
                    if s.last_handshake_secs != 0 {
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

                let mut applied = this.applied_destination_routes();
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
                    "AmneziaWG adapter came up but never completed a handshake — check your connection and try again."
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

/// Builds the UAPI `setconf` string amneziawg-go's `IpcSet` expects: device-level obfuscation
/// params, then one peer block. Keys must be hex, matching `device/uapi.go`'s `FromHex`/
/// `FromMaybeZeroHex` parsing — base64 (the `.conf`-file convention) is rejected outright.
fn build_uapi_config(device: &AmneziaWgDevice, endpoint_ip: std::net::Ipv4Addr) -> Result<String, String> {
    let private_key_hex = base64_key_to_hex(&device.private_key)?;
    let peer_public_hex = base64_key_to_hex(&device.server_public_key)?;
    let preshared_hex = base64_key_to_hex(&device.preshared_key)?;

    Ok(format!(
        "private_key={private_key_hex}\n\
         jc={jc}\njmin={jmin}\njmax={jmax}\ns1={s1}\ns2={s2}\nh1={h1}\nh2={h2}\nh3={h3}\nh4={h4}\n\
         public_key={peer_public_hex}\n\
         preshared_key={preshared_hex}\n\
         endpoint={endpoint_ip}:{port}\n\
         persistent_keepalive_interval=25\n\
         allowed_ip=0.0.0.0/0\n",
        jc = device.jc,
        jmin = device.jmin,
        jmax = device.jmax,
        s1 = device.s1,
        s2 = device.s2,
        h1 = device.h1,
        h2 = device.h2,
        h3 = device.h3,
        h4 = device.h4,
        port = device.server_listen_port,
    ))
}
