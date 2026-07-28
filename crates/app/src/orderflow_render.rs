//! Modular `egui` renderer for the Bookmap-style order-flow projection.
//!
//! This module deliberately owns only pixels. Book synchronization, grouping,
//! aggression clustering and the conservative association between trades and
//! book reductions live in the pure `orderflow` modules. Keeping that boundary
//! lets themes and visual effects evolve without changing market-data facts.

use eframe::egui;
use eframe::egui::epaint::{Vertex, WHITE_UV};
use quantick_engine::Side;
use quantick_orderbook::BookSide;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use crate::orderflow::{
    AggressionPrimitive, BEFORE_CAPTURE, BubbleRenderMode, BubbleStyle, HeatmapConfig,
    HeatmapProjection, HeatmapTheme, LiquidityEvidence, LiveLaneStyle,
};
use crate::viewport::Viewport;

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
    /// Whether the L2 depth layer is active. The legend only advertises keys
    /// for layers that can actually draw something.
    pub(crate) depth_layer: bool,
    /// Whether the aggression layer is active.
    pub(crate) aggression_layer: bool,
    /// Per-layer display switches, mirroring the config flags: the projection
    /// already filters the primitives, so the renderer's only job is keeping
    /// the legend honest about which layers can draw.
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
    /// Follows the chart canvas so the deterministic preview sits on the same
    /// ground as the live chart.
    pub(crate) canvas_background: egui::Color32,
}

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
            show_liquidity: true,
            show_buy: true,
            show_sell: true,
            show_aligned: true,
            show_unattributed: true,
            show_gaps: true,
            legend_max_width: 690.0,
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
        style
    }
}

/// Bubble colours after the panel's overrides are laid over the theme.
///
/// Resolved per draw call and never written back into [`Palette`]: the same
/// theme colours keep driving the liquidity-response layer, which the bubble
/// panel does not own.
#[derive(Debug, Clone, Copy)]
struct BubbleColors {
    buy: egui::Color32,
    sell: egui::Color32,
    front: egui::Color32,
    trail: egui::Color32,
    text: egui::Color32,
}

impl BubbleColors {
    fn resolve(palette: &Palette, bubbles: &BubbleStyle) -> Self {
        let front = bubbles.front_color.map_or(palette.consumption, opaque_rgb);
        Self {
            buy: bubbles.buy_color.map_or(palette.buy, opaque_rgb),
            sell: bubbles.sell_color.map_or(palette.sell, opaque_rgb),
            front,
            // The trail is the front's own glow, so it follows it by default.
            trail: bubbles.trail_color.map_or(front, opaque_rgb),
            text: bubbles.label_color.map_or(palette.bubble_text, opaque_rgb),
        }
    }

    const fn for_side(self, side: Side) -> egui::Color32 {
        match side {
            Side::Buy => self.buy,
            Side::Sell => self.sell,
        }
    }
}

fn opaque_rgb(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
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

/// Vertical nudge, in pixels, that keeps the two sides off the same row.
///
/// Buy aggression lifts the ask, sell aggression hits the bid, so buys sit
/// slightly above the print and sells slightly below. Screen y grows downward.
const fn side_offset_y(side: Side, offset: f32) -> f32 {
    match side {
        Side::Buy => -offset,
        Side::Sell => offset,
    }
}

/// Interior alpha of a hollow bubble, as a fraction of the configured fill
/// alpha: enough tint to keep the disc's area readable, light enough that the
/// ring is what the eye catches.
const HOLLOW_FILL_ALPHA: f32 = 0.22;
/// Ring thickness of a hollow bubble as a fraction of its radius, and the
/// pixel range it is held to — thin enough to stay a ring on a full-size
/// sweep, thick enough to survive at dot size.
const HOLLOW_RING_SCALE: f32 = 0.42;
/// See [`HOLLOW_RING_SCALE`].
const HOLLOW_MIN_RING_PX: f32 = 1.2;
/// See [`HOLLOW_RING_SCALE`].
const HOLLOW_MAX_RING_PX: f32 = 3.0;

/// Ring thickness of a hollow bubble of this radius.
fn hollow_ring_width(radius: f32) -> f32 {
    (radius * HOLLOW_RING_SCALE).clamp(HOLLOW_MIN_RING_PX, HOLLOW_MAX_RING_PX)
}

/// Width of the dark separator ring drawn just outside a bubble's rim. The
/// heat ramp passes through greens the buy side almost matches, so without a
/// dark hair between them "aggression" and "liquidity" melt into one layer
/// wherever a bubble sits on warm heat — the ring is what keeps them two.
const SEPARATOR_RING_PX: f32 = 1.2;
/// Alpha of that ring: translucent black, so it darkens whatever heat is
/// behind it instead of assuming one canvas colour.
const SEPARATOR_RING_ALPHA: u8 = 200;

/// Gap, in pixels, between a bubble's rim and the halo drawn behind it.
const HALO_PADDING_PX: f32 = 2.5;
/// Gap, in pixels, between a bubble's rim and its impact ring.
const IMPACT_RING_PADDING_PX: f32 = 2.2;
/// Pixels added beyond `front_length_scale × radius`, so the consumption mark
/// on even the smallest bubble is long enough to read as a mark.
const FRONT_END_PADDING_PX: f32 = 6.0;
/// How far the halo opens up at full print size, as a fraction of
/// `halo_strength`: a sweep reads heavier than a routine print of the same
/// colour, without needing a second colour for it.
const HALO_SIZE_BOOST: f32 = 0.5;
/// Rim alpha relative to the fill. A hair below opaque keeps the rim reading
/// as the bubble's edge rather than as a separate ring on a dark canvas.
const RIM_ALPHA: f32 = 0.96;
/// Impact-ring alpha every consuming print gets, before the matched share.
const IMPACT_RING_BASE_ALPHA: f32 = 0.75;
/// Share of the impact ring's alpha that tracks how much of the print actually
/// matched resting liquidity, so a full sweep rings brighter than a nibble.
const IMPACT_RING_MATCH_ALPHA: f32 = 0.25;
/// Matched-fraction floor for the consumption marks: a barely matched print
/// still ate something, so it still leaves a visible mark.
const MIN_MATCH_STRENGTH: f32 = 0.25;

/// Fraction of the radius the sphere's lit core is offset toward the upper
/// left. One fixed light direction keeps every bubble shaded identically, so
/// the eye reads the gradient as volume instead of as data.
const SPHERE_LIGHT_OFFSET: f32 = 0.35;
/// Radius of the sphere's full-brightness core ring, as a fraction of the
/// bubble radius. Vertex colours interpolate highlight → side colour inside
/// it and side colour → darkened rim outside it; that gradient is the whole
/// shading model.
const SPHERE_CORE_RADIUS: f32 = 0.62;
/// Ring segments per pixel of radius on a sphere-shaded bubble, bounded by
/// [`SPHERE_MIN_SEGMENTS`] and [`SPHERE_MAX_SEGMENTS`]: a small dressed
/// bubble stays cheap, a full-size sweep stays round.
const SPHERE_SEGMENTS_PER_RADIUS_PX: f32 = 2.0;
/// See [`SPHERE_SEGMENTS_PER_RADIUS_PX`].
const SPHERE_MIN_SEGMENTS: usize = 12;
/// See [`SPHERE_SEGMENTS_PER_RADIUS_PX`].
const SPHERE_MAX_SEGMENTS: usize = 32;

/// Tessellation of a sphere-shaded bubble of this radius.
fn sphere_segments(radius: f32) -> usize {
    ((radius * SPHERE_SEGMENTS_PER_RADIUS_PX) as usize)
        .clamp(SPHERE_MIN_SEGMENTS, SPHERE_MAX_SEGMENTS)
}

/// Label font size as a fraction of the bubble radius, and the range it is
/// held to: too small to read is pointless, too large stops fitting inside.
const LABEL_FONT_SCALE: f32 = 0.68;
/// See [`LABEL_FONT_SCALE`].
const LABEL_MIN_FONT_PX: f32 = 8.0;
/// See [`LABEL_FONT_SCALE`].
const LABEL_MAX_FONT_PX: f32 = 11.0;
/// How far a laid-out label may spill past the radius before it is dropped.
/// Wider than tall: a bubble is a circle, and text is a horizontal band across
/// its middle, where there is more room.
const LABEL_MAX_WIDTH_SCALE: f32 = 1.78;
/// See [`LABEL_MAX_WIDTH_SCALE`].
const LABEL_MAX_HEIGHT_SCALE: f32 = 1.45;
/// Drop shadow that keeps a label legible over any fill colour.
const LABEL_SHADOW_OFFSET_PX: egui::Vec2 = egui::vec2(1.0, 1.0);
/// See [`LABEL_SHADOW_OFFSET_PX`].
const LABEL_SHADOW_ALPHA: u8 = 190;

/// Normalized sizes of the two sample prints in the settings preview: one
/// near full size and one routine print, so the radius range is visible.
const PREVIEW_LARGE_PRINT_SIZE: f32 = 0.85;
/// See [`PREVIEW_LARGE_PRINT_SIZE`].
const PREVIEW_SMALL_PRINT_SIZE: f32 = 0.45;
/// Matched fraction of the preview's consuming print. Mid-range, so the
/// impact ring shows neither its floor nor its ceiling.
const PREVIEW_MATCHED_FRACTION: f32 = 0.6;
/// Buy share of the preview's summarized print. Lopsided rather than even, so
/// the two sectors are visibly unequal and the mark reads as a proportion.
const PREVIEW_SUMMARY_BUY_SHARE: f32 = 0.62;

/// Half-length, in pixels, of the vertical consumption front on a bubble of
/// this radius.
fn front_half_length(radius: f32, bubbles: &BubbleStyle) -> f32 {
    radius * bubbles.front_length_scale + FRONT_END_PADDING_PX
}

/// Halo alpha for a print of this normalized size.
fn halo_alpha(size: f32, bubbles: &BubbleStyle) -> f32 {
    (bubbles.halo_strength * (1.0 + HALO_SIZE_BOOST * finite_unit(size))).min(1.0)
}

/// Impact-ring alpha for a print that matched this fraction of resting
/// liquidity.
fn impact_ring_alpha(matched_fraction: f32) -> f32 {
    IMPACT_RING_BASE_ALPHA
        + finite_unit(matched_fraction).max(MIN_MATCH_STRENGTH) * IMPACT_RING_MATCH_ALPHA
}

/// The consumption trail leaking to the right of a bubble, stopped at
/// `right_edge` so it never paints past the chart.
fn trail_rect(
    center: egui::Pos2,
    half_length: f32,
    trail_length: f32,
    right_edge: f32,
) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(center.x, center.y - half_length),
        egui::pos2(
            (center.x + trail_length).min(right_edge),
            center.y + half_length,
        ),
    )
}

/// The side colour pushed toward white for a sphere's lit core.
fn sphere_core_color(color: egui::Color32, highlight: f32) -> egui::Color32 {
    opaque_rgb(mix_rgb(
        [color.r(), color.g(), color.b()],
        [255, 255, 255],
        highlight,
    ))
}

/// The side colour pushed toward black for a sphere's rim.
fn sphere_edge_color(color: egui::Color32, shading: f32) -> egui::Color32 {
    opaque_rgb(mix_rgb(
        [color.r(), color.g(), color.b()],
        [0, 0, 0],
        shading,
    ))
}

/// Angle a pie starts at: straight up. Screen y grows downward, so a positive
/// sweep from here runs clockwise, the direction a pie chart is read in.
const PIE_START_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;

/// The three colours a shaded bubble interpolates between: lit core, side
/// colour, darkened rim.
#[derive(Debug, Clone, Copy)]
struct SphereShading {
    core: egui::Color32,
    body: egui::Color32,
    edge: egui::Color32,
}

impl SphereShading {
    /// One colour used three times, which flattens the gradient. This is how
    /// the flat render mode draws its pie without a second tessellator.
    const fn flat(color: egui::Color32) -> Self {
        Self {
            core: color,
            body: color,
            edge: color,
        }
    }

    /// A side colour lit from the upper left.
    fn sphere(color: egui::Color32, bubbles: &BubbleStyle) -> Self {
        Self {
            core: sphere_core_color(color, bubbles.sphere_highlight),
            body: color,
            edge: sphere_edge_color(color, bubbles.sphere_shading),
        }
    }

    /// The same shading behind the bubble's configured fill alpha.
    fn faded(self, opacity: f32) -> Self {
        Self {
            core: self.core.gamma_multiply(opacity),
            body: self.body.gamma_multiply(opacity),
            edge: self.edge.gamma_multiply(opacity),
        }
    }
}

