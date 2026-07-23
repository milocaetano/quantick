//! Dollar bars — close after `N` notional traded.
//!
//! Dollar bars close when accumulated notional (`price * quantity`) reaches `N`.
//! Because they sample by value rather than by count or quantity, they stay
//! comparable across large price moves — which is why the research literature
//! (Lopez de Prado) favours them for ML features. They are the [generic
//! threshold accumulator](crate::ThresholdBarBuilder) with the measure
//! "notional" ([`DollarMeasure`]).
//!
//! # Numeric representation of accumulated notional
//!
//! Notional is accumulated as [`rust_decimal::Decimal`], the same exact decimal
//! type used for every price and quantity in the engine. This is the decision
//! this issue calls for, and the reasoning is:
//!
//! - **No float drift.** `price * quantity` is an exact decimal product, and
//!   summing exact decimals is exact. Over a session, binary-float notional
//!   (`f64`) accumulates rounding error and would cross the threshold a trade
//!   early or late depending on the exact drift — a non-determinism the golden
//!   tests would catch. Decimal cannot drift: the close point is exact.
//! - **Determinism holds regardless.** `Decimal` arithmetic uses a fixed
//!   rounding rule, so even the rare product exceeding 28 significant digits
//!   rounds deterministically. Realistic crypto notionals stay far below that
//!   bound, and the accumulator resets every bar, so in practice accumulation is
//!   exact and never overflows.
//!
//! The `notional_accumulation_is_exact` test demonstrates a threshold that an
//! `f64` accumulator would miss (ten 0.1 notionals summing to 0.999… < 1.0).

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, ThresholdBarBuilder, Trade, threshold::Measure};

/// The dollar measure: every trade contributes its notional `price * quantity`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DollarMeasure;

impl Measure for DollarMeasure {
    fn of(&self, trade: &Trade) -> Decimal {
        trade.notional()
    }
}

/// Builds dollar bars: closes once accumulated notional reaches `notional`.
///
/// A thin wrapper over [`ThresholdBarBuilder`] with [`DollarMeasure`]. Feed
/// trades in order with [`push`](BarBuilder::push); the bar closes on the trade
/// that brings cumulative notional to `notional` or beyond.
#[derive(Debug, Clone)]
pub struct DollarBarBuilder {
    inner: ThresholdBarBuilder<DollarMeasure>,
}

impl DollarBarBuilder {
    /// Create a builder that closes a bar every `notional` of traded value.
    ///
    /// # Panics
    ///
    /// Panics if `notional <= 0` (see [`ThresholdBarBuilder::new`]).
    #[must_use]
    pub fn new(notional: Decimal) -> Self {
        Self {
            inner: ThresholdBarBuilder::new(notional, DollarMeasure),
        }
    }

    /// The configured per-bar notional threshold.
    #[must_use]
    pub fn notional(&self) -> Decimal {
        self.inner.threshold()
    }
}

impl BarBuilder for DollarBarBuilder {
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

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(price: &str, qty: &str, side: Side) -> Trade {
        Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: dec(price),
            quantity: dec(qty),
            side,
        }
    }

    #[test]
    #[should_panic(expected = "bar threshold must be > 0")]
    fn rejects_zero_notional() {
        let _ = DollarBarBuilder::new(Decimal::ZERO);
    }

    #[test]
    fn measure_is_price_times_quantity() {
        let t = trade("36000.10", "0.005", Side::Buy);
        assert_eq!(DollarMeasure.of(&t), dec("180.00050"));
    }

    #[test]
    fn notional_accumulation_is_exact() {
        // Ten notionals of 0.1 sum to exactly 1.0 in Decimal, so the bar closes
        // on the tenth trade. In f64 the same sum is 0.9999999999999999 < 1.0,
        // which would NOT close here — proving the exact-decimal choice.
        let f64_sum: f64 = (0..10).map(|_| 1.0_f64 * 0.1_f64).sum();
        assert!(f64_sum < 1.0, "f64 drifts below 1.0: {f64_sum}");

        let mut b = DollarBarBuilder::new(dec("1.0"));
        for _ in 0..9 {
            assert!(
                b.push(&trade("1.0", "0.1", Side::Buy)).is_none(),
                "0.9 notional has not reached 1.0"
            );
        }
        let bar = b
            .push(&trade("1.0", "0.1", Side::Buy))
            .expect("the tenth trade brings exact notional to 1.0");
        assert_eq!(bar.trade_count, 10);
    }
}
