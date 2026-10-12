use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::line_core::{Extend, LINES_FAMILY, hit_line, levelled_far_end, paint_line};
use super::{Constrain, DrawContext, DrawingStyle, MarkTool, ToolFamily, ToolShortcut};

pub(super) static TOOL: TrendLine = TrendLine;

pub(super) struct TrendLine;

impl MarkTool for TrendLine {
    fn id(&self) -> &'static str {
        "trend-line"
    }
    fn name(&self) -> &'static str {
        "Trend line"
    }
    fn settings_title(&self) -> &'static str {
        "Trend line settings"
    }
    fn icon(&self) -> &'static str {
        icons::TREND_UP
    }
    fn hover_text(&self) -> &'static str {
        "Trend line - two swing points, Shift to keep it level (T)"
    }
    fn required_points(&self) -> usize {
        2
    }
    /// Advertised on the step it applies to, beside the cursor, while the
    /// hand is already doing the gesture — a modifier nobody is told about is
    /// a modifier nobody uses.
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        (placed == 1).then_some("Drag out the line - hold Shift to keep it level")
    }
    /// Shift lays the far end at the price the near end is on: a level the
    /// trader drew *as* a line, rather than a level that is nearly one.
    fn pending_anchor(
        &self,
        placed: &[Px],
        cursor: Px,
        constrain: Constrain,
    ) -> Px {
        levelled_far_end(placed, cursor, constrain)
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'T',
            shift: false,
        })
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
        paint_line(painter, chart_rect, style, points, Extend::None, false);
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
