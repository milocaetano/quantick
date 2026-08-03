//! [`IndicatorBar`] — the f64 projection of an engine [`Bar`].
//!
//! Indicator math is `f64` by locked decision: Pine itself is float64,
//! indicator math tolerates float rounding by nature, and `Decimal` is an
//! order of magnitude slower — hostile to the full-recompute budget. The
//! conversion from the engine's exact `Decimal` happens **once per bar, here,
//! at the crate boundary**; nothing downstream ever touches `Decimal` again.
//!
//! Derived series (`volume`, `delta`, `hl2`, …) are computed here rather than
//! in scripts so every consumer sees identical values without re-deriving
//! them. The one cross-bar series, `cvd`, is host-maintained (a running sum
//! of `delta`) and travels in [`Ctx`](crate::Ctx), not in the bar.

use quantick_engine::Bar;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

/// The f64 projection of an engine [`Bar`] plus the fields scripts read.
///
/// `trade_count` is stored as `f64` because scripts consume it as a numeric
/// series like any other; a bar cannot hold enough trades for f64 to lose
/// integer precision (2^53).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndicatorBar {
    /// Timestamp of the first trade in the bar (epoch ms), exposed to scripts
    /// as `time`.
    pub open_time: i64,
    /// Timestamp of the last trade in the bar (epoch ms), exposed to scripts
    /// as `time_close`.
    pub close_time: i64,
    /// Price of the first trade.
    pub open: f64,
    /// Highest trade price in the bar.
    pub high: f64,
    /// Lowest trade price in the bar.
    pub low: f64,
    /// Price of the last trade.
    pub close: f64,
    /// Quantity traded on the buy (taker-buy) side.
    pub buy_volume: f64,
    /// Quantity traded on the sell (taker-sell) side.
    pub sell_volume: f64,
    /// Number of trades aggregated into the bar.
    pub trade_count: f64,
}

/// `Decimal` → `f64`, defensively: an unrepresentable value becomes NaN (`na`)
/// rather than a silently wrong number. `Decimal`'s full range fits in f64, so
/// this is a belt-and-suspenders honesty guard, not an expected path.
fn to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

impl From<&Bar> for IndicatorBar {
    fn from(bar: &Bar) -> Self {
        Self {
            open_time: bar.open_time,
            close_time: bar.close_time,
            open: to_f64(bar.open),
            high: to_f64(bar.high),
            low: to_f64(bar.low),
            close: to_f64(bar.close),
            buy_volume: to_f64(bar.buy_volume),
            sell_volume: to_f64(bar.sell_volume),
            // u64 → f64 is exact for any realistic trade count (< 2^53).
            trade_count: bar.trade_count as f64,
        }
    }
}

impl IndicatorBar {
    /// Total traded quantity: `buy_volume + sell_volume` (`volume` in scripts).
    #[must_use]
    pub fn volume(&self) -> f64 {
        self.buy_volume + self.sell_volume
    }

    /// Order-flow delta: `buy_volume - sell_volume` (`delta` in scripts).
    #[must_use]
    pub fn delta(&self) -> f64 {
        self.buy_volume - self.sell_volume
    }

    /// `(high + low) / 2`.
    #[must_use]
    pub fn hl2(&self) -> f64 {
        (self.high + self.low) / 2.0
    }

    /// `(high + low + close) / 3`.
    #[must_use]
    pub fn hlc3(&self) -> f64 {
        (self.high + self.low + self.close) / 3.0
    }

    /// `(open + high + low + close) / 4`.
    #[must_use]
    pub fn ohlc4(&self) -> f64 {
        (self.open + self.high + self.low + self.close) / 4.0
    }

    /// `(high + low + close + close) / 4`.
    #[must_use]
    pub fn hlcc4(&self) -> f64 {
        (self.high + self.low + self.close + self.close) / 4.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn sample() -> Bar {
        Bar {
            open_time: 1_700_000_000_000,
            close_time: 1_700_000_000_450,
            open: dec("36000.10"),
            high: dec("36000.20"),
            low: dec("36000.00"),
            close: dec("36000.15"),
            buy_volume: dec("1.5"),
            sell_volume: dec("0.4"),
            trade_count: 7,
        }
    }

    #[test]
    fn converts_every_field() {
        let ib = IndicatorBar::from(&sample());
        assert_eq!(ib.open_time, 1_700_000_000_000);
        assert_eq!(ib.close_time, 1_700_000_000_450);
        assert_eq!(ib.open, 36000.10);
        assert_eq!(ib.high, 36000.20);
        assert_eq!(ib.low, 36000.00);
        assert_eq!(ib.close, 36000.15);
        assert_eq!(ib.buy_volume, 1.5);
        assert_eq!(ib.sell_volume, 0.4);
        assert_eq!(ib.trade_count, 7.0);
    }

    #[test]
    fn derived_series_match_their_definitions() {
        let ib = IndicatorBar::from(&sample());
        assert_eq!(ib.volume(), 1.9);
        assert!((ib.delta() - 1.1).abs() < 1e-12);
        assert_eq!(ib.hl2(), (36000.20 + 36000.00) / 2.0);
        assert_eq!(ib.hlc3(), (36000.20 + 36000.00 + 36000.15) / 3.0);
        assert_eq!(
            ib.ohlc4(),
            (36000.10 + 36000.20 + 36000.00 + 36000.15) / 4.0
        );
        assert_eq!(
            ib.hlcc4(),
            (36000.20 + 36000.00 + 36000.15 + 36000.15) / 4.0
        );
    }

    #[test]
    fn conversion_is_deterministic() {
        let bar = sample();
        assert_eq!(IndicatorBar::from(&bar), IndicatorBar::from(&bar));
    }
}
