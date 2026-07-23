//! Golden test for volume bars, including the large-trade boundary case.

use quantick_engine::{VolumeBarBuilder, fixture, golden};
use rust_decimal::Decimal;
use std::str::FromStr as _;

const TRADES: &str = include_str!("fixtures/volume_trades.csv");
const EXPECTED: &str = include_str!("fixtures/volume_t5_expected.csv");

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

#[test]
fn volume_bars_match_golden() {
    golden::assert_golden(|| VolumeBarBuilder::new(dec("5.0")), TRADES, EXPECTED);
}

#[test]
fn large_trade_forms_its_own_overshooting_bar() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut VolumeBarBuilder::new(dec("5.0")), &trades);
    // Bar 1 is the single 8.0-unit trade: whole, overshooting 5.0, not split.
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[1].trade_count, 1, "the big trade is not split");
    assert_eq!(
        bars[1].volume(),
        dec("8.0"),
        "overshoot to 8.0, not capped at 5.0"
    );
    assert_eq!(bars[1].sell_volume, dec("8.0"));
}
