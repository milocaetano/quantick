//! Portable order-flow render choices and theme colour arithmetic.

use super::{BubbleStyle, HeatmapConfig, HeatmapTheme, LiveLaneStyle};
use quantick_orderbook::BookSide;

// A perceptually smoother Bookmap-style thermal ramp. It keeps the signature
// deep-blue → cyan low end but restores the green and orange phases the classic
// Bookmap heatmap passes through, so adjacent liquidity magnitudes stay
// distinguishable instead of collapsing into one cyan-to-yellow jump. The floor
// is pure black so quiet levels fade cleanly into the canvas.
const BOOKMAP_RAMP: [ColorStop; 9] = [
    ColorStop::new(0.00, [0, 0, 0]),
    ColorStop::new(0.09, [4, 10, 40]),
    ColorStop::new(0.22, [10, 46, 120]),
    ColorStop::new(0.38, [0, 120, 196]),
    ColorStop::new(0.55, [0, 194, 196]),
    ColorStop::new(0.70, [60, 208, 120]),
    ColorStop::new(0.83, [208, 220, 60]),
    ColorStop::new(0.93, [250, 158, 44]),
    ColorStop::new(1.00, [255, 250, 232]),
];

const HIGH_CONTRAST_RAMP: [ColorStop; 6] = [
    ColorStop::new(0.00, [0, 0, 0]),
    ColorStop::new(0.14, [0, 18, 76]),
    ColorStop::new(0.40, [0, 116, 255]),
    ColorStop::new(0.64, [0, 240, 255]),
    ColorStop::new(0.84, [255, 230, 0]),
    ColorStop::new(1.00, [255, 255, 255]),
];

// A perceptually ordered, viridis-inspired ramp. It avoids relying on a
// red/green distinction for resting-liquidity magnitude.
const COLOR_BLIND_RAMP: [ColorStop; 6] = [
    ColorStop::new(0.00, [7, 8, 31]),
    ColorStop::new(0.16, [53, 38, 111]),
    ColorStop::new(0.42, [42, 111, 151]),
    ColorStop::new(0.67, [37, 174, 128]),
    ColorStop::new(0.86, [184, 211, 55]),
    ColorStop::new(1.00, [253, 231, 126]),
];

/// Tunable visual choices. No field changes projection or retained history.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderflowRenderStyle {
    pub theme: HeatmapTheme,
    /// Additional multiplier over the projection's factual alpha.
    pub heat_opacity: f32,
    /// Minimum on-screen band height after price projection.
    pub min_cell_height: f32,
    /// Strength of the soft edge laid behind each heat cell.
    pub edge_glow: f32,
    /// Every user-owned bubble choice, straight from the settings panel.
    pub bubbles: BubbleStyle,
    /// The live lane's own choices: how wide the reserved band is, how its
    /// prints cluster, and how much bigger their bubbles read.
    pub live_lane: LiveLaneStyle,
    /// How volume dots are sized on this screen: each dot's cell and its
    /// pane's full size. `None` draws the style's radii on the typed scale.
    pub dot_sizing: Option<crate::DotSizing>,
    /// Exclude known first recorded native bursts only from the tape's auto scale.
    pub ignore_opening_burst_in_scale: bool,
    pub show_gap_labels: bool,
    pub show_legend: bool,
    /// Whether the L2 depth layer is active over the candles. The legend only
    /// advertises keys for layers that can actually draw something.
    pub depth_layer: bool,
    /// Whether the aggression layer is active over the candles.
    pub aggression_layer: bool,
    /// Whether the L2 depth layer is active on the tape.
    ///
    /// The two panes are switched apart because they are read apart: the
    /// compressed history answers "where has size been resting", the rolling
    /// tape answers "what is resting there right now". A trader clearing the
    /// candles to read structure is not thereby asking to lose the book where
    /// the book is being watched.
    pub lane_depth_layer: bool,
    /// Whether the aggression layer is active on the tape. Same reasoning as
    /// [`lane_depth_layer`](Self::lane_depth_layer).
    pub lane_aggression_layer: bool,
    /// Per-layer display switches, mirroring the config flags. The projection
    /// no longer filters the aggression primitives — several surfaces read
    /// them — so for those two switches this is where the decision is made
    /// in the renderer; for the rest the renderer's job is
    /// keeping the legend honest about which layers can draw.
    pub show_liquidity: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub show_buy: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub show_sell: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub show_aligned: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub show_unattributed: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub show_gaps: bool,
    pub legend_max_width: f32,
    /// Vertical space already spoken for at the canvas's top-left corner: the
    /// chart header, plus whatever the pane stacked under it (an indicator
    /// chip per row). The legend starts below it, so the two can never print
    /// over each other — they did, because this used to be a constant that
    /// only knew about the header.
    pub legend_top_inset: f32,
    /// Follows the chart canvas so the deterministic preview sits on the same
    /// ground as the live chart.
    pub canvas_background: [u8; 4],
}

