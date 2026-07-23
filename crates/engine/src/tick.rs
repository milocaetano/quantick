//! Tick bars — close a bar every `N` trades.
//!
//! Tick bars are the simplest activity-sampled bar type: count trades, close a
//! bar every `N`. Unlike time bars, they sample faster when the market is busy
//! and slower when it's quiet, so each bar carries a comparable amount of
//! trading activity.
//!
//! This is the first real [`BarBuilder`], and it establishes the builder
//! skeleton — accumulate an in-progress [`Bar`], finalise and emit it when the
//! bucket fills — that volume and dollar bars reuse via the generic threshold
//! accumulator (#6).

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, Side, Trade};

/// Builds tick bars: one closed [`Bar`] per `N` trades.
///
/// Feed trades in order with [`push`](BarBuilder::push); every `N`th trade
/// returns a closed bar. The in-progress bar is available via
/// [`partial`](BarBuilder::partial) for rendering the forming bar on a chart.
#[derive(Debug, Clone)]
pub struct TickBarBuilder {
    n: u64,
    count: u64,
    current: Option<Bar>,
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
            count: 0,
            current: None,
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
        match &mut self.current {
            None => self.current = Some(open_bar(trade)),
            Some(bar) => extend_bar(bar, trade),
        }
        self.count += 1;
        if self.count >= self.n {
            self.count = 0;
            self.current.take()
        } else {
            None
        }
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }
}

/// Start a fresh one-trade bar from `trade`.
fn open_bar(trade: &Trade) -> Bar {
    let (buy_volume, sell_volume) = split_volume(trade);
    Bar {
        open_time: trade.timestamp_ms,
        close_time: trade.timestamp_ms,
        open: trade.price,
        high: trade.price,
        low: trade.price,
        close: trade.price,
        buy_volume,
        sell_volume,
        trade_count: 1,
    }
}

/// Fold `trade` into the in-progress `bar`.
fn extend_bar(bar: &mut Bar, trade: &Trade) {
    bar.high = bar.high.max(trade.price);
    bar.low = bar.low.min(trade.price);
    bar.close = trade.price;
    bar.close_time = trade.timestamp_ms;
    match trade.side {
        Side::Buy => bar.buy_volume += trade.quantity,
        Side::Sell => bar.sell_volume += trade.quantity,
    }
    bar.trade_count += 1;
}

/// A trade contributes its quantity to exactly one side.
fn split_volume(trade: &Trade) -> (Decimal, Decimal) {
    match trade.side {
        Side::Buy => (trade.quantity, Decimal::ZERO),
        Side::Sell => (Decimal::ZERO, trade.quantity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