/// Append one shaded circular sector to `mesh`: a triangle fan from the offset
/// lit core through a full-brightness ring to the darkened rim, limited to
/// `sweep` radians from `start_angle`. Vertex colours do all the shading, so
/// the 3D look costs one small mesh — no texture, no per-pixel work — and
/// overlapping bubbles keep a visible boundary because each rim is darker than
/// its neighbour's body.
///
/// A whole bubble is one sector sweeping `TAU`; a two-sided bubble is two
/// sectors sharing a centre, each shaded in its own side's colour.
fn add_shaded_sector(
    mesh: &mut egui::Mesh,
    center: egui::Pos2,
    radius: f32,
    start_angle: f32,
    sweep: f32,
    shading: SphereShading,
) {
    let SphereShading { core, body, edge } = shading;
    if !radius.is_finite() || radius <= 0.0 || !center.is_finite() || !sweep.is_finite() {
        return;
    }
    let sweep = sweep.clamp(0.0, std::f32::consts::TAU);
    if sweep <= 0.0 {
        return;
    }
    // Segments are budgeted for a whole circle, so a narrow sector stays
    // cheap without ever falling below the two edges that make it a wedge.
    let full = sphere_segments(radius);
    let segments = (((full as f32) * (sweep / std::f32::consts::TAU)).ceil() as usize).max(2);
    let offset = egui::vec2(-radius, -radius) * SPHERE_LIGHT_OFFSET;
    // The core ring keeps a scaled-down share of the highlight offset, which
    // holds the whole lit zone inside the rim at any radius.
    let core_center = center + offset * (1.0 - SPHERE_CORE_RADIUS);
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(center + offset, core);
    // One more vertex than segments: the arc has two ends and, unlike a full
    // circle, must not wrap the last back onto the first.
    for (ring_center, ring_radius, color) in [
        (core_center, radius * SPHERE_CORE_RADIUS, body),
        (center, radius, edge),
    ] {
        for index in 0..=segments {
            let angle = start_angle + sweep * (index as f32 / segments as f32);
            let direction = egui::vec2(angle.cos(), angle.sin());
            mesh.colored_vertex(ring_center + direction * ring_radius, color);
        }
    }
    let count = segments as u32;
    let core_ring = base + 1;
    let rim_ring = core_ring + count + 1;
    for index in 0..count {
        let next = index + 1;
        mesh.indices
            .extend_from_slice(&[base, core_ring + index, core_ring + next]);
        mesh.indices
            .extend_from_slice(&[core_ring + index, rim_ring + index, rim_ring + next]);
        mesh.indices
            .extend_from_slice(&[core_ring + index, rim_ring + next, core_ring + next]);
    }
}

/// One aggression bubble, already placed in screen space.
#[derive(Debug, Clone, Copy)]
struct BubbleMark {
    center: egui::Pos2,
    radius: f32,
    side: Side,
    /// Normalized print size, which opens up the halo.
    size: f32,
    /// Fraction of the print matched against resting liquidity, when it ate
    /// any. `None` draws no consumption marks.
    matched: Option<f32>,
    /// `[0,1]` share of the quantity buyers took. Anything strictly between
    /// the ends is a bubble carrying both sides, drawn as a pie.
    buy_share: f32,
}

/// Draw one bubble: halo, fill, rim and — when the print ate resting
/// liquidity — the vertical consumption front and the impact ring.
///
/// The live chart and the settings preview both draw through here, so the
/// preview cannot drift into showing something the chart does not draw. The
/// trail is the one mark left out: on the chart every trail is batched into a
/// single mesh behind all the bubbles, which is a draw-order decision rather
/// than a per-bubble one.
fn draw_bubble(
    painter: &egui::Painter,
    mark: BubbleMark,
    bubbles: &BubbleStyle,
    colors: &BubbleColors,
) {
    let BubbleMark {
        center,
        radius,
        side,
        size,
        matched,
        buy_share,
    } = mark;
    let color = colors.for_side(side);

    // Small prints are the common case on a busy tape: one cheap dot each.
    // The full dressing (halo, rim, impact ring — and sphere shading, which
    // is unreadable at dot size anyway) is reserved for bubbles big enough to
    // read it, which also keeps the per-frame tessellation budget flat no
    // matter how fast the tape runs.
    let dressed = radius >= bubbles.detail_min_radius;
    // A bubble carrying both sides shows their proportion as pie sectors, but
    // only where a proportion can actually be read. Two floors, both needed:
    // `readable_min_radius` is the dedicated "too small to read" threshold, and
    // `dressed` keeps the cheap one-circle path intact on a dense tape, which
    // some presets deliberately extend by setting the dressing radius below the
    // minimum. Under either floor the mark is the dot it has always been, in
    // the dominant side's colour.
    let buy_share = finite_unit(buy_share);
    let mixed =
        dressed && radius >= bubbles.readable_min_radius && buy_share > 0.0 && buy_share < 1.0;
    // Shape carries the side exactly where colour stops doing it: below the
    // readability floor a green speck and a red speck are the same speck, and
    // an open ring is not. Gated on that floor rather than on `dressed`,
    // because a sphere-heavy look sets the dressing radius low on purpose and
    // would otherwise leave every bubble solid. A pie is above that same floor
    // by construction, so the two never contend for one bubble.
    let hollow = bubbles.hollow_small_buys
        && matches!(side, Side::Buy)
        && radius < bubbles.readable_min_radius;
    let sphere = !hollow && dressed && bubbles.render_mode == BubbleRenderMode::Sphere;
    if !hollow && dressed && bubbles.halo_strength > 0.0 {
        painter.circle_filled(
            center,
            radius + HALO_PADDING_PX,
            color.gamma_multiply(halo_alpha(size, bubbles)),
        );
    }
    // The dark hair between the mark and the heat behind it. Skipped on the
    // cheap-dot path, which stays a single circle per print.
    if dressed || hollow {
        painter.circle_stroke(
            center,
            radius + SEPARATOR_RING_PX / 2.0,
            egui::Stroke::new(
                SEPARATOR_RING_PX,
                egui::Color32::from_black_alpha(SEPARATOR_RING_ALPHA),
            ),
        );
    }
    if hollow {
        let ring = hollow_ring_width(radius);
        painter.circle_filled(
            center,
            radius,
            color.gamma_multiply(bubbles.opacity * HOLLOW_FILL_ALPHA),
        );
        // Stroked on the inside of the radius, so a hollow bubble occupies
        // exactly the area its quantity earned.
        painter.circle_stroke(
            center,
            (radius - ring / 2.0).max(0.5),
            egui::Stroke::new(ring, color.gamma_multiply(bubbles.opacity)),
        );
    } else if mixed || sphere {
        // Shading is what tells the two modes apart: a sphere reads its core,
        // body and rim colours, a flat bubble uses one colour three times.
        let shaded = |color: egui::Color32| {
            if sphere {
                SphereShading::sphere(color, bubbles)
            } else {
                SphereShading::flat(color)
            }
            .faded(bubbles.opacity)
        };
        // Two sectors for a pie, one whole disc for everything else. A zero
        // sweep draws nothing, so the second entry costs nothing when unused.
        let sectors = if mixed {
            [
                (buy_share * std::f32::consts::TAU, colors.buy),
                ((1.0 - buy_share) * std::f32::consts::TAU, colors.sell),
            ]
        } else {
            [(std::f32::consts::TAU, color), (0.0, color)]
        };
        let mut mesh = egui::Mesh::default();
        let mut angle = PIE_START_ANGLE;
        for (sweep, side_color) in sectors {
            add_shaded_sector(&mut mesh, center, radius, angle, sweep, shaded(side_color));
            angle += sweep;
        }
        painter.add(egui::Shape::mesh(mesh));
    } else {
        painter.circle_filled(center, radius, color.gamma_multiply(bubbles.opacity));
    }
    if !hollow && dressed && bubbles.outline_width > 0.0 {
        // A sphere's rim adopts the darkened edge colour: the dark separator
        // is what keeps two overlapping same-side bubbles readable as two.
        let rim = if sphere {
            sphere_edge_color(color, bubbles.sphere_shading)
        } else {
            color
        };
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(bubbles.outline_width, rim.gamma_multiply(RIM_ALPHA)),
        );
    }

    // This print ate resting liquidity at this exact price.
    let Some(matched_fraction) = matched else {
        return;
    };
    if bubbles.show_consumption_front {
        let half_length = front_half_length(radius, bubbles);
        painter.line_segment(
            [
                egui::pos2(center.x, center.y - half_length),
                egui::pos2(center.x, center.y + half_length),
            ],
            egui::Stroke::new(bubbles.front_width, colors.front),
        );
    }
    if dressed && bubbles.show_impact_ring {
        painter.circle_stroke(
            center,
            radius + IMPACT_RING_PADDING_PX,
            egui::Stroke::new(
                bubbles.impact_ring_width,
                colors
                    .front
                    .gamma_multiply(impact_ring_alpha(matched_fraction)),
            ),
        );
    }
}

/// Mapping between normalized projection coordinates and the chart viewport.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProjectedLayout<'a> {
    pub(crate) chart_rect: egui::Rect,
    pub(crate) viewport: &'a Viewport,
    pub(crate) total_bars: usize,
    pub(crate) first_bar_index: usize,
    pub(crate) slot_count: usize,
    /// Visual width, in candle-widths, of the last slot (the forming bar). `1.0`
    /// keeps equal-width slots; `> 1.0` expands the forming bar's live tail to
    /// the right so its order flow rolls on a real-time scale instead of
    /// recompressing every depth update.
    pub(crate) live_span: f32,
}

impl<'a> ProjectedLayout<'a> {
    #[must_use]
    pub(crate) fn new(
        chart_rect: egui::Rect,
        viewport: &'a Viewport,
        total_bars: usize,
        first_bar_index: usize,
        slot_count: usize,
        live_span: f32,
    ) -> Self {
        Self {
            chart_rect,
            viewport,
            total_bars,
            first_bar_index,
            slot_count,
            live_span: if live_span.is_finite() {
                live_span.max(1.0)
            } else {
                1.0
            },
        }
    }

    #[must_use]
    fn x(self, normalized: f64) -> f32 {
        let normalized = finite_unit_f64(normalized) as f32;
        let slot_count = self.slot_count as f32;
        // Widen only the last slot (the forming bar) by `live_span`; earlier
        // closed slots keep unit width. `slot_pos` is the position in slot units
        // over `[0, slot_count]`; `ext_pos` re-expresses it in bar-width units.
        let slot_pos = normalized * slot_count;
        let ext_pos = if self.live_span <= 1.0 || slot_count < 1.0 {
            slot_pos
        } else {
            let boundary = slot_count - 1.0; // start of the forming slot
            if slot_pos <= boundary {
                slot_pos
            } else {
                // The forming bar's candle owns a clean 1-wide slot as a divider
                // (no heat behind it); the live order flow occupies the
                // `live_span - 1` candle-widths to its right.
                boundary + 1.0 + (slot_pos - boundary) * (self.live_span - 1.0)
            }
        };
        self.x_at_ext(ext_pos)
    }

    /// Screen x of a position measured in candle widths from the first slot.
    #[must_use]
    fn x_at_ext(self, ext_pos: f32) -> f32 {
        let position = self.first_bar_index as f32 - 0.5 + ext_pos;
        self.viewport
            .x_at_bar_position(position, self.chart_rect.right(), self.total_bars)
    }

    /// Screen x where the live lane opens: the right edge of the forming bar's
    /// own candle slot, which no timestamp maps into.
    ///
    /// `None` when this frame reserves no lane.
    #[must_use]
    fn lane_left_x(self) -> Option<f32> {
        (self.live_span > 1.0 && self.slot_count >= 1)
            .then(|| self.x_at_ext(self.slot_count as f32))
    }

    #[must_use]
    fn y(self, normalized: f64) -> f32 {
        self.chart_rect.top() + finite_unit_f64(normalized) as f32 * self.chart_rect.height()
    }

    #[must_use]
    fn band(self, x0: f64, x1: f64, y0: f64, y1: f64, min_height: f32) -> egui::Rect {
        let left = self.x(x0);
        let right = self.x(x1);
        let top = self.y(y0);
        let bottom = self.y(y1);
        readable_band(
            egui::Rect::from_min_max(
                egui::pos2(left.min(right), top.min(bottom)),
                egui::pos2(left.max(right), top.max(bottom)),
            ),
            min_height,
            self.chart_rect,
        )
    }

    #[must_use]
    fn event_band(self, x: f64, y0: f64, y1: f64, min_height: f32) -> EventBand {
        let top = self.y(y0);
        let bottom = self.y(y1);
        let row = readable_band(
            egui::Rect::from_min_max(
                egui::pos2(self.x(x), top.min(bottom)),
                egui::pos2(self.x(x), top.max(bottom)),
            ),
            min_height,
            self.chart_rect,
        );
        EventBand {
            x: self.x(x),
            top: row.top(),
            bottom: row.bottom(),
        }
    }
}

/// Complete input shared by the independently callable rendering layers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RenderContext<'a> {
    pub(crate) projection: &'a HeatmapProjection,
    pub(crate) layout: ProjectedLayout<'a>,
    pub(crate) style: &'a OrderflowRenderStyle,
}

impl<'a> RenderContext<'a> {
    #[must_use]
    pub(crate) fn new(
        projection: &'a HeatmapProjection,
        layout: ProjectedLayout<'a>,
        style: &'a OrderflowRenderStyle,
    ) -> Self {
        Self {
            projection,
            layout,
            style,
        }
    }
}

