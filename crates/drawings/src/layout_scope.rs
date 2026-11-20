//! Which charts of a market show a mark on all timeframes.
//!
//! Every chart of a market holds that market's all-timeframes marks itself
//! (`pane_sync`), because a merge needs the whole set. What it *shows* is
//! narrower: the marks of the layout it shows, the ones the trader asked to
//! show in "All windows", and the ones read from before layouts were
//! recorded, or whose layout the book no longer holds ([`Drawing::home`]). A
//! layout is a strip layout; a pane's own copy counts as the one it came
//! from. Everything that paints, hit-tests, lists, selects, clears or edits
//! by id goes through [`Drawings::shows`] and the reads built on it.

use crate::{Drawing, DrawingId, DrawingScope, Drawings, PaneMarks};
use quantick_workspace::layout_document::LayoutId;
use quantick_workspace::market_settings::MarketSettings;

#[cfg(test)]
#[path = "layout_scope_tests.rs"]
mod layout_scope_tests;

impl<T: Clone + PartialEq> Drawings<T> {
    /// Name the strip layout this store's chart shows. Cheap when nothing
    /// moved, so the host names it every frame; a mark that stops showing
    /// lets go of its selection here ([`Self::retarget_layout`] may take it
    /// back), and the new marks made before any layout was named become this
    /// one's.
    pub fn set_layout(&mut self, layout: Option<LayoutId>) {
        if self.layout != layout {
            self.text_zoom_revision = None;
            self.layout = layout;
            self.claim_unclaimed();
            let held = self.selected_id();
            self.settle_selection();
            self.let_go = held.filter(|_| self.selected_id().is_none());
        }
    }

    /// Name the strip layouts the book holds, for [`Drawing::home`]. Cheap
    /// when the book did not move, so the host names them every frame.
    pub fn set_live_layouts(&mut self, live: impl IntoIterator<Item = LayoutId> + Clone) {
        if !self.live_layouts.iter().copied().eq(live.clone()) {
            self.text_zoom_revision = None;
            self.live_layouts.clear();
            self.live_layouts.extend(live);
            self.settle_selection();
        }
    }

    /// The strip layout this store's chart shows, when one was named.
    #[must_use]
    pub fn layout(&self) -> Option<LayoutId> {
        self.layout
    }

    /// Whether this chart shows `drawing` at all: the one rule every read of
    /// what it shows goes through ([`Drawing::shows_in`] against the layout
    /// and the book this store was told). Its eye and the hide-all layer are
    /// [`Self::is_visible`]'s to add.
    #[must_use]
    pub fn shows(&self, drawing: &Drawing<T>) -> bool {
        drawing.shows_in(self.layout, &self.live_layouts)
    }

    /// Whether this chart shows `drawing` as its own: shown, and not lent by
    /// another layout the book holds through its "All windows". What a bulk
    /// edit made here (clear, lock all) reaches; a lent mark stays as its
    /// own layout left it. A store that names no layout owns all it shows.
    #[must_use]
    pub fn owns(&self, drawing: &Drawing<T>) -> bool {
        owned_in(drawing, self.layout, &self.live_layouts)
    }

    /// How many objects this chart owns ([`Self::owns`]): what a clear of it
    /// removes, and its confirmation counts.
    #[must_use]
    pub fn owned_count(&self) -> usize {
        self.items
            .iter()
            .filter(|drawing| self.owns(drawing))
            .count()
    }

    /// The objects this chart owns ([`Self::owns`]), for a bulk edit.
    pub(crate) fn owned_mut(&mut self) -> impl Iterator<Item = &mut Drawing<T>> {
        let (layout, live) = (self.layout, &self.live_layouts);
        let items = self.items.iter_mut();
        items.filter(move |drawing| owned_in(drawing, layout, live))
    }

    /// Remove every object this chart owns ([`Self::owns`]); answers how many
    /// went.
    pub(crate) fn remove_owned(&mut self) -> usize {
        let (layout, live) = (self.layout, &self.live_layouts);
        let before = self.items.len();
        self.items
            .retain(|drawing| !owned_in(drawing, layout, live));
        before - self.items.len()
    }

