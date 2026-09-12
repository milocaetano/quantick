//! Painting the depth layer: resting liquidity, the coverage gaps, the two
//! live-lane marks and the liquidity-reduction events.
//!
//! Everything here reads facts the projection already decided and turns
//! them into meshes; the per-frame cost is one clip rect and one mesh per
//! pass, whatever the book is doing.

use eframe::egui;
use quantick_orderbook::BookSide;
use quantick_orderflow::{BEFORE_CAPTURE, LiquidityEvidence};

use super::bubbles::{bubble_radius, side_offset_y};
use super::layout::{EventBand, RenderContext};
use super::{
    OrderflowRenderStyle, Palette, add_gradient_rect, draw_dashed_vertical, finite_unit,
    resting_rgb, rgba,
};

/// Draw resting liquidity and explicit L2 coverage gaps behind the chart.
pub(crate) fn draw_heatmap_background(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    let palette = Palette::for_theme(style.theme);
    // Each pane answers for its own canvas, and a run that crosses the divider
    // is cut at it rather than dropped.
    let Some(region) = context
        .layout
        .layer_clip(style.depth_layer, style.lane_depth_layer)
    else {
        return;
    };
    let clip = painter.with_clip_rect(region);
    let mut mesh = egui::Mesh::default();
    mesh.vertices
        .reserve(context.projection.cells.len().saturating_mul(8));
    mesh.indices
        .reserve(context.projection.cells.len().saturating_mul(12));

    for cell in context.projection.cells.iter() {
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
            .intersect(context.layout.span_pane(cell.x0, cell.x1));
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

    for gap in context.projection.gaps.iter() {
        // Same two questions the reductions answer, and for the same reason: a
        // coverage gap explains a hole in the *book*, so it belongs to the pane
        // that draws one, and `show_gaps` reached only the legend until now.
        let pane_draws_book = if context.layout.in_lane(gap.x0) {
            style.lane_depth_layer
        } else {
            style.depth_layer
        };
        if !pane_draws_book || !style.show_gaps {
            continue;
        }
        let x0 = context.layout.x(gap.x0);
        let x1 = context.layout.x(gap.x1);
        let rect = egui::Rect::from_min_max(
            egui::pos2(x0.min(x1), context.layout.chart_rect.top()),
            egui::pos2(x0.max(x1), context.layout.chart_rect.bottom()),
        )
        .intersect(context.layout.span_pane(gap.x0, gap.x1));
        if !rect.is_positive() {
            continue;
        }
        let leading = gap.precedes_capture();
        // Against its own pane, not the chart: a gap cut short by the divider
        // ends there because the pane does, and a boundary mark would claim the
        // coverage resumed at a moment it did not.
        let marks = gap_marks(rect, context.layout.span_pane(gap.x0, gap.x1), leading);
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
        // Two questions, both answered here because nowhere else asks.
        //
        // Does the pane this mark lands on still draw a book? A reduction is a
        // statement about the order book, so it goes when the book does — and
        // *per pane*, because the two switched apart: the candles' map off with
        // the tape's still on has to clear the candles and leave the tape, which
        // is exactly the state the report came from.
        //
        // And is this kind switched on? The projection filters these events by
        // *threshold* and never by the trader's choice — deliberately, since
        // both kinds are factual and the retained history stays complete — so
        // with nothing checking here, unticking "L2 reduction (unattributed)"
        // dropped the legend entry and left every violet mark painting. The
        // legend said the layer was off while the trader looked straight at it.
        let on_tape = context.layout.in_lane(event.x);
        let pane_draws_book = if on_tape {
            style.lane_depth_layer
        } else {
            style.depth_layer
        };
        let kind_shown = match event.evidence {
            LiquidityEvidence::AggressionAligned => style.show_aligned,
            LiquidityEvidence::DepthOnly => style.show_unattributed,
        };
        if !pane_draws_book || !kind_shown {
            continue;
        }
        let band = context
            .layout
            .event_band(event.x, event.y0, event.y1, style.min_cell_height);
        if band.x < context.layout.chart_rect.left() - 1.0 || band.x > right_edge + 1.0 {
            continue;
        }
        // Every mark below reaches to the *right* of the level it happened at,
        // so each one stops at the edge of its own pane: a reduction beside the
        // divider must not bleed its hole and its tail across the tape.
        let pane = context.layout.pane(event.x);

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
                egui::pos2((band.x + hole_w).min(pane.right()), band.bottom),
            )
            .intersect(pane),
            style.canvas_background,
            style.canvas_background,
        );

        match event.evidence {
            LiquidityEvidence::AggressionAligned => fronts.push(EventFront::Aligned {
                band: front,
                matched: finite_unit(event.matched_fraction),
                full,
                pane,
            }),
            LiquidityEvidence::DepthOnly => fronts.push(EventFront::DepthOnly {
                band: front,
                reduction,
                full,
                pane,
            }),
        }
    }

    // Carve a gap around each consumption bubble so a re-stacked wall does not
    // slide through it: the eaten wall ends, the bubble marks the bite, and the
    // fresh wall only resumes to the bubble's right.
    for trade in context.bubbles() {
        if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
            continue;
        }
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        let pane = context.layout.pane(trade.x);
        if !pane.contains(center) {
            continue;
        }
        // Follow the bubble's own vertical nudge, so the carved gap stays
        // centred on the bubble that will be drawn over it.
        let center = center
            + egui::vec2(
                0.0,
                side_offset_y(
                    trade.side,
                    style.bubbles.side_offset,
                    context.layout.inverted,
                ),
            );
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
                egui::pos2((center.x + r + 4.0).min(pane.right()), center.y + r + 2.0),
            )
            .intersect(pane),
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
            pane,
        } = front
        {
            let tail = if *full { 22.0 } else { 10.0 + 10.0 * reduction };
            add_gradient_rect(
                &mut tail_mesh,
                egui::Rect::from_min_max(
                    egui::pos2(band.x, band.top),
                    egui::pos2((band.x + tail).min(pane.right()), band.bottom),
                )
                .intersect(*pane),
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
        pane: egui::Rect,
    ) {
        add_gradient_rect(
            mesh,
            egui::Rect::from_min_max(
                egui::pos2(x - width * 0.5, top),
                egui::pos2(x + width * 0.5, bottom),
            )
            .intersect(pane),
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
                pane,
            } => {
                let strength = matched.max(0.25);
                add_vline(
                    &mut front_mesh,
                    band.x,
                    band.top,
                    band.bottom,
                    if full { 2.0 } else { 1.3 },
                    palette.consumption.gamma_multiply(0.55 + 0.4 * strength),
                    pane,
                );
                if full {
                    // End caps read as "this band was fully taken here".
                    for y in [band.top, band.bottom] {
                        add_gradient_rect(
                            &mut front_mesh,
                            egui::Rect::from_min_max(
                                egui::pos2(band.x - 3.5, y - 0.75),
                                egui::pos2(band.x + 3.5, y + 0.75),
                            )
                            .intersect(pane),
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
                pane,
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
                    pane,
                );
            }
        }
    }
    if !front_mesh.is_empty() {
        clip.add(egui::Shape::mesh(front_mesh));
    }
}

#[derive(Debug, Clone, Copy)]
enum EventFront {
    Aligned {
        band: EventBand,
        matched: f32,
        full: bool,
        /// The pane this reduction happened in — the candles' or the tape's.
        /// Every mark it draws is clipped to it, caps included.
        pane: egui::Rect,
    },
    DepthOnly {
        band: EventBand,
        reduction: f32,
        full: bool,
        /// See [`EventFront::Aligned::pane`].
        pane: egui::Rect,
    },
}

pub(super) fn marker_band(band: EventBand, reduction: f32, full: bool) -> EventBand {
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

/// Pixel slack for deciding that a gap boundary coincides with the chart edge.
/// Gap bounds arrive as normalized floats scaled into screen space, so an
/// exact comparison would miss by a rounding bit and draw a stray frame line.
const GAP_EDGE_EPSILON: f32 = 0.5;

/// Gap between the leading span's label and the divider it annotates. Small
/// enough that the text reads as belonging to the line rather than floating.
const GAP_LABEL_INSET_PX: f32 = 6.0;

/// Which marks one coverage gap gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct GapMarks {
    pub(super) fill: bool,
    pub(super) left_boundary: bool,
    pub(super) right_boundary: bool,
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
pub(super) fn gap_marks(rect: egui::Rect, chart_rect: egui::Rect, leading: bool) -> GapMarks {
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

fn gap_label(reason: &str) -> &'static str {
    match reason {
        BEFORE_CAPTURE => "L2 unavailable before capture",
        "capture_disabled" => "L2 capture disabled",
        "sequence_gap" => "L2 sequence gap · resynchronizing",
        _ => "L2 continuity unavailable",
    }
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

/// Number of discrete magnitude bands the heatmap collapses intensity into.
/// Fewer bands read as flatter walls; more bands recover gradient but let the
/// book's per-update jitter fragment a band. Eight keeps walls crisp while
/// still separating quiet / medium / heavy liquidity.
const HEAT_LEVELS: f32 = 8.0;

fn quantize_heat(intensity: f32) -> f32 {
    ((intensity * HEAT_LEVELS).round() / HEAT_LEVELS).clamp(0.0, 1.0)
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
