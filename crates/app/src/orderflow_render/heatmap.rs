//! Painting the depth layer: resting liquidity, the coverage gaps, the two
//! live-lane marks and the liquidity-reduction events.
//!
//! Everything here reads facts the projection already decided and turns
//! them into meshes; the per-frame cost is one clip rect and one mesh per
//! pass, whatever the book is doing.

use crate::orderflow_render::constants::{
    GAP_HOVER_PX, LANE_DIVIDER_WIDTH_PX, LANE_MARK_WIDTH_PX, LANE_NOW_DASH_PX, LANE_NOW_GAP_PX,
};
use eframe::egui;
use quantick_orderflow::LiquidityEvidence;
use quantick_orderflow::config::theme::{heat_fill_parts, quantize_heat};

use super::bubbles::{bubble_radius, side_offset_y};
use super::layout::{EventBand, RenderContext};
use super::{
    OrderflowRenderStyle, Palette, add_gradient_rect, draw_dashed_vertical, finite_unit, rgba,
};
use quantick_orderflow::projection::{drawn_gaps, marker_band};

/// The lane depth caption's offset from the lane's left edge and chart top.
const DEPTH_CAPTION_OFFSET: egui::Vec2 = egui::vec2(8.0, 30.0);
/// The small print the depth caption and the gap labels share.
const SMALL_PRINT_FONT_PX: f32 = 10.0;

/// Draw resting liquidity and explicit L2 coverage gaps behind the chart.
pub(crate) fn draw_heatmap_background(painter: &egui::Painter, context: &RenderContext<'_>) {
    let appearance = crate::theme::get(painter.ctx());
    let style = context.style.sanitized();
    let palette = style.colors(super::premultiplied);
    let previews = quantick_orderflow::projection::forward_book_preview(
        &style.live_lane,
        context.current_book_live,
    );
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
        let rect =
            context
                .layout
                .heat_cell_rect(cell, style.min_cell_height, context.current_book_live);
        let in_lane = rect.is_positive();
        if !in_lane && !cell.current_book {
            continue;
        }

        let Some((rgb, alpha)) = heat_fill_parts(&style, cell.side, cell.intensity, cell.alpha)
        else {
            continue;
        };
        let fill = rgba(rgb, alpha);

        if in_lane && style.edge_glow > 0.0 {
            let spread = 0.55 + quantize_heat(finite_unit(cell.intensity)) * 0.85;
            let glow_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() - spread),
                egui::pos2(rect.right(), rect.bottom() + spread),
            )
            .intersect(context.layout.span_pane(cell.x0, cell.x1).union(rect));
            let glow = rgba(rgb, alpha * style.edge_glow);
            add_gradient_rect(&mut mesh, glow_rect, glow, glow);
        }
        // Solid fill (no horizontal gradient), so a short run is a clean block
        // rather than a bright-headed streak.
        if in_lane {
            add_gradient_rect(&mut mesh, rect, fill, fill);
        }
        if quantick_orderflow::projection::current_book_preview(cell, previews) {
            let preview = context.layout.current_book_preview_rect(cell.y0, cell.y1);
            if preview.is_positive() && style.depth_layer {
                add_gradient_rect(&mut mesh, preview, fill, fill);
            }
        }
    }

    if !mesh.is_empty() {
        clip.add(egui::Shape::mesh(mesh));
    }

    if style.lane_depth_layer
        && let Some(caption) = quantick_orderflow::projection::depth_caption(
            &context.projection.cells,
            style.depth_layer
                && previews
                && context
                    .layout
                    .current_book_preview_rect(0.0, 1.0)
                    .is_positive(),
        )
        && let Some(left) = context.layout.lane_left_x()
    {
        draw_text_with_shadow(
            &clip,
            egui::pos2(left, context.layout.chart_rect.top()) + DEPTH_CAPTION_OFFSET,
            egui::Align2::LEFT_TOP,
            caption,
            appearance.label_font(SMALL_PRINT_FONT_PX),
            palette.muted_text,
        );
    }

    for gap in drawn_gaps(context.projection, context.current_book_live) {
        // Same two questions the reductions answer, and for the same reason: a
        // coverage gap explains a hole in the *book*, so it belongs to the pane
        // that draws one, and `show_gaps` reached only the legend until now.
        let lead = context.layout.tape_x(|tape| tape.lead_px(gap.lead_span));
        let pane_draws_book = if lead.is_some() || context.layout.in_lane(gap.x0) {
            style.lane_depth_layer
        } else {
            style.depth_layer
        };
        if !pane_draws_book || !style.show_gaps || gap.lead_span.is_some() != lead.is_some() {
            continue;
        }
        let (x0, x1) = lead.unwrap_or((context.layout.x(gap.x0), context.layout.x(gap.x1)));
        let pane = lead.map_or(context.layout.span_pane(gap.x0, gap.x1), |_| {
            context.layout.lane_rect()
        });
        let rect = egui::Rect::from_min_max(
            egui::pos2(x0.min(x1), context.layout.chart_rect.top()),
            egui::pos2(x0.max(x1), context.layout.chart_rect.bottom()),
        )
        .intersect(pane);
        if !rect.is_positive() {
            continue;
        }
        let leading = gap.marks_boundary_only();
        // Against its own pane, not the chart: a gap cut short by the divider
        // ends there because the pane does, and a boundary mark would claim the
        // coverage resumed at a moment it did not.
        let marks = gap_marks(rect, pane, leading);
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
        // The marks carry no words; the reason reads beside the pointer over one.
        if let Some(at) = clip.ctx().pointer_hover_pos()
            && rect.contains(at)
            && (marks.fill || rect.right() - at.x <= GAP_HOVER_PX)
        {
            let label = quantick_orderflow::projection::gap_label(&gap.reason);
            let font = appearance.label_font(SMALL_PRINT_FONT_PX);
            let anchor = at - egui::vec2(GAP_HOVER_PX, GAP_HOVER_PX);
            let align = egui::Align2::RIGHT_BOTTOM;
            draw_text_with_shadow(&clip, anchor, align, label, font, palette.muted_text);
        }
    }
}

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
    let rect = context.layout.chart_rect;
    draw_lane_boundary(painter, rect, context.layout.lane_width_px, &style);
    // The divider is layout; only the moving time mark needs a market clock.
    let Some(now_x) = context.projection.live_now_x else {
        return;
    };
    if context.layout.lane_left_x().is_none() {
        return;
    }
    let palette = style.colors(super::premultiplied);
    let clip = painter.with_clip_rect(rect);
    let x = context.layout.x(now_x);
    if x.is_finite() && rect.x_range().contains(x) {
        draw_dashed_vertical(
            &clip,
            x,
            rect,
            LANE_NOW_DASH_PX,
            LANE_NOW_GAP_PX,
            palette.lane_now,
            LANE_MARK_WIDTH_PX,
        );
    }
}

