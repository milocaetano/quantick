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

use crate::orderflow::{HeatmapConfig, HeatmapProjection, HeatmapTheme, LiquidityEvidence};
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
    pub(crate) bubble_min_radius: f32,
    pub(crate) bubble_max_radius: f32,
    pub(crate) bubble_opacity: f32,
    pub(crate) show_quantity_labels: bool,
    pub(crate) show_trade_count: bool,
    pub(crate) label_min_radius: f32,
    pub(crate) show_gap_labels: bool,
    pub(crate) show_legend: bool,
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
            bubble_min_radius: 2.75,
            bubble_max_radius: 13.0,
            bubble_opacity: 0.78,
            show_quantity_labels: true,
            show_trade_count: true,
            // Only the largest bubbles get a (text-layout-costly) label.
            label_min_radius: 16.0,
            show_gap_labels: true,
            show_legend: true,
            legend_max_width: 690.0,
            canvas_background: egui::Color32::from_rgb(19, 23, 34),
        }
    }
}

impl OrderflowRenderStyle {
    /// Resolve the renderer-owned choices that have a corresponding user
    /// setting. Bubble geometry remains independent so it can later be exposed
    /// without growing the domain configuration again.
    #[must_use]
    pub(crate) fn from_config(config: &HeatmapConfig, canvas_background: egui::Color32) -> Self {
        Self {
            theme: config.theme,
            show_legend: config.show_legend,
            canvas_background,
            ..Self::default()
        }
    }

