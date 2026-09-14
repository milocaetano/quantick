//! The canvas's compact key: which layers can draw right now, one glyph and
//! one label each, flowed into the corner the chart header leaves free.
//!
//! Chrome, not data: everything it names keeps drawing while it is hidden,
//! and it stands down rather than print over a stack of indicator chips.

use eframe::egui;
use quantick_orderflow::HeatmapTheme;

use super::bubbles::BubbleColors;
use super::layout::RenderContext;
use super::{
    OrderflowRenderStyle, Palette, add_gradient_rect, draw_dashed_vertical, rgba, thermal_rgb,
};

/// How far down the canvas the stack above the key may push it, as a share
/// of the canvas height.
///
/// Past this the key would be reading as part of the chart rather than as its
/// key — and a canvas whose top half is chips has no room for it at all, so it
/// stands down instead of printing over them. Chrome yields to the chart;
/// nothing it says is data (the layers keep drawing, and the trader can bring
/// it back from the right-click menu).
pub(super) const MAX_LEGEND_TOP_INSET_FRAC: f32 = 0.5;

/// The legend keys for this style, one per layer that can actually draw.
///
/// A layer draws when its family is active (L2 capture for the depth family,
/// the bubbles switch for aggression) *and* its own display switch is on.
/// Announcing anything else would describe a chart the viewer is not looking
/// at — the legend is a key for what is on screen, not a feature list.
///
/// "On screen" means either pane. The canvas holds two of them and the layers
/// are switched apart, so a key withheld because the candles are clear would
/// deny a mark the tape is drawing right now — the legend has one canvas to
/// describe, not one pane of it.
pub(super) fn legend_entries(
    style: &OrderflowRenderStyle,
    liquidity_label: String,
) -> Vec<(LegendGlyph, String)> {
    let depth = style.depth_layer || style.lane_depth_layer;
    let aggression = style.aggression_layer || style.lane_aggression_layer;
    let mut entries = Vec::new();
    if depth && style.show_liquidity {
        entries.push((LegendGlyph::Heat, liquidity_label));
    }
    if aggression && style.show_buy {
        entries.push((LegendGlyph::Buy, "buy aggression".to_owned()));
    }
    if aggression && style.show_sell {
        entries.push((LegendGlyph::Sell, "sell aggression".to_owned()));
    }
    if depth && style.show_aligned {
        entries.push((
            LegendGlyph::Aligned,
            "aggression-aligned depletion".to_owned(),
        ));
    }
    if depth && style.show_unattributed {
        entries.push((
            LegendGlyph::DepthOnly,
            "L2 reduction (unattributed)".to_owned(),
        ));
    }
    if depth && style.show_gaps {
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
    // The corner may already be full — a tall stack of indicator chips over a
    // short canvas. The key stands down rather than printing over them: it is
    // chrome, everything it names keeps drawing, and it comes back the moment
    // there is room (or a chip goes away).
    if style.legend_top_inset > context.layout.chart_rect.height() * MAX_LEGEND_TOP_INSET_FRAC {
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
    // The chart header owns the first text row at the top-left, and the pane
    // may have stacked indicator chips under it. Keep the legend below all of
    // it, so symbol/bar metadata and every chip remain readable at every
    // width — nothing at this corner prints over anything else.
    let panel = egui::Rect::from_min_size(
        context.layout.chart_rect.left_top()
            + egui::vec2(outer_margin, outer_margin + style.legend_top_inset),
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

#[derive(Debug, Clone, Copy)]
pub(super) enum LegendGlyph {
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
pub(super) struct FlowLayout {
    pub(super) positions: Vec<egui::Vec2>,
    pub(super) size: egui::Vec2,
}

pub(super) fn flow_layout(widths: &[f32], max_width: f32, row_height: f32, gap: f32) -> FlowLayout {
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
