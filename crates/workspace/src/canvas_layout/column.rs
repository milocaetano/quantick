//! What the column painter lays out across the canvas: the context column,
//! its divider and the flow pane, or a rail where one side is folded.

use super::{
    CanvasLayout, MIN_PANE_WIDTH_PX, PaneKind, PaneWidth, Span, clamp_pane_fraction, split,
};

/// One frame of the column painter, along the canvas width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnFrame {
    /// How many context charts are drawn: the lower of what the layout asks
    /// for and what the tab has built.
    pub context_shown: usize,
    pub show_flow: bool,
    /// Both sides are on the canvas, a divider between them.
    pub split: bool,
    /// The context column; `None` when it is hidden or folded to its rail.
    pub time: Option<Span>,
    pub divider: Option<Span>,
    pub flow: Span,
    pub context_rail: Option<Span>,
    pub flow_rail: Option<Span>,
}

/// Lay `layout` across `width` from `start`, with `built` context charts
/// standing and the sides folded as `folds` (context, flow) says. An open
/// split's `fraction` is held to both panes' floors first.
pub fn column_frame(
    layout: CanvasLayout,
    built: usize,
    (start, width): (f32, f32),
    fraction: &mut f32,
    (context_folded, flow_folded): (bool, bool),
) -> ColumnFrame {
    // A layout may name a pane the tab is still building, and half a canvas
    // is better than a frame of nothing.
    let wanted = layout
        .kinds()
        .iter()
        .filter(|kind| **kind == PaneKind::Time);
    let context_shown = wanted.count().min(built);
    let show_time = context_shown > 0;
    // The flow pane stands in for a time pane still being built, so the
    // frame between asking for a time layout and the pane existing shows the
    // market rather than nothing.
    let show_flow = layout.shows_flow() || !show_time;
    let split_both = show_time && show_flow;
    if split_both && !context_folded && !flow_folded && width > 0.0 {
        *fraction = clamp_pane_fraction(fraction.max(MIN_PANE_WIDTH_PX / width));
    }
    let whole = (start, start + width);
    // One visible pane is handed the whole canvas: no divider, no focus
    // frame, though a lone time pane keeps its header.
    let (time, divider, flow) = if split_both {
        let time = match (context_folded, flow_folded) {
            (_, true) => PaneWidth::Auto,
            (true, false) => PaneWidth::Collapsed { restore: *fraction },
            (false, false) => PaneWidth::Manual(*fraction),
        };
        let flow = if flow_folded {
            PaneWidth::Collapsed {
                restore: 1.0 - *fraction,
            }
        } else {
            PaneWidth::Auto
        };
        let row = split(start, width, &[time, flow]);
        (Some(row.panes[0]), Some(row.dividers[0]), row.panes[1])
    } else {
        (show_time.then_some(whole), None, whole)
    };
    // A folded column paints a rail, not charts: eight pixels is a handle,
    // and laying a chart out there would draw a price axis and nothing else.
    let context_rail = time.filter(|_| split_both && context_folded);
    let flow_rail = (split_both && flow_folded).then_some(flow);
    ColumnFrame {
        context_shown,
        show_flow: show_flow && flow_rail.is_none(),
        split: split_both,
        time: time.filter(|_| context_rail.is_none()),
        divider,
        flow,
        context_rail,
        flow_rail,
    }
}
