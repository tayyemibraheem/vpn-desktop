use crate::split_tunnel::app_tunnel::AppTunnel;
use crate::split_tunnel::destination_routes::{self, PhysicalGateway};
use crate::store::SplitTunnelConfig;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const MANAGEMENT_PORT: u16 = 17842;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct OvpnClient {
    child: Mutex<Option<Child>>,
    status: Mutex<(String, Option<String>)>,
    physical: Mutex<Option<PhysicalGateway>>,
    applied_routes: Mutex<Vec<String>>,
    generation: Mutex<u64>, // bumped on each disconnect so a stale reader thread stops emitting
}

impl OvpnClient {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
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
            r"C:\Program Files\OpenVPN\bin\openvpn.exe",
            r"C:\Program Files (x86)\OpenVPN\bin\openvpn.exe",
        ];
        candidates.iter().map(PathBuf::from).find(|p| p.exists())
    }

    pub async fn connect(
        self: &Arc<Self>,
        username: String,
        password: String,
        resources_dir: PathBuf,
        split_tunnel_config: SplitTunnelConfig,
        app_tunnel: Arc<AppTunnel>,
        emit: impl Fn(&str, serde_json::Value) + Send + Sync + Clone + 'static,
    ) -> Result<(), String> {
        if self.child.lock().unwrap().is_some() {
            return Err("Already connecting or connected".into());
        }

        let binary = Self::find_binary()
            .ok_or_else(|| "OpenVPN is not installed. Install the community OpenVPN client from openvpn.net, then try again.".to_string())?;

        let physical = destination_routes::capture_original_gateway()
            .ok_or_else(|| "Could not determine your current network gateway".to_string())?;
        *self.physical.lock().unwrap() = Some(physical.clone());

        *self.status.lock().unwrap() = ("connecting".to_string(), None);
        emit("vpn:status", serde_json::json!({ "state": "connecting", "detail": null }));

        let config_path = resources_dir.join("tayyem-vpn.ovpn");
        let mut cmd = Command::new(&binary);
        cmd.args([
            "--config", config_path.to_str().unwrap_or_default(),
            "--management", "127.0.0.1", &MANAGEMENT_PORT.to_string(),
            "--management-query-passwords",
            "--management-hold",
            "--auth-nocache",
            "--auth-retry", "nointeract",
            "--verb", "3",
        ]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| format!("Failed to start openvpn: {e}"))?;

        if let Some(stdout) = child.stdout.take() {
            let emit_clone = emit.clone();
            thread::spawn(move || {
                for line in BufReader::new(stdout).lines().flatten() {
                    emit_clone("vpn:log", serde_json::json!(line));
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let emit_clone = emit.clone();
            thread::spawn(move || {
                for line in BufReader::new(stderr).lines().flatten() {
                    emit_clone("vpn:log", serde_json::json!(line));
                }
            });
        }

        *self.child.lock().unwrap() = Some(child);

        let my_generation = {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
            *gen
        };

        let this = self.clone();
        let emit_for_mgmt = emit.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500)); // let openvpn bind the management port
            this.run_management_session(my_generation, username, password, physical, split_tunnel_config, app_tunnel, emit_for_mgmt);
        });

        // Watches for the process dying on its own (TAP/driver failure, a rejection the
        // PASSWORD/STATE parsing above didn't catch, a crash) — without this, an unexpected
        // exit left the UI stuck showing "Connecting" forever instead of reporting a failure.
        let this = self.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            let exited = {
                let mut guard = this.child.lock().unwrap();
                match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => {
                            *guard = None;
                            Some(status)
                        }
                        _ => None,
                    },
                    None => return, // already cleaned up elsewhere (normal disconnect)
                }
            };
            if let Some(status) = exited {
                this.finish_disconnected(
                    my_generation,
                    &emit,
                    Some(format!("openvpn exited unexpectedly ({status})")),
                );
                return;
            }
        });

        Ok(())
    }

    fn run_management_session(
        self: Arc<Self>,
        my_generation: u64,
        username: String,
        password: String,
        physical: PhysicalGateway,
        split_tunnel_config: SplitTunnelConfig,
        app_tunnel: Arc<AppTunnel>,
        emit: impl Fn(&str, serde_json::Value) + Send + Sync + Clone + 'static,
    ) {
        let mut attempt = 0;
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", MANAGEMENT_PORT)) {
                Ok(s) => break s,
                Err(_) if attempt < 10 => {
                    attempt += 1;
                    thread::sleep(Duration::from_millis(300));
                }
                Err(e) => {
                    emit("vpn:log", serde_json::json!(format!("management socket error: {e}")));
                    self.finish_disconnected(my_generation, &emit, Some("Could not reach openvpn management interface".into()));
                    return;
                }
            }
        };

        let mut writer = stream.try_clone().expect("clone TcpStream for writing");
        let _ = writer.write_all(b"state on\n");
        let _ = writer.write_all(b"log on\n");
        let _ = writer.write_all(b"hold release\n");

        let reader = BufReader::new(stream);
        for line in reader.lines().flatten() {
            if *self.generation.lock().unwrap() != my_generation {
                return; // superseded by a disconnect/reconnect
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            emit("vpn:log", serde_json::json!(trimmed));

            if trimmed.starts_with(">PASSWORD:Need") && trimmed.contains("'Auth'") {
                let _ = writer.write_all(format!("username \"Auth\" {username}\n").as_bytes());
                let _ = writer.write_all(format!("password \"Auth\" {password}\n").as_bytes());
                continue;
            }
            if trimmed.starts_with(">PASSWORD:Verification Failed") {
                self.finish_disconnected(my_generation, &emit, Some("Authentication failed".into()));
                return;
            }
            if let Some(rest) = trimmed.strip_prefix(">STATE:") {
                let parts: Vec<&str> = rest.split(',').collect();
                let phase = parts.get(1).copied().unwrap_or("");
                match phase {
                    "CONNECTED" => {
                        *self.status.lock().unwrap() = ("connected".to_string(), None);
                        emit("vpn:status", serde_json::json!({ "state": "connected", "detail": null }));

                        let applied = destination_routes::apply(&split_tunnel_config.destinations, &physical);
                        // apply() is async \u2014 block this dedicated OS thread on it rather than
                        // requiring a tokio runtime here.
                        let applied = tauri::async_runtime::block_on(applied);
                        self.set_applied_destination_routes(applied);

                        let resources_dir = physical_resources_dir();
                        let emit_for_log = emit.clone();
                        app_tunnel.start(split_tunnel_config.apps.clone(), physical.clone(), resources_dir, move |msg| {
                            emit_for_log("vpn:log", serde_json::json!(msg));
                        });
                    }
                    "RECONNECTING" | "RESOLVE" | "TCP_CONNECT" | "WAIT" | "AUTH" | "GET_CONFIG" | "ASSIGN_IP" => {
                        *self.status.lock().unwrap() = ("connecting".to_string(), Some(phase.to_string()));
                        emit("vpn:status", serde_json::json!({ "state": "connecting", "detail": phase }));
                    }
                    "EXITING" => {
                        self.finish_disconnected(my_generation, &emit, None);
                        return;
                    }
                    _ => {}
                }
            }
        }

        self.finish_disconnected(my_generation, &emit, None);
    }

    fn finish_disconnected(&self, my_generation: u64, emit: &impl Fn(&str, serde_json::Value), detail: Option<String>) {
        if *self.generation.lock().unwrap() != my_generation {
            return;
        }
        *self.status.lock().unwrap() = ("disconnected".to_string(), detail.clone());
        emit("vpn:status", serde_json::json!({ "state": "disconnected", "detail": detail }));
    }

    pub async fn disconnect(&self) {
        {
            let mut gen = self.generation.lock().unwrap();
            *gen += 1;
        }
        destination_routes::clear_all(&self.applied_destination_routes());
        self.set_applied_destination_routes(vec![]);

        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        *self.status.lock().unwrap() = ("disconnected".to_string(), None);
    }
}

fn physical_resources_dir() -> PathBuf {
    // Best-effort fallback \u2014 the real resources dir is passed in from the Tauri command via
    // app.path().resource_dir() at connect() call time; this only covers the rare case where
    // app_tunnel.start() needs it again outside that path.
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("resources")))
        .unwrap_or_else(|| PathBuf::from("resources"))
}
