//! Painting the aggression layer: one bubble per clustered print, its
//! dressing (halo, rim, sphere shading, consumption crown or front, impact
//! ring, fold ring) and the label inside it.
//!
//! [`draw_bubble`] is the one door both the live chart and the settings
//! preview draw through, which is what keeps the preview honest. Every
//! constant that shapes a bubble lives beside the code that reads it.

use eframe::egui;
use quantick_engine::Side;
use quantick_orderflow::{
    AggressionPrimitive, BubbleRenderMode, BubbleStyle, ConsumptionMark, GOLDEN_ANGLE, INV_PHI,
    INV_PHI_2, INV_PHI_3,
};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::layout::RenderContext;
use super::{Palette, add_gradient_rect, finite_unit, mix_rgb};

/// Bubble colours after the panel's overrides are laid over the theme.
///
/// Resolved per draw call and never written back into [`Palette`]: the same
/// theme colours keep driving the liquidity-response layer, which the bubble
/// panel does not own.
#[derive(Debug, Clone, Copy)]
pub(super) struct BubbleColors {
    pub(super) buy: egui::Color32,
    pub(super) sell: egui::Color32,
    front: egui::Color32,
    pub(super) trail: egui::Color32,
    text: egui::Color32,
    /// The crown's colour per side. Derived from the side colour rather than
    /// from [`front`](Self::front), so consumption reads as the same event
    /// hotter instead of introducing a third hue — unless the panel explicitly
    /// overrode the consumption colour, which stays the one door to change it.
    crown_buy: egui::Color32,
    crown_sell: egui::Color32,
}

impl BubbleColors {
    pub(super) fn resolve(palette: &Palette, bubbles: &BubbleStyle) -> Self {
        let front = bubbles.front_color.map_or(palette.consumption, opaque_rgb);
        let buy = bubbles.buy_color.map_or(palette.buy, opaque_rgb);
        let sell = bubbles.sell_color.map_or(palette.sell, opaque_rgb);
        let crown_of = |side: egui::Color32| match bubbles.front_color {
            Some(rgb) => opaque_rgb(rgb),
            None => opaque_rgb(mix_rgb(
                [side.r(), side.g(), side.b()],
                [255, 255, 255],
                CROWN_WHITE_MIX,
            )),
        };
        Self {
            buy,
            sell,
            front,
            // The trail is the front's own glow, so it follows it by default.
            trail: bubbles.trail_color.map_or(front, opaque_rgb),
            text: bubbles.label_color.map_or(palette.bubble_text, opaque_rgb),
            crown_buy: crown_of(buy),
            crown_sell: crown_of(sell),
        }
    }

    const fn for_side(self, side: Side) -> egui::Color32 {
        match side {
            Side::Buy => self.buy,
            Side::Sell => self.sell,
        }
    }

    pub(super) const fn crown_for_side(self, side: Side) -> egui::Color32 {
        match side {
            Side::Buy => self.crown_buy,
            Side::Sell => self.crown_sell,
        }
    }
}

