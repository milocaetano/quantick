use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::line_core::{Extend, LINES_FAMILY, hit_line, paint_line};
use super::{DrawContext, DrawingStyle, MarkTool, ToolFamily};

pub(super) static TOOL: ExtendedLine = ExtendedLine;

pub(super) struct ExtendedLine;

impl MarkTool for ExtendedLine {
    fn id(&self) -> &'static str {
        "extended-line"
    }
    fn name(&self) -> &'static str {
        "Extended line"
    }
    fn settings_title(&self) -> &'static str {
        "Extended line settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROWS_OUT_LINE_HORIZONTAL
    }
    fn hover_text(&self) -> &'static str {
        "Extended line - two points, running to both chart edges"
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
        _ctxt: &DrawContext<'_>,
    ) {
        paint_line(painter, chart_rect, style, points, Extend::Both, false);
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_line(chart_rect, points, position, radius_px, Extend::Both)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(300.0, 200.0)],
            pos2(200.0, 150.0),
        )
    }
}
