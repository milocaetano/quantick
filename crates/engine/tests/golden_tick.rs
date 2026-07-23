//! Golden test for tick bars, plus mid-stream partial and delta checks.

use quantick_engine::{BarBuilder, TickBarBuilder, fixture, golden};

const TRADES: &str = include_str!("fixtures/tick_trades.csv");
const EXPECTED_N3: &str = include_str!("fixtures/tick_n3_expected.csv");

#[test]
fn tick_bars_n3_match_golden() {
    golden::assert_golden(|| TickBarBuilder::new(3), TRADES, EXPECTED_N3);
}

#[test]
fn delta_matches_hand_computed() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut TickBarBuilder::new(3), &trades);
    // Bar 0: buy 3.0 - sell 1.5 = 1.5; Bar 1: buy 1.5 - sell 2.0 = -0.5.
    assert_eq!(bars[0].delta().to_string(), "1.5");
    assert_eq!(bars[1].delta().to_string(), "-0.5");
    assert_eq!(bars[0].volume().to_string(), "4.5");
}

#[test]
fn trailing_trade_is_a_partial_not_a_closed_bar() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let mut builder = TickBarBuilder::new(3);
    let closed = golden::replay(&mut builder, &trades);
    assert_eq!(closed.len(), 2, "two full bars closed");

    let partial = builder.partial().expect("trade 7 is forming a bar");
    assert_eq!(partial.trade_count, 1);
    assert_eq!(partial.open_time, 1600);
    assert_eq!(partial.close_time, 1600);
    assert_eq!(partial.sell_volume.to_string(), "0.5");
}
