use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{Axis, LINES_FAMILY, hit_axis, paint_axis};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut};

pub(super) static TOOL: HorizontalLine = HorizontalLine;

pub(super) struct HorizontalLine;

impl DrawingToolImpl for HorizontalLine {
    /// Edge to edge: the anchor says *what price*, the drawing covers the
    /// whole width. See [`DrawingToolImpl::painted_bounds`].
    fn painted_bounds(&self, anchors: egui::Rect, chart: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(chart.left(), anchors.top()),
            egui::pos2(chart.right(), anchors.bottom()),
        )
    }
    fn id(&self) -> &'static str {
        "horizontal-line"
    }
    fn name(&self) -> &'static str {
        "Horizontal line"
    }
    fn settings_title(&self) -> &'static str {
        "Horizontal line settings"
    }
    fn icon(&self) -> &'static str {
        icons::MINUS
    }
    fn hover_text(&self) -> &'static str {
        "Horizontal line - click a price (H)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::H,
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(LINES_FAMILY)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) {
        paint_axis(painter, chart_rect, style, points, Axis::Horizontal);
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_axis(chart_rect, points, position, radius_px, Axis::Horizontal)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(100.0, 120.0)], egui::pos2(450.0, 123.0))
    }
}
