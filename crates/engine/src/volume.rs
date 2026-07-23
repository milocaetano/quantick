//! Volume bars — close after `N` units traded.
//!
//! Volume bars close when accumulated traded quantity reaches `N`. A 10-unit
//! trade weighs 10× a 1-unit trade, so bars normalise by real traded quantity
//! instead of by event count (tick bars). They are the [generic threshold
//! accumulator](crate::ThresholdBarBuilder) with the measure "traded quantity"
//! ([`VolumeMeasure`]).
//!
//! Because a single large trade is never split (see the threshold module's
//! boundary rule), a volume bar can overshoot `N` — a 25-unit trade closes a
//! 10-unit bar as one 25-unit bar.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, ThresholdBarBuilder, Trade, threshold::Measure};

/// The volume measure: every trade contributes its traded quantity.
#[derive(Debug, Clone, Copy, Default)]
pub struct VolumeMeasure;

impl Measure for VolumeMeasure {
    fn of(&self, trade: &Trade) -> Decimal {
        trade.quantity
    }
}

/// Builds volume bars: closes once accumulated traded quantity reaches `units`.
///
/// A thin wrapper over [`ThresholdBarBuilder`] with [`VolumeMeasure`]. Feed
/// trades in order with [`push`](BarBuilder::push); the bar closes on the trade
/// that brings cumulative quantity to `units` or beyond.
#[derive(Debug, Clone)]
pub struct VolumeBarBuilder {
    inner: ThresholdBarBuilder<VolumeMeasure>,
}

impl VolumeBarBuilder {
    /// Create a builder that closes a bar every `units` of traded quantity.
    ///
    /// # Panics
    ///
    /// Panics if `units <= 0` (see [`ThresholdBarBuilder::new`]).
    #[must_use]
    pub fn new(units: Decimal) -> Self {
        Self {
            inner: ThresholdBarBuilder::new(units, VolumeMeasure),
        }
    }

    /// The configured per-bar traded quantity.
    #[must_use]
    pub fn units(&self) -> Decimal {
        self.inner.threshold()
    }
}

impl BarBuilder for VolumeBarBuilder {
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
    fn rejects_zero_units() {
        let _ = VolumeBarBuilder::new(Decimal::ZERO);
    }

    #[test]
    fn units_reports_configured_threshold() {
        assert_eq!(VolumeBarBuilder::new(dec("5.0")).units(), dec("5.0"));
    }

    #[test]
    fn closes_when_quantity_reaches_the_threshold() {
        let mut b = VolumeBarBuilder::new(dec("5"));
        assert!(b.push(&trade("100.0", "2.0", Side::Buy)).is_none());
        assert!(b.push(&trade("100.0", "2.0", Side::Buy)).is_none());
        let bar = b
            .push(&trade("100.0", "1.0", Side::Buy))
            .expect("reaches 5.0");
        assert_eq!(bar.volume(), dec("5.0"));
        assert_eq!(bar.trade_count, 3);
    }
}
