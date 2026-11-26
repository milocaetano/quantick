//! Golden harness wired end-to-end against a pass-through builder: parse a trade fixture, replay
//! it through a `BarBuilder`, compare to the expected-bars fixture, with the determinism check.

use quantick_engine::{Bar, BarBuilder, Trade, golden};
use rust_decimal::Decimal;

/// The simplest possible builder: every trade closes its own one-trade bar.
/// It holds no in-progress state, so `partial()` is always `None`.
struct OneBarPerTrade;

impl BarBuilder for OneBarPerTrade {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        let (buy_volume, sell_volume) = match trade.side {
            quantick_engine::Side::Buy => (trade.quantity, Decimal::ZERO),
            quantick_engine::Side::Sell => (Decimal::ZERO, trade.quantity),
        };
        Some(Bar {
            open_time: trade.timestamp_ms,
            close_time: trade.timestamp_ms,
            open: trade.price,
            high: trade.price,
            low: trade.price,
            close: trade.price,
            buy_volume,
            sell_volume,
            trade_count: 1,
        })
    }

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        None
    }
}

const TRADES: &str = include_str!("fixtures/sample_trades.csv");
const EXPECTED: &str = include_str!("fixtures/passthrough_expected.csv");

#[test]
fn pass_through_builder_matches_golden() {
    golden::assert_golden(|| OneBarPerTrade, TRADES, EXPECTED);
}

#[test]
fn replay_produces_one_bar_per_trade() {
    let trades = quantick_engine::fixture::parse_trades(TRADES).unwrap();
    let bars = golden::replay(&mut OneBarPerTrade, &trades);
    assert_eq!(bars.len(), trades.len());
}
