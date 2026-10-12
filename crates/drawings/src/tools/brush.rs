//! The pencil: a freehand stroke, anchored to the tape like everything else.
//!
//! This is the first tool in the registry placed by a *held drag* rather
//! than by N clicks, which is what [`MarkTool::freehand`] exists to
//! say. The host captures the path; the tool only draws it.
//!
//! The honest compromise, and it is the trader's own call: every point is
//! anchored in (bar, price), so a circled cluster deforms into an ellipse
//! when the vertical zoom changes — but it stays on top of the thing that
//! was circled. Anchoring only the first point and keeping the shape in
//! pixels would look better and would start pointing at the wrong bars.
//! Deforming is acceptable; lying is not.

use crate::{MarkScene, MarkShape, Px, PxRect, Stroke};
#[cfg(test)]
use crate::pos2;
use crate::icons;

use super::{
    Constrain, DrawContext, DrawingStyle, MarkTool, Handles, ToolFamily, ToolShortcut,
    distance_to_segment,
};

pub(super) static TOOL: Brush = Brush;

pub(super) struct Brush;

/// The rail family the freehand tools share. `highlighter` is the obvious
/// second member when it lands.
pub(super) const BRUSH_FAMILY: ToolFamily = ToolFamily {
    id: "brush",
    title: "Freehand",
    icon: icons::SCRIBBLE,
    icon_strokes: &[],
    icon_dots: &[],
    icon_letter: None,
};

impl MarkTool for Brush {
    fn id(&self) -> &'static str {
        "brush"
    }
    fn name(&self) -> &'static str {
        "Pencil"
    }
    fn settings_title(&self) -> &'static str {
        "Pencil settings"
    }
    /// `SCRIBBLE`, not `PENCIL`. In an app with an inspector, a pencil glyph
    /// reads as "edit this"; the scribble shows the *result* instead of the
    /// instrument, which is the only thing that says "whatever your hand
    /// does is what you get".
    fn icon(&self) -> &'static str {
        icons::SCRIBBLE
    }
    fn hover_text(&self) -> &'static str {
        "Pencil - hold and draw (P)"
    }
    /// Zero means "as many as the gesture gives": the host never completes
    /// this draft on an anchor count, it completes it on the release.
    fn required_points(&self) -> usize {
        0
    }
    fn freehand(&self) -> bool {
        true
    }
    fn placement_hint(&self, _placed: usize) -> Option<&'static str> {
        Some("hold and draw")
    }
    fn shortcut(&self) -> Option<ToolShortcut> {
        Some(ToolShortcut {
            key: 'P',
            shift: false,
        })
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(BRUSH_FAMILY)
    }
    /// No handles at all, deliberately.
    ///
    /// The default would put one ring on every captured point — a cloud of
    /// discs nobody can aim at, over a shape whose individual points mean
    /// nothing. A scribble is moved as a whole, which dragging its body
    /// already does; a trader who wants a point they can place exactly is
    /// reaching for the line tool, not this one.
    fn handles(
        &self,
        _chart_rect: PxRect,
        _points: &[Px],
        _ctxt: &DrawContext<'_>,
    ) -> Option<Handles> {
        Some(Handles::new())
    }
    fn drag_handle(
        &self,
        _chart_rect: PxRect,
        _points: &[Px],
        _handle: usize,
        _to: Px,
        _ctxt: &DrawContext<'_>,
        _constrain: Constrain,
    ) -> Option<Handles> {
        // Unreachable: there are no handles to drag. Answered explicitly
        // because the port asks a tool that owns its handles to own their
        // drag too, and "none" is an answer, not an omission.
        None
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        _chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        _ctxt: &DrawContext<'_>,
    ) {
        if points.len() < 2 {
            return;
        }
        painter.add(MarkShape::line(
            points.to_vec(),
            Stroke::new(style.width_px, style.color),
        ));
    }
    fn hit_test(
        &self,
        _chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        points
            .windows(2)
            .any(|pair| distance_to_segment(position, pair[0], pair[1]) <= radius_px)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![
                pos2(200.0, 150.0),
                pos2(230.0, 180.0),
                pos2(260.0, 150.0),
            ],
            pos2(230.0, 180.0),
        )
    }
}
