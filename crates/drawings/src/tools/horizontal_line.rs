use crate::{MarkScene, pos2, Px, PxRect};
use crate::icons;

use super::line_core::{Axis, LINES_FAMILY, hit_axis, paint_axis, single_level};
use super::{AxisLevels, DrawContext, DrawingStyle, MarkTool, ToolFamily, ToolShortcut};

pub(super) static TOOL: HorizontalLine = HorizontalLine;

pub(super) struct HorizontalLine;

impl MarkTool for HorizontalLine {
    /// Edge to edge: the anchor says *what price*, the drawing covers the
    /// whole width. See [`MarkTool::painted_bounds`].
    fn painted_bounds(&self, anchors: PxRect, chart: PxRect) -> PxRect {
        PxRect::from_min_max(
            pos2(chart.left(), anchors.top()),
            pos2(chart.right(), anchors.bottom()),
        )
    }
    fn id(&self) -> &'static str {
        "horizontal-line"
    }
    fn name(&self) -> &'static str {
        "Horizontal line"
    }
    fn settings_title(&self) -> &'static str {
        "Horizontal line settings"
    }
    fn icon(&self) -> &'static str {
        icons::MINUS
    }
    fn hover_text(&self) -> &'static str {
        "Horizontal line - click a price (H)"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'H',
            shift: false,
        })
    }
    fn levels_span_the_body(&self) -> bool {
        true
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
        paint_axis(painter, chart_rect, style, points, Axis::Horizontal);
    }
    /// The price it names. This tool exists to say one, so the axis says it
    /// too — see [`MarkTool::axis_levels`].
    fn axis_levels(&self, _chart_rect: PxRect, points: &[Px]) -> AxisLevels {
        // Edge to edge: this tool's stroke spans the whole width whatever its
        // anchor's x, so there is no rect it can be outside of.
        single_level(points)
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_axis(chart_rect, points, position, radius_px, Axis::Horizontal)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (vec![pos2(100.0, 120.0)], pos2(450.0, 123.0))
    }
}
