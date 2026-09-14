//! The drawings collection itself: what it holds, what is selected, what is
//! hidden or locked, and the undo history behind every change.
//!
//! Placement and the geometry edits that follow it live in
//! [`super::placement`]; this module owns the book they write into.

use super::{DeleteOutcome, Drawing, DrawingId, Drawings, UNDO_HISTORY_LIMIT, UndoEntry};
// Named only by the test-side authorship setter below.
#[cfg(test)]
use super::DrawingAuthor;

impl Drawings {
    /// How many times the collection has changed. See [`Self::revision`]'s
    /// field: equal readings mean nothing to write.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Empty the store and hand back what it held — the way a pane puts its
    /// drawings away when the layout or the market under it changes.
    ///
    /// The undo history goes with them: it described objects that are no
    /// longer on this chart, and an undo that brought back a mark from
    /// another market would be the very confusion the scoping exists to
    /// prevent. Ids are not rewound.
    pub fn take_all(&mut self) -> Vec<Drawing> {
        self.revision += 1;
        self.draft = None;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.gesture_baseline = None;
        std::mem::take(&mut self.items)
    }

    /// Fill an emptied store from saved objects.
    ///
    /// An object keeps the id it arrives with when that id is free here, so
    /// whatever named it — a strategy armed on it, an agent's annotation —
    /// still does; the counter moves past it so nothing later collides. An
    /// object with no id (`0`), or one whose id this store already gave out,
    /// takes a fresh one.
    ///
    /// Anchors keep the bar offsets they came with; the pane re-anchors them
    /// against its own series right after, exactly as after a re-cut.
    pub fn adopt(&mut self, items: impl IntoIterator<Item = Drawing>) {
        self.revision += 1;
        for mut drawing in items {
            let wanted = drawing.id.0;
            let taken = self.items.iter().any(|held| held.id.0 == wanted);
            if wanted == 0 || taken {
                drawing.id = self.alloc_id();
            } else {
                self.next_id = self.next_id.max(wanted);
            }
            self.items.push(drawing);
        }
    }

    #[must_use]
    pub fn items(&self) -> &[Drawing] {
        &self.items
    }

    /// Mutable access for **derived-state refresh only** — `frvp::refresh`
    /// bringing cached profiles up to date. Never a user edit: nothing
    /// reached through here may participate in payload equality, or a
    /// refresh would register as an edit against the undo snapshots
    /// (`Self::record` compares them). The cache exclusion in
    /// `FrvpPayload::eq` is the other half of this contract.
    #[must_use]
    pub(crate) fn items_mut(&mut self) -> &mut [Drawing] {
        &mut self.items
    }

    /// The in-flight draft, under the same derived-state-only contract as
    /// [`Self::items_mut`] — the profile refresh folds it so the histogram
    /// is live while the range is still being placed.
    #[must_use]
    pub(crate) fn draft_mut(&mut self) -> Option<&mut Drawing> {
        self.draft.as_mut()
    }

    /// How many objects are painted on the tab's other panes as well — the
    /// per-frame reprojection cost, in the health summary.
    #[must_use]
    pub fn shared_count(&self) -> usize {
        self.items.iter().filter(|item| item.shared()).count()
    }

    pub(super) fn alloc_id(&mut self) -> DrawingId {
        self.next_id += 1;
        DrawingId(self.next_id)
    }

