use super::destination_routes::PhysicalGateway;
use super::windivert::{redirect_addr_to_interface, Handle, WinDivert};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn run_powershell(script: &str) -> std::io::Result<String> {
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output()?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CandidateApp {
    pub name: String,
    pub path: String,
}

/// Lists distinct running process names, for the split-tunnel app picker UI.
pub fn list_candidate_apps() -> Vec<CandidateApp> {
    let stdout = run_powershell(
        "Get-Process | Where-Object { $_.Path } | Select-Object -Property ProcessName, Path -Unique | ConvertTo-Json",
    )
    .unwrap_or_default();
    parse_json_array::<RawProc>(&stdout)
        .into_iter()
        .map(|p| CandidateApp {
            name: format!("{}.exe", p.process_name.to_lowercase()),
            path: p.path,
        })
        .fold(Vec::<CandidateApp>::new(), |mut acc, item| {
            if !acc.iter().any(|a| a.name == item.name) {
                acc.push(item);
            }
            acc
        })
}

#[derive(Deserialize)]
struct RawProc {
    #[serde(rename = "ProcessName")]
    process_name: String,
    #[serde(rename = "Path")]
    path: String,
}

#[derive(Deserialize)]
struct RawConn {
    #[serde(rename = "LocalPort")]
    local_port: u16,
    #[serde(rename = "OwningProcess")]
    owning_process: u32,
}

#[derive(Deserialize)]
struct RawNamedProc {
    #[serde(rename = "Id")]
    id: u32,
    #[serde(rename = "ProcessName")]
    process_name: String,
}

fn parse_json_array<T: for<'de> Deserialize<'de>>(stdout: &str) -> Vec<T> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    if let Ok(list) = serde_json::from_str::<Vec<T>>(trimmed) {
        return list;
    }
    if let Ok(single) = serde_json::from_str::<T>(trimmed) {
        return vec![single];
    }
    vec![]
}

struct SharedState {
    excluded_names: Vec<String>,
    port_to_pid: HashMap<(bool, u16), u32>, // (is_tcp, port) -> pid
    pid_to_name: HashMap<u32, String>,
}

pub struct AppTunnel {
    running: Arc<AtomicBool>,
    state: Arc<Mutex<SharedState>>,
}

