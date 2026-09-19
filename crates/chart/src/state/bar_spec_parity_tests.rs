//! The chart's half of the one-vocabulary proof: given the spec string a trader
//! would type and the engine's hand-computed golden tape, [`ChartState`] cuts
//! exactly the engine's committed bars — by backfill and by live print alike.
//!
//! The backtest asserts its half against the same files
//! (`crates/backtest/tests/harness.rs`,
//! `the_backtest_cuts_every_golden_the_chart_cuts`), and the engine pins
//! [`BarSpec::build`] to them (`crates/engine/tests/bar_spec.rs`). No crate can
//! link both consumers, so the files are what they agree through.

use super::*;
use quantick_engine::{fixture, golden};

/// The engine's golden table, spelled as the chart's config vocabulary.
const GOLDENS: [(&str, &str, &str); 5] = [
    (
        "tick:3",
        include_str!("../../../engine/tests/fixtures/tick_trades.csv"),
        include_str!("../../../engine/tests/fixtures/tick_n3_expected.csv"),
    ),
    (
        "volume:5.0",
        include_str!("../../../engine/tests/fixtures/volume_trades.csv"),
        include_str!("../../../engine/tests/fixtures/volume_t5_expected.csv"),
    ),
    (
        "dollar:500",
        include_str!("../../../engine/tests/fixtures/dollar_trades.csv"),
        include_str!("../../../engine/tests/fixtures/dollar_t500_expected.csv"),
    ),
    (
        "time:1s",
        include_str!("../../../engine/tests/fixtures/time_trades.csv"),
        include_str!("../../../engine/tests/fixtures/time_i1000_expected.csv"),
    ),
    (
        "imbalance:8",
        include_str!("../../../engine/tests/fixtures/imbalance_trades.csv"),
        include_str!("../../../engine/tests/fixtures/imbalance_t8_expected.csv"),
    ),
];

#[test]
fn the_chart_cuts_every_golden_the_engine_pins() {
    for (text, trades_csv, expected_csv) in GOLDENS {
        let spec = BarSpec::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        let trades = fixture::parse_trades(trades_csv).expect("trade fixture parses");
        let expected = fixture::parse_bars(expected_csv).expect("expected fixture parses");

        let mut backfilled = ChartState::new(spec);
        backfilled.ingest_backfill(&trades);
        if let Some(report) = golden::diff_bars(&expected, backfilled.bars()) {
            panic!("{text}, backfilled: {report}");
        }

        let mut live = ChartState::new(spec);
        for trade in &trades {
            live.ingest_live(trade);
        }
        if let Some(report) = golden::diff_bars(&expected, live.bars()) {
            panic!("{text}, live: {report}");
        }
    }
}
