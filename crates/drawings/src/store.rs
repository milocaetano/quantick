//! The per-pane collection of drawn objects and its undo history. The
//! behaviour lives in `collection`, `placement` and `clipboard`.

use crate::Drawing;

/// Undo history depth. One entry per committed command (a whole drag or
/// slider gesture is one command), so this bounds memory without cutting a
/// working session short.
pub const UNDO_HISTORY_LIMIT: usize = 64;

/// One undo step: the whole collection plus the global-hide layer. Selection,
/// viewport and inspector state deliberately stay out, so undo never yanks
/// the camera or the UI around.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoEntry<T> {
    pub(crate) items: Vec<Drawing<T>>,
    pub(crate) all_hidden: bool,
}

/// One call placing several objects, open on a store: the state before the
/// first placement and the history set aside while the call runs, so the
/// call commits as one undo step or retracts to exactly where it began.
#[derive(Debug)]
#[must_use = "a batch is committed or retracted"]
pub struct PlacementBatch<T> {
    pub(crate) baseline: UndoEntry<T>,
    pub(crate) undo: Vec<UndoEntry<T>>,
    pub(crate) redo: Vec<UndoEntry<T>>,
    pub(crate) selected: Option<usize>,
}

#[derive(Debug)]
pub struct Drawings<T> {
    /// Bumped on every change to the collection — a placement, an edit, a
    /// delete, an undo, a load. What the layout store compares against to
    /// know a pane's drawings need writing, at the cost of one integer per
    /// pane per frame rather than a walk of every object.
    pub(crate) revision: u64,
    pub(crate) items: Vec<Drawing<T>>,
    pub(crate) draft: Option<Drawing<T>>,
    pub(crate) selection: quantick_chart_interaction::pane::Selection,
    /// Source of [`DrawingId`](crate::DrawingId)s: incremented on every allocation and never
    /// rewound — not by undo, not by delete — so an id can never be reborn
    /// as a different object.
    pub(crate) next_id: u64,
    /// Global hide layer. Independent from each drawing's own eye, so
    /// "show all" restores exactly the per-object visibility it found.
    pub(crate) all_hidden: bool,
    pub(crate) undo: Vec<UndoEntry<T>>,
    pub(crate) redo: Vec<UndoEntry<T>>,
    /// Snapshot taken when a pointer gesture starts; committed (as one undo
    /// entry) on release, so a whole drag coalesces into one command.
    pub(crate) gesture_baseline: Option<UndoEntry<T>>,
}

// By hand: a derive would ask the tool handle for a `Default` it has no
// meaning for.
impl<T> Default for Drawings<T> {
    fn default() -> Self {
        Self {
            revision: 0,
            items: Vec::new(),
            draft: None,
            selection: quantick_chart_interaction::pane::Selection::default(),
            next_id: 0,
            all_hidden: false,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture_baseline: None,
        }
    }
}
