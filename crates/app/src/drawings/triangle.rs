use eframe::egui;
use egui_phosphor::regular as icons;

use super::shape_core::{SHAPES_FAMILY, hit_outline, paint_outline};
use super::{
    DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, drawing_stroke, off_line_by,
};

/// The shortest a triangle's third corner may stand off the side the first
/// two drew, in pixels. Three collinear corners are a line, not a triangle —
/// and that is exactly where the pointer sits the instant a drag lets go.
/// It floors the shaping phase only; the corner handles move freely after.
const MIN_BORN_HEIGHT_PX: f32 = 12.0;

pub(super) static TOOL: Triangle = Triangle;

pub(super) struct Triangle;

impl DrawingToolImpl for Triangle {
    fn id(&self) -> &'static str {
        "triangle"
    }
    fn name(&self) -> &'static str {
        "Triangle"
    }
    fn settings_title(&self) -> &'static str {
        "Triangle settings"
    }
    fn icon(&self) -> &'static str {
        icons::TRIANGLE
    }
    fn hover_text(&self) -> &'static str {
        "Triangle - drag a side, then click the third corner"
    }
    fn required_points(&self) -> usize {
        3
    }
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        match placed {
            1 => Some("Drag out the first side"),
            2 => Some("Move to shape it, click to place"),
            _ => None,
        }
    }
    /// The third corner never lands on the side the first two drew — see
    /// [`MIN_BORN_HEIGHT_PX`].
    fn pending_anchor(&self, placed: &[egui::Pos2], cursor: egui::Pos2) -> egui::Pos2 {
        let [start, end] = placed else {
            return cursor;
        };
        off_line_by(*start, *end, cursor, MIN_BORN_HEIGHT_PX)
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(SHAPES_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) {
        match points {
            // The first side, while the drag is shaping it. Without this the
            // trader drags a triangle and watches nothing happen at all: the
            // outline only exists once three corners do, and the rubber band
            // is the whole feedback of the gesture.
            [start, end] => {
                painter.line_segment([*start, *end], drawing_stroke(style));
            }
            [_, _, _] => paint_outline(painter, style, points),
            _ => {}
        }
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        points.len() == 3 && hit_outline(points, position, radius_px, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![
                egui::pos2(100.0, 200.0),
                egui::pos2(200.0, 100.0),
                egui::pos2(300.0, 200.0),
            ],
            egui::pos2(200.0, 200.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three corners on one line are a line, not a triangle — and that is
    /// exactly where the pointer stands the instant the drag that drew the
    /// first side lets go.
    #[test]
    fn the_third_corner_never_lands_on_the_first_side() {
        let start = egui::pos2(100.0, 200.0);
        let end = egui::pos2(300.0, 140.0);
        let shaped = TOOL.pending_anchor(&[start, end], end);
        let side = end - start;
        // Twice the triangle's area: zero exactly when the corners are
        // collinear, whatever the units. Divided back by the side gives the
        // height in pixels, which is the number worth asserting on.
        let cross = side.x * (shaped.y - start.y) - side.y * (shaped.x - start.x);
        let height = cross.abs() / side.length();
        // A number of its own, not the constant: a floor shrunk to nothing
        // has to fail here rather than pass vacuously.
        assert!(
            height > 1.0,
            "the triangle stands off its own first side, height was {height}"
        );
        assert!(
            (height - MIN_BORN_HEIGHT_PX).abs() < 1e-2,
            "and it stands exactly at the floor, height was {height}"
        );
    }

    /// A corner the trader has already carried clear of the side is theirs.
    #[test]
    fn a_corner_clear_of_the_first_side_is_left_alone() {
        let start = egui::pos2(100.0, 200.0);
        let end = egui::pos2(300.0, 140.0);
        let cursor = egui::pos2(200.0, 300.0);
        assert_eq!(TOOL.pending_anchor(&[start, end], cursor), cursor);
    }
}
