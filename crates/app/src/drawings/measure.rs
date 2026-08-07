use eframe::egui;
use egui_phosphor::regular as icons;

use super::measure_core::{BOTH_AXES, MEASURE_FAMILY, Measured, hit_measure, paint_measure};
use super::{DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, ToolShortcut};

pub(super) static TOOL: Measure = Measure;

pub(super) struct Measure;

impl DrawingToolImpl for Measure {
    fn id(&self) -> &'static str {
        "measure"
    }
    fn name(&self) -> &'static str {
        "Ruler"
    }
    fn settings_title(&self) -> &'static str {
        "Ruler settings"
    }
    fn icon(&self) -> &'static str {
        icons::RULER
    }
    fn hover_text(&self) -> &'static str {
        "Ruler - drag a leg to read it in points, percent, bars and time (M)"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::M,
            shift: false,
        })
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
            Measured {
                anchors: ctxt.anchors,
                axes: BOTH_AXES,
                unit: ctxt.unit,
                halo: ctxt.halo,
            },
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
            BOTH_AXES,
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
