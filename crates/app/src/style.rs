//! Pure, renderer-independent chart appearance.
//!
//! The structs in this module deliberately use sRGB byte triples and normalized
//! floating-point opacities instead of egui types. This keeps preset selection,
//! direction-aware colour resolution and input sanitization deterministic and
//! unit-testable without a display or graphics backend.

/// Minimum visible line width accepted by the resolved candle style.
pub const MIN_LINE_WIDTH: f32 = 0.5;
/// Maximum line width accepted by the resolved candle style.
pub const MAX_LINE_WIDTH: f32 = 4.0;
/// Minimum candle-body width as a fraction of its bar slot.
pub const MIN_BODY_WIDTH_FRAC: f32 = 0.1;
/// Maximum candle-body width as a fraction of its bar slot.
pub const MAX_BODY_WIDTH_FRAC: f32 = 1.0;
/// Maximum configured corner radius. The renderer may reduce it further to fit
/// a particularly small candle body.
pub const MAX_CORNER_RADIUS: f32 = 8.0;
/// Minimum body height in pixels, including doji candles.
pub const MIN_BODY_HEIGHT: f32 = 1.0;
/// Maximum configured minimum body height in pixels.
pub const MAX_BODY_HEIGHT: f32 = 12.0;
/// Least gap a trader can ask for between neighbouring candle bodies: none,
/// for the dense look some flow traders read tick charts in.
pub const MIN_CANDLE_GAP: f32 = 0.0;
/// Most gap that is still a chart of candles rather than a row of needles.
pub const MAX_CANDLE_GAP: f32 = 8.0;
/// The default gap, in pixels.
///
/// A candle body takes [`CandleStyle::body_width_frac`] of its slot, and a
/// proportion is the right rule while the slot is wide: at the 8 px default
/// zoom, 0.72 already leaves 2.2 px of air and this floor never binds. It is
/// the zoomed-out end that a proportion cannot serve — at 3 px per slot,
/// 28 % of the slot is under a pixel, the gap disappears into antialiasing and
/// the candles read as one solid band, which is exactly where a trader looking
/// for more history spends their time.
///
/// Two pixels is the smallest gap that survives that: one pixel of true
/// background between two antialiased edges. It is the same rule TradingView's
/// Lightweight Charts applies (`optimalCandlestickWidth` keeps at least a
/// pixel of gap and never lets a body reach its neighbour), stated as a
/// configurable floor rather than baked into the renderer.
pub const DEFAULT_CANDLE_GAP: f32 = 2.0;
/// Narrowest a candle body is ever drawn, in pixels. A bar that vanishes is
/// worse than a bar touching its neighbour, so the gap yields before the body.
pub const MIN_BODY_WIDTH_PX: f32 = 1.0;

const DEFAULT_BACKGROUND: [u8; 3] = [19, 23, 34];
const DEFAULT_GRID: [u8; 3] = [35, 41, 54];

const ORDER_FLOW_BULL_FILL: [u8; 3] = [38, 166, 154];
const ORDER_FLOW_BEAR_FILL: [u8; 3] = [239, 83, 80];
const ORDER_FLOW_BULL_OUTLINE: [u8; 3] = [104, 224, 201];
const ORDER_FLOW_BEAR_OUTLINE: [u8; 3] = [255, 137, 139];
const NEUTRAL_WICK: [u8; 3] = [160, 169, 184];

/// Whether a candle body has a translucent fill or leaves the underlying
/// order-flow layers completely visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandleBodyMode {
    /// Draw the direction-specific fill and outline.
    Filled,
    /// Draw only the outline; no body fill primitive is produced.
    OutlineOnly,
}

/// How wick colours are selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WickColorMode {
    /// Use the bullish or bearish outline RGB for the wick.
    MatchOutline,
    /// Use [`CandleStyle::wick`] for both directions.
    Custom,
}

