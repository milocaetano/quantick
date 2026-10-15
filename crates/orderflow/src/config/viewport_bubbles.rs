//! Optional presentation calibration, independent of factual group construction.
use serde::{Deserialize, Serialize};

/// Camera density at which visible-relative bubbles approach their drawing envelope.
/// X units are bars on candles and milliseconds on the native Tape.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewportBubbleScale {
    pub x_units_per_pixel: f64,
    pub price_units_per_pixel: f64,
}

impl ViewportBubbleScale {
    #[must_use]
    pub fn sanitized(self) -> Option<Self> {
        [self.x_units_per_pixel, self.price_units_per_pixel]
            .into_iter()
            .all(|value| value.is_finite() && value > 0.0)
            .then_some(self)
    }

    /// A common, reversible presentation factor. It responds continuously to
    /// either axis, approaching the configured envelope without a hard zoom step.
    /// No remembered camera, grouping scale or elapsed time enters this rule.
    #[must_use]
    pub fn radius_gain(self, px_per_x: f64, px_per_price: f64) -> f32 {
        if self.sanitized().is_none()
            || ![px_per_x, px_per_price]
                .into_iter()
                .all(|value| value.is_finite() && value >= 0.0)
        {
            return 1.0;
        }
        if px_per_x == 0.0 || px_per_price == 0.0 {
            return 0.0;
        }
        let density =
            (px_per_x * self.x_units_per_pixel) * (px_per_price * self.price_units_per_pixel);
        // The reciprocal form also handles overflowing positive densities.
        (1.0 / (1.0 + 1.0 / density)).sqrt().sqrt() as f32
    }

    #[must_use]
    pub fn fingerprint(self) -> [u64; 2] {
        [
            self.x_units_per_pixel.to_bits(),
            self.price_units_per_pixel.to_bits(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE: ViewportBubbleScale = ViewportBubbleScale {
        x_units_per_pixel: 1.0 / 22.0,
        price_units_per_pixel: 1.0,
    };

    #[test]
    fn either_axis_changes_every_radius_smoothly_and_reversibly() {
        let normal = SCALE.radius_gain(22.0, 1.0);
        let compressed_x = SCALE.radius_gain(11.0, 1.0);
        let compressed_y = SCALE.radius_gain(22.0, 0.5);
        assert_eq!(compressed_x, compressed_y);
        assert!(compressed_x < normal);
        assert!(SCALE.radius_gain(11.0, 0.5) < compressed_x);
        assert!(SCALE.radius_gain(44.0, 1.0) > normal);
        assert_eq!(SCALE.radius_gain(22.0, 1.0), normal);
        assert!((SCALE.radius_gain(22.0001, 1.0) - normal).abs() < 0.00001);
        let radii = [1.0, 2.0, 4.0].map(|radius| radius * compressed_y);
        assert_eq!(radii[2] / radii[0], 4.0);
    }

    #[test]
    fn finite_camera_gain_never_exceeds_the_common_envelope() {
        let mut previous = 0.0;
        for density in [0.000001, 0.001, 0.1, 1.0, 10.0, 1000.0, 1e300] {
            let gain = SCALE.radius_gain(density, density);
            assert!(gain.is_finite() && gain > previous && gain <= 1.0);
            previous = gain;
        }
        assert_eq!(SCALE.radius_gain(0.0, 1.0), 0.0);
        assert_eq!(SCALE.radius_gain(1.0, 0.0), 0.0);
        for invalid in [-1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(SCALE.radius_gain(invalid, 1.0), 1.0);
            assert!(
                ViewportBubbleScale {
                    x_units_per_pixel: invalid,
                    ..SCALE
                }
                .sanitized()
                .is_none()
            );
        }
    }
}
