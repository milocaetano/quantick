//! Integration tests for the bridge server: a fake bridge over real TCP.
//!
//! These prove the whole in-process path — accept → hello → backfill → live →
//! goodbye — without MetaTrader, using the exact wire format the EA emits.

use std::time::Duration;

use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use quantick_engine::Side;
use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, SideMode, run_bridge_server};

/// A config bound to an ephemeral port with tight test timeouts.
fn test_config(symbol: &str) -> ServerConfig {
    ServerConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        symbol: symbol.to_string(),
        side_mode: SideMode::TickRule,
        hello_timeout: Duration::from_millis(500),
        read_timeout: Duration::from_millis(1000),
    }
}

/// Receive the next event or panic after 2 s (keeps failures readable).
async fn next_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("server closed the event channel")
}

/// Start a server and return (its bound addr, the event receiver).
async fn start_server(symbol: &str) -> (String, mpsc::Receiver<Mt5Event>) {
    let (tx, mut rx) = mpsc::channel(1024);
    let config = test_config(symbol);
    tokio::spawn(async move {
        let _ = run_bridge_server(config, tx).await;
    });
    let Mt5Event::Status(Mt5Status::Waiting { addr }) = next_event(&mut rx).await else {
        panic!("expected the initial waiting status");
    };
    (addr, rx)
}

fn hello(symbol: &str) -> String {
    format!(
        "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\
         \"symbol\":\"{symbol}\",\"broker_symbol\":\"WINQ26\",\"digits\":0,\
         \"server_utc_offset_s\":-10800}}\n"
    )
}

fn tick(seq: u64, last: &str, volume: u64) -> String {
    format!(
        "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{},\"bid\":\"0\",\"ask\":\"0\",\
         \"last\":\"{last}\",\"volume\":{volume},\"flags\":1080}}\n",
        1_784_824_300_000_i64 + seq as i64
    )
}

#[tokio::test]
async fn full_session_backfill_then_live() {
    let (addr, mut rx) = start_server("WIN$N").await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str("{\"type\":\"backfill_start\",\"count_hint\":3}\n");
    script.push_str(&tick(1, "177795", 3)); // no context → dropped
    script.push_str(&tick(2, "177800", 1)); // uptick → buy
    script.push_str(&tick(3, "177790", 2)); // downtick → sell
    script.push_str("{\"type\":\"backfill_end\"}\n");
    script.push_str(&tick(4, "177795", 1)); // live uptick → buy
    script.push_str("{\"type\":\"bye\",\"reason\":\"test_done\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected {
        symbol,
        broker_symbol,
    }) = next_event(&mut rx).await
    else {
        panic!("expected connected");
    };
    assert_eq!(symbol, "WIN$N");
    assert_eq!(broker_symbol, "WINQ26");

    let Mt5Event::Backfilled(batch) = next_event(&mut rx).await else {
        panic!("expected the backfill block");
    };
    assert_eq!(batch.len(), 2, "first tick honestly dropped (no context)");
    assert_eq!(batch[0].side, Side::Buy);
    assert_eq!(batch[1].side, Side::Sell);
    assert_eq!(batch[0].agg_id, 2, "agg_id is the bridge seq");
    // Server-time 16:31 BRT surfaces as UTC (+3 h).
    assert_eq!(batch[0].timestamp_ms, 1_784_824_300_002 + 10_800_000);

    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected a live trade");
    };
    assert_eq!(trade.side, Side::Buy);
    assert_eq!(trade.agg_id, 4);

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the session to end");
    };
    assert_eq!(reason, "bye: test_done");

    // And the server is accepting again.
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}

#[tokio::test]
async fn symbol_mismatch_is_refused() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("EURUSD").as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected refusal");
    };
    assert!(reason.contains("symbol mismatch"), "{reason}");
}

#[tokio::test]
async fn schema_mismatch_is_refused() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let old = hello("WIN$N").replace("\"schema\":1", "\"schema\":99");
    sock.write_all(old.as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected refusal");
    };
    assert!(reason.contains("schema mismatch"), "{reason}");
}

#[tokio::test]
async fn garbage_lines_are_skipped_not_fatal() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str(&tick(1, "100", 1));
    script.push_str("this is not json\n");
    script.push_str("{\"type\":\"unknown_message\"}\n");
    script.push_str(&tick(2, "101", 1)); // still classified: buy
    sock.write_all(script.as_bytes()).await.unwrap();

    // Skip the Connected status.
    let _ = next_event(&mut rx).await;
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the garbage");
    };
    assert_eq!(trade.agg_id, 2);
    assert_eq!(trade.side, Side::Buy);
}

#[tokio::test]
async fn multibyte_garbage_is_skipped_without_panicking() {
    // Regression: `snippet` used to slice at byte 120, panicking (and killing
    // the whole listener) when that byte fell inside a multibyte char.
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str(&tick(1, "100", 1));
    script.push_str(&format!("x{}\n", "é".repeat(200))); // byte 120 mid-char
    script.push_str(&tick(2, "101", 1)); // still classified: buy
    sock.write_all(script.as_bytes()).await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the multibyte garbage");
    };
    assert_eq!(trade.agg_id, 2);
}

#[tokio::test]
async fn an_invalid_utf8_line_is_skipped_not_fatal() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = Vec::new();
    script.extend_from_slice(hello("WIN$N").as_bytes());
    script.extend_from_slice(tick(1, "100", 1).as_bytes());
    script.extend_from_slice(&[0xff, 0xfe, 0xfd, b'\n']); // not UTF-8
    script.extend_from_slice(tick(2, "101", 1).as_bytes());
    sock.write_all(&script).await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the invalid utf-8 line");
    };
    assert_eq!(trade.agg_id, 2);
}

#[tokio::test]
async fn an_oversized_line_ends_the_session_but_not_the_server() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    let _ = next_event(&mut rx).await; // Connected

    // 65 KiB without a newline: crosses the 64 KiB cap, yet small enough to
    // fit in the OS socket buffers so this write never races the server
    // closing the connection.
    let flood = vec![b'a'; 65 * 1024];
    sock.write_all(&flood).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the oversized line to end the session");
    };
    assert_eq!(reason, "oversized line");
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}

#[tokio::test]
async fn a_silent_bridge_is_dropped_and_the_server_waits_again() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    let _ = next_event(&mut rx).await; // Connected

    // Send nothing further: the 1 s read timeout must end the session.
    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the silent bridge to be dropped");
    };
    assert_eq!(reason, "silent");
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}