fn opaque_rgb(rgb: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Vertical nudge, in pixels, that keeps the two sides off the same row.
///
/// Buy aggression lifts the ask, sell aggression hits the bid, so buys sit
/// on the ask's side of the print and sells on the bid's. That is a *price*
/// direction: `inverted` mirrors the nudge with the chart, or the separation
/// would assert the opposite book side upside down. Screen y grows downward.
pub(super) const fn side_offset_y(side: Side, offset: f32, inverted: bool) -> f32 {
    let toward_ask = match side {
        Side::Buy => -offset,
        Side::Sell => offset,
    };
    if inverted { -toward_ask } else { toward_ask }
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

/// Width of the dark separator hair drawn just outside a bubble's rim, as a
/// fraction of the radius, and the pixel range it is held to.
///
/// The heat ramp passes through greens the buy side almost matches, so without
/// a dark hair between them "aggression" and "liquidity" melt into one layer
/// wherever a bubble sits on warm heat — the hair is what keeps them two.
///
/// Proportional rather than fixed: at a flat 1.2px the hair was over half the
/// radius of a routine 2px print and under a tenth of a full sweep's, so small
/// prints wore a heavy black collar and large ones a thread. One ratio makes
/// every bubble the same drawing at a different size.
const SEPARATOR_RING_SCALE: f32 = 0.14;

/// See [`SEPARATOR_RING_SCALE`].
const SEPARATOR_MIN_RING_PX: f32 = 0.5;

/// See [`SEPARATOR_RING_SCALE`].
const SEPARATOR_MAX_RING_PX: f32 = 1.5;

/// Alpha of that hair: translucent black, so it darkens whatever heat is
/// behind it instead of assuming one canvas colour.
const SEPARATOR_RING_ALPHA: u8 = 170;

/// Separator-hair width for a bubble of this radius.
fn separator_ring_width(radius: f32) -> f32 {
    (radius * SEPARATOR_RING_SCALE).clamp(SEPARATOR_MIN_RING_PX, SEPARATOR_MAX_RING_PX)
}

/// Gap between a bubble's rim and the halo behind it, as a fraction of the
/// radius, and the pixel range it is held to.
const HALO_PADDING_SCALE: f32 = 0.2;

/// See [`HALO_PADDING_SCALE`].
const HALO_MIN_PADDING_PX: f32 = 2.0;

/// See [`HALO_PADDING_SCALE`].
const HALO_MAX_PADDING_PX: f32 = 5.0;

/// Halo gap for a bubble of this radius.
fn halo_padding(radius: f32) -> f32 {
    (radius * HALO_PADDING_SCALE).clamp(HALO_MIN_PADDING_PX, HALO_MAX_PADDING_PX)
}

/// Gap between a bubble's rim and its impact ring, as a fraction of the
/// radius, and the pixel range it is held to.
const IMPACT_RING_PADDING_SCALE: f32 = 0.16;

/// See [`IMPACT_RING_PADDING_SCALE`].
const IMPACT_RING_MIN_PADDING_PX: f32 = 1.6;

/// See [`IMPACT_RING_PADDING_SCALE`].
const IMPACT_RING_MAX_PADDING_PX: f32 = 3.5;

/// Impact-ring gap for a bubble of this radius.
fn impact_ring_padding(radius: f32) -> f32 {
    (radius * IMPACT_RING_PADDING_SCALE)
        .clamp(IMPACT_RING_MIN_PADDING_PX, IMPACT_RING_MAX_PADDING_PX)
}

/// Pixels added beyond `front_length_scale × radius`, so the consumption mark
/// on even the smallest bubble is long enough to read as a mark.
pub(super) const FRONT_END_PADDING_PX: f32 = 6.0;

/// How far the halo opens up at full print size, as a fraction of
/// `halo_strength`: a sweep reads heavier than a routine print of the same
/// colour, without needing a second colour for it.
const HALO_SIZE_BOOST: f32 = 0.5;

/// Rim alpha relative to the fill. A hair below opaque keeps the rim reading
/// as the bubble's edge rather than as a separate ring on a dark canvas.
const RIM_ALPHA: f32 = 0.96;

/// Impact-ring alpha every consuming print gets, before the matched share.
pub(super) const IMPACT_RING_BASE_ALPHA: f32 = 0.75;

/// Share of the impact ring's alpha that tracks how much of the print actually
/// matched resting liquidity, so a full sweep rings brighter than a nibble.
const IMPACT_RING_MATCH_ALPHA: f32 = 0.25;

/// Matched-fraction floor for the consumption marks: a barely matched print
/// still ate something, so it still leaves a visible mark.
const MIN_MATCH_STRENGTH: f32 = 0.25;

/// Fraction of the radius the sphere's lit core is offset toward the upper
/// left. One fixed light direction keeps every bubble shaded identically, so
/// the eye reads the gradient as volume instead of as data.
pub(super) const SPHERE_LIGHT_OFFSET: f32 = 0.35;

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
pub(super) fn sphere_segments(radius: f32) -> usize {
    ((radius * SPHERE_SEGMENTS_PER_RADIUS_PX) as usize)
        .clamp(SPHERE_MIN_SEGMENTS, SPHERE_MAX_SEGMENTS)
}

/// Gap between the rim and the consumption crown, as a fraction of the radius,
/// and the pixel range it is held to. `1/φ³` — the innermost step of the
/// nested `1/φ³` gap + `1/φ²` stroke that lands the whole crown apparatus at
/// about `r/φ²` beyond the rim on a full-size print.
const CROWN_GAP_SCALE: f32 = INV_PHI_3;

/// See [`CROWN_GAP_SCALE`].
const CROWN_MIN_GAP_PX: f32 = 1.4;

/// See [`CROWN_GAP_SCALE`].
const CROWN_MAX_GAP_PX: f32 = 3.0;

/// Stroke width of the crown as a fraction of the radius, and the pixel range
/// it is held to. `1/φ²`.
const CROWN_WIDTH_SCALE: f32 = INV_PHI_2;

/// See [`CROWN_WIDTH_SCALE`].
const CROWN_MIN_WIDTH_PX: f32 = 1.0;

/// See [`CROWN_WIDTH_SCALE`].
const CROWN_MAX_WIDTH_PX: f32 = 2.4;

/// Arc length, in pixels, below which an arc has stopped reading as an arc.
/// Under it the crown collapses to a pip at the pole — a print small enough to
/// be a speck still gets to say it ate something, for about four pixels of ink.
const CROWN_MIN_ARC_PX: f32 = 4.0;

/// Radius of that pip, in pixels.
const CROWN_PIP_RADIUS_PX: f32 = 1.2;

/// How far the crown's colour is pushed from its side colour toward white.
///
/// `1/φ`. Consumption is the same event, hotter — deriving the crown from the
/// side keeps a third hue off the canvas, and means N stacked crowns saturate
/// toward their own green or red instead of toward glare or toward mud.
const CROWN_WHITE_MIX: f32 = INV_PHI;

/// Crown alpha before the matched share.
const CROWN_BASE_ALPHA: f32 = 0.62;

/// Share of the crown's alpha that tracks the matched fraction. Secondary to
/// arc length, which is the channel actually carrying the reading.
const CROWN_MATCH_ALPHA: f32 = 0.38;

/// Extra width of the dark stroke laid under the crown, so the arc survives
/// over a bright heat band without assuming one canvas colour. A hairline each
/// side, never a mass.
const CROWN_BACKING_PX: f32 = 1.0;

/// See [`CROWN_BACKING_PX`].
const CROWN_BACKING_ALPHA: u8 = 110;

/// The pole a crown is centred on.
///
/// Buy aggression lifts the ask, so its crown sits above the print; sell
/// aggression hits the bid and wears it below. Screen y grows downward. This
/// is the same fact [`side_offset_y`] encodes, deliberately restated: on a
/// dense tape the two reinforce each other rather than compete.
const fn crown_center_angle(side: Side) -> f32 {
    match side {
        Side::Buy => -std::f32::consts::FRAC_PI_2,
        Side::Sell => std::f32::consts::FRAC_PI_2,
    }
}

/// The crown's geometry for a bubble of this radius and matched share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CrownGeometry {
    /// Radius of the arc itself — outside the rim, never on it.
    pub(super) arc_radius: f32,
    /// Stroke width.
    pub(super) width: f32,
    /// Angular length, in radians. Never exceeds the [`GOLDEN_ANGLE`], so the
    /// crown cannot close into a second circle around the bubble.
    pub(super) sweep: f32,
}

impl CrownGeometry {
    /// Length of the arc in pixels — what decides whether it can be drawn as
    /// an arc at all.
    pub(super) fn arc_length(self) -> f32 {
        self.arc_radius * self.sweep
    }
}

/// Crown geometry for a print of this radius that matched this fraction of
/// resting liquidity.
pub(super) fn crown_geometry(radius: f32, matched: f32) -> CrownGeometry {
    let gap = (radius * CROWN_GAP_SCALE).clamp(CROWN_MIN_GAP_PX, CROWN_MAX_GAP_PX);
    CrownGeometry {
        arc_radius: radius + gap,
        width: (radius * CROWN_WIDTH_SCALE).clamp(CROWN_MIN_WIDTH_PX, CROWN_MAX_WIDTH_PX),
        // A `1/φ²` floor plus a `1/φ` span: a nibble still shows a mark, a
        // full sweep reaches the golden angle and no further.
        sweep: GOLDEN_ANGLE * (INV_PHI_2 + INV_PHI * finite_unit(matched)),
    }
}

/// The crown's colour for a print that matched this fraction.
fn crown_alpha(matched: f32) -> f32 {
    CROWN_BASE_ALPHA + CROWN_MATCH_ALPHA * finite_unit(matched)
}

/// Draw the consumption crown: an open arc outside the rim, on the side of the
/// book the print ate, whose length carries how much of it matched.
///
/// Additive rather than subtractive — the arc is the side colour pushed toward
/// white — because the canvas is nearly black and the layer underneath is the
/// only one carrying liquidity. A dark mark here is invisible over the canvas
/// and a hole punched in the book everywhere else, which is what the vertical
/// front it replaces had become.
fn draw_crown(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    side: Side,
    matched: f32,
    color: egui::Color32,
) {
    if !center.is_finite() || !radius.is_finite() || radius <= 0.0 {
        return;
    }
    let geometry = crown_geometry(radius, matched);
    let color = color.gamma_multiply(crown_alpha(matched));
    let pole = crown_center_angle(side);
    if geometry.arc_length() < CROWN_MIN_ARC_PX {
        let direction = egui::vec2(pole.cos(), pole.sin());
        painter.circle_filled(
            center + direction * geometry.arc_radius,
            CROWN_PIP_RADIUS_PX,
            color,
        );
        return;
    }
    // The arc is a slice of a circle of this radius, so it inherits the same
    // segment budget and spends only its own share of it.
    let full = sphere_segments(geometry.arc_radius);
    let segments =
        (((full as f32) * (geometry.sweep / std::f32::consts::TAU)).ceil() as usize).max(3);
    let start = pole - geometry.sweep / 2.0;
    let points: Vec<egui::Pos2> = (0..=segments)
        .map(|index| {
            let angle = start + geometry.sweep * (index as f32 / segments as f32);
            center + egui::vec2(angle.cos(), angle.sin()) * geometry.arc_radius
        })
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(
            geometry.width + CROWN_BACKING_PX,
            egui::Color32::from_black_alpha(CROWN_BACKING_ALPHA),
        ),
    ));
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(geometry.width, color),
    ));
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

