//! Dollar bars: close after `N` notional traded.
//! Notional (`price * quantity`) accumulates as exact [`rust_decimal::Decimal`], never `f64`.
//! Built on the [generic threshold accumulator](crate::ThresholdBarBuilder).

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarProgress, ThresholdBarBuilder, Trade, threshold::Measure};

/// The dollar measure: every trade contributes its notional `price * quantity`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DollarMeasure;

impl Measure for DollarMeasure {
    fn of(&self, trade: &Trade) -> Decimal {
        trade.notional()
    }
}

/// Builds dollar bars: closes on the trade that brings cumulative notional to `notional` or beyond.
#[derive(Debug, Clone)]
pub struct DollarBarBuilder {
    inner: ThresholdBarBuilder<DollarMeasure>,
}

impl DollarBarBuilder {
    /// Create a builder that closes a bar every `notional` of traded value.
    /// # Panics
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

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        self.inner.partial()
    }

    fn progress(&self) -> Option<BarProgress> {
        self.inner.progress()
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
        // Ten 0.1 notionals sum to exactly 1.0 in Decimal and close on the tenth trade; f64 would not.
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
