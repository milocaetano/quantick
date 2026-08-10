use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{Extend, LINES_FAMILY, hit_line, levelled_far_end, paint_line};
use super::{Constrain, DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut};

pub(super) static TOOL: TrendLine = TrendLine;

pub(super) struct TrendLine;

impl DrawingToolImpl for TrendLine {
    fn id(&self) -> &'static str {
        "trend-line"
    }
    fn name(&self) -> &'static str {
        "Trend line"
    }
    fn settings_title(&self) -> &'static str {
        "Trend line settings"
    }
    fn icon(&self) -> &'static str {
        icons::TREND_UP
    }
    fn hover_text(&self) -> &'static str {
        "Trend line - two swing points, Shift to keep it level (T)"
    }
    fn required_points(&self) -> usize {
        2
    }
    /// Advertised on the step it applies to, beside the cursor, while the
    /// hand is already doing the gesture — a modifier nobody is told about is
    /// a modifier nobody uses.
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        (placed == 1).then_some("Drag out the line - hold Shift to keep it level")
    }
    /// Shift lays the far end at the price the near end is on: a level the
    /// trader drew *as* a line, rather than a level that is nearly one.
    fn pending_anchor(
        &self,
        placed: &[egui::Pos2],
        cursor: egui::Pos2,
        constrain: Constrain,
    ) -> egui::Pos2 {
        levelled_far_end(placed, cursor, constrain)
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::T,
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
        paint_line(painter, chart_rect, style, points, Extend::None, false);
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_line(chart_rect, points, position, radius_px, Extend::None)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0)],
            egui::pos2(200.0, 150.0),
        )
    }
}