/// Draw resting liquidity and explicit L2 coverage gaps behind the chart.
pub(crate) fn draw_heatmap_background(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    let palette = Palette::for_theme(style.theme);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let mut mesh = egui::Mesh::default();
    mesh.vertices
        .reserve(context.projection.cells.len().saturating_mul(8));
    mesh.indices
        .reserve(context.projection.cells.len().saturating_mul(12));

    for cell in &context.projection.cells {
        let rect = context
            .layout
            .band(cell.x0, cell.x1, cell.y0, cell.y1, style.min_cell_height);
        if !rect.is_positive() {
            continue;
        }

        let Some((rgb, alpha)) = heat_fill_parts(&style, cell.side, cell.intensity, cell.alpha)
        else {
            continue;
        };
        let fill = rgba(rgb, alpha);

        if style.edge_glow > 0.0 {
            let spread = 0.55 + quantize_heat(finite_unit(cell.intensity)) * 0.85;
            let glow_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() - spread),
                egui::pos2(rect.right(), rect.bottom() + spread),
            )
            .intersect(context.layout.chart_rect);
            let glow = rgba(rgb, alpha * style.edge_glow);
            add_gradient_rect(&mut mesh, glow_rect, glow, glow);
        }
        // Solid fill (no horizontal gradient), so a short run is a clean block
        // rather than a bright-headed streak.
        add_gradient_rect(&mut mesh, rect, fill, fill);
    }

    if !mesh.is_empty() {
        clip.add(egui::Shape::mesh(mesh));
    }

    for gap in &context.projection.gaps {
        let x0 = context.layout.x(gap.x0);
        let x1 = context.layout.x(gap.x1);
        let rect = egui::Rect::from_min_max(
            egui::pos2(x0.min(x1), context.layout.chart_rect.top()),
            egui::pos2(x0.max(x1), context.layout.chart_rect.bottom()),
        )
        .intersect(context.layout.chart_rect);
        if !rect.is_positive() {
            continue;
        }
        let leading = gap.precedes_capture();
        let marks = gap_marks(rect, context.layout.chart_rect, leading);
        if marks.fill {
            clip.rect_filled(rect, egui::Rounding::ZERO, palette.gap_fill);
        }
        for (draw, x) in [
            (marks.left_boundary, rect.left()),
            (marks.right_boundary, rect.right()),
        ] {
            if draw {
                draw_dashed_vertical(&clip, x, rect, 4.0, 5.0, palette.gap_boundary, 1.0);
            }
        }

        if style.show_gap_labels && rect.width() >= 112.0 {
            let label = gap_label(&gap.reason);
            // Centering the leading label would park text in the middle of an
            // otherwise clean chart; it belongs to the divider, so it hugs it.
            let (anchor, align) = if leading {
                (
                    rect.right_top() + egui::vec2(-GAP_LABEL_INSET_PX, 17.0),
                    egui::Align2::RIGHT_TOP,
                )
            } else {
                (
                    rect.center_top() + egui::vec2(0.0, 17.0),
                    egui::Align2::CENTER_TOP,
                )
            };
            draw_text_with_shadow(
                &clip,
                anchor,
                align,
                label,
                egui::FontId::proportional(10.0),
                palette.muted_text,
            );
        }
    }
}

/// Dash and gap, in pixels, of the line dividing the forming bar's candle from
/// its live lane. Fine and airy: it marks where the present begins, and a solid
/// rule there would read as a wall in the data.
const LANE_DIVIDER_DASH_PX: f32 = 3.0;
/// See [`LANE_DIVIDER_DASH_PX`].
const LANE_DIVIDER_GAP_PX: f32 = 5.0;
/// Dash and gap of the live-time line. Tighter than the divider's, so the two
/// never read as the same mark even where they nearly touch.
const LANE_NOW_DASH_PX: f32 = 6.0;
/// See [`LANE_NOW_DASH_PX`].
const LANE_NOW_GAP_PX: f32 = 3.0;
/// Stroke width shared by both lane marks.
const LANE_MARK_WIDTH_PX: f32 = 1.0;

/// Draw the live lane's two marks: the boundary it opens at, and the line
/// market time has walked to inside it.
///
/// Together they are what makes the reserved band readable. The boundary says
/// the forming candle ends here and the present begins; the live-time line says
/// how far into the present the tape has come. Space to the right of that line
/// is time the lane is holding open — not liquidity that disappeared — and
/// without the line an empty band would be indistinguishable from a dead feed.
pub(crate) fn draw_live_lane_marks(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    if !style.live_lane.show_marks {
        return;
    }
    // Both marks belong to the lane; without a live edge there is no lane.
    let Some(now_x) = context.projection.live_now_x else {
        return;
    };
    let Some(divider_x) = context.layout.lane_left_x() else {
        return;
    };
    let rect = context.layout.chart_rect;
    let palette = Palette::for_theme(style.theme);
    let clip = painter.with_clip_rect(rect);
    for (x, dash, gap, color) in [
        (
            divider_x,
            LANE_DIVIDER_DASH_PX,
            LANE_DIVIDER_GAP_PX,
            palette.lane_divider,
        ),
        (
            context.layout.x(now_x),
            LANE_NOW_DASH_PX,
            LANE_NOW_GAP_PX,
            palette.lane_now,
        ),
    ] {
        if x.is_finite() && rect.x_range().contains(x) {
            draw_dashed_vertical(&clip, x, rect, dash, gap, color, LANE_MARK_WIDTH_PX);
        }
    }
}

/// Draw reductions after heat cells and before/around the candle layer.
///
/// `AggressionAligned` draws a bright *consumption front* — the instant an
/// aggression met a resting wall — with a short glow leaking into the consumed
/// (later) side, where the heat cells have already darkened. `DepthOnly` draws
/// a calm violet fade, intentionally avoiding the word "cancel": depth alone
/// does not reveal why displayed liquidity decreased.
pub(crate) fn draw_liquidity_events(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    let palette = Palette::for_theme(style.theme);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let mut hole_mesh = egui::Mesh::default();
    hole_mesh
        .vertices
        .reserve(context.projection.liquidity_events.len().saturating_mul(4));
    hole_mesh
        .indices
        .reserve(context.projection.liquidity_events.len().saturating_mul(6));

    let mut fronts = Vec::with_capacity(context.projection.liquidity_events.len());
    let right_edge = context.layout.chart_rect.right();

    for event in &context.projection.liquidity_events {
        let band = context
            .layout
            .event_band(event.x, event.y0, event.y1, style.min_cell_height);
        if band.x < context.layout.chart_rect.left() - 1.0 || band.x > right_edge + 1.0 {
            continue;
        }

        let reduction = finite_unit(event.fraction);
        let full = event.full_removal;
        let front = marker_band(band, reduction, full);

        // A dark hole across the band's full height marks where liquidity
        // dropped. On a busy book the level is re-stacked almost immediately;
        // without the hole the fresh wall abuts the old one and looks
        // continuous, hiding that it was consumed. The marker colour drawn on
        // the hole's left edge tells aggression-aligned from unattributed apart.
        let hole_w = if full { 14.0 } else { 6.0 + 8.0 * reduction };
        add_gradient_rect(
            &mut hole_mesh,
            egui::Rect::from_min_max(
                egui::pos2(band.x, band.top),
                egui::pos2((band.x + hole_w).min(right_edge), band.bottom),
            ),
            style.canvas_background,
            style.canvas_background,
        );

        match event.evidence {
            LiquidityEvidence::AggressionAligned => fronts.push(EventFront::Aligned {
                band: front,
                matched: finite_unit(event.matched_fraction),
                full,
            }),
            LiquidityEvidence::DepthOnly => fronts.push(EventFront::DepthOnly {
                band: front,
                reduction,
                full,
            }),
        }
    }

    // Carve a gap around each consumption bubble so a re-stacked wall does not
    // slide through it: the eaten wall ends, the bubble marks the bite, and the
    // fresh wall only resumes to the bubble's right.
    for trade in &context.projection.aggressions {
        if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
            continue;
        }
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        if !context.layout.chart_rect.contains(center) {
            continue;
        }
        // Follow the bubble's own vertical nudge, so the carved gap stays
        // centred on the bubble that will be drawn over it.
        let center = center + egui::vec2(0.0, side_offset_y(trade.side, style.bubbles.side_offset));
        let r = bubble_radius(
            trade.size,
            style.bubbles.min_radius,
            style.bubbles.max_radius,
        );
        // Carve from the bubble's midriff rightward: the eaten wall still
        // touches the bubble's left half (the bubble reads as biting into
        // it), while re-stacked liquidity cannot slide through to the right.
        add_gradient_rect(
            &mut hole_mesh,
            egui::Rect::from_min_max(
                egui::pos2(center.x - r * 0.4, center.y - r - 2.0),
                egui::pos2((center.x + r + 4.0).min(right_edge), center.y + r + 2.0),
            ),
            style.canvas_background,
            style.canvas_background,
        );
    }

    if !hole_mesh.is_empty() {
        clip.add(egui::Shape::mesh(hole_mesh));
    }

    // A calm violet ghost fading rightward = "the offer was pulled here": the
    // wall's band ends, the fade marks the pull, and the dark canvas after it
    // shows the level stayed empty. Drawn under the cap lines.
    let mut tail_mesh = egui::Mesh::default();
    for front in &fronts {
        if let EventFront::DepthOnly {
            band,
            reduction,
            full,
        } = front
        {
            let tail = if *full { 22.0 } else { 10.0 + 10.0 * reduction };
            add_gradient_rect(
                &mut tail_mesh,
                egui::Rect::from_min_max(
                    egui::pos2(band.x, band.top),
                    egui::pos2((band.x + tail).min(right_edge), band.bottom),
                ),
                palette.depth_only.gamma_multiply(if *full {
                    0.38
                } else {
                    0.16 + 0.18 * reduction
                }),
                egui::Color32::TRANSPARENT,
            );
        }
    }
    if !tail_mesh.is_empty() {
        clip.add(egui::Shape::mesh(tail_mesh));
    }

    // Fronts and caps as solid mesh quads: hundreds of stroked segments per
    // frame would pay stroke tessellation each; one mesh keeps the per-frame
    // cost flat during storms of reductions.
    fn add_vline(
        mesh: &mut egui::Mesh,
        x: f32,
        top: f32,
        bottom: f32,
        width: f32,
        color: egui::Color32,
    ) {
        add_gradient_rect(
            mesh,
            egui::Rect::from_min_max(
                egui::pos2(x - width * 0.5, top),
                egui::pos2(x + width * 0.5, bottom),
            ),
            color,
            color,
        );
    }
    let mut front_mesh = egui::Mesh::default();
    for front in fronts {
        match front {
            EventFront::Aligned {
                band,
                matched,
                full,
            } => {
                let strength = matched.max(0.25);
                add_vline(
                    &mut front_mesh,
                    band.x,
                    band.top,
                    band.bottom,
                    if full { 2.0 } else { 1.3 },
                    palette.consumption.gamma_multiply(0.55 + 0.4 * strength),
                );
                if full {
                    // End caps read as "this band was fully taken here".
                    for y in [band.top, band.bottom] {
                        add_gradient_rect(
                            &mut front_mesh,
                            egui::Rect::from_min_max(
                                egui::pos2(band.x - 3.5, y - 0.75),
                                egui::pos2(band.x + 3.5, y + 0.75),
                            ),
                            palette.consumption.gamma_multiply(0.8),
                            palette.consumption.gamma_multiply(0.8),
                        );
                    }
                }
            }
            EventFront::DepthOnly {
                band,
                reduction,
                full,
            } => {
                add_vline(
                    &mut front_mesh,
                    band.x,
                    band.top,
                    band.bottom,
                    if full { 1.6 } else { 1.1 },
                    palette.depth_only.gamma_multiply(if full {
                        0.9
                    } else {
                        0.55 + 0.3 * reduction
                    }),
                );
            }
        }
    }
    if !front_mesh.is_empty() {
        clip.add(egui::Shape::mesh(front_mesh));
    }
}

