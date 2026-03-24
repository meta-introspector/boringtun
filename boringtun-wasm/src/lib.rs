//! WASM-friendly wrapper around boringtun's Tunn.
//! Exports flat functions callable from JS via wasmer-js.
//!
//! Protocol: browser ↔ WebSocket relay ↔ UDP ↔ WireGuard server
//!   Browser side: plaintext → wireguard_write → encrypted → send over WS
//!                 recv from WS → wireguard_read → plaintext → deliver

use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use std::sync::Mutex;

static TUNNEL: Mutex<Option<Tunn>> = Mutex::new(None);
static BUF: Mutex<[u8; 65536]> = Mutex::new([0u8; 65536]);

/// Initialize tunnel. Keys are 32-byte raw Curve25519.
#[no_mangle]
pub extern "C" fn wg_init(
    private_key: *const u8,
    public_key: *const u8,
    preshared_key: *const u8, // null if none
    keep_alive: u16,
    index: u32,
) -> i32 {
    let sk = unsafe { std::slice::from_raw_parts(private_key, 32) };
    let pk = unsafe { std::slice::from_raw_parts(public_key, 32) };

    let secret = StaticSecret::from(<[u8; 32]>::try_from(sk).unwrap());
    let peer = PublicKey::from(<[u8; 32]>::try_from(pk).unwrap());

    let psk = if preshared_key.is_null() {
        None
    } else {
        let p = unsafe { std::slice::from_raw_parts(preshared_key, 32) };
        Some(<[u8; 32]>::try_from(p).unwrap())
    };

    let ka = if keep_alive > 0 { Some(keep_alive) } else { None };

    let tunn = Tunn::new(secret, peer, psk, ka, index, None);
    *TUNNEL.lock().unwrap() = Some(tunn);
    0
}

/// Encrypt plaintext for sending to network.
/// Returns bytes written to out_buf, or negative on error.
/// op written to *out_op: 0=done, 1=write_to_network
#[no_mangle]
pub extern "C" fn wg_write(
    src: *const u8, src_len: usize,
    dst: *mut u8, dst_len: usize,
    out_op: *mut i32,
) -> i32 {
    let src = unsafe { std::slice::from_raw_parts(src, src_len) };
    let dst = unsafe { std::slice::from_raw_parts_mut(dst, dst_len) };
    let mut guard = TUNNEL.lock().unwrap();
    let tunn = match guard.as_mut() { Some(t) => t, None => return -1 };

    match tunn.encapsulate(src, dst) {
        TunnResult::WriteToNetwork(data) => {
            let n = data.len();
            unsafe { *out_op = 1; }
            n as i32
        }
        TunnResult::Done => { unsafe { *out_op = 0; } 0 }
        TunnResult::Err(_) => -1,
        _ => 0,
    }
}

/// Decrypt packet from network.
/// Returns bytes of plaintext written to dst, or negative on error.
/// op: 0=done, 4=ipv4_packet, 6=ipv6_packet, 1=write_to_network(handshake response)
#[no_mangle]
pub extern "C" fn wg_read(
    src: *const u8, src_len: usize,
    dst: *mut u8, dst_len: usize,
    out_op: *mut i32,
) -> i32 {
    let src = unsafe { std::slice::from_raw_parts(src, src_len) };
    let dst = unsafe { std::slice::from_raw_parts_mut(dst, dst_len) };
    let mut guard = TUNNEL.lock().unwrap();
    let tunn = match guard.as_mut() { Some(t) => t, None => return -1 };

    match tunn.decapsulate(None, src, dst) {
        TunnResult::WriteToTunnelV4(data, _) => {
            let n = data.len();
            unsafe { *out_op = 4; }
            n as i32
        }
        TunnResult::WriteToTunnelV6(data, _) => {
            let n = data.len();
            unsafe { *out_op = 6; }
            n as i32
        }
        TunnResult::WriteToNetwork(data) => {
            let n = data.len();
            unsafe { *out_op = 1; }
            n as i32
        }
        TunnResult::Done => { unsafe { *out_op = 0; } 0 }
        TunnResult::Err(_) => -1,
    }
}

/// Timer tick — call periodically. May produce handshake initiation.
#[no_mangle]
pub extern "C" fn wg_tick(dst: *mut u8, dst_len: usize, out_op: *mut i32) -> i32 {
    let dst = unsafe { std::slice::from_raw_parts_mut(dst, dst_len) };
    let mut guard = TUNNEL.lock().unwrap();
    let tunn = match guard.as_mut() { Some(t) => t, None => return -1 };

    match tunn.update_timers(dst) {
        TunnResult::WriteToNetwork(data) => {
            let n = data.len();
            unsafe { *out_op = 1; }
            n as i32
        }
        TunnResult::Done => { unsafe { *out_op = 0; } 0 }
        TunnResult::Err(_) => -1,
        _ => 0,
    }
}

/// Allocate a buffer in WASM memory (for JS to write into).
#[no_mangle]
pub extern "C" fn wg_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::with_capacity(len);
    v.resize(len, 0u8);
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr
}

/// Free a buffer allocated by wg_alloc.
#[no_mangle]
pub extern "C" fn wg_free(ptr: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(ptr, len, len)); }
}
