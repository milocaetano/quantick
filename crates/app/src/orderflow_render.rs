//! Modular `egui` renderer for the Bookmap-style order-flow projection.
//!
//! This module deliberately owns only pixels. Book synchronization, grouping,
//! aggression clustering and the conservative association between trades and
//! book reductions live in the pure `orderflow` modules. Keeping that boundary
//! lets themes and visual effects evolve without changing market-data facts.

use eframe::egui;
use eframe::egui::epaint::{Vertex, WHITE_UV};
use quantick_orderbook::BookSide;
use quantick_orderflow::{BubbleStyle, HeatmapConfig, HeatmapTheme, LiveLaneStyle};

mod bubbles;
mod heatmap;
mod layout;
mod legend;
mod preview;

pub(crate) use bubbles::draw_aggression_bubbles;
pub(crate) use heatmap::{draw_heatmap_background, draw_liquidity_events, draw_live_lane_marks};
pub(crate) use layout::{ProjectedLayout, RenderContext, lane_divider_x};
pub(crate) use legend::draw_compact_legend;
pub(crate) use preview::draw_preview;

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
pub(crate) struct OrderflowRenderStyle {
    pub(crate) theme: HeatmapTheme,
    /// Additional multiplier over the projection's factual alpha.
    pub(crate) heat_opacity: f32,
    /// Minimum on-screen band height after price projection.
    pub(crate) min_cell_height: f32,
    /// Strength of the soft edge laid behind each heat cell.
    pub(crate) edge_glow: f32,
    /// Every user-owned bubble choice, straight from the settings panel.
    pub(crate) bubbles: BubbleStyle,
    /// The live lane's own choices: how wide the reserved band is, how its
    /// prints cluster, and how much bigger their bubbles read.
    pub(crate) live_lane: LiveLaneStyle,
    pub(crate) show_gap_labels: bool,
    pub(crate) show_legend: bool,
    /// Whether the L2 depth layer is active over the candles. The legend only
    /// advertises keys for layers that can actually draw something.
    pub(crate) depth_layer: bool,
    /// Whether the aggression layer is active over the candles.
    pub(crate) aggression_layer: bool,
    /// Whether the L2 depth layer is active on the tape.
    ///
    /// The two panes are switched apart because they are read apart: the
    /// compressed history answers "where has size been resting", the rolling
    /// tape answers "what is resting there right now". A trader clearing the
    /// candles to read structure is not thereby asking to lose the book where
    /// the book is being watched.
    pub(crate) lane_depth_layer: bool,
    /// Whether the aggression layer is active on the tape. Same reasoning as
    /// [`lane_depth_layer`](Self::lane_depth_layer).
    pub(crate) lane_aggression_layer: bool,
    /// Per-layer display switches, mirroring the config flags. The projection
    /// no longer filters the aggression primitives — several surfaces read
    /// them — so for those two switches this is where the decision is made
    /// ([`RenderContext::bubbles`]); for the rest the renderer's job is
    /// keeping the legend honest about which layers can draw.
    pub(crate) show_liquidity: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub(crate) show_buy: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub(crate) show_sell: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub(crate) show_aligned: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub(crate) show_unattributed: bool,
    /// See [`show_liquidity`](Self::show_liquidity).
    pub(crate) show_gaps: bool,
    pub(crate) legend_max_width: f32,
    /// Vertical space already spoken for at the canvas's top-left corner: the
    /// chart header, plus whatever the pane stacked under it (an indicator
    /// chip per row). The legend starts below it, so the two can never print
    /// over each other — they did, because this used to be a constant that
    /// only knew about the header.
    pub(crate) legend_top_inset: f32,
    /// Follows the chart canvas so the deterministic preview sits on the same
    /// ground as the live chart.
    pub(crate) canvas_background: egui::Color32,
}

/// The chart header's own row at the canvas's top-left corner: the floor
/// every legend inset starts from, whatever the pane measured.
pub(crate) const LEGEND_HEADER_CLEARANCE_PX: f32 = 22.0;

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
            canvas_background: egui::Color32::from_rgb(19, 23, 34),
        }
    }
}