/// Built-in candle appearances. Presets affect candles only; canvas choices
/// remain independent so applying a preset cannot unexpectedly change the
/// user's background or grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CandlePreset {
    /// Low-opacity bodies and strong contours, designed for book heatmaps.
    #[default]
    OrderFlow,
    /// Translucent glass-like bodies with softer contours.
    Glass,
    /// No body fill, preserving every heatmap cell beneath the candle.
    OutlineOnly,
    /// Conventional nearly opaque candlesticks.
    Classic,
}

impl CandlePreset {
    /// Presets in stable UI order.
    pub const ALL: [Self; 4] = [
        Self::OrderFlow,
        Self::Glass,
        Self::OutlineOnly,
        Self::Classic,
    ];

    /// Human-readable preset name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OrderFlow => "Order flow",
            Self::Glass => "Glass",
            Self::OutlineOnly => "Outline only",
            Self::Classic => "Classic",
        }
    }

    /// Stable machine-readable value for structured diagnostics.
    #[must_use]
    pub const fn log_value(self) -> &'static str {
        match self {
            Self::OrderFlow => "order_flow",
            Self::Glass => "glass",
            Self::OutlineOnly => "outline_only",
            Self::Classic => "classic",
        }
    }

    /// Materialize this preset as a fully editable candle style.
    #[must_use]
    pub fn style(self) -> CandleStyle {
        match self {
            Self::OrderFlow => CandleStyle {
                body_mode: CandleBodyMode::Filled,
                bull_fill: ORDER_FLOW_BULL_FILL,
                bear_fill: ORDER_FLOW_BEAR_FILL,
                bull_outline: ORDER_FLOW_BULL_OUTLINE,
                bear_outline: ORDER_FLOW_BEAR_OUTLINE,
                fill_opacity: 0.20,
                outline_opacity: 0.96,
                outline_width: 1.25,
                show_wicks: true,
                wick_color_mode: WickColorMode::MatchOutline,
                wick: NEUTRAL_WICK,
                wick_opacity: 0.90,
                wick_width: 1.0,
                body_width_frac: 0.72,
                gap_px: DEFAULT_CANDLE_GAP,
                corner_radius: 0.75,
                min_body_height: 1.5,
                forming_opacity: 0.62,
            },
            Self::Glass => CandleStyle {
                body_mode: CandleBodyMode::Filled,
                bull_fill: [55, 204, 180],
                bear_fill: [245, 102, 105],
                bull_outline: [132, 235, 216],
                bear_outline: [255, 158, 160],
                fill_opacity: 0.35,
                outline_opacity: 0.82,
                outline_width: 1.0,
                show_wicks: true,
                wick_color_mode: WickColorMode::MatchOutline,
                wick: NEUTRAL_WICK,
                wick_opacity: 0.74,
                wick_width: 1.0,
                body_width_frac: 0.70,
                gap_px: DEFAULT_CANDLE_GAP,
                corner_radius: 1.25,
                min_body_height: 1.5,
                forming_opacity: 0.58,
            },
            Self::OutlineOnly => CandleStyle {
                body_mode: CandleBodyMode::OutlineOnly,
                bull_fill: ORDER_FLOW_BULL_FILL,
                bear_fill: ORDER_FLOW_BEAR_FILL,
                bull_outline: [110, 232, 207],
                bear_outline: [255, 143, 145],
                fill_opacity: 0.0,
                outline_opacity: 0.98,
                outline_width: 1.35,
                show_wicks: true,
                wick_color_mode: WickColorMode::MatchOutline,
                wick: NEUTRAL_WICK,
                wick_opacity: 0.92,
                wick_width: 1.0,
                body_width_frac: 0.72,
                gap_px: DEFAULT_CANDLE_GAP,
                corner_radius: 0.75,
                min_body_height: 1.5,
                forming_opacity: 0.68,
            },
            Self::Classic => CandleStyle {
                body_mode: CandleBodyMode::Filled,
                bull_fill: [38, 166, 154],
                bear_fill: [239, 83, 80],
                bull_outline: [38, 166, 154],
                bear_outline: [239, 83, 80],
                fill_opacity: 1.0,
                outline_opacity: 1.0,
                outline_width: 1.0,
                show_wicks: true,
                wick_color_mode: WickColorMode::MatchOutline,
                wick: NEUTRAL_WICK,
                wick_opacity: 1.0,
                wick_width: 1.0,
                body_width_frac: 0.70,
                gap_px: DEFAULT_CANDLE_GAP,
                corner_radius: 0.0,
                min_body_height: 1.0,
                forming_opacity: 0.50,
            },
        }
    }

    /// Detect an unchanged built-in preset. Any user customization returns
    /// `None`, allowing the UI and logs to report a truthful `custom` state.
    #[must_use]
    pub fn detect(style: &CandleStyle) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.style() == *style)
    }
}

