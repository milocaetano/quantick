//! The per-pane collection of drawn objects and its undo history. The
//! behaviour lives in `collection`, `placement` and `clipboard`.

use super::Drawing;

/// Undo history depth. One entry per committed command (a whole drag or
/// slider gesture is one command), so this bounds memory without cutting a
/// working session short.
pub const UNDO_HISTORY_LIMIT: usize = 64;

/// One undo step: the whole collection plus the global-hide layer. Selection,
/// viewport and inspector state deliberately stay out, so undo never yanks
/// the camera or the UI around.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoEntry {
    pub(super) items: Vec<Drawing>,
    pub(super) all_hidden: bool,
}

#[derive(Debug, Default)]
pub struct Drawings {
    /// Bumped on every change to the collection — a placement, an edit, a
    /// delete, an undo, a load. What the layout store compares against to
    /// know a pane's drawings need writing, at the cost of one integer per
    /// pane per frame rather than a walk of every object.
    pub(super) revision: u64,
    pub(super) items: Vec<Drawing>,
    pub(super) draft: Option<Drawing>,
    pub(super) selected: Option<usize>,
    /// Source of [`DrawingId`](super::DrawingId)s: incremented on every allocation and never
    /// rewound — not by undo, not by delete — so an id can never be reborn
    /// as a different object.
    pub(super) next_id: u64,
    /// Global hide layer. Independent from each drawing's own eye, so
    /// "show all" restores exactly the per-object visibility it found.
    pub(super) all_hidden: bool,
    pub(super) undo: Vec<UndoEntry>,
    pub(super) redo: Vec<UndoEntry>,
    /// Snapshot taken when a pointer gesture starts; committed (as one undo
    /// entry) on release, so a whole drag coalesces into one command.
    pub(super) gesture_baseline: Option<UndoEntry>,
}
