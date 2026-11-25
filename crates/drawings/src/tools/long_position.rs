use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;

use super::position_core::{self as core, LONG_ICON, POSITION_FAMILY, PositionPayload, Side};
use super::{
    Constrain, DrawContext, DrawingPayload, DrawingStyle, Handles, MarkTool, PointerHint,
    ToolFamily,
};

/// The position tool's id for a long plan.
pub const TOOL_ID: &str = "long-position";

pub(super) static TOOL: LongPosition = LongPosition;

pub(super) struct LongPosition;

impl MarkTool for LongPosition {
    fn id(&self) -> &'static str {
        TOOL_ID
    }
    fn name(&self) -> &'static str {
        "Long position"
    }
    fn settings_title(&self) -> &'static str {
        "Long position settings"
    }
    fn icon(&self) -> &'static str {
        crate::icons::ARROW_FAT_UP
    }
    fn icon_strokes(&self) -> super::IconStrokes {
        LONG_ICON
    }
    fn hover_text(&self) -> &'static str {
        "Long position - click the entry, then the stop or the target; the other leg follows the ratio"
    }
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        (placed == 1).then_some("Click the stop (below) or the target (above)")
    }
    fn required_points(&self) -> usize {
        2
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(POSITION_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn price_band_only(&self) -> bool {
        true
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(PositionPayload::default())
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        core::paint(painter, chart_rect, style, points, ctxt, Side::Long);
    }
    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        core::hit_test(points, position, radius_px, ctxt, Side::Long)
    }
    fn drag_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        _ctxt: &DrawContext<'_>,
        constrain: Constrain,
    ) -> Option<Handles> {
        core::drag_line(points, handle, to, constrain)
    }
    fn drag_payload_handle(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        handle: usize,
        to: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<Box<dyn DrawingPayload>> {
        core::drag_projected(points, handle, to, ctxt, Side::Long)
    }
    fn handle_hint(&self, handle: usize) -> Option<PointerHint> {
        core::line_hint(handle)
    }
    fn pointer_hint(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        ctxt: &DrawContext<'_>,
    ) -> Option<PointerHint> {
        core::pointer_hint(points, position, ctxt, Side::Long)
    }
    fn click(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        payload: &mut dyn DrawingPayload,
    ) -> bool {
        core::click(points, position, payload)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 200.0), pos2(300.0, 250.0)],
            pos2(200.0, 225.0),
        )
    }
}
