//! The column painter's own decisions over a canvas: what a divider drag
//! asks for, which pane a layout switch reveals, and whether the column
//! painter draws a canvas at all. The window keeps the flags and the focus;
//! what they become is decided here.

use super::metrics::{COLLAPSE_AT_PX, COLLAPSED_PANE_WIDTH_PX, MIN_PANE_WIDTH_PX};
use super::presets::{LayoutPreset, PaneKind};
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
