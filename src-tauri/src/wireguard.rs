//! Shared Windows networking helpers used by the AmneziaWG client (`amneziawg.rs`) — adapter IP/
//! DNS/route setup, key decoding, and DNS resolution. The plain-WireGuard engine that used to
//! live in this file (driving `wireguard-nt.dll` directly) was retired when TayyemVPN switched
//! to the embedded AmneziaWG engine; what's left here is the OS-level plumbing that engine reuses
//! unchanged, since it operates on the adapter by name/IP and doesn't care which engine created it.

use std::net::Ipv4Addr;
use std::process::Command;
use tokio::net::lookup_host;

pub(crate) const VPN_MANAGER_BASE_URL: &str = "https://api-vpn.tayyem.dev";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

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
    // `dns` may be a single address or a comma-separated list (AmneziaWG's server config uses
    // two) — build a real PowerShell array literal either way, since a plain quoted string with
    // commas in it stays one string, not an array, to -ServerAddresses.
    let dns_array = dns
        .split(',')
        .map(|s| format!("'{}'", s.trim()))
        .collect::<Vec<_>>()
        .join(",");
    run_powershell_checked(&format!(
        "Set-DnsClientServerAddress -InterfaceAlias '{name}' -ServerAddresses @({dns_array}) -ErrorAction Stop | Out-Null"
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
