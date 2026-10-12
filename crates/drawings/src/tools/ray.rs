use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::line_core::{Extend, LINES_FAMILY, hit_line, paint_line};
use super::{DrawContext, DrawingStyle, MarkTool, ToolFamily, ToolShortcut};

pub(super) static TOOL: Ray = Ray;

pub(super) struct Ray;

impl MarkTool for Ray {
    fn id(&self) -> &'static str {
        "ray"
    }
    fn name(&self) -> &'static str {
        "Ray"
    }
    fn settings_title(&self) -> &'static str {
        "Ray settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROW_UP_RIGHT
    }
    fn hover_text(&self) -> &'static str {
        "Ray - two points, then it runs on into the future (Shift+T)"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'T',
            shift: true,
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
        paint_line(painter, chart_rect, style, points, Extend::Forward, false);
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_line(chart_rect, points, position, radius_px, Extend::Forward)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(300.0, 200.0)],
            pos2(200.0, 150.0),
        )
    }
}
