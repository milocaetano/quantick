use eframe::egui;
use egui_phosphor::regular as icons;

use super::shape_core::{SHAPES_FAMILY, hit_outline, paint_outline};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily};

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
        "Triangle - three clicks, for wedges and coils"
    }
    fn required_points(&self) -> usize {
        3
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
        if points.len() == 3 {
            paint_outline(painter, style, points);
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
