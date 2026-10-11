use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::shape_core::{SHAPES_FAMILY, ellipse_outline, hit_outline, paint_outline};
use super::{DrawContext, DrawingStyle, MarkTool, ToolFamily};

pub(super) static TOOL: Ellipse = Ellipse;

pub(super) struct Ellipse;

impl MarkTool for Ellipse {
    fn id(&self) -> &'static str {
        "ellipse"
    }
    fn name(&self) -> &'static str {
        "Ellipse"
    }
    fn settings_title(&self) -> &'static str {
        "Ellipse settings"
    }
    fn icon(&self) -> &'static str {
        icons::CIRCLE
    }
    fn hover_text(&self) -> &'static str {
        "Ellipse - click two corners of the area it fits inside"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(SHAPES_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        _chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        _ctxt: &DrawContext<'_>,
    ) {
        if let [first, second, ..] = points {
            paint_outline(painter, style, &ellipse_outline([*first, *second]));
        }
    }
    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        match points {
            [first, second, ..] => hit_outline(
                &ellipse_outline([*first, *second]),
                position,
                radius_px,
                ctxt,
            ),
            _ => false,
        }
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(300.0, 200.0)],
            pos2(300.0, 150.0),
        )
    }
}
