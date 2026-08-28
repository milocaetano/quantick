use eframe::egui;
use egui_phosphor::regular as icons;

use super::fib::{self, FibKind, FibPayload};
use super::{DrawContext, Drawing, DrawingPayload, DrawingStyle, DrawingToolImpl, ToolShortcut, PresetHost};

pub(super) static TOOL: FibRetracement = FibRetracement;

pub(super) struct FibRetracement;

impl DrawingToolImpl for FibRetracement {
    fn id(&self) -> &'static str {
        "fib-retracement"
    }
    fn name(&self) -> &'static str {
        "Fib retracement"
    }
    fn settings_title(&self) -> &'static str {
        "Fib retracement settings"
    }
    fn icon(&self) -> &'static str {
        icons::ROWS
    }
    fn icon_strokes(&self) -> super::IconStrokes {
        fib::FIB_RETRACEMENT_ICON
    }
    fn icon_dots(&self) -> super::IconDots {
        fib::FIB_RETRACEMENT_DOTS
    }
    fn icon_letter(&self) -> Option<super::IconLetter> {
        Some(fib::FIB_RETRACEMENT_LETTER)
    }
    fn hover_text(&self) -> &'static str {
        "Fib retracement - click two points or drag (F)"
    }
    fn required_points(&self) -> usize {
        FibKind::Retracement.required_points()
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::F,
            shift: false,
        })
    }
    fn family(&self) -> Option<super::ToolFamily> {
        Some(fib::FIB_FAMILY)
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(FibPayload::new(FibKind::Retracement))
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Levels")
    }
    fn draw_extra_tab(
        &self,
        ui: &mut egui::Ui,
        drawing: &mut Drawing,
        host: &mut dyn PresetHost,
    ) -> bool {
        fib::remember_drawing_color(ui, drawing.style.color);
        fib::draw_levels_tab(ui, drawing, host)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        fib::paint(painter, chart_rect, style, points, ctxt);
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        fib::hit_test(chart_rect, points, position, radius_px, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(250.0, 200.0)],
            // Between the anchors, on the 50 % line halfway down the leg:
            // that is where a retracement's levels live now, so it is where
            // one is grabbed. Right of the last anchor there is nothing to
            // hit any more — see `Extend::for_kind`.
            egui::pos2(175.0, 150.0),
        )
    }
}
