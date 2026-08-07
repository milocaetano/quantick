use eframe::egui;
use egui_phosphor::regular as icons;

use super::measure_core::{MEASURE_FAMILY, TIME_ONLY, hit_measure, paint_measure};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily};

pub(super) static TOOL: DateRange = DateRange;

pub(super) struct DateRange;

impl DrawingToolImpl for DateRange {
    fn id(&self) -> &'static str {
        "date-range"
    }
    fn name(&self) -> &'static str {
        "Date range"
    }
    fn settings_title(&self) -> &'static str {
        "Date range settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROWS_HORIZONTAL
    }
    fn hover_text(&self) -> &'static str {
        "Date range - two bars, read in bars and elapsed time"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MEASURE_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        paint_measure(
            painter,
            chart_rect,
            style,
            points,
            ctxt.anchors,
            TIME_ONLY,
            ctxt.halo,
        );
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_measure(
            chart_rect,
            ctxt.style,
            points,
            position,
            radius_px,
            TIME_ONLY,
        )
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 200.0), egui::pos2(300.0, 100.0)],
            egui::pos2(200.0, 150.0),
        )
    }
}
