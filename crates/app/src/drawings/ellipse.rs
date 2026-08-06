use eframe::egui;
use egui_phosphor::regular as icons;

use super::shape_core::{SHAPES_FAMILY, ellipse_outline, hit_outline, paint_outline};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily};

pub(super) static TOOL: Ellipse = Ellipse;

pub(super) struct Ellipse;

impl DrawingToolImpl for Ellipse {
    fn id(&self) -> &'static str {
        "ellipse"
    }
    fn name(&self) -> &'static str {
        "Ellipse"
    }
    fn settings_title(&self) -> &'static str {
        "Ellipse settings"
    }
    fn icon(&self) -> &'static str {
        icons::CIRCLE
    }
    fn hover_text(&self) -> &'static str {
        "Ellipse - click two corners of the area it fits inside"
    }
    fn required_points(&self) -> usize {
        2
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
        if let [first, second, ..] = points {
            paint_outline(painter, style, &ellipse_outline([*first, *second]));
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
        match points {
            [first, second, ..] => hit_outline(
                &ellipse_outline([*first, *second]),
                position,
                radius_px,
                ctxt,
            ),
            _ => false,
        }
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0)],
            egui::pos2(300.0, 150.0),
        )
    }
}
