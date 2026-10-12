use crate::{MarkScene, Px, PxRect};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::line_core::levelled_far_end;
use super::measure_core::{BOTH_AXES, MEASURE_FAMILY, Measured, hit_measure, paint_measure};
use super::{Constrain, DrawContext, DrawingStyle, MarkTool, ToolFamily, ToolShortcut};

pub(super) static TOOL: Measure = Measure;

pub(super) struct Measure;

impl MarkTool for Measure {
    fn id(&self) -> &'static str {
        "measure"
    }
    fn name(&self) -> &'static str {
        "Ruler"
    }
    fn settings_title(&self) -> &'static str {
        "Ruler settings"
    }
    fn icon(&self) -> &'static str {
        icons::RULER
    }
    fn hover_text(&self) -> &'static str {
        "Ruler - drag a leg to read it in points, percent, bars and time, Shift to keep it level (M)"
    }
    fn required_points(&self) -> usize {
        2
    }
    /// Shift holds the far end at the near end's price, as on a trend line.
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
            key: 'M',
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(MEASURE_FAMILY)
    }
    /// The ruler is a leg in a dashed hint box; a fill would hide the
    /// candles it measures.
    fn supports_fill(&self) -> bool {
        false
    }
    fn default_fill_alpha(&self) -> Option<u8> {
        Some(0)
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
                axes: BOTH_AXES,
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
            BOTH_AXES,
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
