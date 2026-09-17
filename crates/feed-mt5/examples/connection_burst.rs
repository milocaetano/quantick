//! One fixed dense public-TCP timing round, for paired base/candidate runs.
//!
//! Build the identical example at each source revision and alternate processes
//! on a quiet host. Generation and admission are outside the measured interval;
//! socket writes, decoding, dispatch, bounded consumption and terminal delivery
//! are inside it. No result from this example is an application FPS claim.

use std::time::{Duration, Instant};

use quantick_engine::Side;
use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, run_bridge_server};
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const TICKS: u64 = 200_000;
const START_MS: i64 = 1_785_000_000_000;

fn burst() -> String {
    let mut bytes = String::with_capacity(36_000_000);
    for seq in 1..=TICKS {
        let time_ms = START_MS + seq as i64;
        bytes.push_str(&format!(
            "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"sent_ms\":{time_ms},\
             \"bid\":\"0\",\"ask\":\"0\",\"last\":\"{price}\",\"volume\":1,\"flags\":56}}\n",
            price = 100 + seq % 2,
        ));
    }
    bytes.push_str(concat!(
        "{\"type\":\"heartbeat\",\"time_ms\":1785000200000,\"seq_last\":200000,\"ticks_sent\":200000}\n",
        "{\"type\":\"bye\",\"reason\":\"burst_complete\"}\n",
    ));
    bytes
}

async fn run() {
    let bytes = burst();
    let input_bytes = bytes.len();
    let mut config = ServerConfig::new("BURST");
    config.listen_addr = "127.0.0.1:0".into();
    config.read_timeout = Duration::from_secs(60);
    let (tx, mut rx) = mpsc::channel(1024);
    let server = tokio::spawn(run_bridge_server(config, tx));
    let Some(Mt5Event::Status(Mt5Status::Waiting { addr })) = rx.recv().await else {
        panic!("waiting status")
    };
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(concat!(
        "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"paired-burst\",\"bridge_version\":\"1\",",
        "\"symbol\":\"BURST\",\"broker_symbol\":\"BURST\",\"digits\":0,\"server_utc_offset_s\":0}\n",
    ).as_bytes()).await.unwrap();
    assert!(matches!(
        rx.recv().await,
        Some(Mt5Event::Status(Mt5Status::Connected { .. }))
    ));
    let started = Instant::now();
    let writer = tokio::spawn(async move { socket.write_all(bytes.as_bytes()).await.unwrap() });
    let mut live = 0_u64;
    let mut latency = 0_u64;
    let mut ids = 0_u64;
    let mut buys = 0_u64;
    loop {
        match rx.recv().await.expect("connected event channel") {
            Mt5Event::Live(trade) => {
                live += 1;
                ids += trade.agg_id;
                buys += u64::from(trade.side == Side::Buy);
            }
            Mt5Event::Latency(_) => latency += 1,
            Mt5Event::Status(Mt5Status::Lost { reason }) => {
                assert_eq!(reason, "bye: burst_complete");
                break;
            }
            other => panic!("unexpected burst event: {other:?}"),
        }
    }
    let elapsed_ns = started.elapsed().as_nanos();
    writer.await.unwrap();
    server.abort();
    assert_eq!(
        (live, latency, ids, buys),
        (199_999, 3_125, 20_000_099_999, 99_999)
    );
    println!(
        "{{\"ticks\":{TICKS},\"live\":{live},\"latency\":{latency},\"lost\":1,\"id_sum\":{ids},\"buys\":{buys},\"input_bytes\":{input_bytes},\"elapsed_ns\":{elapsed_ns}}}"
    );
}

fn main() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(run());
}
