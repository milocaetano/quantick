//! The quiet price path behind tape-only volume dots, and the marks a tape
//! held in the past carries.

use eframe::egui::{self, Align2, FontId};
use quantick_orderflow::projection::{PastTape, TapeHorizontalGeometry};
use quantick_orderflow::tape_view::{PAST_TAPE_LABEL, RETAINED_EDGE_LABEL, retained_edge_fraction};

/// A muted hairline keeps execution order readable behind the volume discs.
const PATH_WIDTH_PX: f32 = 0.8;
const PATH_RGBA: [u8; 4] = [142, 166, 177, 95];

/// Connect factual centres, including offscreen vertices, behind the discs.
/// The painter clips each segment; culling vertices would invent a shortcut.
pub(super) fn draw(painter: &egui::Painter, points: impl Iterator<Item = egui::Pos2>) {
    let [r, g, b, a] = PATH_RGBA;
    let stroke = egui::Stroke::new(
        PATH_WIDTH_PX,
        egui::Color32::from_rgba_unmultiplied(r, g, b, a),
    );
    let mut points: Vec<_> = points.collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)));
    let mut previous = None;
    for point in points {
        if let Some(from) = previous {
            painter.line_segment([from, point], stroke);
        }
        previous = Some(point);
    }
}

/// A held tape says so on its top edge; where the retained tape begins inside
/// its window, a line marks it and the stretch before it is shaded, labelled.
pub(crate) fn draw_past_tape_edge(
    painter: &egui::Painter,
    context: &super::RenderContext<'_>,
    past: Option<&PastTape>,
) {
    let (Some(past), Some((edge, _))) = (past, context.tape_time) else {
        return;
    };
    let (lane, font, [r, g, b, _]) = (
        context.layout.lane_rect(),
        FontId::proportional(11.0),
        PATH_RGBA,
    );
    let (clip, color) = (
        painter.with_clip_rect(lane),
        egui::Color32::from_rgb(r, g, b),
    );
    let top_left = lane.left_top() + egui::vec2(6.0, 4.0);
    clip.text(
        top_left,
        Align2::LEFT_TOP,
        PAST_TAPE_LABEL,
        font.clone(),
        color,
    );
    let edge_at = retained_edge_fraction(edge.now_ms, edge.window_ms, past.retained_from_ms);
    let Some(fraction) = edge_at else {
        return;
    };
    let geometry =
        TapeHorizontalGeometry::resolve(lane.width(), lane.height(), &context.style.bubbles);
    let x = lane.left() + geometry.x(fraction);
    clip.rect_filled(lane.with_max_x(x), 0.0, egui::Color32::from_black_alpha(90));
    clip.vline(x, lane.y_range(), egui::Stroke::new(1.0_f32, color));
    let at = egui::pos2(x - 4.0, lane.top() + 20.0);
    clip.text(at, Align2::RIGHT_TOP, RETAINED_EDGE_LABEL, font, color);
}
