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

    /// A scale over an explicit `[lo, hi]` price range mapped to `[top, bottom]`
    /// pixels. Used when the price axis is under manual pan/zoom rather than
    /// auto-fitting the visible bars.
    #[must_use]
    pub fn from_range(lo: f64, hi: f64, top: f32, bottom: f32) -> Self {
        // Guard against an inverted or zero span.
        let (lo, hi) = if hi > lo {
            (lo, hi)
        } else {
            (lo - 0.5, lo + 0.5)
        };
        Self {
            lo,
            hi,
            top,
            bottom,
        }
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

    /// The price at a given y-pixel — the inverse of [`y`](PriceScale::y), for a
    /// crosshair readout.
    #[must_use]
    pub fn price_at(&self, y: f32) -> f64 {
        let height = self.bottom - self.top;
        if height.abs() < f32::EPSILON {
            return f64::midpoint(self.lo, self.hi);
        }
        let frac = f64::from((y - self.top) / height); // 0 at top (hi), 1 at bottom (lo)
        self.hi - frac * (self.hi - self.lo)
    }

    /// The padded `(lo, hi)` price range this scale covers.
    #[must_use]
    pub fn range(&self) -> (f64, f64) {
        (self.lo, self.hi)
    }
}

/// Round "nice" price ticks spanning `[lo, hi]`, aiming for about `target`
/// labels, using Heckbert's nice-numbers algorithm so labels land on values
/// like 100, 102.5, 105 rather than 100.37, 102.71, ….
#[must_use]
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if hi <= lo || target == 0 {
        return Vec::new();
    }
    let step = nice_num(nice_num(hi - lo, false) / target as f64, true);
    if step <= 0.0 || !step.is_finite() {
        return Vec::new();
    }
    let first = (lo / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = first;
    // Guard the loop count in case of pathological inputs.
    for _ in 0..(target * 4 + 8) {
        if v > hi + step * 0.001 {
            break;
        }
        if v >= lo - step * 0.001 {
            ticks.push(v);
        }
        v += step;
    }
    ticks
}

/// A "nice" number near `range`: 1, 2, 5 or 10 × a power of ten. When `round`,
/// picks the nearest nice value; otherwise the smallest nice value ≥ `range`.
fn nice_num(range: f64, round: bool) -> f64 {
    if range <= 0.0 || !range.is_finite() {
        return 0.0;
    }
    let exp = range.log10().floor();
    let frac = range / 10f64.powf(exp);
    let nice = if round {
        if frac < 1.5 {
            1.0
        } else if frac < 3.0 {
            2.0
        } else if frac < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * 10f64.powf(exp)
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

    #[test]
    fn price_at_is_the_inverse_of_y() {
        let bars = vec![bar("100.0", "110.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        for price in [100.0, 103.0, 107.5, 110.0] {
            let y = scale.y(price);
            assert!(
                (scale.price_at(y) - price).abs() < 1e-6,
                "price_at(y({price})) != {price}"
            );
        }
    }

    #[test]
    fn nice_ticks_are_round_and_in_range() {
        let ticks = nice_ticks(100.0, 110.0, 5);
        assert!(!ticks.is_empty());
        for t in &ticks {
            assert!(*t >= 100.0 && *t <= 110.0, "tick {t} out of range");
        }
        let step = ticks[1] - ticks[0];
        for pair in ticks.windows(2) {
            assert!((pair[1] - pair[0] - step).abs() < 1e-9);
        }
        // 100..110 targeting ~5 gives a round 2.0 step: 100,102,...,110.
        assert!((step - 2.0).abs() < 1e-9, "step = {step}");
    }

    #[test]
    fn nice_ticks_handles_degenerate_ranges() {
        assert!(nice_ticks(100.0, 100.0, 5).is_empty());
        assert!(nice_ticks(110.0, 100.0, 5).is_empty());
        assert!(nice_ticks(100.0, 110.0, 0).is_empty());
    }
}
