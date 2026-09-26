//! Thin FFI binding to `wireguard.dll` (wireguard-nt v1.1, https://download.wireguard.com/wireguard-nt/),
//! the official embeddable driver-loading library WireGuard's own developers publish specifically
//! for baking WireGuard into a third-party Windows app without depending on their standalone GUI
//! client. The DLL is bundled as a resource (`resources/wireguard-nt/amd64/wireguard.dll`) and
//! loaded dynamically at runtime, the same pattern already used for WinDivert.dll.
//!
//! WARNING: the `Wg*` struct layouts below were hand-derived from wireguard-nt's public
//! `wireguard.h` (v1.1) by walking its field order/alignment rules, with no Windows machine
//! available to compile/run/verify the result against the real ABI. Each struct carries a
//! `size_of` assertion next to its definition recording the exact byte count this derivation
//! produced — if `WireGuardSetConfiguration` ever fails or silently no-ops, re-deriving these
//! offsets against a fresh copy of wireguard.h is the first thing to check. Everything else in
//! this file (loading the DLL, calling its exported functions) is a standard, low-risk
//! `libloading` binding, the same pattern already used for WinDivert.dll.

use libloading::{Library, Symbol};
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

// ---- Struct layouts mirroring wireguard.h (v1.1) -------------------------------------------
//
// Every struct below is `#[repr(C, align(8))]` to match the header's `ALIGNED(8)` structs, with
// fields in the exact order/type wireguard.h declares them in. repr(C) then reproduces C's own
// alignment/padding rules for us — the only manual work is picking matching field types (u32 in
// place of each 4-byte C enum, since Rust enums aren't guaranteed to be C-ABI-compatible ints).

#[repr(C, align(4))]
#[derive(Clone, Copy)]
struct WgSockaddrIn {
    family: u16,
    port_be: u16, // network byte order
    addr: [u8; 4],
    zero: [u8; 8],
    _reserved_to_sockaddr_inet_size: [u8; 12], // pads the sockaddr_in6-sized SOCKADDR_INET union
}
const _: () = assert!(std::mem::size_of::<WgSockaddrIn>() == 28);

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgAllowedIp {
    address: [u8; 16], // union of IN_ADDR(4)/IN6_ADDR(16); IPv4 uses the first 4 bytes
    address_family: u16,
    cidr: u8,
    flags: u32, // WIREGUARD_ALLOWED_IP_FLAG — 0 for "add" (the only case this app needs)
}
const _: () = assert!(std::mem::size_of::<WgAllowedIp>() == 24);

const WG_PEER_HAS_PUBLIC_KEY: u32 = 1 << 0;
const WG_PEER_HAS_PERSISTENT_KEEPALIVE: u32 = 1 << 2;
const WG_PEER_HAS_ENDPOINT: u32 = 1 << 3;

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgPeer {
    flags: u32,
    reserved: u32,
    public_key: [u8; 32],
    preshared_key: [u8; 32],
    persistent_keepalive: u16,
    endpoint: WgSockaddrIn,
    tx_bytes: u64,
    rx_bytes: u64,
    last_handshake: u64,
    allowed_ips_count: u32,
}
const _: () = assert!(std::mem::size_of::<WgPeer>() == 136);

const WG_INTERFACE_HAS_PRIVATE_KEY: u32 = 1 << 1;

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct WgInterface {
    flags: u32,
    listen_port: u16,
    private_key: [u8; 32],
    public_key: [u8; 32],
    peers_count: u32,
}
const _: () = assert!(std::mem::size_of::<WgInterface>() == 80);

const WG_ADAPTER_STATE_UP: u32 = 1;
const WG_ADAPTER_STATE_DOWN: u32 = 0;
const AF_INET: u16 = 2;

type CreateAdapterFn = unsafe extern "system" fn(*const u16, *const u16, *const c_void) -> *mut c_void;
type OpenAdapterFn = unsafe extern "system" fn(*const u16) -> *mut c_void;
type CloseAdapterFn = unsafe extern "system" fn(*mut c_void);
type SetConfigurationFn = unsafe extern "system" fn(*mut c_void, *const WgInterface, u32) -> i32;
type GetConfigurationFn = unsafe extern "system" fn(*mut c_void, *mut u8, *mut u32) -> i32;
type SetAdapterStateFn = unsafe extern "system" fn(*mut c_void, u32) -> i32;
type GetRunningDriverVersionFn = unsafe extern "system" fn() -> u32;

