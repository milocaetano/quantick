//! Optional tick-chart spheres, independent of the native tape appearance.
use super::BubbleStyle;
use serde::{Deserialize, Serialize};

impl super::HeatmapConfig {
    /// Whether a change affects the tape or book. Tick painting has its own
    /// cache identity and must not restart their retained factual groups.
    #[must_use]
    pub fn same_tape_settings(&self, other: &Self) -> bool {
        let mut compared = other.clone();
        compared.volume_dots.tick_bubbles = self.volume_dots.tick_bubbles;
        *self == compared
    }

    /// Whether a change keeps the tape's painted groups
    /// ([`TapeDotMemory`](crate::projection::TapeDotMemory)): the same tape
    /// settings but for the lane's width share, a view parameter like the
    /// window the lane shows. An explicit zoom (the lane's window setting)
    /// and every other tape setting start a new display epoch.
    #[must_use]
    pub fn same_tape_grouping(&self, other: &Self) -> bool {
        let mut compared = other.clone();
        compared.live_lane.width_share = self.live_lane.width_share;
        self.same_tape_settings(&compared)
    }
}

#[cfg(test)]
mod grouping_tests {
    use crate::HeatmapConfig;

    #[test]
    fn a_lane_width_keeps_the_tape_grouping_and_a_zoom_does_not() {
        let config = HeatmapConfig::default();
        let mut wider = config.clone();
        wider.live_lane.width_share += 0.1;
        assert!(wider.same_tape_grouping(&config));
        assert!(
            !wider.same_tape_settings(&config),
            "the worker still hears it"
        );
        let mut zoomed = config.clone();
        zoomed.zoom_lane_window(2.0);
        assert!(!zoomed.same_tape_grouping(&config));
    }

    /// Review #26: a reference that is not a positive number falls back to
    /// a named value, never an inline literal.
    #[test]
    fn an_invalid_reference_falls_back_to_the_named_quantity() {
        for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut style = super::TickBubbleStyle {
                reference_quantity: invalid,
                max_radius: 32.0,
                label_min_radius: 8.0,
                opacity: 0.9,
            };
            style.sanitize();
            assert_eq!(
                style.reference_quantity,
                super::FALLBACK_REFERENCE_QUANTITY
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBubbleStyle {
    /// Minimum common reference in instrument units at the full radius.
    /// Larger visible groups raise it uniformly, preserving every area ratio.
    pub reference_quantity: f64,
    /// Full-size radius in logical pixels.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
    pub max_radius: f32,
    /// Smallest disc eligible for an inside quantity label.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
    pub label_min_radius: f32,
    /// Sphere fill opacity; candle contours are painted afterwards.
    #[serde(serialize_with = "super::bubbles::serialize_short_f32")]
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
