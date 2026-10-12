use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::line_core::{Extend, LINES_FAMILY, hit_line, paint_line};
use super::{DrawContext, DrawingStyle, MarkTool, ToolFamily};

pub(super) static TOOL: Arrow = Arrow;

pub(super) struct Arrow;

impl MarkTool for Arrow {
    fn id(&self) -> &'static str {
        "arrow"
    }
    fn name(&self) -> &'static str {
        "Arrow"
    }
    fn settings_title(&self) -> &'static str {
        "Arrow settings"
    }
    fn icon(&self) -> &'static str {
        icons::FLOW_ARROW
    }
    fn hover_text(&self) -> &'static str {
        "Arrow - click the tail, then the point it marks"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(LINES_FAMILY)
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        // The halo pass is a widened copy of the stroke; painting the head
        // twice would blunt it into a blob, so the head is geometry-only.
        paint_line(
            painter,
            chart_rect,
            style,
            points,
            Extend::None,
            !ctxt.halo,
        );
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_line(chart_rect, points, position, radius_px, Extend::None)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(300.0, 200.0)],
            pos2(200.0, 150.0),
        )
    }
}