/// Draw the reserved tape edge even before any depth, trade or projection arrives.
pub(crate) fn draw_lane_boundary(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    style: &super::OrderflowRenderStyle,
) {
    let palette = crate::theme::get(painter.ctx());
    if !style.live_lane.show_marks {
        return;
    }
    if let Some(x) = super::lane_divider_x(rect, lane_width) {
        painter.with_clip_rect(rect).vline(
            x,
            rect.y_range(),
            egui::Stroke::new(LANE_DIVIDER_WIDTH_PX, palette.border()),
        );
    }
}

/// Draw reductions after heat cells and before/around the candle layer.
///
/// `AggressionAligned` leaves a hole where an aggression met a resting wall;
/// the bubble that took it wears the consumption mark. `DepthOnly` draws a
/// calm violet front and fade, intentionally avoiding the word "cancel": depth
/// alone does not reveal why displayed liquidity decreased.
///
/// Three passes, painted in this order: the holes (every reduction's, then
/// the carve behind each consuming bubble), the depth-only tails, and the
/// depth-only fronts on top.
pub(crate) fn draw_liquidity_events(painter: &egui::Painter, context: &RenderContext<'_>) {
    let pass = EventPass::new(context);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let event_count = context.projection.liquidity_events.len();
    let mut hole_mesh = egui::Mesh::default();
    hole_mesh.vertices.reserve(event_count.saturating_mul(4));
    hole_mesh.indices.reserve(event_count.saturating_mul(6));
    let mut fronts = Vec::with_capacity(event_count);

    pass.carve_event_holes(&mut hole_mesh, &mut fronts);
    pass.carve_bubble_gaps(&mut hole_mesh);
    add_mesh(&clip, hole_mesh);

    // A calm violet ghost fading rightward = "the offer was pulled here": the
    // wall's band ends, the fade marks the pull, and the dark canvas after it
    // shows the level stayed empty. Drawn under the cap lines.
    let mut tail_mesh = egui::Mesh::default();
    for front in &fronts {
        front.add_tail(&pass.palette, &mut tail_mesh);
    }
    add_mesh(&clip, tail_mesh);

    // Fronts and caps as solid mesh quads: hundreds of stroked segments per
    // frame would pay stroke tessellation each; one mesh keeps the per-frame
    // cost flat during storms of reductions.
    let mut front_mesh = egui::Mesh::default();
    for front in fronts {
        front.add_front(&pass.palette, &mut front_mesh);
    }
    add_mesh(&clip, front_mesh);
}

