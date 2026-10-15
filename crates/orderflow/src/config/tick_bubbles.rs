//! Optional tick-chart spheres, independent of the native tape appearance.
use super::BubbleStyle;
use rust_decimal::{Decimal, prelude::FromPrimitive as _};
use serde::{Deserialize, Serialize};

/// The reference, in instrument units at full radius, that a stored value
/// which is not a positive amount a decimal can hold reads as
/// ([`TickBubbleStyle::reference`]). A reference is a preset calibration, so
/// a bad one is replaced rather than guessed from the tape.
pub const FALLBACK_REFERENCE_QUANTITY: f64 = 40.0;

impl super::HeatmapConfig {
    /// Whether a change affects the tape or book. Tick painting has its own
    /// cache identity and must not restart their retained factual groups.
    #[must_use]
    pub fn same_tape_settings(&self, other: &Self) -> bool {
        let mut compared = other.clone();
        compared.volume_dots.tick_bubbles = self.volume_dots.tick_bubbles;
        *self == compared
    }

    /// The tick spheres this config draws, with the theme they are painted in.
    #[must_use]
    pub fn tick_look(&self) -> Option<TickLook> {
        self.volume_dots.tick_bubbles.map(|tick| (tick, self.theme))
    }

    /// Native groups whose fixed scale must survive navigation.
    #[must_use]
    pub fn stable_tape_groups(&self) -> bool {
        self.native_tape() && self.live_lane.stable_size && !self.volume_dots.auto_full
    }

    /// Whether a change keeps the tape's painted groups
    /// ([`TapeDotMemory`](crate::projection::TapeDotMemory)): the same tape
    /// settings but for the lane's width share, a view parameter like the
    /// window the lane shows. Stable native groups also survive an explicit
    /// window change; ordinary lanes retain their existing zoom regrouping.
    #[must_use]
    pub fn same_tape_grouping(&self, other: &Self) -> bool {
        let mut compared = other.clone();
        compared.live_lane.width_share = self.live_lane.width_share;
        if self.stable_tape_groups() && other.stable_tape_groups() {
            compared.live_lane.window = self.live_lane.window;
        }
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

    #[test]
    fn only_a_stable_native_typed_tape_keeps_groups_across_window_changes() {
        let mut config = HeatmapConfig::default();
        config.live_lane.native_tape = true;
        config.live_lane.stable_size = true;
        config.volume_dots.enabled = true;
        config.volume_dots.auto_full = false;
        assert!(config.stable_tape_groups());
        let mut zoomed = config.clone();
        zoomed.zoom_lane_window(2.0);
        assert!(zoomed.same_tape_grouping(&config));
        assert!(!zoomed.same_tape_settings(&config));
        zoomed.live_lane.window = crate::LaneWindow::Fixed { ms: 3_000 };
        assert!(zoomed.same_tape_grouping(&config));
        for mode in 0..3 {
            let mut ordinary = config.clone();
            match mode {
                0 => ordinary.live_lane.stable_size = false,
                1 => ordinary.volume_dots.auto_full = true,
                _ => ordinary.live_lane.native_tape = false,
            }
            let mut changed = ordinary.clone();
            changed.zoom_lane_window(2.0);
            assert!(!changed.same_tape_grouping(&ordinary));
        }
        zoomed.bubbles.max_radius += 1.0;
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
            assert_eq!(style.reference_quantity, super::FALLBACK_REFERENCE_QUANTITY);
            assert_eq!(style.reference(), rust_decimal::Decimal::from(40));
        }
    }
}

/// Tick spheres as a pane paints them: their style and the pane's theme.
pub type TickLook = (TickBubbleStyle, super::HeatmapTheme);

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBubbleStyle {
    /// The pane's one reference, in instrument units, drawn at the full
    /// radius: no pan, zoom or new print changes it, and a larger region
    /// caps at the full radius (`FlowReference::Typed`). Read through
    /// [`Self::reference`].
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
        if usable_reference(self.reference_quantity).is_none() {
            self.reference_quantity = FALLBACK_REFERENCE_QUANTITY;
        }
        let style = self.bubbles();
        self.max_radius = style.max_radius;
        self.label_min_radius = style.label_min_radius;
        self.opacity = style.opacity;
    }

    /// The reference the spheres are drawn against, the one rule for it:
    /// [`Self::reference_quantity`] while it is a positive amount a decimal
    /// can hold, else [`FALLBACK_REFERENCE_QUANTITY`].
    #[must_use]
    pub fn reference(self) -> Decimal {
        usable_reference(self.reference_quantity).unwrap_or_else(|| {
            usable_reference(FALLBACK_REFERENCE_QUANTITY)
                .expect("the fallback reference is a positive decimal")
        })
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

/// `quantity` as a reference, when it is a positive amount a decimal holds.
fn usable_reference(quantity: f64) -> Option<Decimal> {
    Decimal::from_f64(quantity).filter(|reference| *reference > Decimal::ZERO)
}
