use crate::icons;
#[cfg(test)]
use crate::pos2;
use crate::{MarkScene, Px, PxRect};

use super::fib::{self, FibKind, FibPayload};
use super::{DrawContext, DrawingPayload, DrawingStyle, MarkTool, ToolShortcut};

pub(super) static TOOL: FibExtension = FibExtension;

pub(super) struct FibExtension;

impl MarkTool for FibExtension {
    fn id(&self) -> &'static str {
        "fib-extension"
    }
    fn name(&self) -> &'static str {
        "Fib extension"
    }
    fn settings_title(&self) -> &'static str {
        "Fib extension settings"
    }
    fn icon(&self) -> &'static str {
        icons::ROWS_PLUS_TOP
    }
    fn icon_strokes(&self) -> super::IconStrokes {
        fib::FIB_EXTENSION_ICON
    }
    fn icon_dots(&self) -> super::IconDots {
        fib::FIB_EXTENSION_DOTS
    }
    fn icon_letter(&self) -> Option<super::IconLetter> {
        Some(fib::FIB_EXTENSION_LETTER)
    }
    /// No key is named here, and that is deliberate: this tool gave up
    /// `Shift+F` because that key flattens the position (see
    /// [`Self::shortcut`]). The tooltip went on advertising it long after,
    /// which told the trader to press the one key that closes their position
    /// to reach a drawing tool.
    ///
    /// It does name the icon's letter. The registry calls this tool
    /// *extension* and the icon says `P`, for the projection it draws; a
    /// trader who has not met the pair yet meets them here, in the one place
    /// they go to ask what a button is.
    fn hover_text(&self) -> &'static str {
        "Fib extension (P for projection) - drag the leg"
    }
    fn placement_hint(&self, placed: usize) -> Option<&'static str> {
        match placed {
            1 => Some("Drag the leg"),
            _ => None,
        }
    }
    fn required_points(&self) -> usize {
        FibKind::Extension.required_points()
    }
    /// Two for every new extension, three for one saved before the gesture
    /// lost its separate origin — that one keeps projecting from it.
    fn accepts_points(&self, count: usize) -> bool {
        FibKind::Extension.accepts_points(count)
    }
    /// No key of its own, and that is the fix rather than the omission.
    ///
    /// It used to be `Shift+F`, which is also *flatten* — close the position
    /// and cancel every working order. The rail reads its shortcuts with
    /// `key_pressed`, which does not consume them, so reaching for this tool
    /// also flattened, and reaching for flatten also armed a Fib. The rail's
    /// family flyout already reaches this tool in two clicks; an order key is
    /// not something a drawing tool may share.
    fn shortcut(&self) -> Option<ToolShortcut> {
        None
    }
    fn family(&self) -> Option<super::ToolFamily> {
        Some(fib::FIB_FAMILY)
    }
    fn default_payload(&self) -> Box<dyn DrawingPayload> {
        Box::new(FibPayload::new(FibKind::Extension))
    }
    fn extra_tab(&self) -> Option<&'static str> {
        Some("Levels")
    }
    fn paint(
        &self,
        painter: &mut MarkScene,
        chart_rect: PxRect,
        style: DrawingStyle,
        points: &[Px],
        ctxt: &DrawContext<'_>,
    ) {
        fib::paint(painter, chart_rect, style, points, ctxt);
    }
    fn hit_test(
        &self,
        chart_rect: PxRect,
        points: &[Px],
        position: Px,
        radius_px: f32,
        ctxt: &DrawContext<'_>,
    ) -> bool {
        fib::hit_test(chart_rect, points, position, radius_px, ctxt)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<Px>, Px) {
        (
            vec![pos2(100.0, 100.0), pos2(200.0, 200.0)],
            // Right of B on the 100 % level: the levels project forward
            // from there, so that is where the object can be grabbed.
            pos2(300.0, 300.0),
        )
    }
}
