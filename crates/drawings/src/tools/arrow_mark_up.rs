//! The buy mark: one click, one filled arrow; the magnet hangs it under the
//! bar's low.

use crate::icons;
#[cfg(test)]
use crate::pos2;
use crate::{MarkScene, Px, PxRect};
use quantick_chart::style::PremultipliedRgba;

use super::mark_core::{Direction, MARKS_FAMILY, MarkPayload};
use super::{
    AnchorSnap, DrawContext, DrawingPayload, DrawingStyle, GlyphSize, MarkTool, ToolFamily,
    ToolShortcut, mark_core,
};
use crate::palette as theme;

pub(super) static TOOL: ArrowMarkUp = ArrowMarkUp;

pub(super) struct ArrowMarkUp;

impl MarkTool for ArrowMarkUp {
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
        "Arrow mark up - one click marks a buy; the magnet hangs it under the bar's low (B)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'B',
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MARKS_FAMILY)
    }
    /// Free by default; the magnet (the rail's, or the object's own) hangs
    /// it off the bar's low.
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
    fn default_color_token(&self) -> quantick_appearance::Token {
        quantick_appearance::Token::DrawingBuy
    }
    fn default_color(&self) -> Option<PremultipliedRgba> {
        Some(theme::BUY)
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        _chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        mark_core::paint(painter, style, points, Direction::Up, ctxt);
    }
    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        mark_core::hit_test(points, position, radius_px, Direction::Up, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (vec![pos2(200.0, 150.0)], pos2(200.0, 162.0))
    }
}
