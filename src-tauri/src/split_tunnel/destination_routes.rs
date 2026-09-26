use serde::{Deserialize, Serialize};
use std::process::Command;
use tokio::net::lookup_host;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PhysicalGateway {
    pub gateway_ip: String,
    pub interface_index: u32,
}

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

#[derive(Deserialize)]
struct RouteJson {
    #[serde(rename = "NextHop")]
    next_hop: String,
    #[serde(rename = "InterfaceIndex")]
    interface_index: u32,
}

/// Snapshots the default route before the WireGuard tunnel comes up and rewrites it — the only
/// reliable window to learn "what the real physical network looks like" before the tunnel's
/// AllowedIPs = 0.0.0.0/0 takes over the default route.
pub fn capture_original_gateway() -> Option<PhysicalGateway> {
    let stdout = run_powershell(
        "Get-NetRoute -DestinationPrefix '0.0.0.0/0' | Sort-Object -Property RouteMetric | \
         Select-Object -First 1 NextHop, InterfaceIndex | ConvertTo-Json",
    )
    .ok()?;
    let parsed: RouteJson = serde_json::from_str(stdout.trim()).ok()?;
    Some(PhysicalGateway {
        gateway_ip: parsed.next_hop,
        interface_index: parsed.interface_index,
    })
}

fn is_ip_or_cidr(entry: &str) -> bool {
    let head = entry.split('/').next().unwrap_or("");
    head.parse::<std::net::Ipv4Addr>().is_ok()
}

async fn resolve_entry(entry: &str) -> Vec<String> {
    if is_ip_or_cidr(entry) {
        return vec![if entry.contains('/') {
            entry.to_string()
        } else {
            format!("{entry}/32")
        }];
    }
    // Domains behind a CDN/load balancer can rotate IPs later — this is a point-in-time
    // resolve, not a live DNS-aware route.
    match lookup_host((entry, 0)).await {
        Ok(addrs) => addrs
            .filter_map(|addr| match addr.ip() {
                std::net::IpAddr::V4(v4) => Some(format!("{v4}/32")),
                _ => None,
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// Adds a more-specific route for each excluded destination pointing back at the original
/// gateway/interface, so it wins over the tunnel's pushed 0.0.0.0/0 route.
pub async fn apply(destinations: &[String], physical: &PhysicalGateway) -> Vec<String> {
    let mut applied = Vec::new();
    for destination in destinations {
        for cidr in resolve_entry(destination).await {
            let script = format!(
                "New-NetRoute -DestinationPrefix '{cidr}' -InterfaceIndex {} -NextHop '{}' -RouteMetric 1 -ErrorAction Stop | Out-Null",
                physical.interface_index, physical.gateway_ip
            );
            if run_powershell(&script).is_ok() {
                applied.push(cidr);
            }
        }
    }
    applied
}

pub fn clear_all(applied: &[String]) {
    for cidr in applied {
        let script =
            format!("Remove-NetRoute -DestinationPrefix '{cidr}' -Confirm:$false -ErrorAction SilentlyContinue");
        let _ = run_powershell(&script);
    }
}