    #[must_use]
    fn sanitized(&self) -> Self {
        let mut style = self.clone();
        style.heat_opacity = finite_clamp(style.heat_opacity, 0.0, 1.0, 1.0);
        style.min_cell_height = finite_clamp(style.min_cell_height, 0.5, 12.0, 1.5);
        style.edge_glow = finite_clamp(style.edge_glow, 0.0, 1.0, 0.18);
        style.bubble_min_radius = finite_clamp(style.bubble_min_radius, 1.0, 24.0, 2.75);
        style.bubble_max_radius =
            finite_clamp(style.bubble_max_radius, style.bubble_min_radius, 64.0, 13.0);
        style.bubble_opacity = finite_clamp(style.bubble_opacity, 0.05, 1.0, 0.78);
        style.label_min_radius = finite_clamp(style.label_min_radius, 4.0, 64.0, 8.5);
        style.legend_max_width = finite_clamp(style.legend_max_width, 160.0, 2_000.0, 690.0);
        style
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
        let position = self.first_bar_index as f32 - 0.5 + ext_pos;
        self.viewport
            .x_at_bar_position(position, self.chart_rect.right(), self.total_bars)
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

        let raw_intensity = finite_unit(cell.intensity);
        let base_alpha = finite_unit(cell.alpha);
        if raw_intensity <= 0.0 || base_alpha <= 0.0 {
            continue;
        }
        // Quantize magnitude into a few bands so the book's per-update jitter
        // maps to the SAME colour: adjacent runs merge into one crisp, stable
        // band instead of a flickering gradient that reads as "meteors". The
        // faintest noise (rounding to zero) drops out entirely.
        let intensity = quantize_heat(raw_intensity);
        if intensity <= 0.0 {
            continue;
        }
        let alpha = finite_unit(base_alpha * (intensity / raw_intensity) * style.heat_opacity);
        if alpha <= 0.0 {
            continue;
        }
        let rgb = resting_rgb(style.theme, cell.side, intensity);
        let fill = rgba(rgb, alpha);

        if style.edge_glow > 0.0 {
            let spread = 0.55 + intensity * 0.85;
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
        clip.rect_filled(rect, egui::Rounding::ZERO, palette.gap_fill);
        draw_gap_hatch(&clip, rect, palette.gap_hatch);
        draw_dashed_vertical(
            &clip,
            rect.left(),
            rect,
            4.0,
            5.0,
            palette.gap_boundary,
            1.0,
        );
        draw_dashed_vertical(
            &clip,
            rect.right(),
            rect,
            4.0,
            5.0,
            palette.gap_boundary,
            1.0,
        );

        if style.show_gap_labels && rect.width() >= 112.0 {
            let label = gap_label(&gap.reason);
            draw_text_with_shadow(
                &clip,
                rect.center_top() + egui::vec2(0.0, 17.0),
                egui::Align2::CENTER_TOP,
                label,
                egui::FontId::proportional(10.0),
                palette.muted_text,
            );
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
        let hole_w = if full { 9.0 } else { 5.0 + 6.0 * reduction };
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
        let r = bubble_radius(trade.size, style.bubble_min_radius, style.bubble_max_radius);
        add_gradient_rect(
            &mut hole_mesh,
            egui::Rect::from_min_max(
                egui::pos2(center.x - r - 1.0, center.y - r - 2.0),
                egui::pos2((center.x + r + 4.0).min(right_edge), center.y + r + 2.0),
            ),
            style.canvas_background,
            style.canvas_background,
        );
    }

    if !hole_mesh.is_empty() {
        clip.add(egui::Shape::mesh(hole_mesh));
    }

    for front in fronts {
        match front {
            EventFront::Aligned {
                band,
                matched,
                full,
            } => {
                let strength = matched.max(0.25);
                clip.line_segment(
                    [
                        egui::pos2(band.x, band.top),
                        egui::pos2(band.x, band.bottom),
                    ],
                    egui::Stroke::new(
                        if full { 2.0_f32 } else { 1.3_f32 },
                        palette.consumption.gamma_multiply(0.55 + 0.4 * strength),
                    ),
                );
                if full {
                    // End caps read as "this band was fully taken here".
                    for y in [band.top, band.bottom] {
                        clip.line_segment(
                            [egui::pos2(band.x - 3.5, y), egui::pos2(band.x + 3.5, y)],
                            egui::Stroke::new(1.5_f32, palette.consumption.gamma_multiply(0.8)),
                        );
                    }
                }
            }
            EventFront::DepthOnly {
                band,
                reduction,
                full,
            } => {
                clip.line_segment(
                    [
                        egui::pos2(band.x, band.top),
                        egui::pos2(band.x, band.bottom),
                    ],
                    egui::Stroke::new(
                        if full { 1.6_f32 } else { 1.1_f32 },
                        palette.depth_only.gamma_multiply(if full {
                            0.9
                        } else {
                            0.55 + 0.3 * reduction
                        }),
                    ),
                );
            }
        }
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
    let palette = Palette::for_theme(style.theme);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let right_edge = context.layout.chart_rect.right();

    // Consumption glow behind the bubbles, so a bubble's own fill never hides it.
    let mut glow_mesh = egui::Mesh::default();
    for trade in &context.projection.aggressions {
        if trade.matched_fraction <= 0.0 && trade.liquidity_event_ids.is_empty() {
            continue;
        }
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        if !context.layout.chart_rect.contains(center) {
            continue;
        }
        let radius = bubble_radius(trade.size, style.bubble_min_radius, style.bubble_max_radius);
        let hh = radius * 1.7 + 2.0;
        add_gradient_rect(
            &mut glow_mesh,
            egui::Rect::from_min_max(
                egui::pos2(center.x, center.y - hh),
                egui::pos2((center.x + 7.0).min(right_edge), center.y + hh),
            ),
            palette.consumption.gamma_multiply(0.22),
            egui::Color32::TRANSPARENT,
        );
    }
    if !glow_mesh.is_empty() {
        clip.add(egui::Shape::mesh(glow_mesh));
    }

    for trade in &context.projection.aggressions {
        let center = egui::pos2(context.layout.x(trade.x), context.layout.y(trade.y));
        if !context.layout.chart_rect.contains(center) {
            continue;
        }
        let radius = bubble_radius(trade.size, style.bubble_min_radius, style.bubble_max_radius);
        let color = match trade.side {
            Side::Buy => palette.buy,
            Side::Sell => palette.sell,
        };
        let linked_reduction =
            trade.matched_fraction > 0.0 || !trade.liquidity_event_ids.is_empty();

        clip.circle_filled(
            center,
            radius + 2.5,
            color.gamma_multiply(0.12 + 0.06 * finite_unit(trade.size)),
        );
        clip.circle_filled(center, radius, color.gamma_multiply(style.bubble_opacity));
        clip.circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0_f32, color.gamma_multiply(0.96)),
        );

        if linked_reduction {
            // Vertical consumption front on the bubble + a bright ring: this
            // print ate resting liquidity at this exact price.
            let strength = finite_unit(trade.matched_fraction).max(0.25);
            let hh = radius * 1.7 + 2.0;
            clip.line_segment(
                [
                    egui::pos2(center.x, center.y - hh),
                    egui::pos2(center.x, center.y + hh),
                ],
                egui::Stroke::new(1.8_f32, palette.consumption.gamma_multiply(0.9)),
            );
            clip.circle_stroke(
                center,
                radius + 1.6,
                egui::Stroke::new(
                    1.3_f32,
                    palette.consumption.gamma_multiply(0.6 + strength * 0.35),
                ),
            );
        }

        if radius >= style.label_min_radius
            && let Some(label) = bubble_label(
                trade.quantity,
                trade.trade_count,
                style.show_quantity_labels,
                style.show_trade_count,
            )
        {
            let font = egui::FontId::proportional((radius * 0.68).clamp(8.0, 11.0));
            let galley = clip.layout_no_wrap(label, font, palette.bubble_text);
            if galley.size().x <= radius * 1.78 && galley.size().y <= radius * 1.45 {
                let pos = center - galley.size() / 2.0;
                clip.galley(
                    pos + egui::vec2(1.0, 1.0),
                    galley.clone(),
                    egui::Color32::from_black_alpha(190),
                );
                clip.galley(pos, galley, palette.bubble_text);
            }
        }
    }
}

/// Draw a responsive legend inside the chart. Labels deliberately distinguish
/// confirmed aggression from aligned or unattributed L2 reductions.
pub(crate) fn draw_compact_legend(painter: &egui::Painter, context: &RenderContext<'_>) {
    let style = context.style.sanitized();
    if !style.show_legend || context.layout.chart_rect.width() < 150.0 {
        return;
    }
    let palette = Palette::for_theme(style.theme);
    let clip = painter.with_clip_rect(context.layout.chart_rect);
    let multiple = context.projection.effective_grouping.multiple;
    let liquidity_label = if multiple > 1 {
        format!("liquidity · {multiple}×")
    } else {
        "liquidity".to_owned()
    };
    let entries = [
        (LegendGlyph::Heat, liquidity_label),
        (LegendGlyph::Buy, "buy aggression".to_owned()),
        (LegendGlyph::Sell, "sell aggression".to_owned()),
        (
            LegendGlyph::Aligned,
            "aggression-aligned depletion".to_owned(),
        ),
        (
            LegendGlyph::DepthOnly,
            "L2 reduction (unattributed)".to_owned(),
        ),
        (LegendGlyph::Gap, "L2 gap".to_owned()),
    ];
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

    if config.show_liquidity_events {
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
        draw_preview_bubble(
            &painter,
            egui::pos2(
                egui::lerp(chart.left()..=chart.right(), 0.58),
                egui::lerp(chart.top()..=chart.bottom(), 0.27),
            ),
            11.0,
            palette.buy,
            config.show_liquidity_events,
            &palette,
        );
        draw_preview_bubble(
            &painter,
            egui::pos2(
                egui::lerp(chart.left()..=chart.right(), 0.43),
                egui::lerp(chart.top()..=chart.bottom(), 0.72),
            ),
            8.0,
            palette.sell,
            false,
            &palette,
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
    gap_hatch: egui::Color32,
    gap_boundary: egui::Color32,
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
                gap_fill: egui::Color32::from_rgba_premultiplied(50, 58, 76, 48),
                gap_hatch: egui::Color32::from_rgba_premultiplied(142, 151, 171, 30),
                gap_boundary: egui::Color32::from_rgba_premultiplied(157, 167, 188, 115),
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
                gap_fill: egui::Color32::from_rgba_premultiplied(80, 80, 92, 64),
                gap_hatch: egui::Color32::from_rgba_premultiplied(235, 235, 245, 42),
                gap_boundary: egui::Color32::from_rgb(218, 222, 235),
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
                gap_fill: egui::Color32::from_rgba_premultiplied(58, 60, 72, 52),
                gap_hatch: egui::Color32::from_rgba_premultiplied(205, 205, 190, 34),
                gap_boundary: egui::Color32::from_rgb(176, 180, 190),
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

fn draw_preview_bubble(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    color: egui::Color32,
    linked_reduction: bool,
    palette: &Palette,
) {
    let hh = radius * 1.7 + 2.0;
    if linked_reduction {
        // Consumption glow behind the bubble (drawn first so the fill sits on
        // top), matching the live "aggression eating a wall" marker.
        let mut mesh = egui::Mesh::default();
        add_gradient_rect(
            &mut mesh,
            egui::Rect::from_min_max(
                egui::pos2(center.x, center.y - hh),
                egui::pos2(center.x + 7.0, center.y + hh),
            ),
            palette.consumption.gamma_multiply(0.22),
            egui::Color32::TRANSPARENT,
        );
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.circle_filled(center, radius + 2.5, color.gamma_multiply(0.14));
    painter.circle_filled(center, radius, color.gamma_multiply(0.82));
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, color));
    if linked_reduction {
        painter.line_segment(
            [
                egui::pos2(center.x, center.y - hh),
                egui::pos2(center.x, center.y + hh),
            ],
            egui::Stroke::new(1.6_f32, palette.consumption.gamma_multiply(0.9)),
        );
        painter.circle_stroke(
            center,
            radius + 1.6,
            egui::Stroke::new(1.3_f32, palette.consumption.gamma_multiply(0.9)),
        );
    }
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

fn draw_gap_hatch(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let start_x = x.max(rect.left());
        let start_y = rect.bottom() - (start_x - x);
        let raw_end_x = x + rect.height();
        let end_x = raw_end_x.min(rect.right());
        let end_y = rect.top() + (raw_end_x - end_x);
        if start_y >= rect.top() && end_y <= rect.bottom() {
            painter.line_segment(
                [egui::pos2(start_x, start_y), egui::pos2(end_x, end_y)],
                egui::Stroke::new(0.55_f32, color),
            );
        }
        x += 18.0;
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
        "book_unavailable_before_capture" => "L2 unavailable before capture",
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

/// Visual width (in candle-widths) of the forming bar's live tail. It grows
/// linearly with elapsed time so a fixed pixels-per-ms scale keeps live events
/// put (instead of recompressing as the book advances), clamped so the tail
/// never dominates the chart. `1.0` means "no tail" (the classic single slot).
pub(crate) fn live_span_for(elapsed_ms: i64, ref_duration_ms: i64, max_span: f32) -> f32 {
    if elapsed_ms <= 0 || ref_duration_ms <= 0 || !max_span.is_finite() || max_span <= 1.0 {
        return 1.0;
    }
    let span = (elapsed_ms as f32 / ref_duration_ms as f32) * max_span;
    span.clamp(1.0, max_span)
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

    #[test]
    fn render_style_sanitizes_non_finite_geometry() {
        let style = OrderflowRenderStyle {
            heat_opacity: f32::NAN,
            min_cell_height: f32::INFINITY,
            edge_glow: -1.0,
            bubble_min_radius: f32::NAN,
            bubble_max_radius: -4.0,
            label_min_radius: f32::NAN,
            legend_max_width: f32::NAN,
            ..OrderflowRenderStyle::default()
        }
        .sanitized();
        assert_eq!(style.heat_opacity, 1.0);
        assert_eq!(style.min_cell_height, 1.5);
        assert_eq!(style.edge_glow, 0.0);
        assert!(style.bubble_max_radius >= style.bubble_min_radius);
        assert!(style.legend_max_width.is_finite());
    }

    #[test]
    fn live_span_grows_with_time_and_clamps() {
        assert_eq!(live_span_for(0, 1000, 8.0), 1.0);
        assert_eq!(live_span_for(500, 0, 8.0), 1.0);
        assert!((live_span_for(1000, 1000, 8.0) - 8.0).abs() < 1e-4);
        assert_eq!(live_span_for(5000, 1000, 8.0), 8.0);
        assert!((live_span_for(500, 1000, 8.0) - 4.0).abs() < 1e-4);
        assert_eq!(live_span_for(1, 1000, 8.0), 1.0);
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
