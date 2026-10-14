//! A fixed temporal/price calibration for candle bursts, independent of the camera.
use crate::constants::NATIVE_TAPE_WINDOW_MS;
use serde::{Deserialize, Serialize};

/// Runs the native tape kernel in deterministic UTC blocks before candle placement.
/// Blocks are independent: a burst crossing a boundary remains two honest groups.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBurstGrouping {
    pub window_ms: i64,
    /// Effective temporal span, after tape-style edge padding.
    pub width_px: f32,
    /// Bounded to 1e-6..=1e6 to keep Decimal conversion positive and geometry finite.
    pub price_units_per_pixel: f64,
    pub max_radius: f32,
}

impl TickBurstGrouping {
    #[must_use]
    pub fn valid(self) -> bool {
        self.window_ms >= NATIVE_TAPE_WINDOW_MS
            && self.window_ms % NATIVE_TAPE_WINDOW_MS == 0
            && self.width_px.is_finite()
            && self.width_px > 0.0
            && self.price_units_per_pixel.is_finite()
            && (0.000_001..=1_000_000.0).contains(&self.price_units_per_pixel)
            && self.max_radius.is_finite()
            && self.max_radius > 0.0
    }

    pub fn sanitize(&mut self) {
        self.window_ms =
            (self.window_ms / NATIVE_TAPE_WINDOW_MS * NATIVE_TAPE_WINDOW_MS).clamp(1_000, 300_000);
        if !self.width_px.is_finite() || self.width_px <= 0.0 {
            self.width_px = 420.0;
        }
        if !self.price_units_per_pixel.is_finite()
            || !(0.000_001..=1_000_000.0).contains(&self.price_units_per_pixel)
        {
            self.price_units_per_pixel = 1.0;
        }
        let mut bubbles = super::BubbleStyle {
            max_radius: self.max_radius,
            ..Default::default()
        };
        bubbles.sanitize();
        self.max_radius = bubbles.max_radius;
    }

    pub fn fingerprint(self) -> [u64; 4] {
        [
            self.window_ms as u64,
            u64::from(self.width_px.to_bits()),
            self.price_units_per_pixel.to_bits(),
            u64::from(self.max_radius.to_bits()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subnormal_and_overflowing_price_units_are_refused_then_sanitized() {
        for unit in [1e-100, 1e28, f64::INFINITY, f64::NAN, 0.0, -1.0] {
            let mut value = TickBurstGrouping {
                window_ms: 72_000,
                width_px: 420.0,
                price_units_per_pixel: unit,
                max_radius: 18.0,
            };
            assert!(!value.valid());
            value.sanitize();
            assert!(value.valid());
            assert_eq!(value.price_units_per_pixel, 1.0);
        }
    }
}
