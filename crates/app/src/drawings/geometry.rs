//! Screen-space geometry shared by the tools.

use eframe::egui;

pub fn distance_to_segment(position: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    let segment = end - start;
    let length_sq = segment.length_sq();
    if length_sq <= f32::EPSILON {
        return position.distance(start);
    }
    let projection = ((position - start).dot(segment) / length_sq).clamp(0.0, 1.0);
    position.distance(start + segment * projection)
}

/// The unit normal of `direction` — the axis a shape's thickness is measured
/// along. A direction of no length has no normal of its own, so it is given
/// the vertical, which is the axis a chart measures in.
pub fn unit_normal(direction: egui::Vec2) -> egui::Vec2 {
    let length = direction.length();
    if length <= f32::EPSILON {
        return egui::vec2(0.0, 1.0);
    }
    egui::vec2(-direction.y, direction.x) / length
}

/// Push `cursor` off the line through `start`–`end` until it stands at least
/// `floor_px` away from it, keeping exactly where it sits *along* that line.
///
/// The shared half of [`DrawingToolImpl::pending_anchor`]: a tool whose third
/// anchor gives a shape its thickness is degenerate when that anchor lands on
/// the line the first two drew — a channel of no width, a triangle of no
/// area — and that is precisely where the pointer is standing the instant a
/// drag lets go. Sliding *along* the line is left alone, so the gesture still
/// means what it always meant; only the collapsed case is refused.
///
/// A cursor exactly on the line opens the shape on the normal's own side, so
/// the same gesture always produces the same object.
pub fn off_line_by(
    start: egui::Pos2,
    end: egui::Pos2,
    cursor: egui::Pos2,
    floor_px: f32,
) -> egui::Pos2 {
    let normal = unit_normal(end - start);
    let offset = (cursor - start).dot(normal);
    if offset.abs() >= floor_px {
        return cursor;
    }
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    cursor + normal * (side * floor_px - offset)
}
