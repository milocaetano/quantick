//! Which pane holds focus: the one the trader asked for while it is drawn,
//! otherwise the first pane that is.

use crate::pane::PaneSide;

/// What the canvas draws this frame: the flow pane, and each context slot in
/// display order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DrawnPanes {
    flow: bool,
    /// Bit `slot` set = that context chart is drawn.
    context: u32,
}

impl DrawnPanes {
    /// `context` yields, top to bottom, whether each carved slot is drawn
    /// (`false` = collapsed or its column put away). Slots past 32 are ignored.
    #[must_use]
    pub fn new(flow: bool, context: impl IntoIterator<Item = bool>) -> Self {
        let context = context
            .into_iter()
            .take(32)
            .enumerate()
            .fold(0, |bits, (slot, drawn)| bits | (u32::from(drawn) << slot));
        Self { flow, context }
    }

    /// Whether `side` is on screen.
    #[must_use]
    pub fn shows(self, side: PaneSide) -> bool {
        match side {
            PaneSide::Flow => self.flow,
            PaneSide::Time(slot) => slot < 32 && self.context & (1 << slot) != 0,
        }
    }

    /// How many panes are on screen. Focus is only worth marking past one.
    #[must_use]
    pub fn count(self) -> usize {
        usize::from(self.flow) + self.context.count_ones() as usize
    }

    /// The top drawn context chart, else the flow pane, else nothing.
    #[must_use]
    pub fn first(self) -> Option<PaneSide> {
        (self.context != 0)
            .then(|| PaneSide::Time(self.context.trailing_zeros() as usize))
            .or(self.flow.then_some(PaneSide::Flow))
    }
}

/// The focused pane: `wanted` while it is drawn, else [`DrawnPanes::first`].
/// `None` only when nothing is drawn, for the caller's own fallback.
///
/// Every drawn pane can hold focus, whatever else is collapsed: deciding by
/// collapse flag instead of by what is drawn is how a click on the second
/// context chart did nothing once the flow pane was put away.
#[must_use]
pub fn resolve_focus(wanted: PaneSide, drawn: DrawnPanes) -> Option<PaneSide> {
    if drawn.shows(wanted) {
        Some(wanted)
    } else {
        drawn.first()
    }
}

#[cfg(test)]
mod pane_focus_tests {
    use super::*;

    const FLOW: PaneSide = PaneSide::Flow;
    const TOP: PaneSide = PaneSide::Time(0);
    const BOTTOM: PaneSide = PaneSide::Time(1);

    fn drawn(flow: bool, context: &[bool]) -> DrawnPanes {
        DrawnPanes::new(flow, context.iter().copied())
    }

    #[test]
    fn every_drawn_pane_takes_focus_in_every_combination() {
        // Each layout × collapse state the canvas can draw, as what it draws.
        let states: [(&str, bool, &[bool]); 9] = [
            ("flow", true, &[]),
            ("time", false, &[true]),
            ("time+flow", true, &[true]),
            ("time+flow, context put away", true, &[false]),
            ("time+flow, flow put away", false, &[true]),
            ("time+time+flow", true, &[true, true]),
            ("time+time+flow, flow put away", false, &[true, true]),
            ("time+time+flow, top collapsed", true, &[false, true]),
            ("time+time+flow, bottom collapsed", true, &[true, false]),
        ];
        for (name, flow, context) in states {
            let panes = drawn(flow, context);
            for side in [FLOW, TOP, BOTTOM] {
                let resolved = resolve_focus(side, panes);
                if panes.shows(side) {
                    assert_eq!(
                        resolved,
                        Some(side),
                        "{name}: a click on {side:?} must hold"
                    );
                } else {
                    let fallback = resolved.expect("a drawn pane to fall back to");
                    assert!(
                        panes.shows(fallback),
                        "{name}: {side:?} fell to hidden {fallback:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_bottom_chart_keeps_focus_with_the_flow_pane_put_away() {
        assert_eq!(
            resolve_focus(BOTTOM, drawn(false, &[true, true])),
            Some(BOTTOM)
        );
    }

    #[test]
    fn a_hidden_pane_falls_back_to_the_top_drawn_context_chart() {
        assert_eq!(resolve_focus(FLOW, drawn(false, &[true, true])), Some(TOP));
        assert_eq!(
            resolve_focus(TOP, drawn(true, &[false, true])),
            Some(BOTTOM)
        );
        assert_eq!(resolve_focus(BOTTOM, drawn(true, &[true])), Some(TOP));
        assert_eq!(resolve_focus(TOP, drawn(true, &[false])), Some(FLOW));
    }

    #[test]
    fn nothing_drawn_leaves_the_fallback_to_the_caller() {
        assert_eq!(resolve_focus(TOP, drawn(false, &[])), None);
    }

    #[test]
    fn only_a_split_has_focus_to_mark() {
        assert_eq!(drawn(true, &[]).count(), 1);
        assert_eq!(drawn(false, &[true, false]).count(), 1);
        assert_eq!(drawn(false, &[true, true]).count(), 2);
        assert_eq!(drawn(true, &[true, true]).count(), 3);
    }
}