/// Owns the loaded `wireguard.dll` and its resolved function pointers. Must outlive every
/// `WgAdapter` handle it created.
pub struct WireGuardNt {
    _lib: Library, // kept alive for as long as the resolved symbols below are used
    create_adapter: CreateAdapterFn,
    open_adapter: OpenAdapterFn,
    close_adapter: CloseAdapterFn,
    set_configuration: SetConfigurationFn,
    get_configuration: GetConfigurationFn,
    set_adapter_state: SetAdapterStateFn,
    get_running_driver_version: GetRunningDriverVersionFn,
}

pub struct WgAdapter(*mut c_void);
// Safety: the handle is just an opaque driver-owned pointer; wireguard.dll's own docs describe it
// as safe to use from a single owning thread at a time, which is how this app uses it.
unsafe impl Send for WgAdapter {}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

impl WireGuardNt {
    pub fn load(dll_path: &Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(dll_path).map_err(|e| format!("Could not load wireguard.dll: {e}"))?;
            let create_adapter: Symbol<CreateAdapterFn> = lib
                .get(b"WireGuardCreateAdapter\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardCreateAdapter: {e}"))?;
            let open_adapter: Symbol<OpenAdapterFn> = lib
                .get(b"WireGuardOpenAdapter\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardOpenAdapter: {e}"))?;
            let close_adapter: Symbol<CloseAdapterFn> = lib
                .get(b"WireGuardCloseAdapter\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardCloseAdapter: {e}"))?;
            let set_configuration: Symbol<SetConfigurationFn> = lib
                .get(b"WireGuardSetConfiguration\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardSetConfiguration: {e}"))?;
            let get_configuration: Symbol<GetConfigurationFn> = lib
                .get(b"WireGuardGetConfiguration\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardGetConfiguration: {e}"))?;
            let set_adapter_state: Symbol<SetAdapterStateFn> = lib
                .get(b"WireGuardSetAdapterState\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardSetAdapterState: {e}"))?;
            let get_running_driver_version: Symbol<GetRunningDriverVersionFn> = lib
                .get(b"WireGuardGetRunningDriverVersion\0")
                .map_err(|e| format!("wireguard.dll missing WireGuardGetRunningDriverVersion: {e}"))?;

            let create_adapter = *create_adapter;
            let open_adapter = *open_adapter;
            let close_adapter = *close_adapter;
            let set_configuration = *set_configuration;
            let get_configuration = *get_configuration;
            let set_adapter_state = *set_adapter_state;
            let get_running_driver_version = *get_running_driver_version;

            Ok(Self {
                _lib: lib,
                create_adapter,
                open_adapter,
                close_adapter,
                set_configuration,
                get_configuration,
                set_adapter_state,
                get_running_driver_version,
            })
        }
    }

    /// Creates the adapter — `name` becomes the adapter's Windows display name/interface alias,
    /// so `New-NetIPAddress -InterfaceAlias name` etc. can target it afterwards. Falls back to
    /// re-opening an existing adapter of the same name (left behind by a prior crash) rather than
    /// failing outright.
    pub fn create_adapter(&self, name: &str) -> Result<WgAdapter, String> {
        let name_w = wide(name);
        let tunnel_type_w = wide("WireGuard");
        let handle = unsafe { (self.create_adapter)(name_w.as_ptr(), tunnel_type_w.as_ptr(), std::ptr::null()) };
        if !handle.is_null() {
            return Ok(WgAdapter(handle));
        }
        let reopened = unsafe { (self.open_adapter)(name_w.as_ptr()) };
        if !reopened.is_null() {
            return Ok(WgAdapter(reopened));
        }
        Err(format!("WireGuardCreateAdapter failed (GetLastError={})", unsafe { get_last_error() }))
    }

    /// Returns the loaded driver's version, or `None` if it couldn't be determined — purely
    /// diagnostic, logged on connect so a failure elsewhere can be cross-checked against it.
    pub fn driver_version(&self) -> Option<u32> {
        let version = unsafe { (self.get_running_driver_version)() };
        if version == 0 {
            None
        } else {
            Some(version)
        }
    }

    pub fn set_configuration(
        &self,
        adapter: &WgAdapter,
        private_key: [u8; 32],
        peer_public_key: [u8; 32],
        endpoint_ipv4: [u8; 4],
        endpoint_port: u16,
    ) -> Result<(), String> {
        let mut iface: WgInterface = unsafe { std::mem::zeroed() };
        iface.flags = WG_INTERFACE_HAS_PRIVATE_KEY;
        iface.private_key = private_key;
        iface.peers_count = 1;

        let mut peer: WgPeer = unsafe { std::mem::zeroed() };
        peer.flags = WG_PEER_HAS_PUBLIC_KEY | WG_PEER_HAS_ENDPOINT | WG_PEER_HAS_PERSISTENT_KEEPALIVE;
        peer.public_key = peer_public_key;
        peer.persistent_keepalive = 25;
        peer.endpoint.family = AF_INET;
        peer.endpoint.port_be = endpoint_port.to_be();
        peer.endpoint.addr = endpoint_ipv4;
        peer.allowed_ips_count = 1;

        // Full tunnel: one allowed-ip entry covering 0.0.0.0/0. This only controls which
        // *outgoing* packets the driver's own crypto-routing sends to this peer — it does not by
        // itself touch the OS routing table (that's done separately after `set_adapter_up`).
        let mut allowed_ip: WgAllowedIp = unsafe { std::mem::zeroed() };
        allowed_ip.address_family = AF_INET;
        allowed_ip.cidr = 0;

        let mut buf = Vec::with_capacity(
            std::mem::size_of::<WgInterface>() + std::mem::size_of::<WgPeer>() + std::mem::size_of::<WgAllowedIp>(),
        );
        buf.extend_from_slice(struct_bytes(&iface));
        buf.extend_from_slice(struct_bytes(&peer));
        buf.extend_from_slice(struct_bytes(&allowed_ip));

        let ok = unsafe { (self.set_configuration)(adapter.0, buf.as_ptr() as *const WgInterface, buf.len() as u32) };
        if ok == 0 {
            return Err(format!("WireGuardSetConfiguration failed (GetLastError={})", unsafe { get_last_error() }));
        }
        Ok(())
    }

    pub fn set_adapter_up(&self, adapter: &WgAdapter) -> Result<(), String> {
        let ok = unsafe { (self.set_adapter_state)(adapter.0, WG_ADAPTER_STATE_UP) };
        if ok == 0 {
            return Err(format!("WireGuardSetAdapterState(UP) failed (GetLastError={})", unsafe { get_last_error() }));
        }
        Ok(())
    }

    /// Best-effort — `close_adapter` removes the adapter regardless, so a failure here isn't fatal.
    pub fn set_adapter_down(&self, adapter: &WgAdapter) {
        unsafe {
            (self.set_adapter_state)(adapter.0, WG_ADAPTER_STATE_DOWN);
        }
    }

    /// Also removes the network adapter entirely, since it was obtained via `create_adapter`.
    pub fn close_adapter(&self, adapter: WgAdapter) {
        unsafe { (self.close_adapter)(adapter.0) };
    }

    /// Reads back the driver's own view of the first peer's traffic/handshake counters — ground
    /// truth for whether a handshake has actually happened, independent of (and more reliable
    /// than) an ICMP ping, which a freshly-created network adapter's firewall profile could block
    /// even once the tunnel itself is working fine.
    pub fn get_peer_stats(&self, adapter: &WgAdapter) -> Option<PeerStats> {
        let expected_size =
            std::mem::size_of::<WgInterface>() + std::mem::size_of::<WgPeer>() + std::mem::size_of::<WgAllowedIp>();
        let mut size = expected_size as u32;
        let mut buf = vec![0u8; size as usize];
        let ok = unsafe { (self.get_configuration)(adapter.0, buf.as_mut_ptr(), &mut size) };
        if ok == 0 || buf.len() < std::mem::size_of::<WgInterface>() + std::mem::size_of::<WgPeer>() {
            return None;
        }
        let peer: WgPeer = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(std::mem::size_of::<WgInterface>()) as *const WgPeer) };
        Some(PeerStats { tx_bytes: peer.tx_bytes, rx_bytes: peer.rx_bytes, last_handshake: peer.last_handshake })
    }
}

pub struct PeerStats {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    /// 100ns intervals since 1601-01-01 UTC, or 0 if no handshake has ever completed.
    pub last_handshake: u64,
}

fn struct_bytes<T>(value: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((value as *const T) as *const u8, std::mem::size_of::<T>()) }
}

#[cfg(target_os = "windows")]
unsafe fn get_last_error() -> u32 {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
    }
    GetLastError()
}

#[cfg(not(target_os = "windows"))]
unsafe fn get_last_error() -> u32 {
    0
}
