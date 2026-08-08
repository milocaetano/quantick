//! The buy mark: one click, one filled arrow under the bar's low.

use eframe::egui;
use egui_phosphor::regular as icons;

use super::mark_core::{Direction, MARKS_FAMILY, MarkPayload};
use super::{
    AnchorSnap, DrawContext, DrawingPayload, DrawingStyle, DrawingToolImpl, GlyphSize, ToolFamily,
    ToolShortcut, mark_core,
};
use crate::theme;

pub(super) static TOOL: ArrowMarkUp = ArrowMarkUp;

pub(super) struct ArrowMarkUp;

impl DrawingToolImpl for ArrowMarkUp {
    fn id(&self) -> &'static str {
        "arrow-mark-up"
    }
    fn name(&self) -> &'static str {
        "Arrow mark up"
    }
    fn settings_title(&self) -> &'static str {
        "Arrow mark settings"
    }
    /// The *filled* arrow, not one of the rail's line arrows. A solid
    /// silhouette reads as a mark placed on the chart; an outlined one reads
    /// as a stroke someone drew.
    fn icon(&self) -> &'static str {
        icons::ARROW_FAT_UP
    }
    fn hover_text(&self) -> &'static str {
        "Arrow mark up - one click marks a buy under the bar's low (B)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: egui::Key::B,
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MARKS_FAMILY)
    }
    /// The rule that makes it a mark: it hangs off the bar's low whether or
    /// not the OHLC magnet is on.
    fn anchor_snap(&self) -> AnchorSnap {
        AnchorSnap::BarLow
    }
    /// A filled glyph has no stroke to widen — it has a size.
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
    /// Born in the buy colour, not the shared drawing blue: a buy mark that
    /// arrives blue makes the trader repaint it every single time.
    fn default_color(&self) -> Option<egui::Color32> {
        Some(theme::BUY)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        ctxt: &DrawContext<'_>,
    ) {
        mark_core::paint(painter, style, points, Direction::Up, ctxt);
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        mark_core::hit_test(points, position, radius_px, Direction::Up, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(200.0, 162.0))
    }
}
