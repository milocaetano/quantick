//! Tick bars — close a bar every `N` trades.
//!
//! Tick bars are the simplest activity-sampled bar type: count trades, close a
//! bar every `N`. Unlike time bars, they sample faster when the market is busy
//! and slower when it's quiet, so each bar carries a comparable amount of
//! trading activity.
//!
//! Tick bars are the [generic threshold accumulator](crate::ThresholdBarBuilder)
//! with the measure "1 per trade" ([`TickMeasure`]). This module is the thin
//! tick-specific layer: an integer `N` and a convenient constructor.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, ThresholdBarBuilder, Trade, threshold::Measure};

/// The tick measure: every trade contributes exactly 1.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickMeasure;

impl Measure for TickMeasure {
    fn of(&self, _trade: &Trade) -> Decimal {
        Decimal::ONE
    }
}

/// Builds tick bars: one closed [`Bar`] per `N` trades.
///
/// A thin wrapper over [`ThresholdBarBuilder`] with [`TickMeasure`]. Feed trades
/// in order with [`push`](BarBuilder::push); every `N`th trade returns a closed
/// bar. The in-progress bar is available via [`partial`](BarBuilder::partial).
///
/// Because the measure is exactly 1 per trade, tick bars close precisely at `N`
/// and never overshoot (unlike volume and dollar bars).
#[derive(Debug, Clone)]
pub struct TickBarBuilder {
    n: u64,
    inner: ThresholdBarBuilder<TickMeasure>,
}

impl TickBarBuilder {
    /// Create a builder that closes a bar every `n` trades.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0`: a bar of zero trades is meaningless, and silently
    /// coercing it would violate the data-honesty rule.
    #[must_use]
    pub fn new(n: u64) -> Self {
        assert!(n >= 1, "tick bar size N must be >= 1, got {n}");
        Self {
            n,
            inner: ThresholdBarBuilder::new(Decimal::from(n), TickMeasure),
        }
    }

    /// The configured bar size (trades per bar).
    #[must_use]
    pub fn size(&self) -> u64 {
        self.n
    }
}

impl BarBuilder for TickBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        self.inner.push(trade)
    }

    fn partial(&self) -> Option<&Bar> {
        self.inner.partial()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    use std::str::FromStr as _;

    fn trade(agg_id: u64, ts: i64, price: &str, qty: &str, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: ts,
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str(qty).unwrap(),
            side,
        }
    }

    #[test]
    #[should_panic(expected = "tick bar size N must be >= 1")]
    fn rejects_zero_size() {
        let _ = TickBarBuilder::new(0);
    }

    #[test]
    fn size_reports_configured_n() {
        assert_eq!(TickBarBuilder::new(7).size(), 7);
    }

    #[test]
    fn n1_closes_every_trade() {
        let mut b = TickBarBuilder::new(1);
        let bar = b.push(&trade(1, 10, "100.0", "1.0", Side::Buy)).unwrap();
        assert_eq!(bar.trade_count, 1);
        assert_eq!(bar.close, Decimal::from_str("100.0").unwrap());
        assert!(b.partial().is_none(), "no partial right after a close");
    }

    #[test]
    fn partial_accumulates_mid_stream() {
        let mut b = TickBarBuilder::new(3);
        assert!(b.push(&trade(1, 10, "100.0", "1.0", Side::Buy)).is_none());
        assert!(b.push(&trade(2, 20, "101.0", "2.0", Side::Sell)).is_none());
        let p = b.partial().expect("bar forming");
        assert_eq!(p.trade_count, 2);
        assert_eq!(p.high, Decimal::from_str("101.0").unwrap());
        assert_eq!(p.low, Decimal::from_str("100.0").unwrap());
        assert_eq!(p.buy_volume, Decimal::from_str("1.0").unwrap());
        assert_eq!(p.sell_volume, Decimal::from_str("2.0").unwrap());
        assert_eq!(p.open_time, 10);
        assert_eq!(p.close_time, 20);
    }
}
