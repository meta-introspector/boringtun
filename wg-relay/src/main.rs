//! wg-relay: WebSocket ↔ UDP bridge for browser-based WireGuard
//!
//! Browser (boringtun.wasm) ←WebSocket→ wg-relay ←UDP→ WireGuard server
//!
//! Usage: wg-relay [ws_port] [wg_endpoint]
//!   default: ws://0.0.0.0:9719 ↔ udp://127.0.0.1:51820

use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::{TcpListener, UdpSocket};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() {
    let ws_addr: SocketAddr = std::env::args().nth(1)
        .unwrap_or_else(|| "0.0.0.0:9719".into()).parse().unwrap();
    let wg_endpoint: SocketAddr = std::env::args().nth(2)
        .unwrap_or_else(|| "127.0.0.1:51820".into()).parse().unwrap();

    let listener = TcpListener::bind(&ws_addr).await.unwrap();
    eprintln!("wg-relay ws://{} → udp://{}", ws_addr, wg_endpoint);

    while let Ok((stream, peer)) = listener.accept().await {
        let wg_endpoint = wg_endpoint;
        tokio::spawn(async move {
            let ws = match tokio_tungstenite::accept_async(stream).await {
                Ok(ws) => ws,
                Err(e) => { eprintln!("{peer}: ws error: {e}"); return; }
            };
            eprintln!("{peer}: connected");

            let udp = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            udp.connect(wg_endpoint).await.unwrap();

            let (mut ws_tx, mut ws_rx) = ws.split();
            let udp = std::sync::Arc::new(udp);
            let udp2 = udp.clone();

            // WS → UDP
            let to_udp = tokio::spawn(async move {
                while let Some(Ok(msg)) = ws_rx.next().await {
                    if let Message::Binary(data) = msg {
                        let _ = udp.send(&data).await;
                    }
                }
            });

            // UDP → WS
            let to_ws = tokio::spawn(async move {
                let mut buf = [0u8; 65536];
                loop {
                    match udp2.recv(&mut buf).await {
                        Ok(n) => {
                            if ws_tx.send(Message::Binary(buf[..n].into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });

            let _ = tokio::select! { r = to_udp => r, r = to_ws => r };
            eprintln!("{peer}: disconnected");
        });
    }
}
