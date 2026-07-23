//! A fake bridge: replays a recorded NDJSON fixture into a running quantick.
//!
//! Lets anyone (human or AI) exercise the whole MT5 path — listener, protocol,
//! mapping, chart — without a MetaTrader terminal, using real recorded market
//! data:
//!
//! ```text
//! cargo run -p quantick-feed-mt5 --example replay_bridge -- \
//!     crates/feed-mt5/tests/fixtures/win_ticks.ndjson 127.0.0.1:9100 [--pace-us N] [--hold]
//! ```
//!
//! `--pace-us` sleeps between ticks (0 = blast). `--hold` skips the fixture's
//! `bye` and keeps the session alive with heartbeats, so the chart stays in
//! "live" mode for interactive testing.

use std::time::Duration;

use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut fixture, mut addr, mut pace_us, mut hold) = (None, None, 0u64, false);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--pace-us" => {
                pace_us = it
                    .next()
                    .ok_or("--pace-us needs a value")?
                    .parse()
                    .map_err(|e| format!("--pace-us: {e}"))?;
            }
            "--hold" => hold = true,
            other if fixture.is_none() => fixture = Some(other.to_string()),
            other if addr.is_none() => addr = Some(other.to_string()),
            other => return Err(format!("unexpected argument: {other}").into()),
        }
    }
    let fixture = fixture
        .ok_or("usage: replay_bridge <fixture.ndjson> <host:port> [--pace-us N] [--hold]")?;
    let addr = addr.unwrap_or_else(|| "127.0.0.1:9100".to_string());

    let text = std::fs::read_to_string(&fixture)?;
    let mut sock = TcpStream::connect(&addr).await?;
    eprintln!(
        "{{\"event_code\":\"REPLAY_CONNECTED\",\"addr\":\"{addr}\",\"fixture\":\"{fixture}\"}}"
    );

    let mut sent = 0u64;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if hold && line.contains("\"type\": \"bye\"") || hold && line.contains("\"type\":\"bye\"") {
            continue; // keep the session open instead of saying goodbye
        }
        sock.write_all(line.as_bytes()).await?;
        sock.write_all(b"\n").await?;
        sent += 1;
        if pace_us > 0 {
            tokio::time::sleep(Duration::from_micros(pace_us)).await;
        }
    }
    sock.flush().await?;
    eprintln!("{{\"event_code\":\"REPLAY_FILE_DONE\",\"lines_sent\":{sent},\"hold\":{hold}}}");

    if hold {
        // Keep the bridge "alive": heartbeat every 5 s until killed.
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let hb = format!(
                "{{\"type\":\"heartbeat\",\"seq_last\":{sent},\"time_ms\":0,\"ticks_sent\":{sent}}}\n"
            );
            sock.write_all(hb.as_bytes()).await?;
        }
    }
    Ok(())
}
