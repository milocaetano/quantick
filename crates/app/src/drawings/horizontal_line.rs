use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{Axis, LINES_FAMILY, hit_axis, paint_axis, single_level};
use super::{AxisLevels, DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut};

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
    /// The price it names. This tool exists to say one, so the axis says it
    /// too — see [`DrawingToolImpl::axis_levels`].
    fn axis_levels(&self, _chart_rect: egui::Rect, points: &[egui::Pos2]) -> AxisLevels {
        // Edge to edge: this tool's stroke spans the whole width whatever its
        // anchor's x, so there is no rect it can be outside of.
        single_level(points)
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