fn add_mesh(clip: &egui::Painter, mesh: egui::Mesh) {
    if !mesh.is_empty() {
        clip.add(egui::Shape::mesh(mesh));
    }
}

/// What every reduction pass reads, resolved once per frame: the sanitized
/// style, its palette and the canvas it paints on.
struct EventPass<'c, 'a> {
    context: &'c RenderContext<'a>,
    style: OrderflowRenderStyle,
    palette: Palette,
}

impl<'c, 'a> EventPass<'c, 'a> {
    fn new(context: &'c RenderContext<'a>) -> Self {
        let style = context.style.sanitized();
        let palette = style.colors(super::premultiplied);
        Self {
            context,
            style,
            palette,
        }
    }

    /// The hole every shown reduction leaves, plus the front each one will
    /// paint later, in projection order.
    fn carve_event_holes(&self, hole_mesh: &mut egui::Mesh, fronts: &mut Vec<EventFront>) {
        let layout = &self.context.layout;
        let style = &self.style;
        let right_edge = layout.chart_rect.right();

        for event in &self.context.projection.liquidity_events {
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
            let on_tape = layout.in_lane(event.x);
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
            let band = layout.event_band(event.x, event.y0, event.y1, style.min_cell_height);
            if band.x < layout.chart_rect.left() - 1.0 || band.x > right_edge + 1.0 {
                continue;
            }
            // Every mark below reaches to the *right* of the level it happened at,
            // so each one stops at the edge of its own pane: a reduction beside the
            // divider must not bleed its hole and its tail across the tape.
            let pane = layout.pane(event.x);

            let reduction = finite_unit(event.fraction);
            let full = event.full_removal;
            let front = marker_band(band, reduction, full);

            // A dark hole across the band's full height marks where liquidity
            // dropped. On a busy book the level is re-stacked almost immediately;
            // without the hole the fresh wall abuts the old one and looks
            // continuous, hiding that it was consumed. The marker colour drawn on
            // the hole's left edge tells aggression-aligned from unattributed apart.
            let hole_w = if full { 14.0 } else { 6.0 + 8.0 * reduction };
            if !(on_tape && style.live_lane.depth_time_averaging) {
                add_gradient_rect(
                    hole_mesh,
                    egui::Rect::from_min_max(
                        egui::pos2(band.x, band.top),
                        egui::pos2((band.x + hole_w).min(pane.right()), band.bottom),
                    )
                    .intersect(pane),
                    super::premultiplied(style.canvas_background),
                    super::premultiplied(style.canvas_background),
                );
            }

            // An aligned bite paints no line on the book: at the top of a busy
            // book every print is one, and a front per print buried the map
            // under strokes. Its hole and the crown on the bubble that took it
            // already say where and who.
            if matches!(event.evidence, LiquidityEvidence::DepthOnly) {
                fronts.push(EventFront {
                    band: front,
                    reduction,
                    full,
                    pane,
                });
            }
        }
    }