/// User-editable candle appearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleStyle {
    /// Whether candle bodies are filled or contour-only.
    pub body_mode: CandleBodyMode,
    /// Body fill colour for up (`close >= open`) candles.
    pub bull_fill: [u8; 3],
    /// Body fill colour for down candles.
    pub bear_fill: [u8; 3],
    /// Contour colour for up candles.
    pub bull_outline: [u8; 3],
    /// Contour colour for down candles.
    pub bear_outline: [u8; 3],
    /// Body fill opacity in `[0, 1]`.
    pub fill_opacity: f32,
    /// Body contour opacity in `[0, 1]`.
    pub outline_opacity: f32,
    /// Body contour width in pixels.
    pub outline_width: f32,
    /// Whether upper and lower wicks are rendered.
    pub show_wicks: bool,
    /// Direction-aware or common custom wick colour.
    pub wick_color_mode: WickColorMode,
    /// Common wick RGB when [`WickColorMode::Custom`] is selected.
    pub wick: [u8; 3],
    /// Wick opacity in `[0, 1]`.
    pub wick_opacity: f32,
    /// Wick width in pixels.
    pub wick_width: f32,
    /// Candle body width as a fraction of its bar slot.
    pub body_width_frac: f32,
    /// Least air kept between neighbouring candle bodies, in pixels.
    ///
    /// The companion of [`body_width_frac`](Self::body_width_frac), and the
    /// one that speaks for the zoomed-out end of the chart: see
    /// [`DEFAULT_CANDLE_GAP`] for why a proportion alone cannot keep candles
    /// apart there.
    pub gap_px: f32,
    /// Body corner radius in pixels.
    pub corner_radius: f32,
    /// Smallest rendered body height in pixels, including doji candles.
    pub min_body_height: f32,
    /// Alpha multiplier applied to all parts of the still-forming candle.
    pub forming_opacity: f32,
}

impl CandleStyle {
    /// Body width fraction sanitized for layout calculations.
    #[must_use]
    pub fn clamped_width_frac(&self) -> f32 {
        finite_range(
            self.body_width_frac,
            MIN_BODY_WIDTH_FRAC,
            MAX_BODY_WIDTH_FRAC,
            0.70,
        )
    }

    /// Candle gap sanitized for layout calculations.
    #[must_use]
    pub fn clamped_gap_px(&self) -> f32 {
        finite_range(
            self.gap_px,
            MIN_CANDLE_GAP,
            MAX_CANDLE_GAP,
            DEFAULT_CANDLE_GAP,
        )
    }

    /// Half the width of a candle body drawn in a `slot_px`-wide bar slot.
    ///
    /// Two rules, the tighter one wins. The body takes
    /// [`clamped_width_frac`](Self::clamped_width_frac) of the slot — a
    /// proportion, which is what the eye reads while the candles are wide —
    /// and neighbouring bodies keep [`clamped_gap_px`](Self::clamped_gap_px)
    /// between them, which is what keeps them apart once a proportion of the
    /// slot is no longer a pixel. Below both, the body is still drawn
    /// [`MIN_BODY_WIDTH_PX`] wide.
    #[must_use]
    pub fn body_half_width(&self, slot_px: f32) -> f32 {
        let slot = if slot_px.is_finite() {
            slot_px.max(0.0)
        } else {
            0.0
        };
        let by_fraction = slot * self.clamped_width_frac();
        let by_gap = slot - self.clamped_gap_px();
        by_fraction.min(by_gap).max(MIN_BODY_WIDTH_PX) / 2.0
    }