/// Draw clustered factual executions over the candle layer.
///
/// A print that aligned with a resting-liquidity reduction is drawn eating the
/// wall: a bright vertical *consumption front* on the bubble, at the exact
/// price level, with a short glow leaking into the consumed (later) side. That
/// keeps the "aggression consuming the book" legible even when price is going
/// sideways and the prints stack into a horizontal band.
pub(crate) fn draw_aggression_bubbles(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    let bubbles = &style.bubbles;
    let palette = Palette::for_theme(style.theme);
    let colors = BubbleColors::resolve(&palette, bubbles);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let right_edge = context.layout.chart_rect.right();

    // The side nudge generalizes to a bubble carrying both sides: it slides
    // continuously with the buy share, so an even split sits on the exact
    // price and a lopsided one leans the way its dominant side would.
    let center_of = |trade: &AggressionPrimitive| {
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        let lean = (finite_unit(trade.buy_share) - 0.5) * 2.0;
        context
            .layout
            .chart_rect
            .contains(center)
            .then(|| center + egui::vec2(0.0, -lean * bubbles.side_offset))
    };
    // The live lane has room the compressed history does not, which is the
    // whole reason it gets a radius range of its own.
    let (lane_min, lane_max) = style.live_lane.scaled_radii(bubbles);
    let radius_of = |trade: &AggressionPrimitive| {
        if trade.live {
            bubble_radius(trade.size, lane_min, lane_max)
        } else {
            bubble_radius(trade.size, bubbles.min_radius, bubbles.max_radius)
        }
    };

    // Consumption trail behind the bubbles, so a bubble's own fill never hides it.
    if bubbles.trail_length > 0.0 {
        let mut trail_mesh = egui::Mesh::default();
        for trade in &context.projection.aggressions {
            if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
                continue;
            }
            let Some(center) = center_of(trade) else {
                continue;
            };
            let half_length = front_half_length(radius_of(trade), bubbles);
            add_gradient_rect(
                &mut trail_mesh,
                trail_rect(center, half_length, bubbles.trail_length, right_edge),
                colors.trail.gamma_multiply(bubbles.trail_opacity),
                egui::Color32::TRANSPARENT,
            );
        }
        if !trail_mesh.is_empty() {
            clip.add(egui::Shape::mesh(trail_mesh));
        }
    }

    for trade in &context.projection.aggressions {
        let Some(center) = center_of(trade) else {
            continue;
        };
        let radius = radius_of(trade);
        let linked_reduction =
            trade.matched_fraction > 0.0 || !trade.liquidity_event_ids.is_empty();
        draw_bubble(
            &clip,
            BubbleMark {
                center,
                radius,
                side: trade.side,
                size: trade.size,
                matched: linked_reduction.then_some(trade.matched_fraction),
                buy_share: trade.buy_share,
            },
            bubbles,
            &colors,
        );

        if radius >= bubbles.label_min_radius
            && let Some(label) = bubble_label(
                trade.quantity,
                trade.trade_count,
                bubbles.show_quantity_labels,
                bubbles.show_trade_count,
            )
        {
            let font = egui::FontId::proportional(
                (radius * LABEL_FONT_SCALE).clamp(LABEL_MIN_FONT_PX, LABEL_MAX_FONT_PX),
            );
            let galley = clip.layout_no_wrap(label, font, colors.text);
            if galley.size().x <= radius * LABEL_MAX_WIDTH_SCALE
                && galley.size().y <= radius * LABEL_MAX_HEIGHT_SCALE
            {
                let pos = center - galley.size() / 2.0;
                clip.galley(
                    pos + LABEL_SHADOW_OFFSET_PX,
                    galley.clone(),
                    egui::Color32::from_black_alpha(LABEL_SHADOW_ALPHA),
                );
                clip.galley(pos, galley, colors.text);
            }
        }
    }
}

/// The legend keys for this style, one per layer that can actually draw.
///
/// A layer draws when its family is active (L2 capture for the depth family,
/// the bubbles switch for aggression) *and* its own display switch is on.
/// Announcing anything else would describe a chart the viewer is not looking
/// at — the legend is a key for what is on screen, not a feature list.
fn legend_entries(
    style: &OrderflowRenderStyle,
    liquidity_label: String,
) -> Vec<(LegendGlyph, String)> {
    let mut entries = Vec::new();
    if style.depth_layer && style.show_liquidity {
        entries.push((LegendGlyph::Heat, liquidity_label));
    }
    if style.aggression_layer && style.show_buy {
        entries.push((LegendGlyph::Buy, "buy aggression".to_owned()));
    }
    if style.aggression_layer && style.show_sell {
        entries.push((LegendGlyph::Sell, "sell aggression".to_owned()));
    }
    if style.depth_layer && style.show_aligned {
        entries.push((
            LegendGlyph::Aligned,
            "aggression-aligned depletion".to_owned(),
        ));
    }
    if style.depth_layer && style.show_unattributed {
        entries.push((
            LegendGlyph::DepthOnly,
            "L2 reduction (unattributed)".to_owned(),
        ));
    }
    if style.depth_layer && style.show_gaps {
        entries.push((LegendGlyph::Gap, "L2 gap".to_owned()));
    }
    entries
}

/// Draw a responsive legend inside the chart. Labels deliberately distinguish
/// confirmed aggression from aligned or unattributed L2 reductions.
pub(crate) fn draw_compact_legend(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    if !style.show_legend || context.layout.chart_rect.width() < 150.0 {
        return;
    }
    // The legend is a key for what is on screen, so the aggression swatches
    // follow the bubble panel's colour overrides.
    let mut palette = Palette::for_theme(style.theme);
    let colors = BubbleColors::resolve(&palette, &style.bubbles);
    palette.buy = colors.buy;
    palette.sell = colors.sell;
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let multiple = context.projection.effective_grouping.multiple;
    let liquidity_label = if multiple > 1 {
        format!("liquidity · {multiple}×")
    } else {
        "liquidity".to_owned()
    };
    let entries = legend_entries(&style, liquidity_label);
    if entries.is_empty() {
        return;
    }
    let font = egui::FontId::proportional(10.0);
    let galleys: Vec<_> = entries
        .iter()
        .map(|(_, label)| clip.layout_no_wrap(label.clone(), font.clone(), palette.legend_text))
        .collect();
    let widths: Vec<f32> = entries
        .iter()
        .zip(&galleys)
        .map(|((glyph, _), galley)| glyph.width() + 5.0 + galley.size().x + 10.0)
        .collect();

    let outer_margin = 6.0;
    let inner_margin = 7.0;
    let max_panel_width = style
        .legend_max_width
        .min((context.layout.chart_rect.width() - outer_margin * 2.0).max(120.0));
    let max_content_width = (max_panel_width - inner_margin * 2.0).max(100.0);
    let flow = flow_layout(&widths, max_content_width, 17.0, 3.0);
    let panel_size = egui::vec2(
        (flow.size.x + inner_margin * 2.0).min(max_panel_width),
        flow.size.y + inner_margin * 2.0,
    );
    // The chart header owns the first text row at the top-left. Keep the
    // legend below it so symbol/bar metadata remains readable at every width.
    let header_clearance = 22.0;
    let panel = egui::Rect::from_min_size(
        context.layout.chart_rect.left_top()
            + egui::vec2(outer_margin, outer_margin + header_clearance),
        panel_size,
    );
    clip.rect_filled(panel, egui::Rounding::same(4.0), palette.legend_background);
    clip.rect_stroke(
        panel,
        egui::Rounding::same(4.0),
        egui::Stroke::new(0.75_f32, palette.legend_border),
    );

    let origin = panel.left_top() + egui::vec2(inner_margin, inner_margin);
    for (((glyph, _), galley), offset) in entries.iter().zip(galleys).zip(flow.positions) {
        let item = origin + offset;
        draw_legend_glyph(&clip, *glyph, item, &palette, style.theme);
        let text_pos = egui::pos2(
            item.x + glyph.width() + 5.0,
            item.y + (14.0 - galley.size().y) / 2.0,
        );
        clip.galley(text_pos, galley, palette.legend_text);
    }
}

/// Deterministic visual sample used by the settings panel and screenshot tests.
///
/// It intentionally does not construct market-domain records. The preview
/// demonstrates the exact painter vocabulary with fixed synthetic geometry:
/// persistent walls, one aggression-aligned bite and one unattributed L2
/// reduction. It therefore works before a live snapshot is available.
pub(crate) fn draw_preview(ui: &mut egui::Ui, config: &HeatmapConfig) -> egui::Response {
    let width = ui.available_width().clamp(240.0, 680.0);
    let desired = egui::vec2(width, 196.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let style =
        OrderflowRenderStyle::from_config(config, egui::Color32::from_rgb(19, 23, 34)).sanitized();
    let palette = Palette::for_theme(style.theme);
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, egui::Rounding::same(4.0), style.canvas_background);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(0.75_f32, palette.legend_border),
    );

    let title = egui::pos2(rect.left() + 9.0, rect.top() + 7.0);
    painter.text(
        title,
        egui::Align2::LEFT_TOP,
        "synthetic order-flow preview",
        egui::FontId::proportional(10.0),
        palette.muted_text,
    );
    let chart = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(8.0, 24.0),
        rect.right_bottom() - egui::vec2(8.0, if config.show_legend { 30.0 } else { 8.0 }),
    );

    for step in 1..5 {
        let y = egui::lerp(chart.top()..=chart.bottom(), step as f32 / 5.0);
        painter.line_segment(
            [egui::pos2(chart.left(), y), egui::pos2(chart.right(), y)],
            egui::Stroke::new(0.5_f32, egui::Color32::from_white_alpha(16)),
        );
    }

    // Each tuple is `(y, height, x0, x1, intensity, side)`. Segment boundaries
    // make additions and reductions visible without animation or live data.
    let segments = [
        (0.16, 0.034, 0.00, 0.48, 0.34, BookSide::Ask),
        (0.16, 0.034, 0.48, 0.76, 0.73, BookSide::Ask),
        (0.27, 0.042, 0.00, 0.58, 0.92, BookSide::Ask),
        (0.27, 0.042, 0.58, 0.98, 0.40, BookSide::Ask),
        (0.39, 0.030, 0.08, 0.88, 0.50, BookSide::Ask),
        (0.61, 0.032, 0.00, 0.44, 0.36, BookSide::Bid),
        (0.61, 0.032, 0.44, 1.00, 0.66, BookSide::Bid),
        (0.72, 0.045, 0.00, 0.43, 0.88, BookSide::Bid),
        (0.72, 0.045, 0.43, 0.82, 0.24, BookSide::Bid),
        (0.84, 0.032, 0.10, 1.00, 0.54, BookSide::Bid),
    ];
    let mut heat_mesh = egui::Mesh::default();
    for (y, height, x0, x1, intensity, side) in segments {
        let band = normalized_rect(chart, x0, x1, y - height / 2.0, y + height / 2.0);
        let rgb = resting_rgb(style.theme, side, intensity);
        let alpha = config.opacity.clamp(0.0, 1.0) * 0.94;
        let glow = egui::Rect::from_min_max(
            egui::pos2(band.left(), band.top() - 0.7),
            egui::pos2(band.right(), band.bottom() + 0.7),
        );
        add_gradient_rect(
            &mut heat_mesh,
            glow,
            rgba(rgb, alpha * style.edge_glow),
            rgba(rgb, alpha * style.edge_glow),
        );
        // Solid fill, matching the live heatmap's crisp bands.
        add_gradient_rect(&mut heat_mesh, band, rgba(rgb, alpha), rgba(rgb, alpha));
    }
    painter.add(egui::Shape::mesh(heat_mesh));

    // A subdued price path gives the liquidity/trade interaction context while
    // keeping the preview focused on the order-flow layers.
    let price_points = [
        (0.00, 0.59),
        (0.15, 0.57),
        (0.29, 0.63),
        (0.43, 0.68),
        (0.56, 0.53),
        (0.66, 0.29),
        (0.79, 0.36),
        (1.00, 0.25),
    ]
    .into_iter()
    .map(|(x, y)| {
        egui::pos2(
            egui::lerp(chart.left()..=chart.right(), x),
            egui::lerp(chart.top()..=chart.bottom(), y),
        )
    })
    .collect();
    painter.add(egui::Shape::line(
        price_points,
        egui::Stroke::new(1.1_f32, egui::Color32::from_white_alpha(145)),
    ));

    if config.show_aligned_depletion {
        // Aggression-aligned consumption front with a glow leaking into the
        // consumed side.
        let aligned = EventBand {
            x: egui::lerp(chart.left()..=chart.right(), 0.58),
            top: egui::lerp(chart.top()..=chart.bottom(), 0.27 - 0.042 / 2.0),
            bottom: egui::lerp(chart.top()..=chart.bottom(), 0.27 + 0.042 / 2.0),
        };
        let mut front_mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut front_mesh,
            egui::Rect::from_min_max(
                egui::pos2(aligned.x, aligned.top),
                egui::pos2((aligned.x + 14.0).min(chart.right()), aligned.bottom),
            ),
            palette.consumption.gamma_multiply(0.24),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(front_mesh));
        painter.line_segment(
            [
                egui::pos2(aligned.x, aligned.top - 2.0),
                egui::pos2(aligned.x, aligned.bottom + 2.0),
            ],
            egui::Stroke::new(1.6_f32, palette.consumption),
        );
    }

    if config.show_unattributed_reductions {
        // Depth-only withdrawal: a calm violet fade with a thin cap.
        let depth_only = EventBand {
            x: egui::lerp(chart.left()..=chart.right(), 0.76),
            top: egui::lerp(chart.top()..=chart.bottom(), 0.16 - 0.034 / 2.0),
            bottom: egui::lerp(chart.top()..=chart.bottom(), 0.16 + 0.034 / 2.0),
        };
        let mut ghost_mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut ghost_mesh,
            egui::Rect::from_min_max(
                egui::pos2(depth_only.x, depth_only.top),
                egui::pos2((depth_only.x + 20.0).min(chart.right()), depth_only.bottom),
            ),
            palette.depth_only.gamma_multiply(0.42),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(ghost_mesh));
        painter.line_segment(
            [
                egui::pos2(depth_only.x, depth_only.top - 1.0),
                egui::pos2(depth_only.x, depth_only.bottom + 1.0),
            ],
            egui::Stroke::new(1.4_f32, palette.depth_only),
        );
    }

    if config.show_aggressions {
        let bubbles = &style.bubbles;
        let colors = BubbleColors::resolve(&palette, bubbles);
        // Two prints at fixed normalized sizes, so every slider (radius range,
        // opacity, rim, front, trail, side offset) shows its effect here.
        draw_preview_bubble(
            &painter,
            PreviewBubble {
                center: egui::pos2(
                    egui::lerp(chart.left()..=chart.right(), 0.58),
                    egui::lerp(chart.top()..=chart.bottom(), 0.27),
                ),
                size: PREVIEW_LARGE_PRINT_SIZE,
                side: Side::Buy,
                linked_reduction: config.show_aligned_depletion,
                // With the closed-bar summary on, the large sample is what a
                // summarized bar actually produces: one mark, both sides.
                buy_share: if config.bubble_candle_summary {
                    PREVIEW_SUMMARY_BUY_SHARE
                } else {
                    1.0
                },
            },
            chart.right(),
            bubbles,
            &colors,
        );
        draw_preview_bubble(
            &painter,
            PreviewBubble {
                center: egui::pos2(
                    egui::lerp(chart.left()..=chart.right(), 0.43),
                    egui::lerp(chart.top()..=chart.bottom(), 0.72),
                ),
                size: PREVIEW_SMALL_PRINT_SIZE,
                side: Side::Sell,
                linked_reduction: false,
                buy_share: 0.0,
            },
            chart.right(),
            bubbles,
            &colors,
        );
    }

    if config.show_legend {
        draw_preview_legend(&painter, rect, &palette, style.theme);
    }
    response.on_hover_text(
        "Deterministic visual sample: green/red dots are confirmed trades; \
         a bright bite is aggression-aligned depletion; violet is an \
         unattributed L2 reduction.",
    )
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

