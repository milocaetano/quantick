//! Golden test for dollar bars, including exact-notional and boundary checks.

use quantick_engine::{DollarBarBuilder, fixture, golden};
use rust_decimal::Decimal;
use std::str::FromStr as _;

const TRADES: &str = include_str!("fixtures/dollar_trades.csv");
const EXPECTED: &str = include_str!("fixtures/dollar_t500_expected.csv");

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).unwrap()
}

#[test]
fn dollar_bars_match_golden() {
    golden::assert_golden(|| DollarBarBuilder::new(dec("500")), TRADES, EXPECTED);
}

#[test]
fn large_trade_crosses_threshold_alone() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut DollarBarBuilder::new(dec("500")), &trades);
    assert_eq!(bars.len(), 2);
    // Bar 1 is trade 3: 40000.00 * 0.020 = 800 notional, crossing 500 alone.
    assert_eq!(bars[1].trade_count, 1);
    assert_eq!(bars[1].close, dec("40000.00"));
    assert_eq!(bars[1].buy_volume, dec("0.020"));
}
