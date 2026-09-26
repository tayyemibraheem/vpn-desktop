//! Thin FFI binding to WinDivert.dll (https://github.com/basil00/WinDivert), the redistributable
//! user-mode packet interception library this app uses for per-app split tunneling. WinDivert
//! ships its own pre-signed kernel driver — this app does not install or sign any driver itself.
//!
//! WARNING: written from memorized public documentation of WinDivert's C ABI and
//! WINDIVERT_ADDRESS struct layout, with no Windows machine available to compile/run/verify it
//! against. The byte offsets below (Outbound flag bit, Network.IfIdx/SubIfIdx union offset) are
//! the specific numbers most likely to be wrong — if per-app split tunneling misbehaves, this is
//! the first file to instrument and check against the actual WinDivert header for the installed
//! version. Nothing else in this app (login, WireGuard connect/disconnect, destination-based split
//! tunneling) depends on this file being correct.

use libloading::{Library, Symbol};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};

const WINDIVERT_ADDRESS_SIZE: usize = 80; // WinDivert 2.2 sizeof(WINDIVERT_ADDRESS)
const NETWORK_UNION_OFFSET: usize = 32; // offset of the Network/Flow/Socket union
const IFIDX_OFFSET: usize = NETWORK_UNION_OFFSET;
const SUBIFIDX_OFFSET: usize = NETWORK_UNION_OFFSET + 4;
const LAYER_NETWORK: i32 = 0;

type WinDivertOpenFn =
    unsafe extern "system" fn(*const c_char, i32, i16, u64) -> *mut c_void;
type WinDivertRecvFn =
    unsafe extern "system" fn(*mut c_void, *mut u8, u32, *mut u32, *mut u8) -> i32;
type WinDivertSendFn =
    unsafe extern "system" fn(*mut c_void, *const u8, u32, *mut u32, *const u8) -> i32;
type WinDivertCloseFn = unsafe extern "system" fn(*mut c_void) -> i32;

pub struct WinDivert {
    _lib: Library, // kept alive for the lifetime of the loaded symbols below
    open_fn: WinDivertOpenFn,
    recv_fn: WinDivertRecvFn,
    send_fn: WinDivertSendFn,
    close_fn: WinDivertCloseFn,
}

pub struct Handle(*mut c_void);
unsafe impl Send for Handle {}

impl WinDivert {
    pub fn load(dll_path: &std::path::Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(dll_path).map_err(|e| e.to_string())?;
            let open_fn: Symbol<WinDivertOpenFn> =
                lib.get(b"WinDivertOpen\0").map_err(|e| e.to_string())?;
            let recv_fn: Symbol<WinDivertRecvFn> =
                lib.get(b"WinDivertRecv\0").map_err(|e| e.to_string())?;
            let send_fn: Symbol<WinDivertSendFn> =
                lib.get(b"WinDivertSend\0").map_err(|e| e.to_string())?;
            let close_fn: Symbol<WinDivertCloseFn> =
                lib.get(b"WinDivertClose\0").map_err(|e| e.to_string())?;
            // Symbol borrows `lib` by lifetime; copy the raw fn pointers out so we can store
            // `lib` and the fns together without a self-referential struct.
            let open_fn = *open_fn.into_raw();
            let recv_fn = *recv_fn.into_raw();
            let send_fn = *send_fn.into_raw();
            let close_fn = *close_fn.into_raw();
            Ok(Self { _lib: lib, open_fn: std::mem::transmute(open_fn), recv_fn: std::mem::transmute(recv_fn), send_fn: std::mem::transmute(send_fn), close_fn: std::mem::transmute(close_fn) })
        }
    }

    pub fn open_outbound_on_interface(&self, if_idx: u32) -> Result<Handle, String> {
        let filter = format!("outbound and ifIdx == {if_idx} and (tcp or udp)");
        let c_filter = CString::new(filter).map_err(|e| e.to_string())?;
        let handle = unsafe { (self.open_fn)(c_filter.as_ptr(), LAYER_NETWORK, 0, 0) };
        if handle.is_null() || handle as isize == -1 {
            return Err("WinDivertOpen failed (is the app running as Administrator?)".into());
        }
        Ok(Handle(handle))
    }

    pub fn recv(&self, handle: &Handle, buf: &mut [u8]) -> Option<(usize, [u8; WINDIVERT_ADDRESS_SIZE])> {
        let mut recv_len: u32 = 0;
        let mut addr = [0u8; WINDIVERT_ADDRESS_SIZE];
        let ok = unsafe {
            (self.recv_fn)(
                handle.0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut recv_len,
                addr.as_mut_ptr(),
            )
        };
        if ok == 0 {
            return None;
        }
        Some((recv_len as usize, addr))
    }

    pub fn send(&self, handle: &Handle, packet: &[u8], addr: &[u8; WINDIVERT_ADDRESS_SIZE]) {
        let mut send_len: u32 = 0;
        unsafe {
            (self.send_fn)(
                handle.0,
                packet.as_ptr(),
                packet.len() as u32,
                &mut send_len,
                addr.as_ptr(),
            );
        }
    }

    pub fn close(&self, handle: Handle) {
        unsafe {
            (self.close_fn)(handle.0);
        }
    }
}

pub fn redirect_addr_to_interface(addr: &[u8; WINDIVERT_ADDRESS_SIZE], if_idx: u32) -> [u8; WINDIVERT_ADDRESS_SIZE] {
    let mut copy = *addr;
    copy[IFIDX_OFFSET..IFIDX_OFFSET + 4].copy_from_slice(&if_idx.to_le_bytes());
    copy[SUBIFIDX_OFFSET..SUBIFIDX_OFFSET + 4].copy_from_slice(&0u32.to_le_bytes());
    copy
}