/// Half-length, in pixels, of the vertical consumption front on a bubble of
/// this radius.
pub(super) fn front_half_length(radius: f32, bubbles: &BubbleStyle) -> f32 {
    radius * bubbles.front_length_scale + FRONT_END_PADDING_PX
}

/// Halo alpha for a print of this normalized size.
pub(super) fn halo_alpha(size: f32, bubbles: &BubbleStyle) -> f32 {
    (bubbles.halo_strength * (1.0 + HALO_SIZE_BOOST * finite_unit(size))).min(1.0)
}

/// Impact-ring alpha for a print that matched this fraction of resting
/// liquidity.
pub(super) fn impact_ring_alpha(matched_fraction: f32) -> f32 {
    IMPACT_RING_BASE_ALPHA
        + finite_unit(matched_fraction).max(MIN_MATCH_STRENGTH) * IMPACT_RING_MATCH_ALPHA
}

/// Half-height of the trail behind a bubble of this radius, and the pixel
/// range it is held to.
///
/// `1/φ` of the radius, so the trail stays strictly *smaller* than the bubble
/// it belongs to. It used to borrow the consumption front's half-length, which
/// carries a fixed 6px addition — on a routine 2px print that made the trail a
/// 17px-tall bar behind a 4px disc, and a chart whose signal is horizontal
/// bands does not need decorative horizontal bands eight times the ink of the
/// mark they decorate.
fn trail_half_height(radius: f32) -> f32 {
    (radius * INV_PHI).clamp(TRAIL_MIN_HALF_HEIGHT_PX, TRAIL_MAX_HALF_HEIGHT_PX)
}