impl OrderflowRenderStyle {
    /// Resolve every renderer choice that has a corresponding user setting.
    ///
    /// The whole bubble vocabulary — alpha, radii, marks, colours — now comes
    /// from the aggression panel in one struct.
    #[must_use]
    pub(crate) fn from_config(config: &HeatmapConfig, canvas_background: egui::Color32) -> Self {
        Self {
            theme: config.theme,
            bubbles: config.bubbles.clone(),
            live_lane: config.live_lane.clone(),
            show_legend: config.show_legend,
            depth_layer: config.depth_visible(),
            aggression_layer: config.show_aggressions,
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
    pub(crate) fn sanitized(&self) -> Self {
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
pub(crate) struct ThemeBubbleRgb {
    pub(crate) buy: [u8; 3],
    pub(crate) sell: [u8; 3],
    pub(crate) front: [u8; 3],
    pub(crate) text: [u8; 3],
}

#[must_use]
pub(crate) fn theme_bubble_rgb(theme: HeatmapTheme) -> ThemeBubbleRgb {
    let palette = Palette::for_theme(theme);
    let rgb = |color: egui::Color32| [color.r(), color.g(), color.b()];
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
struct Palette {
    buy: egui::Color32,
    sell: egui::Color32,
    consumption: egui::Color32,
    depth_only: egui::Color32,
    bubble_text: egui::Color32,
    gap_fill: egui::Color32,
    gap_boundary: egui::Color32,
    /// Boundary between the forming bar's candle and its live lane.
    lane_divider: egui::Color32,
    /// The line market time has walked to inside the lane.
    lane_now: egui::Color32,
    muted_text: egui::Color32,
    legend_text: egui::Color32,
    legend_background: egui::Color32,
    legend_border: egui::Color32,
}

impl Palette {
    fn for_theme(theme: HeatmapTheme) -> Self {
        match theme {
            HeatmapTheme::Bookmap => Self {
                buy: egui::Color32::from_rgb(46, 224, 150),
                sell: egui::Color32::from_rgb(255, 82, 96),
                consumption: egui::Color32::from_rgb(255, 246, 205),
                depth_only: egui::Color32::from_rgb(184, 130, 240),
                bubble_text: egui::Color32::WHITE,
                gap_fill: egui::Color32::from_rgba_premultiplied(21, 24, 32, 20),
                gap_boundary: egui::Color32::from_rgba_premultiplied(157, 167, 188, 115),
                lane_divider: egui::Color32::from_rgba_premultiplied(120, 132, 156, 90),
                lane_now: egui::Color32::from_rgba_premultiplied(226, 234, 250, 150),
                muted_text: egui::Color32::from_rgb(186, 194, 209),
                legend_text: egui::Color32::from_rgb(225, 230, 239),
                legend_background: egui::Color32::from_rgba_premultiplied(8, 12, 23, 225),
                legend_border: egui::Color32::from_rgba_premultiplied(130, 145, 170, 90),
            },
            HeatmapTheme::HighContrast => Self {
                buy: egui::Color32::from_rgb(0, 255, 138),
                sell: egui::Color32::from_rgb(255, 45, 70),
                consumption: egui::Color32::WHITE,
                depth_only: egui::Color32::from_rgb(225, 105, 255),
                bubble_text: egui::Color32::WHITE,
                gap_fill: egui::Color32::from_rgba_premultiplied(35, 35, 40, 28),
                gap_boundary: egui::Color32::from_rgb(218, 222, 235),
                lane_divider: egui::Color32::from_gray(160),
                lane_now: egui::Color32::WHITE,
                muted_text: egui::Color32::WHITE,
                legend_text: egui::Color32::WHITE,
                legend_background: egui::Color32::from_rgba_premultiplied(0, 0, 0, 238),
                legend_border: egui::Color32::from_gray(175),
            },
            HeatmapTheme::ColorBlind => Self {
                buy: egui::Color32::from_rgb(64, 160, 255),
                sell: egui::Color32::from_rgb(255, 159, 28),
                consumption: egui::Color32::from_rgb(255, 238, 170),
                depth_only: egui::Color32::from_rgb(220, 95, 205),
                bubble_text: egui::Color32::WHITE,
                gap_fill: egui::Color32::from_rgba_premultiplied(25, 25, 30, 22),
                gap_boundary: egui::Color32::from_rgb(176, 180, 190),
                lane_divider: egui::Color32::from_rgba_premultiplied(140, 143, 152, 95),
                lane_now: egui::Color32::from_rgba_premultiplied(232, 232, 226, 155),
                muted_text: egui::Color32::from_rgb(214, 215, 210),
                legend_text: egui::Color32::from_rgb(232, 232, 226),
                legend_background: egui::Color32::from_rgba_premultiplied(10, 11, 25, 230),
                legend_border: egui::Color32::from_rgba_premultiplied(165, 166, 180, 100),
            },
        }
    }
}

fn resting_rgb(theme: HeatmapTheme, side: BookSide, intensity: f32) -> [u8; 3] {
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

fn thermal_rgb(theme: HeatmapTheme, intensity: f32) -> [u8; 3] {
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

fn add_gradient_rect(
    mesh: &mut egui::Mesh,
    rect: egui::Rect,
    left: egui::Color32,
    right: egui::Color32,
) {
    if !rect.is_positive() {
        return;
    }
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend_from_slice(&[
        Vertex {
            pos: rect.left_top(),
            uv: WHITE_UV,
            color: left,
        },
        Vertex {
            pos: rect.right_top(),
            uv: WHITE_UV,
            color: right,
        },
        Vertex {
            pos: rect.right_bottom(),
            uv: WHITE_UV,
            color: right,
        },
        Vertex {
            pos: rect.left_bottom(),
            uv: WHITE_UV,
            color: left,
        },
    ]);
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[allow(clippy::too_many_arguments)]
fn draw_dashed_vertical(
    painter: &egui::Painter,
    x: f32,
    rect: egui::Rect,
    dash: f32,
    gap: f32,
    color: egui::Color32,
    width: f32,
) {
    let dash = dash.max(0.5);
    let gap = gap.max(0.0);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + dash).min(rect.bottom())),
            ],
            egui::Stroke::new(width.max(0.5), color),
        );
        y += dash + gap;
    }
}

fn rgba(rgb: [u8; 3], alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        rgb[0],
        rgb[1],
        rgb[2],
        (finite_unit(alpha) * 255.0).round() as u8,
    )
}

fn mix_rgb(from: [u8; 3], to: [u8; 3], amount: f32) -> [u8; 3] {
    let amount = finite_unit(amount);
    [
        (f32::from(from[0]) + (f32::from(to[0]) - f32::from(from[0])) * amount).round() as u8,
        (f32::from(from[1]) + (f32::from(to[1]) - f32::from(from[1])) * amount).round() as u8,
        (f32::from(from[2]) + (f32::from(to[2]) - f32::from(from[2])) * amount).round() as u8,
    ]
}

fn finite_unit(value: f32) -> f32 {
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
mod tests;
