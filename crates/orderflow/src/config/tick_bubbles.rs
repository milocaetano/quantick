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
        compared.bubbles.viewport_scale = self.bubbles.viewport_scale;
        compared.bubbles.display_scale = self.bubbles.display_scale;
        // A close only shown is no setting, and the worker's tape never reads it.
        compared.live_lane.drag_close = self.live_lane.drag_close;
        *self == compared
    }

    /// The tick spheres this config draws, with the theme they are painted in.
    #[must_use]
    pub fn tick_look(&self) -> Option<TickLook> {
        self.volume_dots
            .tick_bubbles
            .map(|tick| (tick, self.palette))
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
        compared.live_lane.fold_market_closed = self.live_lane.fold_market_closed;
        compared.live_lane.market_hold = self.live_lane.market_hold;
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
    fn saved_tick_styles_default_to_fixed_and_mode_changes_only_the_tick_cache() {
        let old =
            "reference_quantity = 1.0\nmax_radius = 32.0\nlabel_min_radius = 8.0\nopacity = 0.9\n";
        let fixed: super::TickBubbleStyle = toml::from_str(old).unwrap();
        assert_eq!(fixed.reference_mode, super::TickReferenceMode::Fixed);
        assert_eq!(fixed.viewport_scale, None);
        assert!(!toml::to_string(&fixed).unwrap().contains("reference_mode"));
        assert!(!toml::to_string(&fixed).unwrap().contains("viewport_scale"));
        assert_eq!(fixed.peak_spheres, None, "a saved style without the key");
        assert!(!fixed.peaks());
        assert!(!toml::to_string(&fixed).unwrap().contains("peak_spheres"));
        let unticked = super::TickBubbleStyle {
            peak_spheres: Some(false),
            ..fixed
        };
        assert!(
            toml::to_string(&unticked)
                .unwrap()
                .contains("peak_spheres = false")
        );
        assert_eq!(unticked.fingerprint(), fixed.fingerprint(), "both draw off");
        let peaks = super::TickBubbleStyle {
            peak_spheres: Some(true),
            peak_max_radius: Some(24.0),
            ..fixed
        };
        assert_eq!(peaks.bubbles().max_radius, 24.0);
        assert_eq!(fixed.bubbles().max_radius, 32.0);
        let off_with_radius = super::TickBubbleStyle {
            peak_max_radius: Some(24.0),
            ..unticked
        };
        assert_eq!(
            off_with_radius.bubbles(),
            fixed.bubbles(),
            "off never reads it"
        );
        let mut sanitized = peaks;
        sanitized.sanitize();
        assert_eq!(
            (sanitized.max_radius, sanitized.peak_max_radius),
            (32.0, Some(24.0))
        );
        assert_eq!(
            toml::from_str::<super::TickBubbleStyle>(&toml::to_string(&peaks).unwrap()).unwrap(),
            peaks
        );
        assert_ne!(peaks.fingerprint(), fixed.fingerprint());
        let visible = super::TickBubbleStyle {
            reference_mode: super::TickReferenceMode::VisibleMax,
            ..fixed
        };
        assert_eq!(
            toml::from_str::<super::TickBubbleStyle>(&toml::to_string(&visible).unwrap()).unwrap(),
            visible
        );
        assert_ne!(visible.fingerprint(), fixed.fingerprint());
        let mut config = HeatmapConfig::default();
        config.volume_dots.tick_bubbles = Some(fixed);
        let mut changed = config.clone();
        changed.volume_dots.tick_bubbles = Some(visible);
        assert!(config.same_tape_settings(&changed));
        assert!(config.same_tape_grouping(&changed));
        changed.bubbles.viewport_scale = Some(super::super::ViewportBubbleScale {
            x_units_per_pixel: 1.0,
            price_units_per_pixel: 1.0,
        });
        assert!(config.same_tape_settings(&changed));
        assert!(config.same_tape_grouping(&changed));
        changed.bubbles.size_reference_quantity = 8.0;
        assert_eq!(
            changed.volume_dots.tick_bubbles,
            Some(visible),
            "the Tape reference cannot change the candle policy"
        );
    }

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

    /// A tuning changes only what it names, and an opacity out of range is
    /// sanitized like a stored one.
    #[test]
    fn a_tuning_changes_only_what_it_names() {
        let mut style: super::TickBubbleStyle = toml::from_str(
            "reference_quantity = 1.0
max_radius = 18.0
label_min_radius = 8.0
opacity = 0.9
zoom_regions = true
",
        )
        .unwrap();
        style.sanitize();
        assert_eq!(super::TickTuning::default().apply(style), style);
        let tuned = super::TickTuning {
            peak_spheres: Some(true),
            opacity: Some(0.5),
        }
        .apply(style);
        assert!(tuned.peaks() && tuned.zoom_regions);
        assert_eq!(tuned.opacity, 0.5);
        assert_eq!(super::TickTuning::worn(tuned).apply(style), tuned);
        let worn = super::TickBubbleStyle {
            peak_spheres: Some(false),
            ..tuned
        };
        let kept = super::TickTuning {
            opacity: Some(0.6),
            ..Default::default()
        }
        .or_worn(Some(worn));
        assert_eq!(
            kept.peak_spheres,
            Some(false),
            "an omitted field keeps what is worn"
        );
        assert_eq!(kept.opacity, Some(0.6));
        let clear = super::TickTuning {
            opacity: Some(f32::NAN),
            ..Default::default()
        }
        .apply(style);
        assert!(clear.opacity.is_finite());
    }

    /// Review #26: a reference that is not a positive number falls back to
    /// a named value, never an inline literal.
    #[test]
    fn an_invalid_reference_falls_back_to_the_named_quantity() {
        for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let mut style = super::TickBubbleStyle {
                zoom_regions: false,
                peak_spheres: None,
                peak_max_radius: None,
                viewport_scale: None,
                reference_mode: crate::TickReferenceMode::Fixed,
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
pub type TickLook = (TickBubbleStyle, super::theme::PaletteChoice);

/// How the candle spheres choose their common quantity scale.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TickReferenceMode {
    /// The configured quantity is always the full-radius quantity.
    #[default]
    Fixed,
    /// The largest visible ordinary region, at least the configured quantity.
    VisibleMax,
}

impl TickReferenceMode {
    fn is_fixed(&self) -> bool {
        *self == Self::Fixed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickBubbleStyle {
    /// Full-radius quantity in fixed mode, or the floor of the common visible
    /// reference in visible-max mode, while `viewport_scale` is absent. The
    /// viewport policy uses the exact visible maximum instead of this amount.
    /// Read through [`Self::reference`].
    pub reference_quantity: f64,
    /// Omitted values retain the fixed scale of existing saved styles.
    #[serde(default, skip_serializing_if = "TickReferenceMode::is_fixed")]
    pub reference_mode: TickReferenceMode,
    /// Exact visible-volume sizing with a common camera gain. Missing values
    /// preserve the fixed or visible-floor policy of saved appearances.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport_scale: Option<super::ViewportBubbleScale>,
    /// Spheres merge inside regions of whole candles chosen by the zoom
    /// rather than inside one second of market time: zooming out pools
    /// neighbouring candles into one sphere, zooming in splits them again.
    /// Omitted values keep the one-second regions of saved appearances.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub zoom_regions: bool,
    /// Each zoom region is divided at the valleys of its volume-by-price
    /// profile instead: one sphere per peak, its area its volume, filled by
    /// its buy/sell imbalance. Peak spheres divide zoom regions, so they imply
    /// them. Read through [`Self::peaks`]. Omitted (`None`) is a look saved
    /// before the switch existed, which takes the value of the preset it
    /// otherwise wears; an untick is saved as `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_spheres: Option<bool>,
    /// The full-size radius while peak spheres are on, so a joined mass has
    /// room to grow; omitted keeps [`Self::max_radius`]. The off path never
    /// reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peak_max_radius: Option<f32>,
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
        self.viewport_scale = self
            .viewport_scale
            .and_then(super::ViewportBubbleScale::sanitized);
        if usable_reference(self.reference_quantity).is_none() {
            self.reference_quantity = FALLBACK_REFERENCE_QUANTITY;
        }
        let off = Self {
            peak_spheres: None,
            ..*self
        };
        self.peak_max_radius = self.peak_max_radius.map(|radius| {
            Self {
                max_radius: radius,
                ..off
            }
            .bubbles()
            .max_radius
        });
        let style = off.bubbles();
        self.max_radius = style.max_radius;
        self.label_min_radius = style.label_min_radius;
        self.opacity = style.opacity;
    }

    /// The configured reference before any optional viewport presentation:
    /// [`Self::reference_quantity`] while it is a positive amount a decimal
    /// can hold, else [`FALLBACK_REFERENCE_QUANTITY`]. Without `viewport_scale`
    /// this is the fixed reference or visible-reference floor.
    #[must_use]
    pub fn reference(self) -> Decimal {
        usable_reference(self.reference_quantity).unwrap_or_else(|| {
            usable_reference(FALLBACK_REFERENCE_QUANTITY)
                .expect("the fallback reference is a positive decimal")
        })
    }

    /// Whether each zoom region is divided at its peaks.
    #[must_use]
    pub fn peaks(self) -> bool {
        self.peak_spheres == Some(true)
    }

    /// The full-size radius this look draws at: the peak radius while peak
    /// spheres are on and one is set.
    #[must_use]
    pub fn full_radius(self) -> f32 {
        match self.peak_max_radius {
            Some(radius) if self.peaks() => radius,
            _ => self.max_radius,
        }
    }

    #[must_use]
    pub fn bubbles(self) -> BubbleStyle {
        let mut style = BubbleStyle {
            max_radius: self.full_radius(),
            label_min_radius: self.label_min_radius,
            opacity: self.opacity,
            show_trade_count: false,
            ..BubbleStyle::default()
        };
        style.sanitize();
        style
    }

    #[must_use]
    pub fn fingerprint(self) -> [u64; 8] {
        let viewport = self
            .viewport_scale
            .and_then(super::ViewportBubbleScale::sanitized);
        let density = viewport.map_or([0; 2], super::ViewportBubbleScale::fingerprint);
        [
            self.reference_quantity.to_bits(),
            u64::from(self.full_radius().to_bits()),
            u64::from(self.label_min_radius.to_bits()),
            u64::from(self.opacity.to_bits()),
            // The enumerated policies share one word: the reference mode in
            // its low bit, the region policy above it, peak spheres above that.
            self.reference_mode as u64
                | (u64::from(self.zoom_regions) << 1)
                | (u64::from(self.peaks()) << 2),
            u64::from(viewport.is_some()),
            density[0],
            density[1],
        ]
    }
}

/// What a pane changes on the tick preset it wears, from the settings panel
/// or `orderflow.bubbles.tick.set`: each `None` keeps the preset's own.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct TickTuning {
    /// [`TickBubbleStyle::peak_spheres`]: `Some` sets it.
    pub peak_spheres: Option<bool>,
    /// [`TickBubbleStyle::opacity`], sanitized like any stored opacity.
    pub opacity: Option<f32>,
}

impl TickTuning {
    /// The tuning `style` wears: every field it carries.
    #[must_use]
    pub fn worn(style: TickBubbleStyle) -> Self {
        Self {
            peak_spheres: style.peak_spheres,
            opacity: Some(style.opacity),
        }
    }

    /// This tuning, each field it leaves out taken from what `worn` wears.
    #[must_use]
    pub fn or_worn(self, worn: Option<TickBubbleStyle>) -> Self {
        let worn = worn.map(Self::worn).unwrap_or_default();
        Self {
            peak_spheres: self.peak_spheres.or(worn.peak_spheres),
            opacity: self.opacity.or(worn.opacity),
        }
    }

    /// `style` with this tuning applied.
    #[must_use]
    pub fn apply(self, mut style: TickBubbleStyle) -> TickBubbleStyle {
        style.peak_spheres = self.peak_spheres.or(style.peak_spheres);
        style.opacity = self.opacity.unwrap_or(style.opacity);
        style.sanitize();
        style
    }
}

/// `quantity` as a reference, when it is a positive amount a decimal holds.
fn usable_reference(quantity: f64) -> Option<Decimal> {
    Decimal::from_f64(quantity).filter(|reference| *reference > Decimal::ZERO)
}
