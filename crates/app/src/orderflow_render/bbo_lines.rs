//! The tape's best bid and best ask: bold stepped lines over its book.

use super::bubbles::bubble_colors;
use super::constants::BBO_LINE_WIDTH_PX;
use super::{RenderContext, premultiplied};
use eframe::egui;
use quantick_orderflow::projection::decimate_columns;

/// Paint the projected lines, while `shown`, over the heatmap and under the
/// dots, clipped to the tape, in the dots' side colours; the carried tail
/// dimmed. Every vertex is a tape position, so it maps through the tape alone.
pub(crate) fn draw_bbo_lines(painter: &egui::Painter, context: &RenderContext<'_>, shown: bool) {
    let (layout, style) = (context.layout, context.style);
    if !shown || context.projection.bbo_lines.is_empty() {
        return;
    }
    let clip = painter.with_clip_rect(layout.lane_rect());
    let tape = layout.with_lane_only();
    let colors = bubble_colors(&style.colors(premultiplied), &style.bubbles);
    let at = |&[x, y]: &[f64; 2]| [tape.x(x), layout.y_unclamped(y)];
    for path in context.projection.bbo_lines.iter() {
        let side = colors.for_resting(path.side).gamma_multiply(path.alpha());
        let points = decimate_columns(path.points.iter().map(at)).into_iter();
        let line = points.map(egui::Pos2::from).collect();
        let stroke = egui::Stroke::new(BBO_LINE_WIDTH_PX, side);
        clip.add(egui::Shape::line(line, stroke));
    }
}
