//! Golden replay of real recorded US500 quotes (Tickmill index CFD,
//! 2026-07-29) — a venue that prints no trades at all.
//!
//! The recording is the counterpart of `fixture_replay.rs`: same tool, same
//! wire format, opposite world. Nothing in these 1200 ticks ever carried a
//! LAST bit, a non-zero volume, or a bid/ask spread other than 0.30, and
//! `COPY_TICKS_TRADE` over the preceding five days returned nothing — which is
//! why its hello declares `"tape": "quotes"`.
//!
//! What is locked in here:
//!
//! 1. **The declaration drives the mapping** — the mapper is configured from
//!    `hello.tape`, never from the symbol name or a guess about the broker.
//! 2. **Golden**: the synthetic prints of this committed file are frozen, so a
//!    change in quote-to-print semantics fails loudly instead of silently
//!    reshaping bars.
//! 3. **Differential**: streaming the file through a real TCP session produces
//!    byte-identical trades to feeding the pure mapper directly.

use std::time::Duration;

use rust_decimal::Decimal;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use quantick_engine::{Side, Trade};
use quantick_feed_mt5::{
    BookCaptureSwitch, BridgeMsg, HistoryPager, MapOutcome, Mt5Event, Mt5Status, ServerConfig,
    SideMode, TapeKind, TickMapper, parse_line, run_bridge_server,
};

const FIXTURE: &str = include_str!("fixtures/us500_quote_ticks.ndjson");

/// Map the fixture through the pure mapper (no I/O): the reference output.
fn reference_trades() -> (Vec<Trade>, quantick_feed_mt5::MapStats) {
    let mut mapper: Option<TickMapper> = None;
    let mut trades = Vec::new();
    for line in FIXTURE.lines().filter(|l| !l.trim().is_empty()) {
        match parse_line(line).expect("fixture lines all parse") {
            BridgeMsg::Hello(h) => {
                // The bridge's declaration decides how ticks are read — that is
                // the whole contract this file exists to prove.
                assert_eq!(
                    h.tape,
                    TapeKind::Quotes,
                    "the recorded venue prints nothing"
                );
                mapper = Some(
                    TickMapper::new(SideMode::TickRule, h.server_utc_offset_s).with_tape(h.tape),
                );
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
fn every_quote_becomes_one_synthetic_print() {
    let (trades, stats) = reference_trades();

    // 1200 recorded quotes; only the opening run, before the mid first moved,
    // cannot be classified — and it is dropped, not guessed.
    assert_eq!(stats.trades() + stats.dropped(), 1200);
    assert_eq!(stats.dropped(), stats.dropped_no_tick_rule_context);
    assert_eq!(
        stats.dropped_missing_quote, 0,
        "both sides quoted throughout"
    );
    assert_eq!(
        stats.dropped_zero_volume, 0,
        "volume is never consulted here"
    );
    assert_eq!(
        stats.quote_only, 0,
        "a quote is the data on this venue, not something skipped"
    );
    assert_eq!(
        stats.trades_from_quotes,
        stats.trades(),
        "every print here is synthetic, and says so"
    );

    // Every print carries exactly one unit: the tick, never a traded size.
    assert!(
        trades.iter().all(|t| t.quantity == Decimal::ONE),
        "synthetic prints never claim volume"
    );

    // Both sides appear: the tick rule reads a real two-way market.
    let buys = trades.iter().filter(|t| t.side == Side::Buy).count();
    let sells = trades.iter().filter(|t| t.side == Side::Sell).count();
    assert!(buys > 0 && sells > 0, "buys={buys} sells={sells}");
    assert_eq!(buys + sells, trades.len());

    // Timestamps: Tickmill's server runs UTC+3, so true UTC is 3 h *earlier* —
    // the opposite sign from the B3 recording, and it must not be assumed.
    let first = trades.first().unwrap();
    assert!(
        trades
            .windows(2)
            .all(|w| w[1].timestamp_ms >= w[0].timestamp_ms),
        "recorded timestamps never go backwards"
    );

    // Frozen: the exact shape of this committed recording.
    assert_eq!(stats.dropped_no_tick_rule_context, 1);
    assert_eq!(trades.len(), 1199);
    assert_eq!(
        first.agg_id, 2,
        "seq 1 was the unclassifiable opening quote"
    );
    assert_eq!(trades.last().unwrap().agg_id, 1200);
    assert_eq!(first.timestamp_ms, 1_785_327_812_517 - 10_800_000);
    assert_eq!(first.price, Decimal::from_str_exact("7446.18").unwrap());

    // This broker holds the spread at exactly 0.30 — an even number of cents —
    // so every mid lands back on the symbol's own 0.01 grid. The mapper still
    // never rounds (a half-cent survives when the spread is odd, see the unit
    // tests); it simply has nothing to round here.
    assert!(
        trades.iter().all(|t| t.price.scale() <= 2),
        "an even spread keeps every mid on the quoted grid"
    );
}

/// Receive the next event or panic after 5 s.
async fn recv_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out")
        .expect("channel closed")
}

#[tokio::test]
async fn tcp_replay_of_a_quote_venue_equals_the_pure_mapper() {
    let (reference, _) = reference_trades();

    let (tx, mut rx) = mpsc::channel(4096);
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        symbol: "US500".to_string(),
        side_mode: SideMode::TickRule,
        hello_timeout: Duration::from_secs(2),
        read_timeout: Duration::from_secs(2),
        book_capture: BookCaptureSwitch::new(),
        history_pager: HistoryPager::new(),
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
            // This venue has no book to capture, and capture is off besides.
            Mt5Event::Depth(event) => panic!("unexpected depth event: {event:?}"),
            // The recording declares no `rates` either.
            Mt5Event::Rates { bars, .. } => panic!("unexpected {} candles", bars.len()),
            // Only one client ever dials this listener.
            Mt5Event::SessionBusy { peer, .. } => panic!("unexpected second client: {peer}"),
            Mt5Event::HistoryPage { .. } => panic!("nothing asked this fixture for older ticks"),
            // A recorded session predates the deal stamp, so no reading can arrive.
            Mt5Event::DealCounter(sample) => panic!("unexpected deal reading: {sample:?}"),
            // The fixture's opening block is one `backfill_start`/`backfill_end`
            // pair; it is small enough that the bridge never slices it.
            Mt5Event::OpeningPage { .. } => panic!("this fixture sends no opening slices"),
            // A recorded session carries no `sent_ms`, so the split is
            // reported unavailable; the fixture is about bars, not lag.
            Mt5Event::Latency(_) => {}
        }
    }

    assert_eq!(streamed.len(), reference.len());
    assert_eq!(streamed, reference, "transport adds and loses nothing");
}
