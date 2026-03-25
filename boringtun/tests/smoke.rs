//! Smoke test: two tunnels do a full Noise handshake + encrypt/decrypt a packet.

use boringtun::noise::Tunn;
use boringtun::noise::TunnResult;
use boringtun::x25519::{PublicKey, StaticSecret};
use rand_core::OsRng;

#[test]
fn handshake_and_data() {
    let sk_i = StaticSecret::random_from_rng(OsRng);
    let pk_i = PublicKey::from(&sk_i);
    let sk_r = StaticSecret::random_from_rng(OsRng);
    let pk_r = PublicKey::from(&sk_r);

    let mut init = Tunn::new(sk_i, pk_r, None, Some(25), 0, None);
    let mut resp = Tunn::new(sk_r, pk_i, None, Some(25), 1, None);

    let mut buf1 = vec![0u8; 65536];
    let mut buf2 = vec![0u8; 65536];

    // Step 1: initiator produces handshake init
    let handshake_init = match init.format_handshake_initiation(&mut buf1, false) {
        TunnResult::WriteToNetwork(data) => data.to_vec(),
        other => panic!("expected handshake init, got {:?}", other),
    };
    eprintln!("handshake init: {} bytes", handshake_init.len());

    // Step 2: responder processes init, produces response
    let handshake_resp = match resp.decapsulate(None, &handshake_init, &mut buf2) {
        TunnResult::WriteToNetwork(data) => data.to_vec(),
        other => panic!("expected handshake response, got {:?}", other),
    };
    eprintln!("handshake resp: {} bytes", handshake_resp.len());

    // Step 3: initiator processes response
    match init.decapsulate(None, &handshake_resp, &mut buf1) {
        TunnResult::Done => {}
        TunnResult::WriteToNetwork(data) => {
            // might be a keepalive
            eprintln!("init sent keepalive: {} bytes", data.len());
        }
        other => panic!("expected done/keepalive, got {:?}", other),
    }

    // Step 4: send a test IP packet through the tunnel
    let test_packet = b"\x45\x00\x00\x1c\x00\x00\x00\x00\x40\x01\x00\x00\x0a\x64\x00\x02\x0a\x64\x00\x01\x08\x00\x00\x00\x00\x00\x00\x00"; // minimal IPv4
    let encrypted = match init.encapsulate(test_packet, &mut buf1) {
        TunnResult::WriteToNetwork(data) => data.to_vec(),
        other => panic!("expected encrypted packet, got {:?}", other),
    };
    eprintln!("encrypted: {} bytes", encrypted.len());

    // Step 5: responder decrypts
    match resp.decapsulate(None, &encrypted, &mut buf2) {
        TunnResult::WriteToTunnelV4(data, _) => {
            assert_eq!(&data[..test_packet.len()], test_packet);
            eprintln!("decrypted OK: {} bytes", data.len());
        }
        other => panic!("expected decrypted IPv4, got {:?}", other),
    }
}
