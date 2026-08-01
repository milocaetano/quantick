use eframe::egui;
use egui_phosphor::regular as icons;

use super::{DrawingStyle, DrawingToolImpl, drawing_stroke};

pub(super) static TOOL: HorizontalLine = HorizontalLine;

pub(super) struct HorizontalLine;

impl DrawingToolImpl for HorizontalLine {
    fn id(&self) -> &'static str {
        "horizontal-line"
    }
    fn settings_title(&self) -> &'static str {
        "Horizontal line settings"
    }
    fn icon(&self) -> &'static str {
        icons::MINUS
    }
    fn hover_text(&self) -> &'static str {
        "Horizontal line - click a price"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
    ) {
        if let Some(point) = points.first() {
            painter.line_segment(
                [
                    egui::pos2(chart_rect.left(), point.y),
                    egui::pos2(chart_rect.right(), point.y),
                ],
                drawing_stroke(style),
            );
        }
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool {
        points.first().is_some_and(|point| {
            chart_rect.x_range().contains(position.x)
                && (position.y - point.y).abs() <= radius_px
        })
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(100.0, 120.0)], egui::pos2(450.0, 123.0))
    }
}
