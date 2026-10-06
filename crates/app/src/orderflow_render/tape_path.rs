//! The quiet price path behind tape-only volume dots, and the marks a tape
//! held in the past carries.

use crate::orderflow_render::constants::{
    PAST_TAPE_LABEL_FONT_PX, PAST_TAPE_LABEL_INSET, PAST_TAPE_SHADE_ALPHA, PATH_RGBA,
    PATH_WIDTH_PX, RETAINED_EDGE_LABEL_GAP_PX, RETAINED_EDGE_LABEL_TOP_PX,
};
use eframe::egui::{self, Align2, FontId};
use quantick_orderflow::projection::{PastTape, TapeHorizontalGeometry};
use quantick_orderflow::tape_view::{PAST_TAPE_LABEL, RETAINED_EDGE_LABEL, retained_edge_fraction};

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
        FontId::proportional(PAST_TAPE_LABEL_FONT_PX),
        PATH_RGBA,
    );
    let (clip, color) = (
        painter.with_clip_rect(lane),
        egui::Color32::from_rgb(r, g, b),
    );
    let top_left = lane.left_top() + PAST_TAPE_LABEL_INSET;
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
    clip.rect_filled(
        lane.with_max_x(x),
        0.0,
        egui::Color32::from_black_alpha(PAST_TAPE_SHADE_ALPHA),
    );
    clip.vline(x, lane.y_range(), egui::Stroke::new(1.0_f32, color));
    let at = egui::pos2(
        x - RETAINED_EDGE_LABEL_GAP_PX,
        lane.top() + RETAINED_EDGE_LABEL_TOP_PX,
    );
    clip.text(at, Align2::RIGHT_TOP, RETAINED_EDGE_LABEL, font, color);
}
