//! The canvas's compact key: which layers can draw right now, one glyph and
//! one label each, flowed into the corner the chart header leaves free.
//!
//! Chrome, not data: everything it names keeps drawing while it is hidden,
//! and it stands down rather than print over a stack of indicator chips.

use crate::orderflow_render::constants::{
    LEGEND_BAND_WIDTH_PX, LEGEND_BORDER_WIDTH_PX, LEGEND_CORNER_RADIUS_PX, LEGEND_DOT_WIDTH_PX,
    LEGEND_ENTRY_GAP_PX, LEGEND_ENTRY_HEIGHT_PX, LEGEND_ENTRY_PADDING_PX, LEGEND_FONT_PX,
    LEGEND_GLYPH_GAP_PX, LEGEND_HEAT_HEIGHT_PX, LEGEND_HEAT_WIDTH_PX, LEGEND_INNER_MARGIN_PX,
    LEGEND_MIN_CHART_WIDTH_PX, LEGEND_MIN_CONTENT_WIDTH_PX, LEGEND_MIN_PANEL_WIDTH_PX,
    LEGEND_OUTER_MARGIN_PX, LEGEND_ROW_HEIGHT_PX, MAX_LEGEND_TOP_INSET_FRAC,
};
use eframe::egui;
use quantick_chart::flow_execution::flow_chrome_layout;
use quantick_orderflow::HeatmapTheme;
use quantick_orderflow::config::legend::{LegendGlyph, legend_entries};

use super::bubbles::BubbleColors;
use super::layout::RenderContext;
use super::{Palette, add_gradient_rect, draw_dashed_vertical, rgba, thermal_rgb};

