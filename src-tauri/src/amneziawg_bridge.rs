//! FFI binding to `awgbridge.dll` — a small Go shared library (`src-tauri/awg-bridge/`) built by
//! this repo's own CI, wrapping amneziawg-go's `device`/`tun` packages. Every exported function
//! it has is one this codebase defines and controls (see `awg-bridge/bridge.go`), so unlike
//! `wireguard_nt.rs`'s bindings to a vendor's fixed C struct ABI, there's no hand-derived byte
//! layout here to get wrong — just plain integers and null-terminated UTF-8 strings.

use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;

type AwgStartFn = unsafe extern "C" fn(*const c_char, i32) -> i32;
type AwgConfigureFn = unsafe extern "C" fn(i32, *const c_char) -> i32;
type AwgStatusFn = unsafe extern "C" fn(i32) -> *mut c_char;
type AwgFreeStringFn = unsafe extern "C" fn(*mut c_char);
type AwgStopFn = unsafe extern "C" fn(i32);

pub struct AmneziaWgBridge {
    _lib: Library, // kept alive for as long as the resolved symbols below are used
    start: AwgStartFn,
    configure: AwgConfigureFn,
    status: AwgStatusFn,
    free_string: AwgFreeStringFn,
    stop: AwgStopFn,
}

/// A running tunnel's opaque handle. Safety: the Go runtime behind it is safe to call from any
/// thread — that's the whole point of `bridge.go`'s mutex-guarded handle map.
pub struct AwgTunnel(i32);
unsafe impl Send for AwgTunnel {}

pub struct PeerStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    /// Unix seconds of the last completed handshake, or 0 if none yet.
    pub last_handshake_secs: u64,
}

impl AmneziaWgBridge {
    pub fn load(dll_path: &Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(dll_path).map_err(|e| format!("Could not load awgbridge.dll: {e}"))?;
            let start: Symbol<AwgStartFn> =
                lib.get(b"awgStart\0").map_err(|e| format!("awgbridge.dll missing awgStart: {e}"))?;
            let configure: Symbol<AwgConfigureFn> =
                lib.get(b"awgConfigure\0").map_err(|e| format!("awgbridge.dll missing awgConfigure: {e}"))?;
            let status: Symbol<AwgStatusFn> =
                lib.get(b"awgStatus\0").map_err(|e| format!("awgbridge.dll missing awgStatus: {e}"))?;
            let free_string: Symbol<AwgFreeStringFn> =
                lib.get(b"awgFreeString\0").map_err(|e| format!("awgbridge.dll missing awgFreeString: {e}"))?;
            let stop: Symbol<AwgStopFn> =
                lib.get(b"awgStop\0").map_err(|e| format!("awgbridge.dll missing awgStop: {e}"))?;

            Ok(Self {
                start: *start,
                configure: *configure,
                status: *status,
                free_string: *free_string,
                stop: *stop,
                _lib: lib,
            })
        }
    }

    /// Creates a Wintun adapter named `adapter_name` and brings up an (unconfigured) AmneziaWG
    /// device on it. Call `configure` next before any traffic can flow.
    pub fn start(&self, adapter_name: &str, mtu: i32) -> Result<AwgTunnel, String> {
        let name = CString::new(adapter_name).map_err(|e| e.to_string())?;
        let handle = unsafe { (self.start)(name.as_ptr(), mtu) };
        if handle <= 0 {
            return Err(format!("Could not start the AmneziaWG engine (code {handle})"));
        }
        Ok(AwgTunnel(handle))
    }

    /// `uapi_config` is the same key=value protocol `wg`/`awg` use for `setconf` — see
    /// `device/uapi.go`'s `handleDeviceLine`/`handlePeerLine` for the exact grammar. Keys
    /// (private_key, public_key, preshared_key) must be hex, not base64.
    pub fn configure(&self, tunnel: &AwgTunnel, uapi_config: &str) -> Result<(), String> {
        let cfg = CString::new(uapi_config).map_err(|e| e.to_string())?;
        let result = unsafe { (self.configure)(tunnel.0, cfg.as_ptr()) };
        if result != 0 {
            return Err(format!("AmneziaWG engine rejected the configuration (code {result})"));
        }
        Ok(())
    }

    /// Ground-truth handshake/traffic stats read back from the engine itself — the same role
    /// `wireguard_nt::get_peer_stats` plays for the plain-WireGuard integration.
    pub fn peer_stats(&self, tunnel: &AwgTunnel) -> Option<PeerStats> {
        let raw = unsafe { (self.status)(tunnel.0) };
        if raw.is_null() {
            return None;
        }
        let text = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { (self.free_string)(raw) };

        let mut stats = PeerStats { tx_bytes: 0, rx_bytes: 0, last_handshake_secs: 0 };
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("tx_bytes=") {
                stats.tx_bytes = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("rx_bytes=") {
                stats.rx_bytes = v.parse().unwrap_or(0);
            } else if let Some(v) = line.strip_prefix("last_handshake_time_sec=") {
                stats.last_handshake_secs = v.parse().unwrap_or(0);
            }
        }
        Some(stats)
    }

    pub fn stop(&self, tunnel: AwgTunnel) {
        unsafe { (self.stop)(tunnel.0) };
    }
}

/// Encodes a WireGuard/AmneziaWG base64 key as the lowercase hex the UAPI protocol expects.
pub fn base64_key_to_hex(base64_key: &str) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let bytes = STANDARD.decode(base64_key).map_err(|e| format!("Invalid key: {e}"))?;
    if bytes.len() != 32 {
        return Err("Invalid key length".to_string());
    }
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