    /// Resolve direction, body mode, forming state and unsafe numeric inputs to
    /// a finite paint description. RGB bytes are copied without modification;
    /// only alpha and geometry values are sanitized.
    #[must_use]
    pub fn resolved(&self, up: bool, forming: bool) -> ResolvedCandlePaint {
        let fill_rgb = if up { self.bull_fill } else { self.bear_fill };
        let outline_rgb = if up {
            self.bull_outline
        } else {
            self.bear_outline
        };
        let forming_factor = if forming {
            unit_interval(self.forming_opacity, 0.62)
        } else {
            1.0
        };

        let fill = match self.body_mode {
            CandleBodyMode::Filled => Some(rgba(
                fill_rgb,
                unit_interval(self.fill_opacity, 0.20) * forming_factor,
            )),
            CandleBodyMode::OutlineOnly => None,
        };
        let outline = rgba(
            outline_rgb,
            unit_interval(self.outline_opacity, 0.96) * forming_factor,
        );
        let wick = self.show_wicks.then(|| {
            let rgb = match self.wick_color_mode {
                WickColorMode::MatchOutline => outline_rgb,
                WickColorMode::Custom => self.wick,
            };
            rgba(rgb, unit_interval(self.wick_opacity, 0.90) * forming_factor)
        });

        ResolvedCandlePaint {
            fill,
            outline,
            wick,
            outline_width: finite_range(self.outline_width, MIN_LINE_WIDTH, MAX_LINE_WIDTH, 1.0),
            wick_width: finite_range(self.wick_width, MIN_LINE_WIDTH, MAX_LINE_WIDTH, 1.0),
            corner_radius: finite_range(self.corner_radius, 0.0, MAX_CORNER_RADIUS, 0.0),
            min_body_height: finite_range(
                self.min_body_height,
                MIN_BODY_HEIGHT,
                MAX_BODY_HEIGHT,
                MIN_BODY_HEIGHT,
            ),
        }
    }
}

impl Default for CandleStyle {
    fn default() -> Self {
        CandlePreset::OrderFlow.style()
    }
}

/// A renderer-ready candle paint description.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedCandlePaint {
    /// Direction-specific body RGBA, or `None` in outline-only mode.
    pub fill: Option<[u8; 4]>,
    /// Direction-specific contour RGBA.
    pub outline: [u8; 4],
    /// Resolved wick RGBA, or `None` when wicks are hidden.
    pub wick: Option<[u8; 4]>,
    /// Sanitized contour width in pixels.
    pub outline_width: f32,
    /// Sanitized wick width in pixels.
    pub wick_width: f32,
    /// Sanitized corner radius in pixels.
    pub corner_radius: f32,
    /// Sanitized minimum body height in pixels.
    pub min_body_height: f32,
}

/// User-editable chart canvas appearance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasStyle {
    /// Whether the renderer paints a chart background.
    pub background_enabled: bool,
    /// Chart background RGB.
    pub background: [u8; 3],
    /// Whether horizontal price grid lines are rendered.
    pub grid_enabled: bool,
    /// Price grid RGB.
    pub grid: [u8; 3],
    /// Price grid opacity in `[0, 1]`.
    pub grid_opacity: f32,
}

impl CanvasStyle {
    /// Background RGBA suitable for direct renderer conversion. A disabled
    /// background resolves to canonical transparent black.
    #[must_use]
    pub fn background_rgba(&self) -> [u8; 4] {
        if self.background_enabled {
            rgba(self.background, 1.0)
        } else {
            [0, 0, 0, 0]
        }
    }

    /// Grid RGBA suitable for direct renderer conversion, or `None` when grid
    /// painting is disabled.
    #[must_use]
    pub fn grid_rgba(&self) -> Option<[u8; 4]> {
        self.grid_enabled
            .then(|| rgba(self.grid, unit_interval(self.grid_opacity, 0.65)))
    }
}

