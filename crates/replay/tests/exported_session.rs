//! What `tools/mt5/export_session.py` writes, this crate reads.
//!
//! The fixtures are the first lines of a real export: WINQ26 on 2026-08-12, the
//! contract's expiry day, taken from a live XP terminal. Small on purpose, real
//! on purpose — a hand-written sample would agree with whatever the parser does
//! today, and the thing worth guarding is the seam between two languages.
//!
//! Regenerate with:
//!
//! ```text
//! python tools/mt5/export_session.py --symbol WINQ26 --day 2026-08-12 \
//!     --context-sessions 3 --out <folder>
//! ```

use std::path::Path;

use quantick_replay::format::ParseOptions;
use quantick_replay::{Session, parse_context};

const TAPE: &str = include_str!("fixtures/WINQ26-2026-08-12.csv");
const CONTEXT: &str = include_str!("fixtures/WINQ26-2026-08-12.context.csv");

#[test]
fn the_exported_tape_loads_as_a_session() {
    let session = Session::from_text(
        Path::new("WINQ26/2026-08-12.csv"),
        TAPE,
        ParseOptions::default(),
    )
    .expect("the exporter writes what the format declares");

    assert_eq!(session.symbol, "WINQ26");
    assert_eq!(
        session.date.map(|d| d.label()).as_deref(),
        Some("2026-08-12")
    );
    assert_eq!(session.timezone().minutes(), -180, "B3 runs at -03:00");
    assert!(!session.trades.is_empty());
    assert!(
        session
            .trades
            .windows(2)
            .all(|w| w[0].timestamp_ms <= w[1].timestamp_ms),
        "a replay plays time forward only"
    );
    assert_eq!(
        session.header.side_source.as_deref(),
        Some("venue_flags"),
        "B3 flags every print with its aggressor, so nothing was inferred"
    );
}

#[test]
fn a_crossed_opening_auction_quote_does_not_stop_the_load() {
    // MetaTrader reports the opening auction's book crossed — a bid above the
    // ask. It is real, it is what the terminal serves, and the tape's prices
    // are unaffected, so the session must still load. Anything reading the
    // spread here reads nonsense, which is the caller's problem to label, not
    // a reason to reject the recording.
    let session = Session::from_text(
        Path::new("WINQ26/2026-08-12.csv"),
        TAPE,
        ParseOptions { keep_quotes: true },
    )
    .expect("a crossed auction quote is data, not corruption");

    let crossed = session.quotes.iter().filter(|q| q.bid > q.ask).count();
    assert!(crossed > 0, "the fixture is meant to contain the auction");
}

#[test]
fn the_exported_context_folds_to_the_timeframes_a_trader_switches_between() {
    let series = parse_context(CONTEXT).expect("the exporter writes a declared interval");

    assert_eq!(series.symbol.as_deref(), Some("WINQ26"));
    assert_eq!(series.interval_ms, 60_000, "one minute, folded in the app");
    assert!(series.complete, "three sessions were asked for and served");
    assert!(!series.bars.is_empty());

    // Broker candles carry no aggressor split, and that has to keep reading as
    // "not measured" all the way from the exporter to the chart.
    for bar in &series.bars {
        assert_eq!(bar.delta(), rust_decimal::Decimal::ZERO);
        assert!(bar.volume() > rust_decimal::Decimal::ZERO);
    }

    // The context sits strictly before the tape it belongs to.
    let session = Session::from_text(
        Path::new("WINQ26/2026-08-12.csv"),
        TAPE,
        ParseOptions::default(),
    )
    .unwrap();
    assert!(
        series.end_ms().unwrap() < session.start_ms(),
        "context is the market's past, not an overlap with the recording"
    );
}
