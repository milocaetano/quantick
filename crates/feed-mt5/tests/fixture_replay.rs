//! Golden replay of real recorded WIN$N ticks (B3 mini índice, 2026-07-23).
//!
//! The fixture was captured from a live terminal by `tools/mt5/record_ticks.py`
//! in the exact bridge wire format. Two guarantees are locked in here:
//!
//! 1. **Differential**: streaming the file through a real TCP session produces
//!    byte-identical trades to feeding the pure mapper directly — the
//!    transport layer adds and loses nothing.
//! 2. **Golden**: the mapping of this committed file is frozen (counts and
//!    endpoints), so any change to the tick-rule/mapping semantics fails
//!    loudly here instead of silently reshaping bars.

use std::time::Duration;

use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use quantick_engine::{Side, Trade};
use quantick_feed_mt5::{
    BridgeMsg, MapOutcome, Mt5Event, Mt5Status, ServerConfig, SideMode, TickMapper, parse_line,
    run_bridge_server,
};

const FIXTURE: &str = include_str!("fixtures/win_ticks.ndjson");

/// Map the fixture through the pure mapper (no I/O): the reference output.
fn reference_trades() -> (Vec<Trade>, quantick_feed_mt5::MapStats) {
    let mut mapper: Option<TickMapper> = None;
    let mut trades = Vec::new();
    for line in FIXTURE.lines().filter(|l| !l.trim().is_empty()) {
        match parse_line(line).expect("fixture lines all parse") {
            BridgeMsg::Hello(h) => {
                mapper = Some(TickMapper::new(SideMode::TickRule, h.server_utc_offset_s));
            }
            BridgeMsg::Tick(t) => {
                let m = mapper.as_mut().expect("hello precedes ticks");
                if let MapOutcome::Trade { trade, .. } = m.map(&t) {
                    trades.push(trade);
                }
            }
            _ => {}
        }
    }
    (trades, mapper.expect("fixture has a hello").stats)
}

#[test]
fn the_committed_fixture_maps_to_frozen_golden_numbers() {
    let (trades, stats) = reference_trades();

    // 1500 recorded trade ticks: the leading unchanged-price run cannot be
    // classified (honestly dropped); everything after the first price move is.
    assert_eq!(stats.trades() + stats.dropped(), 1500);
    assert_eq!(
        stats.quote_only, 0,
        "this recording had no quote-only ticks"
    );
    assert_eq!(stats.dropped(), stats.dropped_no_tick_rule_context);
    assert_eq!(stats.dropped_no_tick_rule_context, 2);
    assert_eq!(trades.len(), 1498);

    // Both sides must appear — the whole point of not trusting the broker's
    // all-BUY flags.
    let buys = trades.iter().filter(|t| t.side == Side::Buy).count();
    let sells = trades.iter().filter(|t| t.side == Side::Sell).count();
    assert!(buys > 0 && sells > 0, "buys={buys} sells={sells}");
    assert_eq!(buys + sells, 1498);

    // Timestamps: server time (BRT) converted to true UTC (+3 h), monotonic
    // non-decreasing across the whole recording.
    let first = trades.first().unwrap();
    assert_eq!(first.timestamp_ms, 1_784_824_300_832 + 10_800_000);
    assert!(
        trades
            .windows(2)
            .all(|w| w[1].timestamp_ms >= w[0].timestamp_ms),
        "recorded timestamps never go backwards"
    );

    // Synthetic ids come straight from the bridge seq.
    assert_eq!(first.agg_id, 3, "seqs 1-2 were the unclassifiable run");
    assert_eq!(trades.last().unwrap().agg_id, 1500);
}

/// Receive the next event or panic after 5 s.
async fn recv_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out")
        .expect("channel closed")
}

#[tokio::test]
async fn tcp_replay_equals_the_pure_mapper() {
    let (reference, _) = reference_trades();

    let (tx, mut rx) = mpsc::channel(4096);
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        symbol: "WIN$N".to_string(),
        side_mode: SideMode::TickRule,
        hello_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
    };
    tokio::spawn(async move {
        let _ = run_bridge_server(config, tx).await;
    });

    let Mt5Event::Status(Mt5Status::Waiting { addr }) = recv_event(&mut rx).await else {
        panic!("expected waiting");
    };
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(FIXTURE.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let mut streamed = Vec::new();
    loop {
        match recv_event(&mut rx).await {
            Mt5Event::Live(trade) => streamed.push(trade),
            Mt5Event::Backfilled(batch) => streamed.extend(batch),
            Mt5Event::Status(Mt5Status::Lost { .. }) => break,
            Mt5Event::Status(_) => {}
        }
    }

    assert_eq!(streamed.len(), reference.len());
    assert_eq!(streamed, reference, "transport adds and loses nothing");
}
