use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand_core::OsRng;
use wg_stego::{stego_encode, stego_decode};

#[test]
fn stego_roundtrip() {
    let data = b"hello wireguard stego tunnel";
    let encoded = stego_encode(data);
    let decoded = stego_decode(&encoded).expect("decode failed");
    assert_eq!(&decoded, data);
}

#[test]
fn stego_handshake_and_data() {
    let sk_i = StaticSecret::random_from_rng(OsRng);
    let pk_i = PublicKey::from(&sk_i);
    let sk_r = StaticSecret::random_from_rng(OsRng);
    let pk_r = PublicKey::from(&sk_r);

    let mut init = Tunn::new(sk_i, pk_r, None, Some(25), 0, None);
    let mut resp = Tunn::new(sk_r, pk_i, None, Some(25), 1, None);
    let mut buf = vec![0u8; 65536];

    // Step 1: init → stego → resp
    let raw = match init.format_handshake_initiation(&mut buf, false) {
        TunnResult::WriteToNetwork(d) => d.to_vec(), _ => panic!("no init"),
    };
    let decoded = stego_decode(&stego_encode(&raw)).unwrap();
    assert_eq!(raw, decoded);

    // Step 2: resp processes → stego → init
    let raw2 = match resp.decapsulate(None, &decoded, &mut buf) {
        TunnResult::WriteToNetwork(d) => d.to_vec(), _ => panic!("no resp"),
    };
    let decoded2 = stego_decode(&stego_encode(&raw2)).unwrap();

    // Step 3: init completes handshake
    match init.decapsulate(None, &decoded2, &mut buf) {
        TunnResult::Done | TunnResult::WriteToNetwork(_) => {}
        other => panic!("handshake fail: {:?}", other),
    }

    // Step 4: data through stego
    let pkt = b"\x45\x00\x00\x1c\x00\x00\x00\x00\x40\x01\x00\x00\x0a\x00\x00\x02\x0a\x00\x00\x01\x08\x00\x00\x00\x00\x00\x00\x00";
    let enc = match init.encapsulate(pkt, &mut buf) {
        TunnResult::WriteToNetwork(d) => d.to_vec(), _ => panic!("no enc"),
    };
    let dec_enc = stego_decode(&stego_encode(&enc)).unwrap();
    match resp.decapsulate(None, &dec_enc, &mut buf) {
        TunnResult::WriteToTunnelV4(d, _) => {
            assert_eq!(&d[..pkt.len()], pkt);
            eprintln!("stego handshake + data: OK");
        }
        other => panic!("decrypt fail: {:?}", other),
    }
}