    /// [`Self::shows`] for the object at `index`.
    #[must_use]
    pub fn in_layout(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|drawing| self.shows(drawing))
    }

    /// The objects this chart shows, each with its index: what its lists read.
    pub fn shown(&self) -> impl DoubleEndedIterator<Item = (usize, &Drawing<T>)> {
        let items = self.items.iter().enumerate();
        items.filter(|(_, drawing)| self.shows(drawing))
    }

    /// The objects this chart shows, for a derived-state refresh
    /// ([`Self::items_mut`]'s contract).
    pub(crate) fn shown_mut(&mut self) -> impl Iterator<Item = &mut Drawing<T>> {
        let (layout, live) = (self.layout, &self.live_layouts);
        let items = self.items.iter_mut();
        items.filter(move |drawing| drawing.shows_in(layout, live))
    }

    /// How many objects this chart shows: what its list and its counts read.
    #[must_use]
    pub fn shown_count(&self) -> usize {
        self.shown().count()
    }

    /// Where the object with this identity sits, if this chart shows it: the
    /// lookup every edit addressed by id goes through. A mark this chart only
    /// holds for merges is not its to edit, select or read back.
    #[must_use]
    pub fn shown_index_of(&self, id: DrawingId) -> Option<usize> {
        self.index_of(id).filter(|&index| self.in_layout(index))
    }

    /// The object with this identity, if this chart shows it: the read side of
    /// [`Self::shown_index_of`].
    #[must_use]
    pub fn shown_by_id(&self, id: DrawingId) -> Option<&Drawing<T>> {
        self.shown_index_of(id).map(|index| &self.items[index])
    }

    /// The row of [`Self::shown`] the selection sits on.
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        let selected = self.selected()?;
        self.shown().position(|(index, _)| index == selected)
    }

    /// Whether every chart of its market shows the object at `index`,
    /// whatever layout ([`Drawing::in_all_windows`] against the book this
    /// store was told): what the "All windows" switch and the wire read.
    #[must_use]
    pub fn in_all_windows(&self, index: usize) -> bool {
        let live = &self.live_layouts;
        self.items
            .get(index)
            .is_some_and(|drawing| drawing.in_all_windows(live))
    }

    /// The marks this store lends another pane of its tab showing `viewer`,
    /// each with its index: shared, not hidden, and shown on a chart showing
    /// `viewer` by the rule of [`Self::shows`], never by this chart's own
    /// layout.
    pub fn lent_to(
        &self,
        viewer: Option<LayoutId>,
    ) -> impl DoubleEndedIterator<Item = (usize, &Drawing<T>)> {
        let items = self.items.iter().enumerate();
        items.filter(move |(_, drawing)| {
            !self.all_hidden && drawing.shared() && drawing.shows_in(viewer, &self.live_layouts)
        })
    }

    /// Turn the object at `index`'s "All windows" on or off as one undo step;
    /// answers whether it moved ([`Self::pick_where_shown`]).
    pub fn set_all_windows_at(&mut self, index: usize, on: bool) -> bool {
        let Some(scope) = self.items.get(index).map(|drawing| drawing.scope) else {
            return false;
        };
        self.pick_where_shown(index, scope, Some(on))
    }

    /// Where the object at `index` shows, picked on this chart as one undo
    /// step: its scope, and its "All windows" when named. Answers whether it
    /// moved. "All windows" moves only when what shows would, or to clear a
    /// pick left on a mark kept to its chart: a mark every chart already
    /// shows stays as it is when asked on, and one kept to its chart is
    /// never turned on: "All windows" is for a mark on all timeframes.
    /// Turned off, a mark with no [`Drawing::home`] becomes this layout's;
    /// turned off on a layout that is not its own, the mark leaves this
    /// chart, and its selection with it.
    pub fn pick_where_shown(
        &mut self,
        index: usize,
        scope: DrawingScope,
        all_windows: Option<bool>,
    ) -> bool {
        let Some(before) = self.items.get(index).cloned() else {
            return false;
        };
        let live = &self.live_layouts;
        let mut after = before.clone();
        if after.scope != scope {
            after.set_scope(scope);
        }
        let windows = all_windows
            .filter(|&on| !on || after.scope == DrawingScope::AllCharts)
            .filter(|&on| after.in_all_windows(live) != on || (after.all_windows && !on));
        if let Some(on) = windows {
            after.all_windows = on;
            after.layout = after.home(live).or(self.layout);
        }
        if after == before {
            return false;
        }
        self.items[index] = after;
        self.record_edit_of(index, before);
        true
    }

    /// A pick of where the object at `index` shows was made on this chart;
    /// `before` is the object ahead of it. Put on or off all timeframes, it
    /// becomes this layout's: the trader turned it on looking at this
    /// layout. Its "All windows" moved, a mark with no [`Drawing::home`]
    /// becomes this layout's, and one with a home keeps it.
    pub(crate) fn claim_for_layout(&mut self, index: usize, before: &Drawing<T>) {
        let (here, live) = (self.layout, &self.live_layouts);
        let Some(drawing) = self.items.get_mut(index) else {
            return;
        };
        if drawing.scope != before.scope {
            drawing.layout = here.or(drawing.layout);
        } else if drawing.all_windows != before.all_windows {
            drawing.layout = drawing.home(live).or(here);
        }
    }

    /// The layout a new mark made on this chart belongs to: the one it shows.
    /// Made before the host named one, the mark is remembered, and the first
    /// layout named claims it ([`Self::set_layout`]): a new mark is never
    /// saved as one from before layouts were recorded. The ones remembered
    /// that the store no longer holds, the history included, are let go, so
    /// a store never named a layout remembers no more than it holds.
    pub(crate) fn new_mark_layout(&mut self, id: DrawingId) -> Option<LayoutId> {
        if self.layout.is_none() {
            if !self.unclaimed.is_empty() {
                let held: std::collections::BTreeSet<DrawingId> =
                    self.every_mark().map(|drawing| drawing.id).collect();
                self.unclaimed.retain(|id| held.contains(id));
            }
            self.unclaimed.push(id);
        }
        self.layout
    }

    /// The new marks made before a layout was named become the named one's,
    /// in the history and in the open draft too.
    fn claim_unclaimed(&mut self) {
        let Some(here) = self.layout.filter(|_| !self.unclaimed.is_empty()) else {
            return;
        };
        let unclaimed = std::mem::take(&mut self.unclaimed);
        for drawing in self.every_mark_mut() {
            if drawing.layout.is_none() && unclaimed.contains(&drawing.id) {
                drawing.layout = Some(here);
            }
        }
    }

    /// Strip layout `from` was deleted and its charts moved to `to`: every
    /// mark this store holds that belonged to it, in the history too, belongs
    /// to `to` now, and so does this chart if it showed `from`. Marks and
    /// chart move together, so a selection on one of them holds, and one the
    /// chart's switch to `to` let go of comes back.
    pub fn retarget_layout(&mut self, from: LayoutId, to: LayoutId) {
        self.text_zoom_revision = None;
        let moved = |drawing: &Drawing<T>| drawing.layout == Some(from);
        let back = self.let_go.take();
        let back = back.filter(|&id| self.index_of(id).is_some_and(|i| moved(&self.items[i])));
        if self.layout == Some(from) {
            self.layout = Some(to);
        }
        for drawing in self.every_mark_mut() {
            if drawing.layout == Some(from) {
                drawing.layout = Some(to);
            }
        }
        self.settle_selection();
        if self.selected_id().is_none() {
            self.selection
                .set(back.filter(|&id| self.shown_index_of(id).is_some()));
        }
    }

    /// Let go of a selection this chart no longer shows: a mark that left it
    /// cannot stay the target of a delete or an edit made here.
    pub(crate) fn settle_selection(&mut self) {
        let shown = self.selected().is_some_and(|index| self.in_layout(index));
        if !shown {
            self.selection.set(None);
        }
    }

    /// Every mark the store holds: the history, an open gesture's baseline
    /// and the open draft included.
    fn every_mark(&self) -> impl Iterator<Item = &Drawing<T>> {
        let steps = self.undo.iter().chain(self.redo.iter());
        let steps = steps.chain(self.gesture_baseline.iter());
        let held = steps.flat_map(|entry| entry.items.iter());
        self.items.iter().chain(held).chain(self.draft.iter())
    }

    /// [`Self::every_mark`], to edit.
    fn every_mark_mut(&mut self) -> impl Iterator<Item = &mut Drawing<T>> {
        let steps = self.undo.iter_mut().chain(self.redo.iter_mut());
        let steps = steps.chain(self.gesture_baseline.iter_mut());
        let held = steps.flat_map(|entry| entry.items.iter_mut());
        let held = held.chain(self.draft.iter_mut());
        self.items.iter_mut().chain(held)
    }
}

/// [`Drawings::owns`] for a chart showing `layout`, the book holding `live`.
fn owned_in<T>(drawing: &Drawing<T>, layout: Option<LayoutId>, live: &[LayoutId]) -> bool {
    let lent = layout.is_some()
        && drawing.all_timeframes()
        && drawing.home(live).is_some_and(|home| Some(home) != layout);
    !lent && drawing.shows_in(layout, live)
}

/// Strip layout `from` was deleted and its charts moved to `to`: its marks
/// belong to `to` from now on, in the store and in every pane of `panes`,
/// whose last-loaded sets follow so each stays level with the store. Answers
/// whether the store changed.
pub fn retarget_layout<'a, B: 'a>(
    settings: &mut MarketSettings,
    from: LayoutId,
    to: LayoutId,
    panes: impl IntoIterator<Item = PaneMarks<'a, B>>,
) -> bool {
    for pane in panes {
        pane.drawings.retarget_layout(from, to);
        for entry in &mut pane.layout.shared_base {
            if entry.layout == Some(from) {
                entry.layout = Some(to);
            }
        }
    }
    settings.retarget_layout(from, to)
}
