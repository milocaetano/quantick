use eframe::egui;
use egui_phosphor::regular as icons;

use super::shape_core::SHAPES_FAMILY;
use super::{
    DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut, drawing_fill,
    drawing_stroke,
};

pub(super) static TOOL: Rectangle = Rectangle;

pub(super) struct Rectangle;

impl DrawingToolImpl for Rectangle {
    fn id(&self) -> &'static str {
        "rectangle"
    }
    fn name(&self) -> &'static str {
        "Rectangle"
    }
    fn settings_title(&self) -> &'static str {
        "Rectangle settings"
    }
    fn icon(&self) -> &'static str {
        icons::RECTANGLE
    }
    fn hover_text(&self) -> &'static str {
        "Rectangle - click two corners or drag (R)"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::R,
            shift: false,
        })
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
        if points.len() == 2 {
            let rect = egui::Rect::from_two_pos(points[0], points[1]);
            painter.rect_filled(rect, egui::Rounding::ZERO, drawing_fill(style));
            painter.rect_stroke(
                rect,
                egui::Rounding::ZERO,
                drawing_stroke(style),
            );
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
        if points.len() != 2 {
            return false;
        }
        let rect = egui::Rect::from_two_pos(points[0], points[1]);
        if !rect.expand(radius_px).contains(position) {
            return false;
        }
        // The interior only takes part in the hit-test while the fill is
        // visible; an outline-only rectangle is selectable by its border,
        // and clicks through its middle keep belonging to the chart.
        ctxt.style.fill_alpha > 0 || !rect.shrink(radius_px).contains(position)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)],
            egui::pos2(150.0, 150.0),
        )
    }
}
