use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::measure_core::{Measured, MEASURE_FAMILY, PRICE_ONLY, hit_measure, paint_measure};
use super::{DrawContext, DrawingStyle, MarkTool, ToolFamily};

pub(super) static TOOL: PriceRange = PriceRange;

pub(super) struct PriceRange;

impl MarkTool for PriceRange {
    fn id(&self) -> &'static str {
        "price-range"
    }
    fn name(&self) -> &'static str {
        "Price range"
    }
    fn settings_title(&self) -> &'static str {
        "Price range settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROWS_VERTICAL
    }
    fn hover_text(&self) -> &'static str {
        "Price range - two prices, read in points and percent"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn levels_span_the_body(&self) -> bool {
        true
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MEASURE_FAMILY)
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        paint_measure(
            painter,
            chart_rect,
            style,
            points,
            Measured {
                anchors: ctxt.anchors,
                axes: PRICE_ONLY,
                unit: ctxt.unit,
                halo: ctxt.halo,
                primary_band: ctxt.primary_band,
                text: ctxt.text,
            },
        );
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        hit_measure(
            chart_rect,
            ctxt.style,
            points,
            position,
            radius_px,
            PRICE_ONLY,
        )
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 200.0), pos2(300.0, 100.0)],
            pos2(200.0, 150.0),
        )
    }
}