#[derive(Debug, Clone, Copy)]
struct EventBand {
    x: f32,
    top: f32,
    bottom: f32,
}

impl EventBand {
    fn height(self) -> f32 {
        (self.bottom - self.top).max(0.0)
    }

    fn center_y(self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

#[derive(Debug, Clone, Copy)]
enum EventFront {
    Aligned {
        band: EventBand,
        matched: f32,
        full: bool,
    },
    DepthOnly {
        band: EventBand,
        reduction: f32,
        full: bool,
    },
}

#[derive(Debug, Clone, Copy)]
enum LegendGlyph {
    Heat,
    Buy,
    Sell,
    Aligned,
    DepthOnly,
    Gap,
}

impl LegendGlyph {
    const fn width(self) -> f32 {
        match self {
            Self::Heat => 42.0,
            Self::Buy | Self::Sell => 12.0,
            Self::Aligned | Self::DepthOnly | Self::Gap => 18.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct FlowLayout {
    positions: Vec<egui::Vec2>,
    size: egui::Vec2,
}

fn flow_layout(widths: &[f32], max_width: f32, row_height: f32, gap: f32) -> FlowLayout {
    let max_width = max_width.max(1.0);
    let row_height = row_height.max(1.0);
    let gap = gap.max(0.0);
    let mut positions = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    let mut y = 0.0;
    let mut widest: f32 = 0.0;

    for &raw_width in widths {
        let width = raw_width.max(0.0);
        if x > 0.0 && x + width > max_width {
            widest = widest.max((x - gap).max(0.0));
            x = 0.0;
            y += row_height;
        }
        positions.push(egui::vec2(x, y));
        x += width + gap;
    }
    widest = widest.max((x - gap).max(0.0)).min(max_width);
    let height = if widths.is_empty() {
        0.0
    } else {
        y + row_height
    };
    FlowLayout {
        positions,
        size: egui::vec2(widest, height),
    }
}

fn normalized_rect(bounds: egui::Rect, x0: f32, x1: f32, y0: f32, y1: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            egui::lerp(bounds.left()..=bounds.right(), finite_unit(x0)),
            egui::lerp(bounds.top()..=bounds.bottom(), finite_unit(y0)),
        ),
        egui::pos2(
            egui::lerp(bounds.left()..=bounds.right(), finite_unit(x1)),
            egui::lerp(bounds.top()..=bounds.bottom(), finite_unit(y1)),
        ),
    )
}

/// One sample print in the settings preview.
#[derive(Debug, Clone, Copy)]
struct PreviewBubble {
    center: egui::Pos2,
    /// Normalized print size, as the projection would report it.
    size: f32,
    side: Side,
    /// Whether this sample ate resting liquidity, so it shows the marks.
    linked_reduction: bool,
    /// Buy share of the sample, so a preview of the closed-bar summary shows
    /// the pie the chart would actually draw.
    buy_share: f32,
}

/// Draw one preview print exactly the way the chart would draw it.
///
/// Everything past the trail goes through [`draw_bubble`], the same function
/// the live chart uses: a preview that renders its own approximation would
/// send the user tuning sliders against a picture the chart never produces.
fn draw_preview_bubble(
    painter: &egui::Painter,
    preview: PreviewBubble,
    right_edge: f32,
    bubbles: &BubbleStyle,
    colors: &BubbleColors,
) {
    let PreviewBubble {
        center,
        size,
        side,
        linked_reduction,
        buy_share,
    } = preview;
    let lean = (finite_unit(buy_share) - 0.5) * 2.0;
    let center = center + egui::vec2(0.0, -lean * bubbles.side_offset);
    let radius = bubble_radius(size, bubbles.min_radius, bubbles.max_radius);
    if linked_reduction && bubbles.trail_length > 0.0 {
        // Consumption trail behind the bubble (drawn first so the fill sits on
        // top). On the chart this is batched across every bubble; here there
        // are two, so one mesh each costs nothing.
        let mut mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut mesh,
            trail_rect(
                center,
                front_half_length(radius, bubbles),
                bubbles.trail_length,
                right_edge,
            ),
            colors.trail.gamma_multiply(bubbles.trail_opacity),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(mesh));
    }
    draw_bubble(
        painter,
        BubbleMark {
            center,
            radius,
            side,
            size,
            matched: linked_reduction.then_some(PREVIEW_MATCHED_FRACTION),
            buy_share,
        },
        bubbles,
        colors,
    );
}

fn draw_preview_legend(
    painter: &egui::Painter,
    bounds: egui::Rect,
    palette: &Palette,
    theme: HeatmapTheme,
) {
    let baseline = bounds.bottom() - 13.0;
    let mut x = bounds.left() + 10.0;
    let font = egui::FontId::proportional(9.5);

    let heat_rect = egui::Rect::from_min_size(egui::pos2(x, baseline - 4.0), egui::vec2(34.0, 7.0));
    let mut mesh = egui::Mesh::default();
    for index in 0..8 {
        let t0 = index as f32 / 8.0;
        let t1 = (index + 1) as f32 / 8.0;
        add_gradient_rect(
            &mut mesh,
            egui::Rect::from_min_max(
                egui::pos2(
                    egui::lerp(heat_rect.left()..=heat_rect.right(), t0),
                    heat_rect.top(),
                ),
                egui::pos2(
                    egui::lerp(heat_rect.left()..=heat_rect.right(), t1),
                    heat_rect.bottom(),
                ),
            ),
            rgba(thermal_rgb(theme, t0), 1.0),
            rgba(thermal_rgb(theme, t1), 1.0),
        );
    }
    painter.add(egui::Shape::mesh(mesh));
    x += 39.0;
    painter.text(
        egui::pos2(x, baseline),
        egui::Align2::LEFT_CENTER,
        "liquidity",
        font.clone(),
        palette.legend_text,
    );
    x += 54.0;

    for (color, label) in [(palette.buy, "buy"), (palette.sell, "sell")] {
        painter.circle_filled(
            egui::pos2(x + 4.0, baseline),
            4.0,
            color.gamma_multiply(0.82),
        );
        painter.circle_stroke(
            egui::pos2(x + 4.0, baseline),
            4.0,
            egui::Stroke::new(0.75_f32, color),
        );
        painter.text(
            egui::pos2(x + 11.0, baseline),
            egui::Align2::LEFT_CENTER,
            label,
            font.clone(),
            palette.legend_text,
        );
        x += 41.0;
    }

    // On narrow settings windows the hover text remains the complete legend.
    if x + 200.0 > bounds.right() {
        return;
    }
    painter.line_segment(
        [
            egui::pos2(x + 3.0, baseline - 5.0),
            egui::pos2(x + 3.0, baseline + 5.0),
        ],
        egui::Stroke::new(1.3_f32, palette.consumption),
    );
    painter.text(
        egui::pos2(x + 9.0, baseline),
        egui::Align2::LEFT_CENTER,
        "aligned depletion",
        font.clone(),
        palette.legend_text,
    );
    x += 105.0;
    painter.line_segment(
        [
            egui::pos2(x + 3.0, baseline - 5.0),
            egui::pos2(x + 3.0, baseline + 5.0),
        ],
        egui::Stroke::new(1.3_f32, palette.depth_only),
    );
    painter.text(
        egui::pos2(x + 9.0, baseline),
        egui::Align2::LEFT_CENTER,
        "unattributed L2 reduction",
        font,
        palette.legend_text,
    );
}

fn draw_legend_glyph(
    painter: &egui::Painter,
    glyph: LegendGlyph,
    origin: egui::Pos2,
    palette: &Palette,
    theme: HeatmapTheme,
) {
    let center = origin + egui::vec2(glyph.width() / 2.0, 7.0);
    match glyph {
        LegendGlyph::Heat => {
            let rect =
                egui::Rect::from_min_size(origin + egui::vec2(0.0, 3.0), egui::vec2(42.0, 8.0));
            let mut mesh = egui::Mesh::default();
            for index in 0..12 {
                let t0 = index as f32 / 12.0;
                let t1 = (index + 1) as f32 / 12.0;
                let x0 = egui::lerp(rect.left()..=rect.right(), t0);
                let x1 = egui::lerp(rect.left()..=rect.right(), t1);
                add_gradient_rect(
                    &mut mesh,
                    egui::Rect::from_min_max(
                        egui::pos2(x0, rect.top()),
                        egui::pos2(x1, rect.bottom()),
                    ),
                    rgba(thermal_rgb(theme, t0), 1.0),
                    rgba(thermal_rgb(theme, t1), 1.0),
                );
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        LegendGlyph::Buy => {
            painter.circle_filled(center, 5.0, palette.buy.gamma_multiply(0.82));
            painter.circle_stroke(center, 5.0, egui::Stroke::new(0.8_f32, palette.buy));
        }
        LegendGlyph::Sell => {
            painter.circle_filled(center, 5.0, palette.sell.gamma_multiply(0.82));
            painter.circle_stroke(center, 5.0, egui::Stroke::new(0.8_f32, palette.sell));
        }
        LegendGlyph::Aligned => {
            let band = egui::Rect::from_center_size(center, egui::vec2(18.0, 6.0));
            let mut mesh = egui::Mesh::default();
            // Resting wall on the left, consumed (fading) on the right.
            add_gradient_rect(
                &mut mesh,
                egui::Rect::from_min_max(band.left_top(), egui::pos2(center.x, band.bottom())),
                rgba(thermal_rgb(theme, 0.72), 0.92),
                rgba(thermal_rgb(theme, 0.72), 0.92),
            );
            add_gradient_rect(
                &mut mesh,
                egui::Rect::from_min_max(egui::pos2(center.x, band.top()), band.right_bottom()),
                palette.consumption.gamma_multiply(0.22),
                egui::Color32::TRANSPARENT,
            );
            painter.add(egui::Shape::mesh(mesh));
            painter.line_segment(
                [
                    egui::pos2(center.x, band.top() - 1.5),
                    egui::pos2(center.x, band.bottom() + 1.5),
                ],
                egui::Stroke::new(1.4_f32, palette.consumption),
            );
        }
        LegendGlyph::DepthOnly => {
            let rect = egui::Rect::from_center_size(center, egui::vec2(18.0, 7.0));
            let mut mesh = egui::Mesh::default();
            add_gradient_rect(
                &mut mesh,
                rect,
                palette.depth_only.gamma_multiply(0.4),
                egui::Color32::TRANSPARENT,
            );
            painter.add(egui::Shape::mesh(mesh));
            painter.line_segment(
                [
                    egui::pos2(rect.left(), rect.top()),
                    egui::pos2(rect.left(), rect.bottom()),
                ],
                egui::Stroke::new(1.3_f32, palette.depth_only),
            );
        }
        LegendGlyph::Gap => {
            let rect = egui::Rect::from_center_size(center, egui::vec2(18.0, 8.0));
            painter.rect_filled(rect, egui::Rounding::ZERO, palette.gap_fill);
            draw_dashed_vertical(
                painter,
                rect.left(),
                rect,
                2.0,
                1.5,
                palette.gap_boundary,
                0.9,
            );
            draw_dashed_vertical(
                painter,
                rect.right(),
                rect,
                2.0,
                1.5,
                palette.gap_boundary,
                0.9,
            );
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

fn marker_band(band: EventBand, reduction: f32, full: bool) -> EventBand {
    if full {
        return band;
    }
    let fraction = finite_unit(reduction);
    let height = (band.height() * (0.30 + 0.70 * fraction)).max(1.5);
    let center = band.center_y();
    EventBand {
        x: band.x,
        top: center - height / 2.0,
        bottom: center + height / 2.0,
    }
}

fn bubble_radius(size: f32, minimum: f32, maximum: f32) -> f32 {
    let minimum = minimum.max(0.0);
    let maximum = maximum.max(minimum);
    let normalized_quantity = finite_unit(size).powi(2);
    (minimum.powi(2) + normalized_quantity * (maximum.powi(2) - minimum.powi(2))).sqrt()
}

fn bubble_label(
    quantity: Decimal,
    trade_count: usize,
    show_quantity: bool,
    show_count: bool,
) -> Option<String> {
    match (show_quantity, show_count && trade_count > 1) {
        (false, false) => None,
        (true, false) => Some(format_quantity(quantity)),
        (false, true) => Some(format!("×{trade_count}")),
        (true, true) => Some(format!("{} · ×{trade_count}", format_quantity(quantity))),
    }
}

fn format_quantity(quantity: Decimal) -> String {
    let value = quantity.to_f64().unwrap_or(0.0);
    let absolute = value.abs();
    let (scaled, suffix) = if absolute >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "B")
    } else if absolute >= 1_000_000.0 {
        (value / 1_000_000.0, "M")
    } else if absolute >= 1_000.0 {
        (value / 1_000.0, "K")
    } else {
        (value, "")
    };
    let decimals = if scaled.abs() >= 100.0 {
        0
    } else if scaled.abs() >= 10.0 {
        1
    } else {
        2
    };
    let formatted = format!("{scaled:.decimals$}");
    format!("{}{suffix}", trim_decimal_zeros(&formatted))
}

fn trim_decimal_zeros(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
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

/// Pixel slack for deciding that a gap boundary coincides with the chart edge.
/// Gap bounds arrive as normalized floats scaled into screen space, so an
/// exact comparison would miss by a rounding bit and draw a stray frame line.
const GAP_EDGE_EPSILON: f32 = 0.5;

/// Gap between the leading span's label and the divider it annotates. Small
/// enough that the text reads as belonging to the line rather than floating.
const GAP_LABEL_INSET_PX: f32 = 6.0;

/// Which marks one coverage gap gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GapMarks {
    fill: bool,
    left_boundary: bool,
    right_boundary: bool,
}

/// Decide how to mark a coverage gap.
///
/// The leading span — everything older than the first snapshot this session
/// captured — routinely covers a third of the chart. Tinting it would bury the
/// candles, bubbles and strip that are perfectly real there, so one boundary
/// carries the whole message: where the book begins. Its left side is not a
/// boundary at all, whether it lands on the viewport edge or on the oldest bar
/// the chart holds — there is nothing on the far side of it to separate from.
///
/// An interior gap is different on both counts. It is narrow, so it keeps a
/// faint fill (an untinted sliver reads as "no resting liquidity" rather than
/// "no data"), and both its ends are real transitions. A boundary landing on
/// the chart edge is still dropped: that marks the viewport, not a change in
/// coverage, and drawing it would just frame the chart.
fn gap_marks(rect: egui::Rect, chart_rect: egui::Rect, leading: bool) -> GapMarks {
    let on_chart_edge = |x: f32| {
        (x - chart_rect.left()).abs() < GAP_EDGE_EPSILON
            || (x - chart_rect.right()).abs() < GAP_EDGE_EPSILON
    };
    GapMarks {
        fill: !leading,
        left_boundary: !leading && !on_chart_edge(rect.left()),
        right_boundary: !on_chart_edge(rect.right()),
    }
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

fn draw_text_with_shadow(
    painter: &egui::Painter,
    anchor: egui::Pos2,
    align: egui::Align2,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    painter.text(
        anchor + egui::vec2(1.0, 1.0),
        align,
        text,
        font.clone(),
        egui::Color32::from_black_alpha(190),
    );
    painter.text(anchor, align, text, font, color);
}

fn gap_label(reason: &str) -> &'static str {
    match reason {
        BEFORE_CAPTURE => "L2 unavailable before capture",
        "capture_disabled" => "L2 capture disabled",
        "sequence_gap" => "L2 sequence gap · resynchronizing",
        _ => "L2 continuity unavailable",
    }
}

fn readable_band(rect: egui::Rect, minimum_height: f32, clip: egui::Rect) -> egui::Rect {
    if !rect.is_finite() {
        return egui::Rect::NOTHING;
    }
    let minimum_height = minimum_height.max(0.5);
    let readable = if rect.height() < minimum_height {
        egui::Rect::from_center_size(
            rect.center(),
            egui::vec2(rect.width().max(0.5), minimum_height),
        )
    } else {
        rect
    };
    readable.intersect(clip)
}

fn rgba(rgb: [u8; 3], alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        rgb[0],
        rgb[1],
        rgb[2],
        (finite_unit(alpha) * 255.0).round() as u8,
    )
}

/// Colour and opacity of one resting-liquidity block — the heatmap's exact
/// pipeline (quantized magnitude bands, thermal ramp, side tint), factored out
/// so the live strip reads on the very same ramp by construction. `None`
/// means the block is too faint to draw at all.
fn heat_fill_parts(
    style: &OrderflowRenderStyle,
    side: BookSide,
    raw_intensity: f32,
    base_alpha: f32,
) -> Option<([u8; 3], f32)> {
    let raw_intensity = finite_unit(raw_intensity);
    let base_alpha = finite_unit(base_alpha);
    if raw_intensity <= 0.0 || base_alpha <= 0.0 {
        return None;
    }
    // Quantize magnitude into a few bands so the book's per-update jitter
    // maps to the SAME colour: adjacent runs merge into one crisp, stable
    // band instead of a flickering gradient that reads as "meteors". The
    // faintest noise (rounding to zero) drops out entirely.
    let intensity = quantize_heat(raw_intensity);
    if intensity <= 0.0 {
        return None;
    }
    let alpha = finite_unit(base_alpha * (intensity / raw_intensity) * style.heat_opacity);
    if alpha <= 0.0 {
        return None;
    }
    Some((resting_rgb(style.theme, side, intensity), alpha))
}

fn mix_rgb(from: [u8; 3], to: [u8; 3], amount: f32) -> [u8; 3] {
    let amount = finite_unit(amount);
    [
        (f32::from(from[0]) + (f32::from(to[0]) - f32::from(from[0])) * amount).round() as u8,
        (f32::from(from[1]) + (f32::from(to[1]) - f32::from(from[1])) * amount).round() as u8,
        (f32::from(from[2]) + (f32::from(to[2]) - f32::from(from[2])) * amount).round() as u8,
    ]
}

/// Number of discrete magnitude bands the heatmap collapses intensity into.
/// Fewer bands read as flatter walls; more bands recover gradient but let the
/// book's per-update jitter fragment a band. Eight keeps walls crisp while
/// still separating quiet / medium / heavy liquidity.
const HEAT_LEVELS: f32 = 8.0;

fn quantize_heat(intensity: f32) -> f32 {
    ((intensity * HEAT_LEVELS).round() / HEAT_LEVELS).clamp(0.0, 1.0)
}

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn finite_unit_f64(value: f64) -> f64 {
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

    /// The dust threshold is defined by inverting this module's radius
    /// mapping, but lives in `config` beside the style it reads. This pins the
    /// two together: a print at the threshold must land exactly on the
    /// readability floor.
    #[test]
    fn the_dust_threshold_lands_on_the_readability_floor() {
        use rust_decimal::prelude::ToPrimitive as _;

        let bubbles = BubbleStyle::default();
        let reference = rust_decimal::Decimal::from(400);
        let dust = bubbles
            .dust_quantity(reference)
            .expect("the default style has a readability floor above its minimum");
        let size = (dust / reference)
            .to_f32()
            .expect("the threshold share converts")
            .sqrt();
        let radius = bubble_radius(size, bubbles.min_radius, bubbles.max_radius);
        assert!(
            (radius - bubbles.readable_min_radius).abs() < 1e-3,
            "a dust print rendered at {radius}, not {}",
            bubbles.readable_min_radius
        );
    }

    /// The shipped presets tune `detail_min_radius` down to buy sphere shading
    /// on small prints — "dense tape btc" (the default open) sets it *below*
    /// `min_radius`. Anchoring the readability floor there made both the dust
    /// merge and the hollow ring inert on exactly the look the project opens
    /// with, which is the regression this guards.
    #[test]
    fn a_low_detail_radius_does_not_disarm_the_readability_floor() {
        let dense_tape_btc = BubbleStyle {
            min_radius: 2.2,
            max_radius: 14.0,
            detail_min_radius: 2.0,
            ..BubbleStyle::default()
        };
        assert!(dense_tape_btc.detail_min_radius < dense_tape_btc.min_radius);
        assert!(
            dense_tape_btc
                .dust_quantity(rust_decimal::Decimal::from(400))
                .is_some(),
            "prints must still be foldable when the dressing radius is low"
        );

        let colors =
            BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &dense_tape_btc);
        let mark = BubbleMark {
            center: egui::pos2(40.0, 40.0),
            radius: dense_tape_btc.min_radius,
            side: Side::Buy,
            size: 0.02,
            matched: None,
            buy_share: 1.0,
        };
        let solid = BubbleStyle {
            hollow_small_buys: false,
            ..dense_tape_btc.clone()
        };
        assert_ne!(
            painted(|painter| draw_bubble(painter, mark, &dense_tape_btc, &colors)),
            painted(|painter| draw_bubble(painter, mark, &solid, &colors)),
            "the ring must still fire when the dressing radius is below the minimum"
        );
    }

    #[test]
    fn nothing_is_dust_without_a_reference_or_a_readability_floor() {
        let bubbles = BubbleStyle::default();
        assert!(bubbles.dust_quantity(rust_decimal::Decimal::ZERO).is_none());

        let flat = BubbleStyle {
            readable_min_radius: 0.0,
            ..BubbleStyle::default()
        };
        assert!(
            flat.dust_quantity(rust_decimal::Decimal::from(400))
                .is_none()
        );
    }

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

    #[test]
    fn bubble_area_above_floor_tracks_normalized_quantity() {
        let minimum = 3.0;
        let maximum = 13.0;
        let quarter_quantity_radius = bubble_radius(0.5, minimum, maximum);
        let full_radius = bubble_radius(1.0, minimum, maximum);
        let quarter_area = quarter_quantity_radius.powi(2) - minimum.powi(2);
        let full_area = full_radius.powi(2) - minimum.powi(2);
        assert!((quarter_area / full_area - 0.25).abs() < 1e-5);
    }

    #[test]
    fn partial_marker_height_grows_with_reduction_fraction() {
        let band = EventBand {
            x: 50.0,
            top: 10.0,
            bottom: 30.0,
        };
        let quiet = marker_band(band, 0.1, false);
        let strong = marker_band(band, 0.8, false);
        assert!(quiet.height() < strong.height());
        assert!(strong.height() < band.height());
        assert_eq!(marker_band(band, 0.2, true).height(), band.height());
    }

    #[test]
    fn compact_legend_wraps_without_exceeding_width() {
        let widths = [90.0, 90.0, 90.0];
        let layout = flow_layout(&widths, 190.0, 17.0, 3.0);
        assert_eq!(layout.positions[0], egui::vec2(0.0, 0.0));
        assert_eq!(layout.positions[1], egui::vec2(93.0, 0.0));
        assert_eq!(layout.positions[2], egui::vec2(0.0, 17.0));
        assert!(layout.size.x <= 190.0);
        assert_eq!(layout.size.y, 34.0);
    }

    #[test]
    fn labels_are_honest_and_compact() {
        assert_eq!(
            bubble_label(Decimal::from(1_250), 4, true, true),
            Some("1.25K · ×4".to_owned())
        );
        assert_eq!(format_quantity(Decimal::from(100)), "100");
        assert_eq!(format_quantity(Decimal::ZERO), "0");
        assert_eq!(
            bubble_label(Decimal::ONE, 1, false, true),
            None,
            "one trade does not need a redundant count"
        );
    }

    /// The stretch older than this session's capture used to be a hatched,
    /// tinted block covering a third of the chart. It is now its boundary and
    /// nothing else, so the candles and bubbles recorded there stay readable.
    #[test]
    fn the_pre_capture_span_is_marked_by_its_boundary_alone() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
        let leading = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(120.0, 200.0));
        assert_eq!(
            gap_marks(leading, chart, true),
            GapMarks {
                fill: false,
                left_boundary: false,
                right_boundary: true,
            }
        );

        // A chart the bars do not fill starts the span at the oldest bar
        // instead of at the viewport edge. Still one line: there is nothing to
        // the left of it to separate the span from.
        let inset = egui::Rect::from_min_max(egui::pos2(60.0, 0.0), egui::pos2(120.0, 200.0));
        assert_eq!(
            gap_marks(inset, chart, true),
            GapMarks {
                fill: false,
                left_boundary: false,
                right_boundary: true,
            }
        );
    }

    /// An interior gap is a handful of pixels wide. Without a fill it would
    /// read as an empty book rather than as missing data.
    #[test]
    fn an_interior_gap_keeps_its_fill_and_both_boundaries() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
        let interior = egui::Rect::from_min_max(egui::pos2(180.0, 0.0), egui::pos2(186.0, 200.0));
        assert_eq!(
            gap_marks(interior, chart, false),
            GapMarks {
                fill: true,
                left_boundary: true,
                right_boundary: true,
            }
        );
    }

