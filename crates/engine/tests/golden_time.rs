//! Golden test for time bars, with an explicit empty-interval-policy check.

use quantick_engine::{TimeBarBuilder, fixture, golden};

const TRADES: &str = include_str!("fixtures/time_trades.csv");
const EXPECTED: &str = include_str!("fixtures/time_i1000_expected.csv");

#[test]
fn time_bars_match_golden() {
    golden::assert_golden(|| TimeBarBuilder::new(1000), TRADES, EXPECTED);
}

#[test]
fn empty_bucket_3000_produces_no_bar() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut TimeBarBuilder::new(1000), &trades);

    // Three closed bars, for buckets 1000, 2000 and 4000 — not four. The empty
    // [3000,4000) bucket is absent.
    assert_eq!(bars.len(), 3);
    let buckets: Vec<i64> = bars.iter().map(|b| b.open_time / 1000 * 1000).collect();
    assert_eq!(buckets, vec![1000, 2000, 4000]);
    assert!(
        !buckets.contains(&3000),
        "the empty interval must not be fabricated as a bar"
    );

    // The gap is visible: bar 1 closes at 2100, bar 2 opens at 4200.
    assert_eq!(bars[1].close_time, 2100);
    assert_eq!(bars[2].open_time, 4200);
}
