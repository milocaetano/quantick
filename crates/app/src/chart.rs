//! Pure chart geometry: mapping bars to pixel positions.
//!
//! This module has no dependency on egui, so the coordinate math — price → y,
//! bar index → x, auto-scaling to the visible range — is unit-testable in CI
//! without a display. The egui layer ([`crate::app`]) only turns these
//! positions into shapes.
//!
//! Prices are `Decimal` in the engine for exact arithmetic; here, at the display
//! boundary, they become `f64` (pixels are floating-point and determinism no
//! longer applies once we're drawing).

use quantick_engine::Bar;
use rust_decimal::prelude::ToPrimitive as _;

/// A `Decimal` price as an `f64` for pixel math (display only).
#[must_use]
pub fn to_f64(price: rust_decimal::Decimal) -> f64 {
    price.to_f64().unwrap_or(0.0)
}

/// Vertical price → y-pixel mapping, auto-scaled to a price range.
///
/// `hi` maps to `top` (smaller y, screen coordinates grow downward) and `lo`
/// maps to `bottom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceScale {
    lo: f64,
    hi: f64,
    top: f32,
    bottom: f32,
}

impl PriceScale {
    /// Auto-scale to the high/low range of `bars` (and the forming `partial`),
    /// padded by `pad_frac` of the range on each side so candles don't touch the
    /// edges. Returns `None` if there is nothing to scale.
    #[must_use]
    pub fn auto(
        bars: &[Bar],
        partial: Option<&Bar>,
        top: f32,
        bottom: f32,
        pad_frac: f64,
    ) -> Option<Self> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for bar in bars.iter().chain(partial) {
            lo = lo.min(to_f64(bar.low));
            hi = hi.max(to_f64(bar.high));
        }
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        // Pad; guarantee a non-zero span even when hi == lo (a flat range).
        let span = (hi - lo).max(f64::EPSILON);
        let pad = span * pad_frac;
        Some(Self {
            lo: lo - pad,
            hi: hi + pad,
            top,
            bottom,
        })
    }

    /// The y-pixel for `price`.
    #[must_use]
    pub fn y(&self, price: f64) -> f32 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON {
            return f32::midpoint(self.top, self.bottom);
        }
        let frac = ((self.hi - price) / span) as f32;
        self.top + frac * (self.bottom - self.top)
    }
}

/// Horizontal bar-index → x-pixel mapping, packing `count` bars into a width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeAxis {
    left: f32,
    step: f32,
    bar_width: f32,
}

impl TimeAxis {
    /// Fit `count` bars between `left` and `right`. Each bar occupies a `step`
    /// slot; its drawn body is `body_frac` of the slot, capped at `max_width`.
    #[must_use]
    pub fn new(left: f32, right: f32, count: usize, body_frac: f32, max_width: f32) -> Self {
        let slots = count.max(1) as f32;
        let step = (right - left) / slots;
        let bar_width = (step * body_frac).min(max_width).max(1.0);
        Self {
            left,
            step,
            bar_width,
        }
    }

    /// The x-pixel of the centre of bar `index`.
    #[must_use]
    pub fn x_center(&self, index: usize) -> f32 {
        self.left + self.step * (index as f32 + 0.5)
    }

    /// The x-pixel of the left edge of bar `index`'s slot — used to draw the
    /// divider *between* bar `index - 1` and bar `index`.
    #[must_use]
    pub fn x_left(&self, index: usize) -> f32 {
        self.left + self.step * index as f32
    }

    /// The width of a candle body in pixels.
    #[must_use]
    pub fn bar_width(&self) -> f32 {
        self.bar_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    fn bar(low: &str, high: &str) -> Bar {
        let l = Decimal::from_str(low).unwrap();
        let h = Decimal::from_str(high).unwrap();
        Bar {
            open_time: 0,
            close_time: 0,
            open: l,
            high: h,
            low: l,
            close: h,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn empty_range_has_no_scale() {
        assert!(PriceScale::auto(&[], None, 0.0, 100.0, 0.05).is_none());
    }

    #[test]
    fn hi_maps_to_top_and_lo_to_bottom() {
        let bars = vec![bar("100.0", "110.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        // With zero padding, 110 -> top (0), 100 -> bottom (100).
        assert!((scale.y(110.0) - 0.0).abs() < 0.001, "{}", scale.y(110.0));
        assert!((scale.y(100.0) - 100.0).abs() < 0.001, "{}", scale.y(100.0));
        // Midpoint price -> midpoint pixel.
        assert!((scale.y(105.0) - 50.0).abs() < 0.001, "{}", scale.y(105.0));
    }

    #[test]
    fn partial_bar_extends_the_range() {
        let bars = vec![bar("100.0", "110.0")];
        let partial = bar("90.0", "120.0");
        let scale = PriceScale::auto(&bars, Some(&partial), 0.0, 100.0, 0.0).unwrap();
        // The partial's 120/90 now bound the range.
        assert!((scale.y(120.0) - 0.0).abs() < 0.001);
        assert!((scale.y(90.0) - 100.0).abs() < 0.001);
    }

    #[test]
    fn flat_range_maps_to_the_middle() {
        let bars = vec![bar("100.0", "100.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        assert!((scale.y(100.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn time_axis_centers_are_ordered_and_within_bounds() {
        let axis = TimeAxis::new(0.0, 100.0, 10, 0.7, 20.0);
        let mut last = f32::NEG_INFINITY;
        for i in 0..10 {
            let x = axis.x_center(i);
            assert!(x > last, "centres increase");
            assert!(x > 0.0 && x < 100.0, "within bounds: {x}");
            last = x;
        }
        assert!(axis.bar_width() > 0.0);
    }

    #[test]
    fn x_left_sits_between_adjacent_centres() {
        let axis = TimeAxis::new(0.0, 100.0, 10, 0.7, 20.0);
        // The divider before bar 3 is left of bar 3's centre and right of bar 2's.
        let left = axis.x_left(3);
        assert!(axis.x_center(2) < left && left < axis.x_center(3));
    }

    #[test]
    fn time_axis_handles_empty() {
        let axis = TimeAxis::new(0.0, 100.0, 0, 0.7, 20.0);
        assert!(axis.bar_width() >= 1.0);
    }
}
