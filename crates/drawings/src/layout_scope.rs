//! Which charts of a market show a mark on all timeframes.
//!
//! Every chart of a market holds that market's all-timeframes marks itself
//! (`pane_sync`), because a merge needs the whole set. What it *shows* is
//! narrower: the marks of the layout it shows, the ones the trader asked to
//! show in "All windows", and the ones read from before layouts were
//! recorded. A layout is a strip layout; a pane's own copy counts as the one
//! it came from. Everything that paints, hit-tests, lists, selects or clears
//! goes through [`Drawings::in_layout`].

use crate::{Drawing, Drawings, PaneMarks};
use quantick_workspace::layout_document::LayoutId;
use quantick_workspace::market_settings::MarketSettings;

#[cfg(test)]
#[path = "layout_scope_tests.rs"]
mod layout_scope_tests;

impl<T: Clone + PartialEq> Drawings<T> {
    /// Name the strip layout this store's chart shows. Cheap when nothing
    /// moved, so the host names it every frame; a mark that stops showing
    /// lets go of its selection here.
    pub fn set_layout(&mut self, layout: Option<LayoutId>) {
        if self.layout != layout {
            self.layout = layout;
            self.settle_selection();
        }
    }

    /// The strip layout this store's chart shows, when one was named.
    #[must_use]
    pub fn layout(&self) -> Option<LayoutId> {
        self.layout
    }

    /// Whether this chart shows the object at `index` at all; its eye and
    /// the hide-all layer are [`Self::is_visible`]'s to add.
    #[must_use]
    pub fn in_layout(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|drawing| drawing.shows_in(self.layout))
    }

    /// The objects this chart shows, each with its index: what its lists read.
    pub fn shown(&self) -> impl Iterator<Item = (usize, &Drawing<T>)> {
        let layout = self.layout;
        let items = self.items.iter().enumerate();
        items.filter(move |(_, drawing)| drawing.shows_in(layout))
    }

    /// How many objects this chart shows: what its list and its counts read.
    #[must_use]
    pub fn shown_count(&self) -> usize {
        self.shown().count()
    }

    /// Turn the object at `index`'s "All windows" on or off as one undo step;
    /// answers whether it moved. Turned off on a layout that is not its own,
    /// the mark leaves this chart, and its selection here with it.
    /// A mark from before layouts were recorded becomes this layout's.
    pub fn set_all_windows_at(&mut self, index: usize, on: bool) -> bool {
        let here = self.layout;
        let Some(drawing) = self.items.get_mut(index) else {
            return false;
        };
        let before = drawing.clone();
        drawing.all_windows = on;
        drawing.layout = drawing.layout.or(here);
        if *drawing == before {
            return false;
        }
        self.record_edit_of(index, before);
        true
    }

    /// A pick of where the object at `index` shows was made on this chart.
    /// Put on or off all timeframes (`rescoped`), it becomes this layout's:
    /// the trader turned it on looking at this layout. Its "All windows"
    /// moved (`rewindowed`), a mark from before layouts were recorded becomes
    /// this layout's, and one with a layout keeps it.
    pub(crate) fn claim_for_layout(&mut self, index: usize, rescoped: bool, rewindowed: bool) {
        let here = self.layout;
        let Some(drawing) = self.items.get_mut(index) else {
            return;
        };
        if rescoped {
            drawing.layout = here.or(drawing.layout);
        } else if rewindowed {
            drawing.layout = drawing.layout.or(here);
        }
    }

    /// Strip layout `from` was deleted and its charts moved to `to`: every
    /// mark this store holds that belonged to it, in the history too, belongs
    /// to `to` now, so it shows on the charts it showed on.
    pub fn retarget_layout(&mut self, from: LayoutId, to: LayoutId) {
        for drawing in self.every_mark_mut() {
            if drawing.layout == Some(from) {
                drawing.layout = Some(to);
            }
        }
        self.settle_selection();
    }

    /// Let go of a selection this chart no longer shows: a mark that left it
    /// cannot stay the target of a delete or an edit made here.
    pub(crate) fn settle_selection(&mut self) {
        let shown = self.selected().is_some_and(|index| self.in_layout(index));
        if !shown {
            self.selection.set(None);
        }
    }

    /// Every mark the store holds, the history and an open gesture's
    /// baseline included.
    fn every_mark_mut(&mut self) -> impl Iterator<Item = &mut Drawing<T>> {
        let steps = self.undo.iter_mut().chain(self.redo.iter_mut());
        let steps = steps.chain(self.gesture_baseline.iter_mut());
        let held = steps.flat_map(|entry| entry.items.iter_mut());
        self.items.iter_mut().chain(held)
    }
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
