//! Optional tick-chart spheres, independent of the native tape appearance.
use super::BubbleStyle;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBubbleStyle {
    /// Fixed executed quantity in the instrument's units at the full radius.
    pub reference_quantity: f64,
    /// Full-size radius in logical pixels.
    pub max_radius: f32,
    /// Smallest disc eligible for an inside quantity label.
    pub label_min_radius: f32,
    /// Sphere fill opacity; candle contours are painted afterwards.
    pub opacity: f32,
}

impl TickBubbleStyle {
    pub fn sanitize(&mut self) {
        if !self.reference_quantity.is_finite() || self.reference_quantity <= 0.0 {
            self.reference_quantity = 40.0;
        }
        let style = self.bubbles();
        self.max_radius = style.max_radius;
        self.label_min_radius = style.label_min_radius;
        self.opacity = style.opacity;
    }

    #[must_use]
    pub fn bubbles(self) -> BubbleStyle {
        let mut style = BubbleStyle {
            max_radius: self.max_radius,
            label_min_radius: self.label_min_radius,
            opacity: self.opacity,
            show_trade_count: false,
            ..BubbleStyle::default()
        };
        style.sanitize();
        style
    }

    #[must_use]
    pub fn fingerprint(self) -> [u64; 4] {
        [
            self.reference_quantity.to_bits(),
            u64::from(self.max_radius.to_bits()),
            u64::from(self.label_min_radius.to_bits()),
            u64::from(self.opacity.to_bits()),
        ]
    }
}