impl AppTunnel {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(SharedState {
                excluded_names: vec![],
                port_to_pid: HashMap::new(),
                pid_to_name: HashMap::new(),
            })),
        }
    }

    pub fn set_excluded_apps(&self, apps: Vec<String>) {
        let mut state = self.state.lock().unwrap();
        state.excluded_names = apps.into_iter().map(|a| a.to_lowercase()).collect();
    }

    /// Starts the packet loop. Any failure here (missing WinDivert.dll, adapter not found, not
    /// elevated) is logged and swallowed — destination-based split tunneling and the VPN
    /// connection itself must keep working even if per-app enforcement can't start.
    pub fn start(&self, apps: Vec<String>, physical: PhysicalGateway, resources_dir: std::path::PathBuf, log: impl Fn(String) + Send + 'static) {
        self.stop();
        self.set_excluded_apps(apps);
        if self.state.lock().unwrap().excluded_names.is_empty() {
            return;
        }

        let dll_path = resources_dir.join("windivert").join("WinDivert.dll");
        if !dll_path.exists() {
            log("appTunnel: WinDivert.dll not bundled — per-app split tunneling is inactive (destination-based split tunneling still works).".into());
            return;
        }

        let vpn_if_idx = match get_vpn_interface_index() {
            Some(idx) => idx,
            None => {
                log("appTunnel: could not determine the VPN adapter interface index".into());
                return;
            }
        };

        let running = self.running.clone();
        let state = self.state.clone();
        running.store(true, Ordering::SeqCst);

        thread::spawn(move || {
            let windivert = match WinDivert::load(&dll_path) {
                Ok(w) => w,
                Err(e) => {
                    log(format!("appTunnel: failed to load WinDivert.dll: {e}"));
                    return;
                }
            };
            let handle = match windivert.open_outbound_on_interface(vpn_if_idx) {
                Ok(h) => h,
                Err(e) => {
                    log(format!("appTunnel: {e}"));
                    return;
                }
            };

            let refresh_running = running.clone();
            let refresh_state = state.clone();
            thread::spawn(move || {
                while refresh_running.load(Ordering::SeqCst) {
                    refresh_port_pid_map(&refresh_state);
                    thread::sleep(Duration::from_secs(1));
                }
            });

            let mut buf = vec![0u8; 65535];
            while running.load(Ordering::SeqCst) {
                let Some((len, addr)) = windivert.recv(&handle, &mut buf) else {
                    continue;
                };
                let packet = &buf[..len];
                let owner = owner_name_for_packet(packet, &state);
                let is_excluded = owner
                    .as_ref()
                    .map(|name| state.lock().unwrap().excluded_names.contains(name))
                    .unwrap_or(false);

                if is_excluded {
                    let redirected = redirect_addr_to_interface(&addr, physical.interface_index);
                    windivert.send(&handle, packet, &redirected);
                } else {
                    windivert.send(&handle, packet, &addr);
                }
            }

            windivert.close(handle);
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

fn owner_name_for_packet(packet: &[u8], state: &Arc<Mutex<SharedState>>) -> Option<String> {
    if packet.len() < 24 {
        return None;
    }
    let ihl = ((packet[0] & 0x0f) as usize) * 4;
    let proto = packet[9];
    if packet.len() < ihl + 4 {
        return None;
    }
    let src_port = u16::from_be_bytes([packet[ihl], packet[ihl + 1]]);
    let is_tcp = proto == 6;

    let state = state.lock().unwrap();
    let pid = state.port_to_pid.get(&(is_tcp, src_port))?;
    state.pid_to_name.get(pid).cloned()
}

fn refresh_port_pid_map(state: &Arc<Mutex<SharedState>>) {
    let tcp_out = run_powershell(
        "Get-NetTCPConnection -State Established,Listen -ErrorAction SilentlyContinue | Select-Object LocalPort, OwningProcess | ConvertTo-Json -Compress",
    )
    .unwrap_or_default();
    let udp_out = run_powershell(
        "Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Select-Object LocalPort, OwningProcess | ConvertTo-Json -Compress",
    )
    .unwrap_or_default();

    let tcp_rows = parse_json_array::<RawConn>(&tcp_out);
    let udp_rows = parse_json_array::<RawConn>(&udp_out);

    let mut port_to_pid = HashMap::new();
    for row in &tcp_rows {
        port_to_pid.insert((true, row.local_port), row.owning_process);
    }
    for row in &udp_rows {
        port_to_pid.insert((false, row.local_port), row.owning_process);
    }

    let pids: Vec<u32> = {
        let mut set: Vec<u32> = port_to_pid.values().cloned().collect();
        set.sort_unstable();
        set.dedup();
        set
    };
    let mut pid_to_name = HashMap::new();
    if !pids.is_empty() {
        let ids = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
        let proc_out = run_powershell(&format!(
            "Get-Process -Id {ids} -ErrorAction SilentlyContinue | Select-Object Id, ProcessName | ConvertTo-Json -Compress"
        ))
        .unwrap_or_default();
        for row in parse_json_array::<RawNamedProc>(&proc_out) {
            pid_to_name.insert(row.id, format!("{}.exe", row.process_name.to_lowercase()));
        }
    }

    let mut state = state.lock().unwrap();
    state.port_to_pid = port_to_pid;
    state.pid_to_name = pid_to_name;
}

/// The app names its own WireGuard adapter "TayyemVPN" (see `wireguard.rs`'s `INTERFACE_NAME`),
/// so this can look it up directly by name rather than guessing from its driver description.
fn get_vpn_interface_index() -> Option<u32> {
    let stdout = run_powershell(
        "Get-NetAdapter -Name 'TayyemVPN' -ErrorAction SilentlyContinue | Where-Object { $_.Status -eq 'Up' } | Select-Object -First 1 -ExpandProperty ifIndex",
    )
    .ok()?;
    stdout.trim().parse::<u32>().ok()
}
