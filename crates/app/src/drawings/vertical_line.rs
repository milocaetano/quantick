use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{Axis, LINES_FAMILY, hit_axis, paint_axis};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut};

pub(super) static TOOL: VerticalLine = VerticalLine;

pub(super) struct VerticalLine;

impl DrawingToolImpl for VerticalLine {
    fn id(&self) -> &'static str {
        "vertical-line"
    }
    fn name(&self) -> &'static str {
        "Vertical line"
    }
    fn settings_title(&self) -> &'static str {
        "Vertical line settings"
    }
    fn icon(&self) -> &'static str {
        icons::LINE_VERTICAL
    }
    fn hover_text(&self) -> &'static str {
        "Vertical line - click a bar to mark the moment (V)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::V,
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(LINES_FAMILY)
    }
    /// A moment, not a level: the anchor's second coordinate is whatever
    /// pixel the click landed on. So the line belongs to no band and runs
    /// through all of them, as one object.
    fn value_axis(&self) -> bool {
        false
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) {
        paint_axis(painter, chart_rect, style, points, Axis::Vertical);
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_axis(chart_rect, points, position, radius_px, Axis::Vertical)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(202.0, 300.0))
    }
}
