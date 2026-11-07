//! The tape's best bid and best ask: bold stepped lines over its book.

use super::bubbles::bubble_colors;
use super::{RenderContext, premultiplied};
use eframe::egui;
use quantick_orderflow::projection::bbo_strokes;

/// Paint the projected lines, while `shown`, over the heatmap and under the
/// dots, clipped to the tape, as [`bbo_strokes`] lays them. Every vertex is a
/// tape position, so it maps through the tape alone.
pub(crate) fn draw_bbo_lines(painter: &egui::Painter, context: &RenderContext<'_>, shown: bool) {
    let (layout, style) = (context.layout, context.style);
    if !shown {
        return;
    }
    let clip = painter.with_clip_rect(layout.lane_rect());
    let tape = layout.with_lane_only();
    let colors = bubble_colors(&style.colors(premultiplied), &style.bubbles);
    let halo = crate::theme::get(painter.ctx()).quantity_shadow();
    let place = |[x, y]: [f64; 2]| [tape.x(x), layout.y_unclamped(y)];
    let high_up = layout.y_unclamped(0.0) < layout.y_unclamped(1.0);
    for stroke in bbo_strokes(&context.projection.bbo_lines, place, high_up) {
        let colour = stroke.side.map_or(halo, |side| colors.for_resting(side));
        let line = stroke.points.into_iter().map(egui::Pos2::from).collect();
        let pen = egui::Stroke::new(stroke.width, colour.gamma_multiply(stroke.alpha));
        clip.add(egui::Shape::line(line, pen));
    }
}
