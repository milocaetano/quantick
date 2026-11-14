//! The column painter's own decisions over a canvas: what a divider drag
//! asks for and the split a fold keeps, which pane a layout switch reveals,
//! and whether the column painter draws a canvas at all. The window keeps
//! the split and the focus; what they become is decided here.

use super::metrics::{COLLAPSE_AT_PX, COLLAPSED_PANE_WIDTH_PX, MIN_PANE_WIDTH_PX};
use super::presets::{LayoutPreset, PaneKind};
use super::rail::Hand;
use super::state::CanvasState;

/// The kind of pane a switch from `previous` to `next` reveals, which takes
/// the focus: the pane that just appeared is the one the trader asked to work
/// on, so the chrome speaks for it without a second click.
///
/// A layout without one kind reveals the other; a split reveals whichever
/// pane `previous` was not showing: the time pane coming from the flow pane
/// alone, the flow pane coming from the time pane alone. `None` when nothing
/// new appeared, and the focus stays where it was.
#[must_use]
pub fn revealed(previous: &LayoutPreset, next: &LayoutPreset) -> Option<PaneKind> {
    let shows = |preset: &LayoutPreset, kind| preset.kinds.contains(&kind);
    match () {
        _ if !shows(next, PaneKind::Flow) => Some(PaneKind::Time),
        _ if !shows(next, PaneKind::Time) => Some(PaneKind::Flow),
        _ if !shows(previous, PaneKind::Time) => Some(PaneKind::Time),
        _ if !shows(previous, PaneKind::Flow) => Some(PaneKind::Flow),
        _ => None,
    }
}

impl LayoutPreset {
    /// How many panes of `kind` this preset lays out.
    #[must_use]
    pub fn count(&self, kind: PaneKind) -> usize {
        self.kinds.iter().filter(|held| **held == kind).count()
    }
}

impl CanvasState {
    /// Whether the column painter draws this canvas for the layout `preset`,
    /// a column layout when `column`: laid out as its preset, nothing
    /// floating or maximized, no strip folded, and at most one side of the
    /// flow pane on its rail, the fold the column painter draws as its own.
    /// Anything else is painted from the tree, the same rail kept.
    #[must_use]
    pub fn column_painted(&self, preset: &LayoutPreset, column: bool) -> bool {
        column
            && self.floating().is_empty()
            && self.maximized().is_none()
            && self.minimized().is_empty()
            && self.one_side_railed()
            && self.preset() == Some(preset)
    }
}

/// The column painter's divider or rail drag in flight: where the divider
/// stood at the press, the pointer's travel since, and the folds and the
/// split the press found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnHold {
    at: f32,
    travel: f32,
    from: (bool, bool),
    split: f32,
}

impl CanvasState {
    /// The hand on the column painter's divider or rail, one frame, the
    /// divider drawn `at` points into a canvas `width` wide and `split` the
    /// painter's: a press takes `hold`, a move lays the columns out by
    /// [`column_drag`], and a fold keeps the split the press found, so a
    /// restore opens the side where it stood rather than at the floor the
    /// drag passed. Letting go drops the hold; a press without a move does
    /// nothing. Answers whether the canvas or the split changed.
    pub fn column_hand(
        &mut self,
        (hold, split): (&mut Option<ColumnHold>, &mut f32),
        hand: Hand,
        (at, width): (f32, f32),
    ) -> bool {
        if hand.pressed || (hand.held && hold.is_none()) {
            let from = self.flow_side_folds();
            *hold = Some(ColumnHold {
                at,
                travel: 0.0,
                from,
                split: *split,
            });
        }
        let was = *split;
        let mut changed = false;
        let moving = hand.held && hand.delta.abs() > f32::EPSILON && width > 0.0;
        if let Some(held) = hold.as_mut().filter(|_| moving) {
            held.travel += hand.delta;
            let asked = column_drag(held.at + held.travel, width, held.from);
            if let ColumnDrag::Open(fraction) = asked {
                changed = self.fold_flow_side(false, false) | self.fold_flow_side(true, false);
                *split = fraction;
            } else if asked != ColumnDrag::Hold {
                *split = held.split;
                if let Some(preset) = self.preset() {
                    self.mirror_split(preset, split, true);
                }
                changed = self.fold_flow_side(asked == ColumnDrag::FoldFlow, true);
            }
        }
        hold.take_if(|_| hand.released);
        changed || *split != was
    }
}

/// What a column divider drag asks for with the pointer `wanted_px` into a
/// `width` canvas, the drag having started with the context or the flow
/// column folded (`from`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnDrag {
    /// Nothing: a drag from a rail still pressed against its own edge.
    Hold,
    FoldContext,
    FoldFlow,
    /// Both columns open, the context column at this share of the width.
    Open(f32),
}

/// The column painter's divider rule: within [`COLLAPSE_AT_PX`] of an edge
/// that side folds; anywhere between, both open with the context column
/// held to [`MIN_PANE_WIDTH_PX`] where the canvas affords two.
#[must_use]
pub fn column_drag(wanted_px: f32, width: f32, from: (bool, bool)) -> ColumnDrag {
    let pressed = COLLAPSED_PANE_WIDTH_PX / 2.0;
    if (from.0 && wanted_px <= pressed) || (from.1 && width - wanted_px <= pressed) {
        return ColumnDrag::Hold;
    }
    if wanted_px < COLLAPSE_AT_PX {
        return ColumnDrag::FoldContext;
    }
    if width - wanted_px < COLLAPSE_AT_PX {
        return ColumnDrag::FoldFlow;
    }
    let px = if width >= 2.0 * MIN_PANE_WIDTH_PX {
        wanted_px.clamp(MIN_PANE_WIDTH_PX, width - MIN_PANE_WIDTH_PX)
    } else {
        wanted_px.clamp(0.0, width)
    };
    ColumnDrag::Open((px / width).clamp(0.0, 1.0))
}