/// Draw the canvas key. The tape alone wears none: its bubbles' colours are
/// the only Buy/Sell key it needs.
pub(crate) fn draw_compact_legend_avoiding(
    painter: &egui::Painter,
    context: &RenderContext<'_>,
    tick_regions: Option<&[quantick_chart::flow_execution::FlowRegionVisual]>,
) -> Option<egui::Rect> {
    let style = context.style.sanitized();
    let tape_alone = style.live_lane.tape_only && style.live_lane.enabled;
    if tape_alone
        || !style.show_legend
        || context.layout.chart_rect.width() < LEGEND_MIN_CHART_WIDTH_PX
    {
        return None;
    }
    // The corner may already be full — a tall stack of indicator chips over a
    // short canvas. The key stands down rather than printing over them: it is
    // chrome, everything it names keeps drawing, and it comes back the moment
    // there is room (or a chip goes away).
    if style.legend_top_inset > context.layout.chart_rect.height() * MAX_LEGEND_TOP_INSET_FRAC {
        return None;
    }
    // The legend is a key for what is on screen, so the aggression swatches
    // follow the bubble panel's colour overrides.
    let mut palette = super::palette_for_theme(style.theme);
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
        return None;
    }
    let font = egui::FontId::proportional(LEGEND_FONT_PX);
    let galleys: Vec<_> = entries
        .iter()
        .map(|(_, label)| clip.layout_no_wrap(label.clone(), font.clone(), palette.legend_text))
        .collect();
    let widths: Vec<f32> = entries
        .iter()
        .zip(&galleys)
        .map(|((glyph, _), galley)| {
            glyph_width(*glyph) + LEGEND_GLYPH_GAP_PX + galley.size().x + LEGEND_ENTRY_PADDING_PX
        })
        .collect();

    let max_panel_width = style.legend_max_width.min(
        (context.layout.chart_rect.width() - LEGEND_OUTER_MARGIN_PX * 2.0)
            .max(LEGEND_MIN_PANEL_WIDTH_PX),
    );
    let max_panel_width = tick_regions.map_or(max_panel_width, |_| {
        max_panel_width.min(quantick_chart::flow_execution::FLOW_CHROME_MAX_WIDTH_PX)
    });
    let inner_margin = LEGEND_INNER_MARGIN_PX;
    let max_content_width = (max_panel_width - inner_margin * 2.0).max(LEGEND_MIN_CONTENT_WIDTH_PX);
    let flow = flow_chrome_layout(
        &widths,
        max_content_width,
        LEGEND_ROW_HEIGHT_PX,
        LEGEND_ENTRY_GAP_PX,
    );
    let panel_size = egui::vec2(
        (flow.size[0] + inner_margin * 2.0).min(max_panel_width),
        flow.size[1] + inner_margin * 2.0,
    );
    // The chart header owns the first text row at the top-left, and the pane
    // may have stacked indicator chips under it. Keep the legend below all of
    // it, so symbol/bar metadata and every chip remain readable at every
    // width — nothing at this corner prints over anything else.
    let corner = LEGEND_OUTER_MARGIN_PX;
    let mut panel = egui::Rect::from_min_size(
        context.layout.chart_rect.left_top() + egui::vec2(corner, corner + style.legend_top_inset),
        panel_size,
    );
    if let Some(regions) = tick_regions {
        let area = context.layout.chart_rect;
        let placed = quantick_chart::flow_execution::place_flow_chrome(
            [area.min.into(), area.max.into()],
            [panel.min.into(), panel.max.into()],
            regions,
        )?;
        panel = egui::Rect::from_min_max(placed[0].into(), placed[1].into());
    }
    let rounding = egui::Rounding::same(LEGEND_CORNER_RADIUS_PX);
    clip.rect_filled(panel, rounding, palette.legend_background);
    clip.rect_stroke(
        panel,
        rounding,
        egui::Stroke::new(LEGEND_BORDER_WIDTH_PX, palette.legend_border),
    );

    let mut footprint = panel.expand(LEGEND_BORDER_WIDTH_PX / 2.0);
    let origin = panel.left_top() + egui::vec2(inner_margin, inner_margin);
    for (((glyph, _), galley), offset) in entries.iter().zip(galleys).zip(flow.positions) {
        let item = origin + egui::Vec2::from(offset);
        draw_legend_glyph(&clip, *glyph, item, &palette, style.theme);
        let text_pos = egui::pos2(
            item.x + glyph_width(*glyph) + LEGEND_GLYPH_GAP_PX,
            item.y + (LEGEND_ENTRY_HEIGHT_PX - galley.size().y) / 2.0,
        );
        footprint = footprint.union(egui::Rect::from_min_size(text_pos, galley.size()));
        clip.galley(text_pos, galley, palette.legend_text);
    }
    Some(footprint.intersect(clip.clip_rect()))
}

/// A key's glyph width, in pixels.
const fn glyph_width(glyph: LegendGlyph) -> f32 {
    match glyph {
        LegendGlyph::Heat => LEGEND_HEAT_WIDTH_PX,
        LegendGlyph::Buy | LegendGlyph::Sell => LEGEND_DOT_WIDTH_PX,
        LegendGlyph::Aligned | LegendGlyph::DepthOnly | LegendGlyph::Gap => LEGEND_BAND_WIDTH_PX,
    }
}

fn draw_legend_glyph(
    painter: &egui::Painter,
    glyph: LegendGlyph,
    origin: egui::Pos2,
    palette: &Palette,
    theme: HeatmapTheme,
) {
    let center = origin + egui::vec2(glyph_width(glyph), LEGEND_ENTRY_HEIGHT_PX) / 2.0;
    match glyph {
        LegendGlyph::Heat => {
            let rect = egui::Rect::from_center_size(
                center,
                egui::vec2(LEGEND_HEAT_WIDTH_PX, LEGEND_HEAT_HEIGHT_PX),
            );
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
            let band = egui::Rect::from_center_size(center, egui::vec2(LEGEND_BAND_WIDTH_PX, 6.0));
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
            let rect = egui::Rect::from_center_size(center, egui::vec2(LEGEND_BAND_WIDTH_PX, 7.0));
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
            let rect = egui::Rect::from_center_size(center, egui::vec2(LEGEND_BAND_WIDTH_PX, 8.0));
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