    /// Where the object with this identity currently sits, if it still
    /// exists. The answer is only good for this frame: every reorder or
    /// delete moves it, which is the whole reason callers hold the id.
    #[must_use]
    pub fn index_of(&self, id: DrawingId) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }

    /// Rename the object at `index` as one undo step. Whitespace-only input
    /// clears the name back to the derived label.
    pub fn rename_at(&mut self, index: usize, name: &str) {
        if index >= self.items.len() {
            return;
        }
        let trimmed = name.trim();
        let name = (!trimmed.is_empty()).then(|| trimmed.to_owned());
        let before = self.snapshot();
        self.items[index].name = name;
        self.record(before);
    }

    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, selected: Option<usize>) {
        self.selected = selected.filter(|&index| index < self.items.len());
    }

    #[must_use]
    pub fn selected_mut(&mut self) -> Option<&mut Drawing> {
        self.selected.and_then(|index| self.items.get_mut(index))
    }

    #[must_use]
    pub fn draft(&self) -> Option<&Drawing> {
        self.draft.as_ref()
    }

    #[must_use]
    pub fn draft_len(&self) -> usize {
        self.draft.as_ref().map_or(0, |draft| draft.points.len())
    }

    #[must_use]
    pub fn all_hidden(&self) -> bool {
        self.all_hidden
    }

    /// Whether the object at `index` paints and hit-tests this frame.
    #[must_use]
    pub fn is_visible(&self, index: usize) -> bool {
        !self.all_hidden && self.items.get(index).is_some_and(|item| !item.hidden)
    }

    pub(super) fn snapshot(&self) -> UndoEntry {
        UndoEntry {
            items: self.items.clone(),
            all_hidden: self.all_hidden,
        }
    }

    /// Push `before` as one undo step if the store actually changed since.
    pub(super) fn record(&mut self, before: UndoEntry) {
        if before == self.snapshot() {
            return;
        }
        self.revision += 1;
        self.undo.push(before);
        if self.undo.len() > UNDO_HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Whether a coalescing gesture is open on this store.
    ///
    /// Test-only, and it exists for one question: when a pane edits a mark it
    /// does not own, did the gesture open on the store that holds the mark?
    /// The answer is invisible from outside otherwise — the baseline is
    /// private, as it should be. The layout store asks it too: a drag in
    /// flight is not a change to write yet.
    #[must_use]
    pub fn in_gesture(&self) -> bool {
        self.gesture_baseline.is_some()
    }

    /// Start coalescing: the next [`Self::commit_gesture`] records everything
    /// mutated in between as a single undo entry. Idempotent within a gesture.
    pub fn begin_gesture(&mut self) {
        if self.gesture_baseline.is_none() {
            self.gesture_baseline = Some(self.snapshot());
        }
    }

    /// End coalescing. A gesture that changed nothing records nothing.
    pub fn commit_gesture(&mut self) {
        if let Some(baseline) = self.gesture_baseline.take() {
            self.record(baseline);
        }
    }

    /// Record an already-applied edit to one object, given its pre-edit
    /// state. Used by the inspector to coalesce slider/color gestures.
    pub fn record_edit_of(&mut self, index: usize, before_drawing: Drawing) {
        let mut before = self.snapshot();
        let Some(slot) = before.items.get_mut(index) else {
            return;
        };
        *slot = before_drawing;
        self.record(before);
    }

    #[cfg(test)]
    pub(crate) fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub(super) fn restore(&mut self, entry: UndoEntry) {
        self.revision += 1;
        self.items = entry.items;
        self.all_hidden = entry.all_hidden;
        self.draft = None;
        self.selected = self.selected.filter(|&index| index < self.items.len());
    }

    pub fn undo(&mut self) -> bool {
        let Some(entry) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(entry);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(entry) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(entry);
        true
    }

    /// Remove every drawing as one undoable step, and report how many went.
    /// Locked objects go too: the caller gates this behind a count-bearing
    /// confirmation, and a lock protects against a stray click, not against
    /// an explicit clear-everything — `Ctrl+Z` brings the whole set back.
    pub fn delete_all(&mut self) -> usize {
        let count = self.items.len();
        if count == 0 {
            return 0;
        }
        let before = self.snapshot();
        self.items.clear();
        self.selected = None;
        self.record(before);
        count
    }

    /// One delete command for every trigger (button, manager, keyboard).
    /// A locked object is never deleted without `force`.
    pub fn delete_selected(&mut self, force: bool) -> DeleteOutcome {
        let Some(index) = self.selected.filter(|&index| index < self.items.len()) else {
            return DeleteOutcome::NothingSelected;
        };
        if self.items[index].locked && !force {
            return DeleteOutcome::NeedsConfirmation;
        }
        let before = self.snapshot();
        self.items.remove(index);
        self.selected = None;
        self.record(before);
        DeleteOutcome::Deleted
    }

    pub fn set_selected_locked(&mut self, locked: bool) {
        if let Some(index) = self.selected {
            self.set_locked_at(index, locked);
        }
    }

    pub fn set_selected_hidden(&mut self, hidden: bool) {
        if let Some(index) = self.selected {
            self.set_hidden_at(index, hidden);
        }
    }

    /// Remove one object by identity, wherever it sits and whatever is
    /// selected — the removal an operator's own annotation gets, and the one
    /// the trader's "remove what an assistant placed" gesture repeats.
    /// Locked objects placed by an operator are included: a lock the trader
    /// did not set is not a reason to keep someone else's mark.
    pub fn remove_by_id(&mut self, id: DrawingId) -> bool {
        let Some(index) = self.items.iter().position(|drawing| drawing.id == id) else {
            return false;
        };
        let before = self.snapshot();
        self.items.remove(index);
        self.selected = match self.selected {
            Some(selected) if selected == index => None,
            Some(selected) if selected > index => Some(selected - 1),
            other => other,
        };
        self.record(before);
        true
    }

    /// Remove every object an operator other than the trader placed, and
    /// report how many went. One gesture, one undo entry — the trader's way
    /// back from an assistant that drew too much.
    pub fn remove_authored(&mut self) -> usize {
        if !self.items.iter().any(|drawing| drawing.author.is_some()) {
            return 0;
        }
        let before = self.snapshot();
        let previous = self.items.len();
        let selected_id = self
            .selected
            .and_then(|index| self.items.get(index))
            .map(|drawing| drawing.id);
        self.items.retain(|drawing| drawing.author.is_none());
        self.selected =
            selected_id.and_then(|id| self.items.iter().position(|drawing| drawing.id == id));
        self.record(before);
        previous - self.items.len()
    }

    /// How many objects on this pane were placed by an operator other than
    /// the trader — what the sweep gesture shows before it offers itself.
    #[must_use]
    pub fn authored_count(&self) -> usize {
        self.items
            .iter()
            .filter(|drawing| drawing.author.is_some())
            .count()
    }

    /// Say who placed one object, after the fact.
    ///
    /// The annotate tier stamps authorship as it places, so this exists for
    /// the tests that need an object to *be* an assistant's without standing
    /// up a gateway to produce one.
    #[cfg(test)]
    pub fn set_author_at(&mut self, index: usize, author: Option<DrawingAuthor>) {
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.author = author;
        }
    }

    pub fn set_locked_at(&mut self, index: usize, locked: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.locked = locked;
            self.record(before);
        }
    }

    pub fn set_hidden_at(&mut self, index: usize, hidden: bool) {
        let before = self.snapshot();
        if let Some(drawing) = self.items.get_mut(index) {
            drawing.hidden = hidden;
            self.record(before);
        }
    }

    /// Z-order: painting walks the list front-to-back, hit-testing walks it
    /// back-to-front, so the last item is the topmost object.
    pub fn bring_to_front(&mut self, index: usize) {
        if index >= self.items.len() || index + 1 == self.items.len() {
            return;
        }
        let before = self.snapshot();
        let drawing = self.items.remove(index);
        self.items.push(drawing);
        // Selection follows the object, not the slot it used to occupy.
        self.selected = self.selected.map(|selected| {
            if selected == index {
                self.items.len() - 1
            } else if selected > index {
                selected - 1
            } else {
                selected
            }
        });
        self.record(before);
    }

    /// The *opening* hidden state, seeded as a pane is built.
    ///
    /// Deliberately not [`Self::set_all_hidden`]: that one records an undo
    /// entry, which is right for a click and wrong for a pane that has just
    /// come into existence. A time pane seeded through the recording setter
    /// opens with a non-empty history on a store holding zero objects, so the
    /// trader's first Ctrl+Z there un-hides drawings instead of doing nothing.
    pub fn open_all_hidden(&mut self, hidden: bool) {
        self.all_hidden = hidden;
    }

    pub fn set_all_hidden(&mut self, hidden: bool) {
        let before = self.snapshot();
        self.all_hidden = hidden;
        self.record(before);
    }

    /// Whether every drawing is individually locked (used by the toolbox's
    /// lock-all toggle). An empty collection is not "all locked".
    #[must_use]
    pub fn all_locked(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|item| item.locked)
    }

    /// Reversible bulk protection: locks (or unlocks) every drawing as one
    /// undo entry. Never deletes anything.
    pub fn set_all_locked(&mut self, locked: bool) {
        let before = self.snapshot();
        for item in &mut self.items {
            item.locked = locked;
        }
        self.record(before);
    }
}
