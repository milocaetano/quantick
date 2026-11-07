//! The tape's best bid and best ask: bold stepped lines over its book.

use super::bubbles::bubble_colors;
use super::constants::BBO_LINE_WIDTH_PX;
use super::{RenderContext, premultiplied};
use eframe::egui;
use quantick_orderbook::BookSide;

/// Paint the projected lines over the heatmap and under the dots, clipped to
/// the tape, in the dots' own side colours; the carried tail dimmed.
pub(crate) fn draw_bbo_lines(painter: &egui::Painter, context: &RenderContext<'_>) {
    let (layout, style) = (context.layout, context.style);
    if !style.live_lane.bbo_lines || context.projection.bbo_lines.is_empty() {
        return;
    }
    let clip = painter.with_clip_rect(layout.lane_rect());
    let colors = bubble_colors(&style.colors(premultiplied), &style.bubbles);
    for path in context.projection.bbo_lines.iter() {
        let side = [colors.sell, colors.buy][usize::from(path.side == BookSide::Bid)];
        let to_screen = |&[x, y]: &[f64; 2]| egui::pos2(layout.x(x), layout.y_unclamped(y));
        let points = path.points.iter().map(to_screen).collect();
        let stroke = egui::Stroke::new(BBO_LINE_WIDTH_PX, side.gamma_multiply(path.alpha()));
        clip.add(egui::Shape::line(points, stroke));
    }
}