impl Default for CanvasStyle {
    fn default() -> Self {
        Self {
            background_enabled: true,
            background: DEFAULT_BACKGROUND,
            grid_enabled: true,
            grid: DEFAULT_GRID,
            grid_opacity: 0.65,
        }
    }
}

/// Complete chart appearance, keeping candle and canvas concerns separate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartStyle {
    /// Candle-specific appearance.
    pub candles: CandleStyle,
    /// Background and grid appearance.
    pub canvas: CanvasStyle,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self {
            candles: CandlePreset::OrderFlow.style(),
            canvas: CanvasStyle::default(),
        }
    }
}

/// Clamp a floating-point value while giving NaN a deterministic fallback.
///
/// Positive and negative infinity naturally clamp to the corresponding bound.
fn finite_range(value: f32, min: f32, max: f32, nan_fallback: f32) -> f32 {
    if value.is_nan() {
        nan_fallback.clamp(min, max)
    } else {
        value.clamp(min, max)
    }
}

fn unit_interval(value: f32, nan_fallback: f32) -> f32 {
    finite_range(value, 0.0, 1.0, nan_fallback)
}

fn rgba(rgb: [u8; 3], opacity: f32) -> [u8; 4] {
    let alpha = (unit_interval(opacity, 1.0) * 255.0).round() as u8;
    [rgb[0], rgb[1], rgb[2], alpha]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(opacity: f32) -> u8 {
        (opacity * 255.0).round() as u8
    }

    #[test]
    fn default_chart_uses_order_flow_candles_and_dark_canvas() {
        let style = ChartStyle::default();
        assert_eq!(style.candles, CandlePreset::OrderFlow.style());
        assert_eq!(
            CandlePreset::detect(&style.candles),
            Some(CandlePreset::OrderFlow)
        );
        assert_eq!(style.canvas.background_rgba(), [19, 23, 34, 255]);
        assert_eq!(style.canvas.grid_rgba(), Some([35, 41, 54, alpha(0.65)]));
    }

    #[test]
    fn preset_metadata_and_order_are_stable() {
        assert_eq!(
            CandlePreset::ALL,
            [
                CandlePreset::OrderFlow,
                CandlePreset::Glass,
                CandlePreset::OutlineOnly,
                CandlePreset::Classic
            ]
        );
        assert_eq!(CandlePreset::OrderFlow.label(), "Order flow");
        assert_eq!(CandlePreset::Glass.log_value(), "glass");
        assert_eq!(CandlePreset::OutlineOnly.log_value(), "outline_only");
        assert_eq!(CandlePreset::Classic.label(), "Classic");
        assert_eq!(CandlePreset::default(), CandlePreset::OrderFlow);
    }

    #[test]
    fn every_preset_is_detected_and_customization_is_not() {
        for preset in CandlePreset::ALL {
            let style = preset.style();
            assert_eq!(CandlePreset::detect(&style), Some(preset));
        }

        let mut custom = CandlePreset::Glass.style();
        custom.outline_width += 0.1;
        assert_eq!(CandlePreset::detect(&custom), None);
    }

    #[test]
    fn preset_fill_levels_match_their_visual_intent() {
        let order_flow = CandlePreset::OrderFlow.style();
        let glass = CandlePreset::Glass.style();
        let outline = CandlePreset::OutlineOnly.style();
        let classic = CandlePreset::Classic.style();

        assert!((0.18..=0.22).contains(&order_flow.fill_opacity));
        assert!((glass.fill_opacity - 0.35).abs() < f32::EPSILON);
        assert_eq!(outline.body_mode, CandleBodyMode::OutlineOnly);
        assert_eq!(outline.resolved(true, false).fill, None);
        assert!((classic.fill_opacity - 1.0).abs() < f32::EPSILON);
        assert!(order_flow.outline_opacity > order_flow.fill_opacity);
    }

    #[test]
    fn direction_selects_independent_fill_outline_and_matching_wick_rgb() {
        let style = CandleStyle {
            bull_fill: [1, 2, 3],
            bear_fill: [4, 5, 6],
            bull_outline: [7, 8, 9],
            bear_outline: [10, 11, 12],
            fill_opacity: 1.0,
            outline_opacity: 1.0,
            wick_opacity: 1.0,
            ..CandleStyle::default()
        };

        let bull = style.resolved(true, false);
        assert_eq!(bull.fill, Some([1, 2, 3, 255]));
        assert_eq!(bull.outline, [7, 8, 9, 255]);
        assert_eq!(bull.wick, Some([7, 8, 9, 255]));

        let bear = style.resolved(false, false);
        assert_eq!(bear.fill, Some([4, 5, 6, 255]));
        assert_eq!(bear.outline, [10, 11, 12, 255]);
        assert_eq!(bear.wick, Some([10, 11, 12, 255]));
    }

    #[test]
    fn outline_only_omits_fill_but_preserves_contour() {
        let style = CandleStyle {
            body_mode: CandleBodyMode::OutlineOnly,
            fill_opacity: 1.0,
            bull_outline: [21, 22, 23],
            outline_opacity: 0.5,
            ..CandleStyle::default()
        };
        let paint = style.resolved(true, false);
        assert_eq!(paint.fill, None);
        assert_eq!(paint.outline, [21, 22, 23, alpha(0.5)]);
    }

    #[test]
    fn custom_wick_and_hidden_wick_are_resolved_without_affecting_outline() {
        let mut style = CandleStyle {
            wick_color_mode: WickColorMode::Custom,
            wick: [40, 41, 42],
            wick_opacity: 0.4,
            ..CandleStyle::default()
        };
        let visible = style.resolved(true, false);
        assert_eq!(visible.wick, Some([40, 41, 42, alpha(0.4)]));
        assert_eq!(
            &visible.outline[..3],
            &style.bull_outline,
            "custom wick must not change the contour"
        );

        style.show_wicks = false;
        assert_eq!(style.resolved(true, false).wick, None);
    }

    #[test]
    fn forming_opacity_multiplies_every_alpha_without_changing_rgb() {
        let style = CandleStyle {
            bull_fill: [1, 2, 3],
            bull_outline: [4, 5, 6],
            wick_color_mode: WickColorMode::Custom,
            wick: [7, 8, 9],
            fill_opacity: 0.8,
            outline_opacity: 0.6,
            wick_opacity: 0.4,
            forming_opacity: 0.5,
            ..CandleStyle::default()
        };

        let closed = style.resolved(true, false);
        let forming = style.resolved(true, true);
        assert_eq!(closed.fill, Some([1, 2, 3, alpha(0.8)]));
        assert_eq!(closed.outline, [4, 5, 6, alpha(0.6)]);
        assert_eq!(closed.wick, Some([7, 8, 9, alpha(0.4)]));
        assert_eq!(forming.fill, Some([1, 2, 3, alpha(0.4)]));
        assert_eq!(forming.outline, [4, 5, 6, alpha(0.3)]);
        assert_eq!(forming.wick, Some([7, 8, 9, alpha(0.2)]));
    }

    #[test]
    fn opacity_inputs_clamp_nan_and_infinities_while_preserving_rgb() {
        let style = CandleStyle {
            bull_fill: [1, 2, 3],
            bull_outline: [4, 5, 6],
            wick_color_mode: WickColorMode::Custom,
            wick: [7, 8, 9],
            fill_opacity: f32::NAN,
            outline_opacity: f32::INFINITY,
            wick_opacity: f32::NEG_INFINITY,
            forming_opacity: f32::NAN,
            ..CandleStyle::default()
        };

        let closed = style.resolved(true, false);
        assert_eq!(closed.fill, Some([1, 2, 3, alpha(0.20)]));
        assert_eq!(closed.outline, [4, 5, 6, 255]);
        assert_eq!(closed.wick, Some([7, 8, 9, 0]));

        let forming = style.resolved(true, true);
        assert_eq!(&forming.fill.unwrap()[..3], &[1, 2, 3]);
        assert_eq!(&forming.outline[..3], &[4, 5, 6]);
        assert_eq!(&forming.wick.unwrap()[..3], &[7, 8, 9]);
        assert!(forming.fill.unwrap()[3] < 255);
    }

    #[test]
    fn nan_opacities_fall_back_to_order_flow_safe_visibility() {
        let style = CandleStyle {
            fill_opacity: f32::NAN,
            outline_opacity: f32::NAN,
            wick_opacity: f32::NAN,
            forming_opacity: 1.0,
            ..CandleStyle::default()
        };
        let paint = style.resolved(true, false);
        assert_eq!(paint.fill.unwrap()[3], alpha(0.20));
        assert_eq!(paint.outline[3], alpha(0.96));
        assert_eq!(paint.wick.unwrap()[3], alpha(0.90));
    }

    #[test]
    fn geometric_inputs_are_always_finite_and_bounded() {
        let lower = CandleStyle {
            outline_width: f32::NEG_INFINITY,
            wick_width: -100.0,
            body_width_frac: f32::NEG_INFINITY,
            corner_radius: -10.0,
            min_body_height: f32::NEG_INFINITY,
            ..CandleStyle::default()
        }
        .resolved(true, false);
        assert_eq!(lower.outline_width, MIN_LINE_WIDTH);
        assert_eq!(lower.wick_width, MIN_LINE_WIDTH);
        assert_eq!(lower.corner_radius, 0.0);
        assert_eq!(lower.min_body_height, MIN_BODY_HEIGHT);
        assert_eq!(
            CandleStyle {
                body_width_frac: f32::NEG_INFINITY,
                ..CandleStyle::default()
            }
            .clamped_width_frac(),
            MIN_BODY_WIDTH_FRAC
        );

        let upper = CandleStyle {
            outline_width: f32::INFINITY,
            wick_width: 100.0,
            body_width_frac: f32::INFINITY,
            corner_radius: f32::INFINITY,
            min_body_height: 100.0,
            ..CandleStyle::default()
        }
        .resolved(false, false);
        assert_eq!(upper.outline_width, MAX_LINE_WIDTH);
        assert_eq!(upper.wick_width, MAX_LINE_WIDTH);
        assert_eq!(upper.corner_radius, MAX_CORNER_RADIUS);
        assert_eq!(upper.min_body_height, MAX_BODY_HEIGHT);
        assert_eq!(
            CandleStyle {
                body_width_frac: f32::INFINITY,
                ..CandleStyle::default()
            }
            .clamped_width_frac(),
            MAX_BODY_WIDTH_FRAC
        );

        let nan = CandleStyle {
            outline_width: f32::NAN,
            wick_width: f32::NAN,
            body_width_frac: f32::NAN,
            corner_radius: f32::NAN,
            min_body_height: f32::NAN,
            ..CandleStyle::default()
        }
        .resolved(true, false);
        for value in [
            nan.outline_width,
            nan.wick_width,
            nan.corner_radius,
            nan.min_body_height,
        ] {
            assert!(value.is_finite());
        }
        assert_eq!(nan.outline_width, 1.0);
        assert_eq!(nan.wick_width, 1.0);
        assert_eq!(nan.corner_radius, 0.0);
        assert_eq!(nan.min_body_height, MIN_BODY_HEIGHT);
        assert_eq!(
            CandleStyle {
                body_width_frac: f32::NAN,
                ..CandleStyle::default()
            }
            .clamped_width_frac(),
            0.70
        );
    }

    #[test]
    fn canvas_enabled_flags_and_grid_alpha_are_resolved() {
        let canvas = CanvasStyle {
            background_enabled: false,
            background: [1, 2, 3],
            grid_enabled: true,
            grid: [4, 5, 6],
            grid_opacity: 0.25,
        };
        assert_eq!(canvas.background_rgba(), [0, 0, 0, 0]);
        assert_eq!(canvas.grid_rgba(), Some([4, 5, 6, alpha(0.25)]));

        let hidden_grid = CanvasStyle {
            background_enabled: true,
            grid_enabled: false,
            ..canvas
        };
        assert_eq!(hidden_grid.background_rgba(), [1, 2, 3, 255]);
        assert_eq!(hidden_grid.grid_rgba(), None);
    }

    #[test]
    fn canvas_grid_alpha_clamps_invalid_values() {
        let positive = CanvasStyle {
            grid_opacity: f32::INFINITY,
            ..CanvasStyle::default()
        };
        assert_eq!(positive.grid_rgba().unwrap()[3], 255);

        let negative = CanvasStyle {
            grid_opacity: f32::NEG_INFINITY,
            ..CanvasStyle::default()
        };
        assert_eq!(negative.grid_rgba().unwrap()[3], 0);

        let nan = CanvasStyle {
            grid_opacity: f32::NAN,
            ..CanvasStyle::default()
        };
        assert_eq!(nan.grid_rgba().unwrap()[3], alpha(0.65));
    }

    /// The two rules divide the zoom range between them: the proportion shapes
    /// the chart while the slots are wide, the gap takes over once a proportion
    /// of the slot stops being a visible amount of screen.
    #[test]
    fn the_gap_takes_over_from_the_proportion_as_the_candles_narrow() {
        let style = CandleStyle::default(); // 0.72 of the slot, 2 px gap
        // Default zoom: 8 px slots. The proportion is the tighter rule and the
        // look a trader already knows is untouched.
        assert!((style.body_half_width(8.0) - 8.0 * 0.72 / 2.0).abs() < 0.001);
        // 4 px slots: 0.72 would leave 1.1 px of air, which antialiasing eats.
        // The gap binds instead — 2 px of body, 2 px of air.
        assert!((style.body_half_width(4.0) - 1.0).abs() < 0.001);
        // 2 px slots: nothing is left for a gap, so the body holds its floor
        // rather than vanishing.
        assert!((style.body_half_width(2.0) - MIN_BODY_WIDTH_PX / 2.0).abs() < 0.001);
    }

    /// The gap is a preference, not a law: asking for none brings back the
    /// dense look, where a body is exactly its share of the slot.
    #[test]
    fn a_zero_gap_leaves_the_proportion_alone() {
        let dense = CandleStyle {
            gap_px: 0.0,
            ..CandleStyle::default()
        };
        for slot in [4.0_f32, 8.0, 24.0, 160.0] {
            let expected = (slot * 0.72).max(MIN_BODY_WIDTH_PX) / 2.0;
            assert!(
                (dense.body_half_width(slot) - expected).abs() < 0.001,
                "slot {slot}"
            );
        }
    }

    /// Every preset ships the same gap: it is a legibility floor, not a look,
    /// and a preset that dropped it would read as broken at the zoom this
    /// exists for.
    #[test]
    fn every_preset_keeps_the_default_gap() {
        for preset in CandlePreset::ALL {
            assert!(
                (preset.style().clamped_gap_px() - DEFAULT_CANDLE_GAP).abs() < f32::EPSILON,
                "{preset:?}"
            );
        }
    }

    #[test]
    fn a_hostile_gap_still_draws_a_candle() {
        for gap in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -10.0, 1_000.0] {
            let style = CandleStyle {
                gap_px: gap,
                ..CandleStyle::default()
            };
            let half = style.body_half_width(8.0);
            assert!(
                (MIN_BODY_WIDTH_PX / 2.0..=8.0).contains(&half),
                "gap {gap}: half = {half}"
            );
        }
        // A slot that is not a number cannot be divided up, and the candle
        // falls back to its floor rather than to a NaN rectangle.
        for slot in [f32::NAN, f32::INFINITY, -4.0] {
            assert!(
                (CandleStyle::default().body_half_width(slot) - MIN_BODY_WIDTH_PX / 2.0).abs()
                    < 0.001,
                "slot {slot}"
            );
        }
    }
}