/// See [`trail_half_height`].
const TRAIL_MIN_HALF_HEIGHT_PX: f32 = 1.5;

/// See [`trail_half_height`].
const TRAIL_MAX_HALF_HEIGHT_PX: f32 = 9.0;

/// The consumption trail leaking to the right of a bubble, stopped at
/// `right_edge` so it never paints past the chart.
pub(super) fn trail_rect(
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
pub(super) fn sphere_core_color(color: egui::Color32, highlight: f32) -> egui::Color32 {
    opaque_rgb(mix_rgb(
        [color.r(), color.g(), color.b()],
        [255, 255, 255],
        highlight,
    ))
}

/// The side colour pushed toward black for a sphere's rim.
pub(super) fn sphere_edge_color(color: egui::Color32, shading: f32) -> egui::Color32 {
    opaque_rgb(mix_rgb(
        [color.r(), color.g(), color.b()],
        [0, 0, 0],
        shading,
    ))
}

/// Angle a pie starts at: straight up. Screen y grows downward, so a positive
/// sweep from here runs clockwise, the direction a pie chart is read in.
pub(super) const PIE_START_ANGLE: f32 = -std::f32::consts::FRAC_PI_2;

/// Gap between a folded bubble's disc and the ring that marks it as a fold,
/// in points. Wide enough to read as a separate ring at dot size, narrow
/// enough that two neighbouring folds do not run into each other.
const FOLD_RING_GAP: f32 = 2.0;

/// Stroke width of that ring.
const FOLD_RING_WIDTH: f32 = 1.0;

/// Its alpha. Below the rim's, because a fold ring is a caveat about the mark
/// and not part of the mark: it has to be findable without competing with the
/// pressure the bubble is there to show.
const FOLD_RING_ALPHA: f32 = 0.55;

/// The three colours a shaded bubble interpolates between: lit core, side
/// colour, darkened rim.
#[derive(Debug, Clone, Copy)]
pub(super) struct SphereShading {
    pub(super) core: egui::Color32,
    pub(super) body: egui::Color32,
    pub(super) edge: egui::Color32,
}

impl SphereShading {
    /// One colour used three times, which flattens the gradient. This is how
    /// the flat render mode draws its pie without a second tessellator.
    pub(super) const fn flat(color: egui::Color32) -> Self {
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
pub(super) fn add_shaded_sector(
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
pub(super) struct BubbleMark {
    pub(super) center: egui::Pos2,
    pub(super) radius: f32,
    pub(super) side: Side,
    /// Normalized print size, which opens up the halo.
    pub(super) size: f32,
    /// Fraction of the print matched against resting liquidity, when it ate
    /// any. `None` draws no consumption marks.
    pub(super) matched: Option<f32>,
    /// `[0,1]` share of the quantity buyers took. Anything strictly between
    /// the ends is a bubble carrying both sides, drawn as a pie.
    pub(super) buy_share: f32,
    /// How many separate marks the frame's budget folded into this one; zero
    /// on a bubble that is what it looks like.
    pub(super) folded: u32,
}

/// Draw one bubble: halo, fill, rim and — when the print ate resting
/// liquidity — the vertical consumption front and the impact ring.
///
/// The live chart and the settings preview both draw through here, so the
/// preview cannot drift into showing something the chart does not draw. The
/// trail is the one mark left out: on the chart every trail is batched into a
/// single mesh behind all the bubbles, which is a draw-order decision rather
/// than a per-bubble one.
pub(super) fn draw_bubble(
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
        folded,
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
    // The halo says "this one is big", so it is gated on size rather than
    // merely scaled with it: the top rung of the φ ladder, `max / φ`. Under a
    // gate it is a handful of prints a minute; under none it was a fog beneath
    // every speck on the tape, which is the same ink spent to say nothing.
    let haloed = !hollow && dressed && radius >= bubbles.max_radius * INV_PHI;
    if haloed && bubbles.halo_strength > 0.0 {
        painter.circle_filled(
            center,
            radius + halo_padding(radius),
            color.gamma_multiply(halo_alpha(size, bubbles)),
        );
    }
    // The dark hair between the mark and the heat behind it. Skipped on the
    // cheap-dot path, which stays a single circle per print.
    if dressed || hollow {
        let hair = separator_ring_width(radius);
        painter.circle_stroke(
            center,
            radius + hair / 2.0,
            egui::Stroke::new(hair, egui::Color32::from_black_alpha(SEPARATOR_RING_ALPHA)),
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

    // A fold is not a print, and must not read as one. The budget merges marks
    // rather than discarding them — nothing a trader needs is ever missing —
    // but a merged bubble carries a quantity that never crossed the tape at
    // once, and sizing a position off it as if it had is exactly the harm this
    // whole change exists to prevent. So a fold wears a ring, and says how many
    // marks are under it wherever there is room to say it.
    if folded > 1 {
        painter.circle_stroke(
            center,
            radius + FOLD_RING_GAP,
            egui::Stroke::new(
                FOLD_RING_WIDTH,
                color.gamma_multiply(FOLD_RING_ALPHA * bubbles.opacity),
            ),
        );
        // The count itself lives in the bubble's own label, where the eye
        // already is, as `⊕4` against a cluster's `×4` — see `bubble_label`.
        // Drawing it a second time here put two glyph runs on the same pixel,
        // because a fold's size saturates at the top of the radius range and so
        // its label always draws too.
    }

    // This print ate resting liquidity at this exact price.
    let Some(matched_fraction) = matched else {
        return;
    };
    match bubbles.consumption_mark {
        // The crown lives outside the rim, so it costs the disc nothing and
        // the smallest prints can still afford their pip.
        ConsumptionMark::Crown => draw_crown(
            painter,
            center,
            radius,
            side,
            matched_fraction,
            colors.crown_for_side(side),
        ),
        // The old vertical front, for a preset that asked for it by name. It
        // outgrows its own bubble by construction, so it stays behind the
        // dressing gate, where the bubble is at least big enough to carry it.
        ConsumptionMark::Front if dressed => {
            let half_length = front_half_length(radius, bubbles);
            painter.line_segment(
                [
                    egui::pos2(center.x, center.y - half_length),
                    egui::pos2(center.x, center.y + half_length),
                ],
                egui::Stroke::new(bubbles.front_width, colors.front),
            );
        }
        ConsumptionMark::Front | ConsumptionMark::None => {}
    }
    if dressed && bubbles.show_impact_ring {
        painter.circle_stroke(
            center,
            radius + impact_ring_padding(radius),
            egui::Stroke::new(
                bubbles.impact_ring_width,
                colors
                    .front
                    .gamma_multiply(impact_ring_alpha(matched_fraction)),
            ),
        );
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
    // Off on *both* panes, this pass has nothing to do: the frame still
    // carries every cluster (the strip reads them), so without this it would
    // clip, sanitize and walk up to `max_aggression_primitives` marks per
    // frame to draw none of them. One pane still drawing keeps the pass —
    // dropping out on the candles' switch alone would blank the tape with it.
    if !context.style.aggression_layer && !context.style.lane_aggression_layer {
        return;
    }
    let style = context.style.sanitized();
    let bubbles = &style.bubbles;
    let palette = Palette::for_theme(style.theme);
    let colors = BubbleColors::resolve(&palette, bubbles);
    let clip = painter.with_clip_rect(context.layout.chart_rect);

    // The side nudge generalizes to a bubble carrying both sides: it slides
    // continuously with the buy share, so an even split sits on the exact
    // price and a lopsided one leans the way its dominant side would.
    // A print is drawn only inside its own pane: one panned off the right of
    // the candles is out of sight, not on top of the tape.
    // The lean is toward the dominant side's book half — a price direction,
    // so it mirrors with the chart like side_offset_y does.
    let lean_sign = if context.layout.inverted { 1.0 } else { -1.0 };
    let center_of = |trade: &AggressionPrimitive| {
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        let lean = (finite_unit(trade.buy_share) - 0.5) * 2.0;
        context
            .layout
            .pane(trade.x)
            .contains(center)
            .then(|| center + egui::vec2(0.0, lean_sign * lean * bubbles.side_offset))
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

    // A bubble is a disc, not a rect, so its own pane has to clip it: keeping
    // the *centre* inside the pane still lets a fat radius (and its label)
    // spill across the divider, which is exactly the two charts drawing into
    // each other. One painter per pane, picked per print.
    let history_clip = painter.with_clip_rect(context.layout.history_rect());
    let lane_clip = if context.layout.lane_left_x().is_some() {
        painter.with_clip_rect(context.layout.lane_rect())
    } else {
        history_clip.clone()
    };
    let clip_for = |trade: &AggressionPrimitive| {
        if context.layout.in_lane(trade.x) {
            &lane_clip
        } else {
            &history_clip
        }
    };

    // Consumption trail behind the bubbles, so a bubble's own fill never hides it.
    if bubbles.trail_length > 0.0 {
        let mut trail_mesh = egui::Mesh::default();
        for trade in context.bubbles() {
            if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
                continue;
            }
            let Some(center) = center_of(trade) else {
                continue;
            };
            let pane = context.layout.pane(trade.x);
            let half_height = trail_half_height(radius_of(trade));
            add_gradient_rect(
                &mut trail_mesh,
                trail_rect(center, half_height, bubbles.trail_length, pane.right()).intersect(pane),
                colors.trail.gamma_multiply(bubbles.trail_opacity),
                egui::Color32::TRANSPARENT,
            );
        }
        if !trail_mesh.is_empty() {
            clip.add(egui::Shape::mesh(trail_mesh));
        }
    }

    for trade in context.bubbles() {
        let Some(center) = center_of(trade) else {
            continue;
        };
        let clip = clip_for(trade);
        let radius = radius_of(trade);
        let linked_reduction =
            trade.matched_fraction > 0.0 || !trade.liquidity_event_ids.is_empty();
        draw_bubble(
            clip,
            BubbleMark {
                center,
                radius,
                side: trade.side,
                size: trade.size,
                matched: linked_reduction.then_some(trade.matched_fraction),
                buy_share: trade.buy_share,
                folded: trade.folded_marks,
            },
            bubbles,
            &colors,
        );

        if radius >= bubbles.label_min_radius
            && let Some(label) = bubble_label(
                trade.quantity,
                trade.trade_count,
                trade.folded_marks,
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

pub(super) fn bubble_radius(size: f32, minimum: f32, maximum: f32) -> f32 {
    let minimum = minimum.max(0.0);
    let maximum = maximum.max(minimum);
    let normalized_quantity = finite_unit(size).powi(2);
    (minimum.powi(2) + normalized_quantity * (maximum.powi(2) - minimum.powi(2))).sqrt()
}

/// The text inside a bubble: what traded, and how many prints it stands for.
///
/// `folded` is what the frame's budget merged into this mark, and it changes
/// the *separator* rather than adding a second number. `×4` is a cluster — four
/// prints that happened together at one price, which is a fact about the
/// market. `⊕4` is a fold — four marks the frame put together to fit, which is
/// a fact about the canvas. A trader sizing a position off the first would be
/// right and off the second would be wrong, so they may not share a glyph. The
/// ring around a folded disc says the same thing again, further out, for the
/// dots too small to carry text.
pub(super) fn bubble_label(
    quantity: Decimal,
    trade_count: usize,
    folded: u32,
    show_quantity: bool,
    show_count: bool,
) -> Option<String> {
    let mark = if folded > 1 { '⊕' } else { '×' };
    match (show_quantity, show_count && trade_count > 1) {
        (false, false) => None,
        (true, false) => Some(format_quantity(quantity)),
        (false, true) => Some(format!("{mark}{trade_count}")),
        (true, true) => Some(format!(
            "{} · {mark}{trade_count}",
            format_quantity(quantity)
        )),
    }
}

pub(super) fn format_quantity(quantity: Decimal) -> String {
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
