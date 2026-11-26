//! The generic threshold accumulator shared by tick, volume and dollar bars, parameterised by a [`Measure`].
//! Boundary rule: a trade is never split, so the whole trade joins the bar it completes (volume and
//! dollar bars may overshoot) and the overshoot is not carried: the accumulator resets to zero.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarProgress, Trade};

/// How much one trade contributes toward closing a bar. Usually a zero-sized marker
/// ([`crate::TickMeasure`], volume, dollar); custom measures reuse the same closing logic.
pub trait Measure {
    /// The contribution of `trade` to the running total.
    ///
    /// Must be non-negative for the closing rule to make sense.
    fn of(&self, trade: &Trade) -> Decimal;
}

/// Builds bars that close when an accumulated [`Measure`] reaches a threshold.
/// See the [module docs](self) for the boundary rule.
#[derive(Debug, Clone)]
pub struct ThresholdBarBuilder<M: Measure> {
    threshold: Decimal,
    measure: M,
    acc: Decimal,
    current: Option<Bar>,
}

impl<M: Measure> ThresholdBarBuilder<M> {
    /// Create a builder that closes a bar once the accumulated measure reaches `threshold`.
    /// # Panics
    /// Panics if `threshold <= 0`: coercing it silently would violate data honesty.
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

impl<M: Measure + Send> BarBuilder for ThresholdBarBuilder<M> {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        match &mut self.current {
            None => self.current = Some(Bar::opened_by(trade)),
            Some(bar) => bar.extend(trade),
        }
        // Saturating: the measure comes from untrusted feed data. A saturated total is `>= threshold`
        // and closes the bar deterministically.
        self.acc = self.acc.saturating_add(self.measure.of(trade));
        if self.acc >= self.threshold {
            self.acc = Decimal::ZERO;
            self.current.take()
        } else {
            None
        }
    }

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    /// The accumulator itself; reported between bars as a plain zero (nothing taken yet).
    fn progress(&self) -> Option<BarProgress> {
        Some(BarProgress {
            done: self.acc,
            target: self.threshold,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    use std::str::FromStr as _;

    /// Test measure: accumulate traded quantity, enough to exercise the boundary rule.
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
        // Threshold 10, one 25-unit trade: not split; one bar overshooting to 25.
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
        // 3 + 4 + 5 crosses 10 on the third trade; the next starts from zero, not from the overshoot.
        let mut b = ThresholdBarBuilder::new(dec("10"), QtyMeasure);
        assert!(b.push(&trade("3")).is_none());
        assert!(b.push(&trade("4")).is_none());
        let first = b.push(&trade("5")).expect("closes on the crossing trade");
        assert_eq!(first.volume(), dec("12"), "12 = 3+4+5, whole trades");
        assert_eq!(first.trade_count, 3);

        // A fresh 9-unit trade must not close a bar; a carried overshoot (2 + 9) would.
        assert!(
            b.push(&trade("9")).is_none(),
            "next bar started from zero, so 9 < 10 does not close"
        );
        assert_eq!(b.partial().unwrap().volume(), dec("9"));
    }

    #[test]
    fn extreme_measure_saturates_and_closes_without_panicking() {
        // A quantity near Decimal::MAX must not panic: it saturates, crosses the threshold, closes.
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