/// The chart header's own row at the canvas's top-left corner: the floor
/// every legend inset starts from, whatever the pane measured.
pub const LEGEND_HEADER_CLEARANCE_PX: f32 = 22.0;

impl Default for OrderflowRenderStyle {
    fn default() -> Self {
        Self {
            theme: HeatmapTheme::Bookmap,
            heat_opacity: 1.0,
            min_cell_height: 1.5,
            // Off by default: the per-cell glow doubles the heatmap's quad count,
            // which is the single biggest render cost on a dense book.
            edge_glow: 0.0,
            bubbles: BubbleStyle::default(),
            live_lane: LiveLaneStyle::default(),
            dot_sizing: None,
            ignore_opening_burst_in_scale: false,
            show_gap_labels: true,
            show_legend: true,
            depth_layer: true,
            aggression_layer: true,
            lane_depth_layer: true,
            lane_aggression_layer: true,
            show_liquidity: true,
            show_buy: true,
            show_sell: true,
            show_aligned: true,
            show_unattributed: true,
            show_gaps: true,
            legend_max_width: 690.0,
            legend_top_inset: LEGEND_HEADER_CLEARANCE_PX,
            canvas_background: [19, 23, 34, 255],
        }
    }
}

impl OrderflowRenderStyle {
    /// Resolve every renderer choice that has a corresponding user setting.
    ///
    /// The whole bubble vocabulary — alpha, radii, marks, colours — now comes
    /// from the aggression panel in one struct.
    #[must_use]
    pub fn from_config(config: &HeatmapConfig, canvas_background: [u8; 4]) -> Self {
        Self {
            theme: config.theme,
            bubbles: config.bubbles.clone(),
            dot_sizing: None,
            ignore_opening_burst_in_scale: config.volume_dots.ignore_opening_burst_in_scale,
            live_lane: config.live_lane.clone(),
            show_legend: config.show_legend,
            // A tape-only pane draws no candles, so nothing rides on them.
            // The native tape keys every print on its own execution time and
            // price, so beside the candles it leaves no candle-slot mark; the
            // book still reaches the candles there.
            depth_layer: config.depth_visible() && !config.tape_only(),
            aggression_layer: config.show_aggressions && !config.native_tape(),
            lane_depth_layer: config.lane_depth_drawn(),
            lane_aggression_layer: config.lane_aggressions_drawn(),
            // The trader's own four switches, carried raw. Whether a *pane*
            // still draws a book is a second question, and it is asked where
            // the pane is known — `draw_liquidity_events` for the reductions,
            // the depth clip for the cells. Folding the two together here would
            // answer "is a book drawn anywhere", which clears the candles only
            // when the tape's map is off too.
            show_liquidity: config.show_liquidity,
            show_buy: config.show_buy_aggressions,
            show_sell: config.show_sell_aggressions,
            show_aligned: config.show_aligned_depletion,
            show_unattributed: config.show_unattributed_reductions,
            show_gaps: config.show_gaps,
            canvas_background,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn sanitized(&self) -> Self {
        let mut style = self.clone();
        style.heat_opacity = finite_clamp(style.heat_opacity, 0.0, 1.0, 1.0);
        style.min_cell_height = finite_clamp(style.min_cell_height, 0.5, 12.0, 1.5);
        style.edge_glow = finite_clamp(style.edge_glow, 0.0, 1.0, 0.18);
        style.bubbles.sanitize();
        style.live_lane.sanitize();
        style.legend_max_width = finite_clamp(style.legend_max_width, 160.0, 2_000.0, 690.0);
        // A caller that measured nothing still clears the header. The ceiling
        // is the canvas's, applied where the canvas is known (`draw_compact_legend`).
        style.legend_top_inset = finite_clamp(
            style.legend_top_inset,
            LEGEND_HEADER_CLEARANCE_PX,
            f32::MAX,
            LEGEND_HEADER_CLEARANCE_PX,
        );
        style
    }
}

/// The theme's own bubble colours, so the settings panel can show what
/// "follows the theme" actually looks like next to a custom swatch.
#[derive(Debug, Clone, Copy)]
pub struct ThemeBubbleRgb {
    pub buy: [u8; 3],
    pub sell: [u8; 3],
    pub front: [u8; 3],
    pub text: [u8; 3],
}

#[must_use]
pub fn theme_bubble_rgb(theme: HeatmapTheme) -> ThemeBubbleRgb {
    let palette = Palette::for_theme(theme, |color| color);
    let rgb = |color: [u8; 4]| [color[0], color[1], color[2]];
    ThemeBubbleRgb {
        buy: rgb(palette.buy),
        sell: rgb(palette.sell),
        front: rgb(palette.consumption),
        text: rgb(palette.bubble_text),
    }
}

#[derive(Debug, Clone, Copy)]
struct ColorStop {
    at: f32,
    rgb: [u8; 3],
}

impl ColorStop {
    const fn new(at: f32, rgb: [u8; 3]) -> Self {
        Self { at, rgb }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Palette<C> {
    pub buy: C,
    pub sell: C,
    pub consumption: C,
    pub depth_only: C,
    pub bubble_text: C,
    pub gap_fill: C,
    pub gap_boundary: C,
    /// Boundary between the forming bar's candle and its live lane.
    pub lane_divider: C,
    /// The line market time has walked to inside the lane.
    pub lane_now: C,
    pub muted_text: C,
    pub legend_text: C,
    pub legend_background: C,
    pub legend_border: C,
}

impl<C> Palette<C> {
    pub fn for_theme(theme: HeatmapTheme, rgba: impl Fn([u8; 4]) -> C) -> Self {
        match theme {
            HeatmapTheme::Bookmap => Self {
                buy: rgba([46, 224, 150, 255]),
                sell: rgba([255, 82, 96, 255]),
                consumption: rgba([255, 246, 205, 255]),
                depth_only: rgba([184, 130, 240, 255]),
                bubble_text: rgba([255, 255, 255, 255]),
                gap_fill: rgba([21, 24, 32, 20]),
                gap_boundary: rgba([157, 167, 188, 115]),
                lane_divider: rgba([120, 132, 156, 90]),
                lane_now: rgba([226, 234, 250, 150]),
                muted_text: rgba([186, 194, 209, 255]),
                legend_text: rgba([225, 230, 239, 255]),
                legend_background: rgba([8, 12, 23, 225]),
                legend_border: rgba([130, 145, 170, 90]),
            },
            HeatmapTheme::HighContrast => Self {
                buy: rgba([0, 255, 138, 255]),
                sell: rgba([255, 45, 70, 255]),
                consumption: rgba([255, 255, 255, 255]),
                depth_only: rgba([225, 105, 255, 255]),
                bubble_text: rgba([255, 255, 255, 255]),
                gap_fill: rgba([35, 35, 40, 28]),
                gap_boundary: rgba([218, 222, 235, 255]),
                lane_divider: rgba([160, 160, 160, 255]),
                lane_now: rgba([255, 255, 255, 255]),
                muted_text: rgba([255, 255, 255, 255]),
                legend_text: rgba([255, 255, 255, 255]),
                legend_background: rgba([0, 0, 0, 238]),
                legend_border: rgba([175, 175, 175, 255]),
            },
            HeatmapTheme::ColorBlind => Self {
                buy: rgba([64, 160, 255, 255]),
                sell: rgba([255, 159, 28, 255]),
                consumption: rgba([255, 238, 170, 255]),
                depth_only: rgba([220, 95, 205, 255]),
                bubble_text: rgba([255, 255, 255, 255]),
                gap_fill: rgba([25, 25, 30, 22]),
                gap_boundary: rgba([176, 180, 190, 255]),
                lane_divider: rgba([140, 143, 152, 95]),
                lane_now: rgba([232, 232, 226, 155]),
                muted_text: rgba([214, 215, 210, 255]),
                legend_text: rgba([232, 232, 226, 255]),
                legend_background: rgba([10, 11, 25, 230]),
                legend_border: rgba([165, 166, 180, 100]),
            },
        }
    }
}

pub fn resting_rgb(theme: HeatmapTheme, side: BookSide, intensity: f32) -> [u8; 3] {
    let base = thermal_rgb(theme, intensity);
    let tint = match (theme, side) {
        (HeatmapTheme::ColorBlind, BookSide::Bid) => [68, 153, 230],
        (HeatmapTheme::ColorBlind, BookSide::Ask) => [235, 150, 45],
        (_, BookSide::Bid) => [0, 174, 231],
        (_, BookSide::Ask) => [255, 90, 108],
    };
    // Side is a secondary cue. Brightness remains the primary magnitude cue,
    // so strong bid and ask walls share the same warm-white endpoint.
    mix_rgb(base, tint, (1.0 - finite_unit(intensity)) * 0.045)
}

pub fn thermal_rgb(theme: HeatmapTheme, intensity: f32) -> [u8; 3] {
    let stops: &[ColorStop] = match theme {
        HeatmapTheme::Bookmap => &BOOKMAP_RAMP,
        HeatmapTheme::HighContrast => &HIGH_CONTRAST_RAMP,
        HeatmapTheme::ColorBlind => &COLOR_BLIND_RAMP,
    };
    sample_ramp(stops, intensity)
}

fn sample_ramp(stops: &[ColorStop], intensity: f32) -> [u8; 3] {
    let t = finite_unit(intensity);
    let Some(first) = stops.first() else {
        return [0, 0, 0];
    };
    if t <= first.at {
        return first.rgb;
    }
    for pair in stops.windows(2) {
        let from = pair[0];
        let to = pair[1];
        if t <= to.at {
            let span = (to.at - from.at).max(f32::EPSILON);
            return mix_rgb(from.rgb, to.rgb, (t - from.at) / span);
        }
    }
    stops.last().map_or(first.rgb, |stop| stop.rgb)
}

pub fn mix_rgb(from: [u8; 3], to: [u8; 3], amount: f32) -> [u8; 3] {
    let amount = finite_unit(amount);
    [
        (f32::from(from[0]) + (f32::from(to[0]) - f32::from(from[0])) * amount).round() as u8,
        (f32::from(from[1]) + (f32::from(to[1]) - f32::from(from[1])) * amount).round() as u8,
        (f32::from(from[2]) + (f32::from(to[2]) - f32::from(from[2])) * amount).round() as u8,
    ]
}

pub fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_clamp(value: f32, low: f32, high: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        fallback.clamp(low, high)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn luminance(rgb: [u8; 3]) -> f32 {
        0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2])
    }

    #[test]
    fn every_theme_moves_from_dark_to_bright() {
        for theme in [
            HeatmapTheme::Bookmap,
            HeatmapTheme::HighContrast,
            HeatmapTheme::ColorBlind,
        ] {
            let dark = thermal_rgb(theme, 0.0);
            let middle = thermal_rgb(theme, 0.55);
            let bright = thermal_rgb(theme, 1.0);
            assert!(
                luminance(dark) < luminance(middle),
                "{theme:?} dark={dark:?} middle={middle:?}",
            );
            assert!(
                luminance(middle) < luminance(bright),
                "{theme:?} middle={middle:?} bright={bright:?}",
            );
        }
    }

    #[test]
    fn thermal_ramp_clamps_invalid_and_out_of_range_values() {
        assert_eq!(
            thermal_rgb(HeatmapTheme::Bookmap, -10.0),
            BOOKMAP_RAMP[0].rgb
        );
        assert_eq!(
            thermal_rgb(HeatmapTheme::Bookmap, 10.0),
            BOOKMAP_RAMP.last().unwrap().rgb
        );
        assert_eq!(
            thermal_rgb(HeatmapTheme::Bookmap, f32::NAN),
            BOOKMAP_RAMP[0].rgb
        );
    }

    #[test]
    fn bookmap_ramp_spans_black_to_warm_white_through_green() {
        // The refined Bookmap ramp starts at pure black so quiet liquidity
        // fades into the canvas, and ends warm-white for the strongest walls.
        assert_eq!(thermal_rgb(HeatmapTheme::Bookmap, 0.0), [0, 0, 0]);
        let top = thermal_rgb(HeatmapTheme::Bookmap, 1.0);
        assert!(top.iter().all(|&channel| channel > 220), "top={top:?}");
        // It passes through a green phase (restored versus the older ramp, which
        // jumped cyan straight to yellow), so mid magnitudes stay separable.
        let mid_high = thermal_rgb(HeatmapTheme::Bookmap, 0.70);
        assert!(
            mid_high[1] > mid_high[0] && mid_high[1] > mid_high[2],
            "expected a green-dominant phase, got {mid_high:?}",
        );
    }

    #[test]
    fn strong_walls_converge_to_same_brightness_on_both_sides() {
        for theme in [
            HeatmapTheme::Bookmap,
            HeatmapTheme::HighContrast,
            HeatmapTheme::ColorBlind,
        ] {
            assert_eq!(
                resting_rgb(theme, BookSide::Bid, 1.0),
                resting_rgb(theme, BookSide::Ask, 1.0)
            );
        }
    }
}
