//! Real TCP decoding, availability checks, image diffs and publication cost.
//! Run the identical fixture on both revisions; the receiver drains each event.

use std::time::Instant;

use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, run_bridge_server};
use quantick_orderbook::DepthEvent;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const IMAGES: usize = 20_000;
const ROUNDS: usize = 5;

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut best = u128::MAX;
    for round in 1..=ROUNDS {
        let ns = runtime.block_on(measure());
        println!("round {round}: {IMAGES} complete book images, {ns} ns/image");
        best = best.min(ns);
    }
    println!("best: {best} ns/image over real loopback TCP");
}

async fn measure() -> u128 {
    let mut config = ServerConfig::new("WIN");
    config.listen_addr = "127.0.0.1:0".to_owned();
    config.book_capture.enable(100);
    let (tx, mut rx) = mpsc::channel(1024);
    let server = tokio::spawn(run_bridge_server(config, tx));
    let Some(Mt5Event::Status(Mt5Status::Waiting { addr })) = rx.recv().await else {
        panic!("expected listener address");
    };
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(b"{\"type\":\"hello\",\"schema\":1,\"bridge\":\"bench\",\"bridge_version\":\"0\",\"symbol\":\"WIN\",\"broker_symbol\":\"WIN\",\"digits\":0,\"server_utc_offset_s\":0,\"book_levels\":20}\n").await.unwrap();
    let mut burst = String::new();
    for seq in 1..=IMAGES {
        // Twenty levels per side; only the top bid quantity changes.
        let bids: Vec<_> = (0..20)
            .map(|level| {
                format!(
                    "[\"{}\",\"{}\"]",
                    1000 - level * 5,
                    if level == 0 { seq % 9 + 1 } else { 10 }
                )
            })
            .collect();
        let asks: Vec<_> = (0..20)
            .map(|level| format!("[\"{}\",\"10\"]", 1005 + level * 5))
            .collect();
        burst.push_str(&format!(
            "{{\"type\":\"book\",\"seq\":{seq},\"time_ms\":{seq},\"bids\":[{}],\"asks\":[{}]}}\n",
            bids.join(","),
            asks.join(",")
        ));
    }
    let start = Instant::now();
    let writer = tokio::spawn(async move {
        socket.write_all(burst.as_bytes()).await.unwrap();
        socket
    });
    let mut images = 0;
    while images < IMAGES {
        match rx.recv().await.unwrap() {
            Mt5Event::Depth(DepthEvent::Snapshot { .. } | DepthEvent::Update { .. }) => images += 1,
            Mt5Event::Status(Mt5Status::Lost { reason }) => {
                panic!("benchmark session lost: {reason}")
            }
            _ => {}
        }
    }
    let elapsed = start.elapsed().as_nanos() / IMAGES as u128;
    drop(writer.await.unwrap());
    server.abort();
    elapsed
}
