//! The sell mark: one click, one filled arrow above the bar's high.

use eframe::egui;
use egui_phosphor::regular as icons;

use super::mark_core::{Direction, MARKS_FAMILY, MarkPayload};
use super::{
    AnchorSnap, DrawContext, DrawingPayload, DrawingStyle, DrawingToolImpl, GlyphSize, ToolFamily,
    ToolShortcut, mark_core,
};
use crate::theme;

pub(super) static TOOL: ArrowMarkDown = ArrowMarkDown;

pub(super) struct ArrowMarkDown;

impl DrawingToolImpl for ArrowMarkDown {
    fn id(&self) -> &'static str {
        "arrow-mark-down"
    }
    fn name(&self) -> &'static str {
        "Arrow mark down"
    }
    fn settings_title(&self) -> &'static str {
        "Arrow mark settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROW_FAT_DOWN
    }
    fn hover_text(&self) -> &'static str {
        "Arrow mark down - one click marks a sell above the bar's high (S)"
    }
    fn required_points(&self) -> usize {
        1
    }
    /// `S` for sell, matching `B` for buy on its twin — deliberately *not*
    /// `Shift+B`, which sends a simulated buy at market. Arming a drawing
    /// tool must never share a chord with an order: the rail reads its keys
    /// with `key_pressed`, which does not consume them, so a shared chord
    /// fires both and one stray keystroke both trades and arms a tool.
    ///
    /// The pairing that survives is one letter per side, with the modifier
    /// deciding mark-or-order: `B`/`S` mark, `Shift+B`/`Shift+S` trade. A
    /// missed shift then costs a drawing, never an order.
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::S,
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MARKS_FAMILY)
    }
    fn anchor_snap(&self) -> AnchorSnap {
        AnchorSnap::BarHigh
    }
    fn supports_stroke_width(&self) -> bool {
        false
    }
    fn glyph_size(&self, payload: &dyn DrawingPayload) -> Option<GlyphSize> {
        mark_core::glyph_size(payload)
    }
    fn set_glyph_size(&self, payload: &mut dyn DrawingPayload, px: f32) {
        mark_core::set_glyph_size(payload, px);
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(MarkPayload::default())
    }
    fn default_color(&self) -> Option<egui::Color32> {
        Some(theme::SELL)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        mark_core::paint(painter, style, points, Direction::Down, ctxt);
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        mark_core::hit_test(points, position, radius_px, Direction::Down, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(200.0, 138.0))
    }
}
