//! The column painter's own decisions over a canvas: which column a fold
//! puts away, which pane a layout switch reveals, and whether the column
//! painter draws a canvas at all. The window keeps the flags and the focus;
//! what they become is decided here.

use super::presets::{LayoutPreset, PaneKind};
use super::state::CanvasState;

/// Put one column away (`collapsed`) or bring it back, `other` being the
/// column across the divider. Answers whether anything changed.
///
/// **The one collapse path.** The divider drag, the rail, the View menu,
/// `Ctrl+0` and `layout.pane.collapse` all arrive here. Before it existed the
/// only way a *trader* could collapse the context column was a mouse drag,
/// while an assistant had a named call for it: the second operator holding a
/// capability the first could not reach from the keyboard, which is the rule
/// inverted.
///
/// The two columns never fold together, so folding one opens the other. The
/// split's width is deliberately not here: it is what the column springs back
/// to, and spending it would hand back a different chart from the one that
/// was put away.
pub fn fold_column(folded: &mut bool, other: &mut bool, collapsed: bool) -> bool {
    if *folded == collapsed && (!collapsed || !*other) {
        return false;
    }
    *folded = collapsed;
    if collapsed {
        *other = false;
    }
    true
}

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
    /// floating, folded or maximized. Anything else is painted from the tree.
    #[must_use]
    pub fn column_painted(&self, preset: &LayoutPreset, column: bool) -> bool {
        column
            && self.floating().is_empty()
            && self.maximized().is_none()
            && self.minimized().is_empty()
            && self.preset() == Some(preset)
    }
}