    /// Carve a gap around each consumption bubble so a re-stacked wall does not
    /// slide through it: the eaten wall ends, the bubble marks the bite, and the
    /// fresh wall only resumes to the bubble's right.
    fn carve_bubble_gaps(&self, hole_mesh: &mut egui::Mesh) {
        let layout = &self.context.layout;
        let bubbles = &self.style.bubbles;
        let dots = self.context.projection.volume_dots;
        for trade in self.context.bubbles() {
            if layout.in_lane(trade.x) && self.style.live_lane.depth_time_averaging {
                continue;
            }
            if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
                continue;
            }
            let center = egui::pos2(layout.x(trade.x), layout.y_unclamped(trade.y));
            let pane = layout.pane(trade.x);
            if !pane.contains(center) {
                continue;
            }
            // Follow the bubble's own vertical nudge, so the carved gap stays
            // centred on the bubble that will be drawn over it.
            let center = center
                + egui::vec2(
                    0.0,
                    side_offset_y(trade.side, bubbles.side_offset_for(dots), layout.inverted),
                );
            let r = bubble_radius(trade.size, bubbles.min_radius, bubbles.max_radius);
            // Carve from the bubble's midriff rightward: the eaten wall still
            // touches the bubble's left half (the bubble reads as biting into
            // it), while re-stacked liquidity cannot slide through to the right.
            add_gradient_rect(
                hole_mesh,
                egui::Rect::from_min_max(
                    egui::pos2(center.x - r * 0.4, center.y - r - 2.0),
                    egui::pos2((center.x + r + 4.0).min(pane.right()), center.y + r + 2.0),
                )
                .intersect(pane),
                super::premultiplied(self.style.canvas_background),
                super::premultiplied(self.style.canvas_background),
            );
        }
    }
}

/// The line and fade an unattributed reduction paints on the book.
#[derive(Debug, Clone, Copy)]
struct EventFront {
    band: EventBand,
    reduction: f32,
    full: bool,
    /// The pane this reduction happened in — the candles' or the tape's.
    /// Every mark it draws is clipped to it.
    pane: egui::Rect,
}

impl EventFront {
    /// The violet fade a depth-only pull leaves.
    fn add_tail(&self, palette: &Palette, mesh: &mut egui::Mesh) {
        let Self {
            band,
            reduction,
            full,
            pane,
        } = *self;
        let tail = if full { 22.0 } else { 10.0 + 10.0 * reduction };
        add_gradient_rect(
            mesh,
            egui::Rect::from_min_max(
                egui::pos2(band.x, band.top),
                egui::pos2((band.x + tail).min(pane.right()), band.bottom),
            )
            .intersect(pane),
            palette
                .depth_only
                .gamma_multiply(if full { 0.38 } else { 0.16 + 0.18 * reduction }),
            egui::Color32::TRANSPARENT,
        );
    }

    /// The front line itself.
    fn add_front(self, palette: &Palette, mesh: &mut egui::Mesh) {
        add_vline(
            mesh,
            self.band,
            if self.full { 1.6 } else { 1.1 },
            palette.depth_only.gamma_multiply(if self.full {
                0.9
            } else {
                0.55 + 0.3 * self.reduction
            }),
            self.pane,
        );
    }
}

/// One front as a solid quad spanning the band at its `x`, clipped to its pane.
fn add_vline(
    mesh: &mut egui::Mesh,
    band: EventBand,
    width: f32,
    color: egui::Color32,
    pane: egui::Rect,
) {
    add_gradient_rect(
        mesh,
        egui::Rect::from_min_max(
            egui::pos2(band.x - width * 0.5, band.top),
            egui::pos2(band.x + width * 0.5, band.bottom),
        )
        .intersect(pane),
        color,
        color,
    );
}

pub(super) use quantick_orderflow::projection::GapMarks;

pub(super) fn gap_marks(rect: egui::Rect, chart_rect: egui::Rect, leading: bool) -> GapMarks {
    quantick_orderflow::projection::gap_marks_bounds(
        rect.left(),
        rect.right(),
        chart_rect.left(),
        chart_rect.right(),
        leading,
    )
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
        crate::theme::get(painter.ctx()).quantity_shadow(),
    );
    painter.text(anchor, align, text, font, color);
}
