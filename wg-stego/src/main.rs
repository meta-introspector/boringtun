//! wg-stego: WireGuard protocol in step mode over a stego pastebin
//!
//! Each Noise handshake/data message is encoded as a "paste" and exchanged
//! via any HTTP pastebin (or stdin/stdout for manual mode).
//!
//! Usage:
//!   wg-stego init <peer_pubkey_b64>              # initiator, manual mode
//!   wg-stego resp <peer_pubkey_b64>              # responder, manual mode
//!   wg-stego init <peer_pubkey_b64> <paste_url>  # initiator, pastebin mode
//!   wg-stego resp <peer_pubkey_b64> <paste_url>  # responder, pastebin mode

use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use rand_core::OsRng;
use std::io::{self, BufRead, Write};
use wg_stego::{stego_encode, stego_decode};

/// Post a paste to a pastebin URL (expects dpaste.org-compatible API).
fn paste_post(url: &str, content: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let resp = client.post(url)
        .form(&[("content", content), ("expiry_days", "1")])
        .send().map_err(|e| e.to_string())?;
    Ok(resp.url().to_string())
}

/// Fetch a paste from a URL.
fn paste_get(url: &str) -> Result<String, String> {
    let raw_url = if url.contains("dpaste.org") && !url.ends_with(".txt") {
        format!("{}.txt", url.trim_end_matches('/'))
    } else {
        url.to_string()
    };
    reqwest::blocking::get(&raw_url).map_err(|e| e.to_string())?
        .text().map_err(|e| e.to_string())
}

fn prompt(msg: &str) -> String {
    eprint!("{}", msg);
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap();
    line.trim().to_string()
}

fn send_msg(data: &[u8], paste_url: Option<&str>, step: &str) {
    let encoded = stego_encode(data);
    eprintln!("\n=== {} ({} bytes raw, {} chars stego) ===", step, data.len(), encoded.len());

    if let Some(url) = paste_url {
        match paste_post(url, &encoded) {
            Ok(link) => eprintln!("posted: {}", link),
            Err(e) => {
                eprintln!("paste failed ({}), falling back to stdout", e);
                print!("{}", encoded);
                io::stdout().flush().ok();
            }
        }
    } else {
        print!("{}", encoded);
        io::stdout().flush().ok();
    }
}

fn recv_msg(paste_url: Option<&str>, step: &str) -> Vec<u8> {
    eprintln!("\n=== waiting for {} ===", step);

    let paste = if let Some(_) = paste_url {
        let url = prompt("paste URL> ");
        paste_get(&url).expect("failed to fetch paste")
    } else {
        eprintln!("(paste stego text, end with ---)");
        let mut buf = String::new();
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.unwrap();
            buf.push_str(&line);
            buf.push('\n');
            if line.trim() == "---" { break; }
        }
        buf
    };

    stego_decode(&paste).expect("stego decode failed")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: wg-stego <init|resp> <peer_pubkey_b64> [paste_url]");
        std::process::exit(1);
    }

    let role = &args[1];
    let peer_pub_b64 = &args[2];
    let paste_url = args.get(3).map(|s| s.as_str());

    let peer_pub_bytes: [u8; 32] = base64::decode(peer_pub_b64).expect("bad pubkey")
        .try_into().expect("pubkey must be 32 bytes");
    let peer_pub = PublicKey::from(peer_pub_bytes);

    let secret = StaticSecret::random_from_rng(OsRng);
    let my_pub = PublicKey::from(&secret);
    eprintln!("my pubkey: {}", base64::encode(my_pub.as_bytes()));

    let mut tunn = Tunn::new(secret, peer_pub, None, Some(25), 0, None);
    let mut buf = vec![0u8; 65536];

    match role.as_str() {
        "init" => {
            // Step 1: send handshake init
            let init_msg = match tunn.format_handshake_initiation(&mut buf, false) {
                TunnResult::WriteToNetwork(data) => data.to_vec(),
                other => { eprintln!("unexpected: {:?}", other); return; }
            };
            send_msg(&init_msg, paste_url, "HANDSHAKE INIT");

            // Step 2: receive handshake response
            let resp_data = recv_msg(paste_url, "HANDSHAKE RESPONSE");
            match tunn.decapsulate(None, &resp_data, &mut buf) {
                TunnResult::Done => eprintln!("handshake complete (no keepalive)"),
                TunnResult::WriteToNetwork(ka) => {
                    eprintln!("handshake complete, sending keepalive");
                    send_msg(ka, paste_url, "KEEPALIVE");
                }
                other => { eprintln!("unexpected: {:?}", other); return; }
            }

            // Step 3: send data
            eprintln!("\n=== TUNNEL ESTABLISHED ===");
            loop {
                let msg = prompt("send> ");
                if msg.is_empty() || msg == "quit" { break; }
                let padded = format!("{:<28}", msg); // pad to min IP packet size
                match tunn.encapsulate(padded.as_bytes(), &mut buf) {
                    TunnResult::WriteToNetwork(data) => {
                        send_msg(data, paste_url, &format!("DATA: {}", msg));
                    }
                    other => eprintln!("encrypt error: {:?}", other),
                }
            }
        }
        "resp" => {
            // Step 1: receive handshake init
            let init_data = recv_msg(paste_url, "HANDSHAKE INIT");
            let resp_msg = match tunn.decapsulate(None, &init_data, &mut buf) {
                TunnResult::WriteToNetwork(data) => data.to_vec(),
                other => { eprintln!("unexpected: {:?}", other); return; }
            };
            send_msg(&resp_msg, paste_url, "HANDSHAKE RESPONSE");

            // Step 2: maybe receive keepalive
            eprintln!("\n=== TUNNEL ESTABLISHED (waiting for data) ===");
            loop {
                let data = recv_msg(paste_url, "DATA or KEEPALIVE");
                match tunn.decapsulate(None, &data, &mut buf) {
                    TunnResult::WriteToTunnelV4(pkt, _) | TunnResult::WriteToTunnelV6(pkt, _) => {
                        let text = String::from_utf8_lossy(pkt);
                        eprintln!("received: {}", text.trim());
                    }
                    TunnResult::Done => eprintln!("(keepalive)"),
                    TunnResult::WriteToNetwork(resp) => {
                        send_msg(resp, paste_url, "RESPONSE");
                    }
                    other => eprintln!("decrypt: {:?}", other),
                }
            }
        }
        _ => eprintln!("role must be 'init' or 'resp'"),
    }
}
