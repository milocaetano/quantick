//! Golden test for imbalance bars: warm-up, regime bar, contrary burst.

use quantick_engine::{ImbalanceBarBuilder, fixture, golden};

const TRADES: &str = include_str!("fixtures/imbalance_trades.csv");
const EXPECTED: &str = include_str!("fixtures/imbalance_t8_expected.csv");

#[test]
fn imbalance_bars_match_golden() {
    golden::assert_golden(|| ImbalanceBarBuilder::new(8), TRADES, EXPECTED);
}

#[test]
fn bar_lengths_tell_the_information_story() {
    let trades = fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut ImbalanceBarBuilder::new(8), &trades);

    // Warm-up bar: closes at exactly the 8-trade target, threshold unused.
    assert_eq!(bars[0].trade_count, 8);
    // Sell regime continues: the adapted threshold tracks it, so the second
    // bar runs a full expected length too.
    assert_eq!(bars[1].trade_count, 8);
    // Contrary buy burst closes the third bar after only 3 trades: sampling accelerates on new
    // information. Three because the threshold floor is `round(sqrt(8))` = 3 typical trades.
    assert_eq!(bars[2].trade_count, 3);
    assert!(bars[2].delta() > rust_decimal::Decimal::ZERO, "a buy bar");
    // The choppy two-way flow after it carries no imbalance, so it waits in the forming bar.
    assert_eq!(bars.len(), 3, "no bar closes on noise after the burst");
}
