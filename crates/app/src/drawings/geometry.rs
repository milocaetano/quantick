//! Screen-space geometry shared by the tools, in the window's coordinates.
//! The maths is headless (`quantick_drawings`); these cross into egui.

use eframe::egui;
use quantick_drawings::Px;

fn px(pos: egui::Pos2) -> Px {
    Px::new(pos.x, pos.y)
}

fn pos(px: Px) -> egui::Pos2 {
    egui::pos2(px.x, px.y)
}

/// Paint `from`–`to` as `dash_px` dashes separated by `gap_px` gaps, starting
/// on a dash. egui's dashed helper allocates a `Vec` of shapes per call; a
/// drawing repaints every frame, so this walks the segment itself and emits
/// plain line segments.
pub fn dashed_segment(
    painter: &egui::Painter,
    from: egui::Pos2,
    to: egui::Pos2,
    stroke: egui::Stroke,
    dash_px: f32,
    gap_px: f32,
) {
    let span = to - from;
    let length = span.length();
    let step = dash_px + gap_px;
    if !(length > f32::EPSILON && length.is_finite() && step > 0.0 && step.is_finite()) {
        return;
    }
    let direction = span / length;
    let mut travelled = 0.0_f32;
    while travelled < length {
        let dash_end = (travelled + dash_px).min(length);
        painter.line_segment(
            [from + direction * travelled, from + direction * dash_end],
            stroke,
        );
        travelled += step;
    }
}

pub fn distance_to_segment(position: egui::Pos2, start: egui::Pos2, end: egui::Pos2) -> f32 {
    quantick_drawings::distance_to_segment(px(position), px(start), px(end))
}

/// See [`quantick_drawings::unit_normal`].
pub fn unit_normal(direction: egui::Vec2) -> egui::Vec2 {
    let normal = quantick_drawings::unit_normal(Px::new(direction.x, direction.y));
    egui::vec2(normal.x, normal.y)
}

/// See [`quantick_drawings::off_line_by`].
pub fn off_line_by(
    start: egui::Pos2,
    end: egui::Pos2,
    cursor: egui::Pos2,
    floor_px: f32,
) -> egui::Pos2 {
    pos(quantick_drawings::off_line_by(
        px(start),
        px(end),
        px(cursor),
        floor_px,
    ))
}

/// See [`quantick_drawings::level_with`].
pub fn level_with(anchor: egui::Pos2, cursor: egui::Pos2) -> egui::Pos2 {
    pos(quantick_drawings::level_with(px(anchor), px(cursor)))
}