    /// Nothing captured at all: both bounds are the viewport, so no line is
    /// drawn — framing the whole chart would say nothing the label does not.
    #[test]
    fn a_gap_spanning_the_whole_chart_draws_no_boundary() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 200.0));
        assert_eq!(
            gap_marks(chart, chart, true),
            GapMarks {
                fill: false,
                left_boundary: false,
                right_boundary: false,
            }
        );
    }

    #[test]
    fn render_style_sanitizes_non_finite_geometry() {
        let style = OrderflowRenderStyle {
            heat_opacity: f32::NAN,
            min_cell_height: f32::INFINITY,
            edge_glow: -1.0,
            bubbles: BubbleStyle {
                min_radius: f32::NAN,
                max_radius: -4.0,
                label_min_radius: f32::NAN,
                ..BubbleStyle::default()
            },
            legend_max_width: f32::NAN,
            ..OrderflowRenderStyle::default()
        }
        .sanitized();
        assert_eq!(style.heat_opacity, 1.0);
        assert_eq!(style.min_cell_height, 1.5);
        assert_eq!(style.edge_glow, 0.0);
        assert!(style.bubbles.max_radius >= style.bubbles.min_radius);
        assert!(style.bubbles.label_min_radius.is_finite());
        assert!(style.legend_max_width.is_finite());
    }

    #[test]
    fn buy_and_sell_bubbles_are_nudged_to_opposite_sides() {
        // Screen y grows downward: buys sit above the print, sells below.
        assert!(side_offset_y(Side::Buy, 4.0) < 0.0);
        assert!(side_offset_y(Side::Sell, 4.0) > 0.0);
        assert_eq!(side_offset_y(Side::Buy, 0.0), 0.0);
        assert_eq!(
            side_offset_y(Side::Buy, 4.0).abs(),
            side_offset_y(Side::Sell, 4.0).abs()
        );
    }

    /// Paint through `draw` off-screen and return the shapes it emitted.
    fn painted(draw: impl Fn(&egui::Painter)) -> String {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw(&ctx.layer_painter(egui::LayerId::background()));
        });
        format!("{:?}", output.shapes)
    }

    /// The cheap-dot path is the fps contract on a dense tape: below the
    /// dressing radius a solid print stays exactly one filled circle — no
    /// halo, no rim, and no separator ring may sneak onto it.
    #[test]
    fn a_cheap_dot_stays_a_single_circle() {
        let bubbles = BubbleStyle {
            min_radius: 2.0,
            detail_min_radius: 6.0,
            hollow_small_buys: false,
            ..BubbleStyle::default()
        };
        let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
        let shapes = painted(|painter| {
            draw_bubble(
                painter,
                BubbleMark {
                    center: egui::pos2(50.0, 50.0),
                    radius: 3.0,
                    side: Side::Sell,
                    size: 0.1,
                    matched: None,
                    buy_share: 0.0,
                },
                &bubbles,
                &colors,
            )
        });
        assert_eq!(
            shapes.matches("CircleShape").count(),
            1,
            "a cheap dot must stay one circle: {shapes}"
        );
    }

    #[test]
    fn the_preview_draws_a_bubble_exactly_the_way_the_chart_does() {
        // The preview is the instrument the user tunes the sliders against, so
        // it must not render its own approximation. Both paths go through
        // draw_bubble; with the trail off (the one mark the chart batches
        // separately) they must emit identical shapes.
        let bubbles = BubbleStyle {
            trail_length: 0.0,
            ..BubbleStyle::default()
        };
        let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
        let at = egui::pos2(120.0, 80.0);
        let radius = bubble_radius(
            PREVIEW_LARGE_PRINT_SIZE,
            bubbles.min_radius,
            bubbles.max_radius,
        );

        let live = painted(|painter| {
            draw_bubble(
                painter,
                BubbleMark {
                    center: at + egui::vec2(0.0, side_offset_y(Side::Buy, bubbles.side_offset)),
                    radius,
                    side: Side::Buy,
                    size: PREVIEW_LARGE_PRINT_SIZE,
                    matched: Some(PREVIEW_MATCHED_FRACTION),
                    buy_share: 1.0,
                },
                &bubbles,
                &colors,
            );
        });
        let preview = painted(|painter| {
            draw_preview_bubble(
                painter,
                PreviewBubble {
                    center: at,
                    size: PREVIEW_LARGE_PRINT_SIZE,
                    side: Side::Buy,
                    linked_reduction: true,
                    buy_share: 1.0,
                },
                f32::INFINITY,
                &bubbles,
                &colors,
            );
        });
        assert!(
            live.contains("Circle"),
            "the sample must actually draw a bubble: {live}"
        );
        assert_eq!(live, preview);
    }

    #[test]
    fn bubble_marks_scale_with_size_and_matched_share() {
        let bubbles = BubbleStyle::default();
        // The front grows with the radius, and never collapses to nothing on
        // the smallest bubble.
        assert!(front_half_length(10.0, &bubbles) > front_half_length(2.0, &bubbles));
        assert!(front_half_length(0.0, &bubbles) >= FRONT_END_PADDING_PX);
        // A sweep haloes brighter than a routine print, and alpha stays legal.
        assert!(halo_alpha(1.0, &bubbles) > halo_alpha(0.0, &bubbles));
        assert!(halo_alpha(1.0, &bubbles) <= 1.0);
        assert_eq!(
            halo_alpha(0.0, &bubbles),
            bubbles.halo_strength,
            "an unsized print gets the plain halo"
        );
        assert!(
            halo_alpha(f32::NAN, &bubbles).is_finite(),
            "a non-finite size must not poison the alpha"
        );
        // The ring brightens with the share of the print that matched, from a
        // floor that keeps a nibble visible.
        assert!(impact_ring_alpha(1.0) > impact_ring_alpha(0.0));
        assert!(impact_ring_alpha(0.0) >= IMPACT_RING_BASE_ALPHA);
        assert!(impact_ring_alpha(1.0) <= 1.0);
    }

    #[test]
    fn bubble_colours_fall_back_to_the_theme_and_the_trail_follows_the_front() {
        let palette = Palette::for_theme(HeatmapTheme::Bookmap);
        let default = BubbleColors::resolve(&palette, &BubbleStyle::default());
        assert_eq!(default.buy, palette.buy);
        assert_eq!(default.sell, palette.sell);
        assert_eq!(default.trail, palette.consumption);

        let overridden = BubbleColors::resolve(
            &palette,
            &BubbleStyle {
                buy_color: Some([1, 2, 3]),
                front_color: Some([9, 9, 9]),
                ..BubbleStyle::default()
            },
        );
        assert_eq!(overridden.buy, egui::Color32::from_rgb(1, 2, 3));
        assert_eq!(
            overridden.sell, palette.sell,
            "untouched side keeps the theme"
        );
        assert_eq!(
            overridden.trail,
            egui::Color32::from_rgb(9, 9, 9),
            "the trail follows the front colour unless overridden itself"
        );
    }

    #[test]
    fn sphere_colours_brighten_the_core_and_darken_the_rim() {
        let color = egui::Color32::from_rgb(40, 200, 120);
        let rgb = |c: egui::Color32| [c.r(), c.g(), c.b()];
        assert!(luminance(rgb(sphere_core_color(color, 0.35))) > luminance(rgb(color)));
        assert!(luminance(rgb(sphere_edge_color(color, 0.55))) < luminance(rgb(color)));
        // Zero strength is the identity, so "sphere with no shading" degrades
        // honestly into the flat colour instead of some third look.
        assert_eq!(sphere_core_color(color, 0.0), color);
        assert_eq!(sphere_edge_color(color, 0.0), color);
        assert_eq!(sphere_edge_color(color, 1.0), egui::Color32::BLACK);
    }

    #[test]
    fn a_sphere_disc_is_a_bounded_two_ring_fan() {
        let mut mesh = egui::Mesh::default();
        let center = egui::pos2(50.0, 50.0);
        let radius = 10.0;
        let full = std::f32::consts::TAU;
        let shading = SphereShading {
            core: egui::Color32::WHITE,
            body: egui::Color32::GRAY,
            edge: egui::Color32::BLACK,
        };
        add_shaded_sector(&mut mesh, center, radius, 0.0, full, shading);
        let segments = sphere_segments(radius);
        // One vertex more per ring than the wrapping fan needed: an arc has two
        // ends, and a whole circle is the arc whose ends coincide.
        assert_eq!(mesh.vertices.len(), 1 + 2 * (segments + 1));
        assert_eq!(mesh.indices.len(), segments * 9);
        for vertex in &mesh.vertices {
            assert!(
                (vertex.pos - center).length() <= radius + 0.001,
                "shading must stay inside the bubble: {:?}",
                vertex.pos
            );
        }

        // Degenerate geometry appends nothing rather than poisoning the mesh.
        let before = mesh.vertices.len();
        for (center, radius, sweep) in [
            (egui::pos2(f32::NAN, 0.0), radius, full),
            (center, 0.0, full),
            (center, radius, 0.0),
            (center, radius, f32::NAN),
        ] {
            add_shaded_sector(&mut mesh, center, radius, 0.0, sweep, shading);
        }
        assert_eq!(mesh.vertices.len(), before);
    }

    /// A pie is two wedges, and each one stays a wedge: bounded by the radius,
    /// anchored on the shared centre, and cheaper than a whole disc.
    #[test]
    fn a_sector_covers_only_its_own_slice() {
        let center = egui::pos2(50.0, 50.0);
        let radius = 12.0;
        let shading = SphereShading::flat(egui::Color32::GRAY);
        let mut quarter = egui::Mesh::default();
        add_shaded_sector(
            &mut quarter,
            center,
            radius,
            PIE_START_ANGLE,
            std::f32::consts::FRAC_PI_2,
            shading,
        );
        let mut whole = egui::Mesh::default();
        add_shaded_sector(
            &mut whole,
            center,
            radius,
            PIE_START_ANGLE,
            std::f32::consts::TAU,
            shading,
        );
        assert!(quarter.vertices.len() < whole.vertices.len());
        // Straight up and to the right of centre: the quarter starting at
        // twelve o'clock sweeps clockwise into exactly that quadrant.
        assert!(
            quarter.vertices.iter().all(|vertex| vertex.pos.x
                >= center.x - radius * SPHERE_LIGHT_OFFSET - 0.001
                && vertex.pos.y <= center.y + 0.001),
            "a quarter must not paint the other three: {:?}",
            quarter.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sphere_mode_swaps_the_flat_fill_for_a_shaded_mesh() {
        let mark = BubbleMark {
            center: egui::pos2(120.0, 80.0),
            radius: 12.0,
            side: Side::Buy,
            size: 0.8,
            matched: None,
            buy_share: 1.0,
        };
        let palette = Palette::for_theme(HeatmapTheme::Bookmap);
        let flat_style = BubbleStyle::default();
        let sphere_style = BubbleStyle {
            render_mode: BubbleRenderMode::Sphere,
            ..BubbleStyle::default()
        };
        let colors = BubbleColors::resolve(&palette, &flat_style);

        let flat = painted(|painter| draw_bubble(painter, mark, &flat_style, &colors));
        let sphere = painted(|painter| draw_bubble(painter, mark, &sphere_style, &colors));
        assert_ne!(flat, sphere, "the mode must change what is painted");
        assert!(
            sphere.contains("Mesh"),
            "sphere mode paints a vertex-shaded mesh: {sphere}"
        );

        // Below the detail floor both modes paint the same cheap dot, keeping
        // the tessellation budget flat on a fast tape.
        let dot = BubbleMark {
            radius: flat_style.detail_min_radius - 1.0,
            ..mark
        };
        let flat_dot = painted(|painter| draw_bubble(painter, dot, &flat_style, &colors));
        let sphere_dot = painted(|painter| draw_bubble(painter, dot, &sphere_style, &colors));
        assert_eq!(flat_dot, sphere_dot);
    }

    #[test]
    fn the_preview_draws_a_sphere_bubble_exactly_the_way_the_chart_does() {
        // Same contract as the flat parity test: the preview must not render
        // its own approximation of the sphere look.
        let bubbles = BubbleStyle {
            render_mode: BubbleRenderMode::Sphere,
            trail_length: 0.0,
            ..BubbleStyle::default()
        };
        let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
        let at = egui::pos2(120.0, 80.0);
        let radius = bubble_radius(
            PREVIEW_LARGE_PRINT_SIZE,
            bubbles.min_radius,
            bubbles.max_radius,
        );

        let live = painted(|painter| {
            draw_bubble(
                painter,
                BubbleMark {
                    center: at + egui::vec2(0.0, side_offset_y(Side::Buy, bubbles.side_offset)),
                    radius,
                    side: Side::Buy,
                    size: PREVIEW_LARGE_PRINT_SIZE,
                    matched: Some(PREVIEW_MATCHED_FRACTION),
                    buy_share: 1.0,
                },
                &bubbles,
                &colors,
            );
        });
        let preview = painted(|painter| {
            draw_preview_bubble(
                painter,
                PreviewBubble {
                    center: at,
                    size: PREVIEW_LARGE_PRINT_SIZE,
                    side: Side::Buy,
                    linked_reduction: true,
                    buy_share: 1.0,
                },
                f32::INFINITY,
                &bubbles,
                &colors,
            );
        });
        assert!(
            live.contains("Mesh"),
            "the sample must shade a sphere: {live}"
        );
        assert_eq!(live, preview);
    }

    #[test]
    fn hollow_small_buys_opens_the_dot_and_leaves_dressed_bubbles_alone() {
        let hollow = BubbleStyle {
            trail_length: 0.0,
            ..BubbleStyle::default()
        };
        assert!(hollow.hollow_small_buys);
        let solid = BubbleStyle {
            hollow_small_buys: false,
            ..hollow.clone()
        };
        let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &hollow);
        let mark = |radius| BubbleMark {
            center: egui::pos2(40.0, 40.0),
            radius,
            side: Side::Buy,
            size: 0.05,
            matched: None,
            buy_share: 1.0,
        };

        // Below the readability floor — where colour alone stops working —
        // the setting must change what is painted.
        let small = hollow.min_radius;
        assert!(small < hollow.readable_min_radius);
        assert_ne!(
            painted(|painter| draw_bubble(painter, mark(small), &hollow, &colors)),
            painted(|painter| draw_bubble(painter, mark(small), &solid, &colors)),
            "a buy below the floor must not paint the same with the ring off"
        );

        // Above it the setting must change nothing: at that size the fill and
        // its sphere shading already say which side the bubble is.
        let big = hollow.readable_min_radius + 1.0;
        assert_eq!(
            painted(|painter| draw_bubble(painter, mark(big), &hollow, &colors)),
            painted(|painter| draw_bubble(painter, mark(big), &solid, &colors)),
            "a buy above the floor must be untouched by the ring setting"
        );
    }

    /// The legend is a key for what is on screen: exactly one entry per layer
    /// that is both active as a family and switched on individually.
    #[test]
    fn the_legend_lists_only_the_layers_that_are_on() {
        let labels = |style: &OrderflowRenderStyle| -> Vec<String> {
            legend_entries(style, "liquidity".to_owned())
                .into_iter()
                .map(|(_, label)| label)
                .collect()
        };

        let all = OrderflowRenderStyle::default();
        assert_eq!(
            labels(&all),
            [
                "liquidity",
                "buy aggression",
                "sell aggression",
                "aggression-aligned depletion",
                "L2 reduction (unattributed)",
                "L2 gap",
            ]
        );

        let mut some = all.clone();
        some.show_liquidity = false;
        some.show_sell = false;
        some.show_unattributed = false;
        assert_eq!(
            labels(&some),
            ["buy aggression", "aggression-aligned depletion", "L2 gap"]
        );

        // Family switches still trump the per-layer ones: without L2 capture
        // no depth entry may appear, whatever its individual flag says.
        let mut bubbles_only = all.clone();
        bubbles_only.depth_layer = false;
        assert_eq!(labels(&bubbles_only), ["buy aggression", "sell aggression"]);

        let mut nothing = all;
        nothing.depth_layer = false;
        nothing.aggression_layer = false;
        assert!(labels(&nothing).is_empty());
    }

    /// A two-sided bubble is a pie: both side colours on one mark, and the
    /// proportion is what the sectors carry. Where a pie cannot be read the
    /// mark falls back to exactly the dot it has always been.
    #[test]
    fn a_two_sided_bubble_draws_both_sides_and_a_small_one_falls_back() {
        let bubbles = BubbleStyle {
            min_radius: 2.0,
            max_radius: 20.0,
            detail_min_radius: 6.0,
            hollow_small_buys: false,
            render_mode: BubbleRenderMode::Flat,
            ..BubbleStyle::default()
        };
        let colors = BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &bubbles);
        let mark = |radius: f32, buy_share: f32| BubbleMark {
            center: egui::pos2(60.0, 60.0),
            radius,
            side: Side::Buy,
            size: 0.6,
            matched: None,
            buy_share,
        };
        // Compared after the fill alpha, which is what actually lands in the
        // mesh vertices.
        let ink = |color: egui::Color32| format!("{:?}", color.gamma_multiply(bubbles.opacity));

        // A dressed pie paints both colours into one mesh.
        let pie = painted(|painter| draw_bubble(painter, mark(12.0, 0.4), &bubbles, &colors));
        assert!(pie.contains("Mesh"), "a pie is a mesh: {pie}");
        assert!(
            pie.contains(&ink(colors.buy)) && pie.contains(&ink(colors.sell)),
            "both sides must be inked: {pie}"
        );

        // A single-sided bubble is untouched by the pie path: it still takes
        // the cheap flat circle it always did.
        let solid = painted(|painter| draw_bubble(painter, mark(12.0, 1.0), &bubbles, &colors));
        assert!(
            !solid.contains("Mesh"),
            "a plain bubble stays flat: {solid}"
        );
        assert!(!solid.contains(&ink(colors.sell)));

        // Below the dressing radius the pie is unreadable, so the mark returns
        // to one dot in the dominant side's colour.
        let dot = painted(|painter| {
            draw_bubble(
                painter,
                mark(bubbles.detail_min_radius - 1.0, 0.4),
                &bubbles,
                &colors,
            )
        });
        assert_eq!(
            dot.matches("CircleShape").count(),
            1,
            "a mixed dot must stay one circle: {dot}"
        );
        assert!(!dot.contains(&ink(colors.sell)));
    }

    /// The presets the user actually runs push the dressing radius *below* the
    /// minimum to buy sphere shading on small bubbles, which makes every mark
    /// "dressed". The pie must not ride in on that: it needs the dedicated
    /// readability floor too, or a summarized bar turns into a rash of
    /// two-tone specks nobody can read a proportion from.
    #[test]
    fn a_pie_needs_the_readability_floor_on_the_shipped_presets() {
        // "dense tape btc" — the project's default open — as shipped.
        let dense_tape_btc = BubbleStyle {
            min_radius: 2.2,
            max_radius: 14.0,
            detail_min_radius: 2.0,
            readable_min_radius: crate::orderflow::config::DEFAULT_READABLE_MIN_RADIUS,
            hollow_small_buys: true,
            render_mode: BubbleRenderMode::Sphere,
            ..BubbleStyle::default()
        };
        assert!(dense_tape_btc.detail_min_radius < dense_tape_btc.min_radius);
        let colors =
            BubbleColors::resolve(&Palette::for_theme(HeatmapTheme::Bookmap), &dense_tape_btc);
        let mark = |radius: f32| BubbleMark {
            center: egui::pos2(60.0, 60.0),
            radius,
            side: Side::Buy,
            size: 0.5,
            matched: None,
            buy_share: 0.5,
        };
        let sell_ink = format!("{:?}", colors.sell.gamma_multiply(dense_tape_btc.opacity));

        // At the smallest drawn radius every bubble is "dressed" here, and the
        // mark must still be one speck of one colour.
        let speck = painted(|painter| {
            draw_bubble(
                painter,
                mark(dense_tape_btc.min_radius),
                &dense_tape_btc,
                &colors,
            )
        });
        assert!(
            !speck.contains(&sell_ink),
            "a speck must not try to be a pie: {speck}"
        );

        // Past the readability floor the proportion is worth drawing.
        let readable = painted(|painter| {
            draw_bubble(
                painter,
                mark(dense_tape_btc.readable_min_radius + 1.0),
                &dense_tape_btc,
                &colors,
            )
        });
        assert!(readable.contains(&sell_ink), "a readable pie: {readable}");
    }

    /// The lane's radius multiplier has to survive all the way to the circle
    /// that gets drawn, and it must touch nothing outside the lane.
    #[test]
    fn the_lane_scale_reaches_the_bubbles_and_stops_at_the_boundary() {
        let viewport = Viewport::new();
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
        let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 6.0);
        let style = OrderflowRenderStyle {
            bubbles: BubbleStyle {
                min_radius: 2.0,
                max_radius: 10.0,
                detail_min_radius: 100.0, // cheap dots only: one circle each
                hollow_small_buys: false,
                halo_strength: 0.0,
                trail_length: 0.0,
                show_quantity_labels: false,
                show_trade_count: false,
                ..BubbleStyle::default()
            },
            live_lane: LiveLaneStyle {
                radius_scale: 2.0,
                ..LiveLaneStyle::default()
            },
            ..OrderflowRenderStyle::default()
        };

        let radii = |live: bool| {
            let mut projection = HeatmapProjection::empty(
                true,
                crate::orderflow::EffectiveGrouping::resolve(
                    crate::orderflow::DisplayGrouping::Native,
                    rust_decimal::Decimal::ONE,
                    rust_decimal::Decimal::from(100),
                ),
            );
            projection.aggressions.push(AggressionPrimitive {
                agg_id: 1,
                agg_ids: vec![1],
                generation: None,
                side: Side::Buy,
                consumed_side: BookSide::Ask,
                quantity: rust_decimal::Decimal::ONE,
                buy_share: 1.0,
                live,
                price_bucket: rust_decimal::Decimal::ONE,
                trade_count: 1,
                first_timestamp_ms: 0,
                last_timestamp_ms: 0,
                matched_quantity: rust_decimal::Decimal::ZERO,
                matched_fraction: 0.0,
                liquidity_event_ids: Vec::new(),
                x: 0.5,
                y: 0.5,
                size: 1.0,
            });
            painted(|painter| {
                draw_aggression_bubbles(painter, &RenderContext::new(&projection, layout, &style));
            })
        };

        // At full size the radius is the configured maximum, doubled inside
        // the lane and untouched outside it.
        let history = radii(false);
        let lane = radii(true);
        assert!(
            history.contains("radius: 10.0"),
            "history radius: {history}"
        );
        assert!(lane.contains("radius: 20.0"), "lane radius: {lane}");
    }

    /// The two sides never both get the side nudge: an even split sits on the
    /// exact price, and the lean grows continuously from there.
    #[test]
    fn a_pie_leans_with_its_buy_share() {
        let offset = 4.0;
        let lean = |buy_share: f32| -((finite_unit(buy_share) - 0.5) * 2.0) * offset;
        assert_eq!(lean(1.0), side_offset_y(Side::Buy, offset));
        assert_eq!(lean(0.0), side_offset_y(Side::Sell, offset));
        assert_eq!(lean(0.5), 0.0);
        assert!(lean(0.75) < 0.0 && lean(0.75) > lean(1.0));
    }

    /// The divider sits between the forming candle and the lane, on the one
    /// stretch of chart no timestamp maps into. Without a lane there is
    /// nothing to divide.
    #[test]
    fn the_lane_boundary_lands_past_the_forming_candle() {
        let viewport = Viewport::new(); // candle_width 8, following
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
        let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 4.0);
        let boundary = layout.lane_left_x().expect("a widened slot has a lane");
        // Everything the timeline can address is on one side of it or the
        // other: the forming candle's own slot ends there, the lane starts.
        let candle_start = layout.x(2.0 / 3.0);
        assert!(candle_start < boundary);
        assert!((boundary - candle_start - viewport.candle_width()).abs() < 0.01);
        assert!(layout.x(2.0 / 3.0 + 1e-4) >= boundary);

        let flat = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 1.0);
        assert_eq!(flat.lane_left_x(), None);
    }

    /// The lane never draws marks it cannot place, and hiding them is exactly
    /// one switch away.
    #[test]
    fn the_lane_marks_need_a_lane_a_live_edge_and_permission() {
        let viewport = Viewport::new();
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
        let style = OrderflowRenderStyle::default();
        let mut projection = HeatmapProjection::empty(
            true,
            crate::orderflow::EffectiveGrouping::resolve(
                crate::orderflow::DisplayGrouping::Native,
                rust_decimal::Decimal::ONE,
                rust_decimal::Decimal::from(100),
            ),
        );
        projection.live_now_x = Some(0.95);

        let with_lane = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 6.0);
        let drawn = painted(|painter| {
            draw_live_lane_marks(painter, &RenderContext::new(&projection, with_lane, &style));
        });
        assert!(
            drawn.matches("LineSegment").count() >= 2,
            "both the boundary and the live-time line must draw: {drawn}"
        );

        // No live edge: the frame is history, and history has no present.
        let mut settled = projection.clone();
        settled.live_now_x = None;
        // No lane reserved: nothing to divide.
        let no_lane = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 1.0);
        // Switched off by the user.
        let hidden = OrderflowRenderStyle {
            live_lane: LiveLaneStyle {
                show_marks: false,
                ..LiveLaneStyle::default()
            },
            ..OrderflowRenderStyle::default()
        };
        let nothing = painted(|_| {});
        for (frame, layout, style) in [
            (&settled, with_lane, &style),
            (&projection, no_lane, &style),
            (&projection, with_lane, &hidden),
        ] {
            assert_eq!(
                painted(|painter| draw_live_lane_marks(
                    painter,
                    &RenderContext::new(frame, layout, style)
                )),
                nothing
            );
        }
    }

    #[test]
    fn live_span_widens_only_the_forming_slot() {
        let viewport = Viewport::new(); // candle_width 8, following
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 100.0));
        // 3 slots: 2 closed + the forming bar, widened 4x.
        let layout = ProjectedLayout::new(rect, &viewport, 3, 0, 3, 4.0);
        let closed_w = (layout.x(1.0 / 3.0) - layout.x(0.0)).abs();
        let live_w = (layout.x(1.0) - layout.x(2.0 / 3.0)).abs();
        assert!(closed_w > 0.0);
        assert!(
            (live_w - closed_w * 4.0).abs() < 0.01,
            "forming slot should be 4x a closed slot: closed={closed_w} live={live_w}"
        );
    }
}
