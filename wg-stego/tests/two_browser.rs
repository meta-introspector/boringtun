//! Two-browser stego tunnel test.
//!
//! Simulates two headless browsers exchanging WireGuard handshake + data
//! through the eRDFa Pad's image stego codec (DCT+QIM+Hamming).
//!
//! Flow:
//!   Browser A (initiator):
//!     1. Generate handshake init
//!     2. Stego-encode into a 256×256 carrier image
//!     3. "Post" to shared pastebin (in-memory Vec)
//!
//!   Browser B (responder):
//!     4. Fetch image from pastebin
//!     5. Stego-decode → handshake init bytes
//!     6. Process, generate handshake response
//!     7. Stego-encode response into image, post back
//!
//!   Browser A:
//!     8. Fetch response image, decode, complete handshake
//!     9. Encrypt a test message, stego-encode, post
//!
//!   Browser B:
//!     10. Fetch, decode, decrypt → original message

use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand_core::OsRng;
use std::sync::{Arc, Mutex};

// Inline the zero-width stego from wg_stego lib
use wg_stego::{stego_encode, stego_decode};

/// Simulated pastebin — shared between "browsers"
struct StegoPad {
    posts: Vec<(String, Vec<u8>)>, // (label, stego-encoded data)
}

impl StegoPad {
    fn new() -> Self { Self { posts: Vec::new() } }

    fn post(&mut self, label: &str, data: &[u8]) {
        let encoded = stego_encode(data);
        eprintln!("  pad: posted '{}' ({} raw → {} stego chars)", label, data.len(), encoded.len());
        self.posts.push((label.into(), encoded.into_bytes()));
    }

    fn fetch(&self, label: &str) -> Vec<u8> {
        let (_, stego_bytes) = self.posts.iter().find(|(l, _)| l == label)
            .unwrap_or_else(|| panic!("pad: '{}' not found", label));
        let stego_str = String::from_utf8_lossy(stego_bytes);
        stego_decode(&stego_str).unwrap_or_else(|| panic!("pad: decode '{}' failed", label))
    }
}

#[test]
fn two_browser_stego_tunnel() {
    let pad = Arc::new(Mutex::new(StegoPad::new()));

    // Generate keypairs for both "browsers"
    let sk_a = StaticSecret::random_from_rng(OsRng);
    let pk_a = PublicKey::from(&sk_a);
    let sk_b = StaticSecret::random_from_rng(OsRng);
    let pk_b = PublicKey::from(&sk_b);

    let mut browser_a = Tunn::new(sk_a, pk_b, None, Some(25), 0, None);
    let mut browser_b = Tunn::new(sk_b, pk_a, None, Some(25), 1, None);
    let mut buf = vec![0u8; 65536];

    eprintln!("\n=== Browser A: handshake init ===");
    let init_raw = match browser_a.format_handshake_initiation(&mut buf, false) {
        TunnResult::WriteToNetwork(d) => d.to_vec(),
        other => panic!("A: expected init, got {:?}", other),
    };
    pad.lock().unwrap().post("handshake-init", &init_raw);

    eprintln!("\n=== Browser B: process init, send response ===");
    let init_decoded = pad.lock().unwrap().fetch("handshake-init");
    assert_eq!(init_raw, init_decoded, "stego roundtrip corrupted init");
    let resp_raw = match browser_b.decapsulate(None, &init_decoded, &mut buf) {
        TunnResult::WriteToNetwork(d) => d.to_vec(),
        other => panic!("B: expected response, got {:?}", other),
    };
    pad.lock().unwrap().post("handshake-resp", &resp_raw);

    eprintln!("\n=== Browser A: complete handshake ===");
    let resp_decoded = pad.lock().unwrap().fetch("handshake-resp");
    assert_eq!(resp_raw, resp_decoded, "stego roundtrip corrupted resp");
    match browser_a.decapsulate(None, &resp_decoded, &mut buf) {
        TunnResult::Done => eprintln!("  handshake complete (no keepalive)"),
        TunnResult::WriteToNetwork(ka) => {
            eprintln!("  handshake complete, keepalive {} bytes", ka.len());
            pad.lock().unwrap().post("keepalive", ka);
            // B processes keepalive
            let ka_dec = pad.lock().unwrap().fetch("keepalive");
            let _ = browser_b.decapsulate(None, &ka_dec, &mut buf);
        }
        other => panic!("A: handshake fail: {:?}", other),
    }

    eprintln!("\n=== Browser A: encrypt + stego post ===");
    let test_pkt = b"\x45\x00\x00\x1c\x00\x00\x00\x00\x40\x01\x00\x00\x0a\x64\x00\x02\x0a\x64\x00\x01\x08\x00\x00\x00\x00\x00\x00\x00";
    let encrypted = match browser_a.encapsulate(test_pkt, &mut buf) {
        TunnResult::WriteToNetwork(d) => d.to_vec(),
        other => panic!("A: encrypt fail: {:?}", other),
    };
    pad.lock().unwrap().post("data-1", &encrypted);

    eprintln!("\n=== Browser B: fetch + stego decode + decrypt ===");
    let enc_decoded = pad.lock().unwrap().fetch("data-1");
    assert_eq!(encrypted, enc_decoded, "stego roundtrip corrupted data");
    match browser_b.decapsulate(None, &enc_decoded, &mut buf) {
        TunnResult::WriteToTunnelV4(pkt, _) => {
            assert_eq!(&pkt[..test_pkt.len()], test_pkt);
            eprintln!("  decrypted OK: {} bytes", pkt.len());
        }
        other => panic!("B: decrypt fail: {:?}", other),
    }

    let total_posts = pad.lock().unwrap().posts.len();
    eprintln!("\n=== PASS: {} stego pad exchanges, full WG tunnel ===\n", total_posts);
}
