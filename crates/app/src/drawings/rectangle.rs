use eframe::egui;
use egui_phosphor::regular as icons;

use super::{DrawingStyle, DrawingToolImpl, drawing_fill, drawing_stroke};

pub(super) static TOOL: Rectangle = Rectangle;

pub(super) struct Rectangle;

impl DrawingToolImpl for Rectangle {
    fn id(&self) -> &'static str {
        "rectangle"
    }
    fn settings_title(&self) -> &'static str {
        "Rectangle settings"
    }
    fn icon(&self) -> &'static str {
        icons::RECTANGLE
    }
    fn hover_text(&self) -> &'static str {
        "Rectangle - click two corners or drag"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        selected: bool,
    ) {
        if points.len() == 2 {
            let rect = egui::Rect::from_two_pos(points[0], points[1]);
            painter.rect_filled(rect, egui::Rounding::ZERO, drawing_fill(style));
            painter.rect_stroke(
                rect,
                egui::Rounding::ZERO,
                drawing_stroke(style, selected),
            );
        }
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool {
        points.len() == 2
            && egui::Rect::from_two_pos(points[0], points[1])
                .expand(radius_px)
                .contains(position)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)],
            egui::pos2(150.0, 150.0),
        )
    }
}
