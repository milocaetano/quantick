//! The generic threshold accumulator shared by tick, volume and dollar bars.
//!
//! Tick, volume and dollar bars are the *same* algorithm with a different
//! accumulated measure: 1 per trade (tick), traded quantity (volume), or
//! notional `price * quantity` (dollar). "One engine, three consumers" applies
//! internally too — there is one closing rule here, parameterised by a
//! [`Measure`], never three forked implementations.
//!
//! # Boundary rule: whole trades, overshoot allowed, no carry
//!
//! When a single trade pushes the accumulated measure to or past the threshold,
//! **the whole trade is included in the bar it completes** — a trade is never
//! split across two bars. A trade is an atomic market event; splitting one would
//! fabricate synthetic sub-trades that never happened, violating the
//! data-honesty rule.
//!
//! A consequence: volume and dollar bars can **overshoot** the threshold (a
//! 25-unit trade closes a 10-unit bar as a single 25-unit bar). That is honest —
//! real trades are not divisible. Tick bars never overshoot, because each trade
//! contributes exactly 1 and the bar closes precisely at `N`.
//!
//! On close the accumulator **resets to zero — the overshoot is not carried**
//! into the next bar. Each bar's accumulated measure is therefore exactly the
//! sum of its own trades' measures, with no phantom offset borrowed from a
//! previous bar. "This bar closed because its own trades reached the threshold"
//! stays literally true.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, Side, Trade};

/// How much one trade contributes toward closing a bar.
///
/// Implementors are usually zero-sized markers ([`crate::TickMeasure`],
/// volume, dollar). Custom measures let the same accumulator drive
/// application-specific bar types without forking the closing logic.
pub trait Measure {
    /// The contribution of `trade` to the running total.
    ///
    /// Must be non-negative for the closing rule to make sense.
    fn of(&self, trade: &Trade) -> Decimal;
}

/// Builds bars that close when an accumulated [`Measure`] reaches a threshold.
///
/// See the [module docs](self) for the boundary rule (whole trades, overshoot
/// allowed, no carry). Tick, volume and dollar builders are this type with a
/// different `M`.
#[derive(Debug, Clone)]
pub struct ThresholdBarBuilder<M: Measure> {
    threshold: Decimal,
    measure: M,
    acc: Decimal,
    current: Option<Bar>,
}

impl<M: Measure> ThresholdBarBuilder<M> {
    /// Create a builder that closes a bar once the accumulated measure reaches
    /// `threshold`.
    ///
    /// # Panics
    ///
    /// Panics if `threshold <= 0`: a non-positive threshold would close a bar on
    /// (or before) the first trade forever. Coercing it silently would violate
    /// the data-honesty rule.
    pub fn new(threshold: Decimal, measure: M) -> Self {
        assert!(
            threshold > Decimal::ZERO,
            "bar threshold must be > 0, got {threshold}"
        );
        Self {
            threshold,
            measure,
            acc: Decimal::ZERO,
            current: None,
        }
    }

    /// The configured threshold.
    #[must_use]
    pub fn threshold(&self) -> Decimal {
        self.threshold
    }
}

impl<M: Measure> BarBuilder for ThresholdBarBuilder<M> {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        match &mut self.current {
            None => self.current = Some(open_bar(trade)),
            Some(bar) => extend_bar(bar, trade),
        }
        // Saturating: the measure comes from untrusted feed data and must never
        // panic the builder on overflow. A saturated accumulator is `>= threshold`
        // and simply closes the bar — deterministic, and identical to exact
        // arithmetic for every representable total.
        self.acc = self.acc.saturating_add(self.measure.of(trade));
        if self.acc >= self.threshold {
            self.acc = Decimal::ZERO;
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
pub(crate) fn open_bar(trade: &Trade) -> Bar {
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
pub(crate) fn extend_bar(bar: &mut Bar, trade: &Trade) {
    bar.high = bar.high.max(trade.price);
    bar.low = bar.low.min(trade.price);
    bar.close = trade.price;
    bar.close_time = trade.timestamp_ms;
    // Saturating for the same reason as the accumulator: untrusted feed
    // quantities must not panic the builder if a running side total overflows.
    match trade.side {
        Side::Buy => bar.buy_volume = bar.buy_volume.saturating_add(trade.quantity),
        Side::Sell => bar.sell_volume = bar.sell_volume.saturating_add(trade.quantity),
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

    /// Test measure: accumulate traded quantity (a stand-in for volume bars,
    /// which land in #7). Enough to exercise the boundary rule here.
    struct QtyMeasure;
    impl Measure for QtyMeasure {
        fn of(&self, trade: &Trade) -> Decimal {
            trade.quantity
        }
    }

    fn trade(qty: &str) -> Trade {
        Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::from_str("100.0").unwrap(),
            quantity: Decimal::from_str(qty).unwrap(),
            side: Side::Buy,
        }
    }

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    #[should_panic(expected = "bar threshold must be > 0")]
    fn rejects_non_positive_threshold() {
        let _ = ThresholdBarBuilder::new(Decimal::ZERO, QtyMeasure);
    }

    #[test]
    fn a_single_trade_crossing_the_threshold_closes_a_whole_trade_bar() {
        // threshold 10, one 25-unit trade: the trade is NOT split — it closes a
        // single bar that overshoots to 25.
        let mut b = ThresholdBarBuilder::new(dec("10"), QtyMeasure);
        let bar = b
            .push(&trade("25"))
            .expect("the crossing trade closes a bar");
        assert_eq!(bar.trade_count, 1, "the whole trade, not a fragment");
        assert_eq!(bar.volume(), dec("25"), "overshoot kept, not split at 10");
        assert!(
            b.partial().is_none(),
            "accumulator reset: no partial afterwards"
        );
    }

    #[test]
    fn overshoot_is_not_carried_into_the_next_bar() {
        // 3 + 4 + 5 crosses 10 on the third trade (sum 12). The next trade
        // starts a fresh bar from zero, not from the overshoot of 2.
        let mut b = ThresholdBarBuilder::new(dec("10"), QtyMeasure);
        assert!(b.push(&trade("3")).is_none());
        assert!(b.push(&trade("4")).is_none());
        let first = b.push(&trade("5")).expect("closes on the crossing trade");
        assert_eq!(first.volume(), dec("12"), "12 = 3+4+5, whole trades");
        assert_eq!(first.trade_count, 3);

        // A fresh 9-unit trade must NOT close a bar; if the overshoot (2) were
        // carried, 2 + 9 = 11 >= 10 would close here. It stays open.
        assert!(
            b.push(&trade("9")).is_none(),
            "next bar started from zero, so 9 < 10 does not close"
        );
        assert_eq!(b.partial().unwrap().volume(), dec("9"));
    }

    #[test]
    fn extreme_measure_saturates_and_closes_without_panicking() {
        // An adversarial/corrupt feed quantity near Decimal::MAX must not panic
        // the accumulator: it saturates, crosses the threshold, and closes the
        // bar (which is `>= threshold` by construction).
        let mut b = ThresholdBarBuilder::new(dec("10"), QtyMeasure);
        let huge = Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::from_str("1.0").unwrap(),
            quantity: Decimal::MAX,
            side: Side::Buy,
        };
        let bar = b.push(&huge).expect("the saturated measure closes the bar");
        assert_eq!(bar.trade_count, 1);
        // The bar's own volume also saturates rather than panicking.
        assert_eq!(bar.volume(), Decimal::MAX);
    }
}
