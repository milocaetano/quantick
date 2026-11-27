//! The "All windows" marks of a market's layouts that no chart of a tab
//! holds, lent to every chart of that tab.
//!
//! Contract: a chart holds its own market/workspace/layout set
//! (`pane_sync`), and a chart of the same tab holding another layout's set
//! lends its marks live, editable where they are held. A layout no chart of
//! the tab holds has no chart to lend from, so its "All windows" marks are
//! read here from the store, painted beside the charts' own and never
//! copied into their sets: nothing here is written back, and nothing here
//! takes an edit. Only the tab's workspace lends. Owner: the tab, which
//! follows the store each frame.

use crate::gesture::SharedSource;
use crate::{DrawingTool, Drawings};
use quantick_workspace::layout_document::SavedDrawing;
use quantick_workspace::market_settings::{DrawingKey, DrawingScope, MarketKey, PaneRole};
use quantick_workspace::market_settings_store::MarketSettingsStore;
use smallvec::SmallVec;

#[cfg(test)]
#[path = "lent_marks_tests.rs"]
mod lent_marks_tests;

/// The charts of a tab whose shared marks the chart at `viewer` paints
/// beside its own, given the drawing key each of the tab's `count` charts
/// holds, in tab order. Every chart of a market and layout scope holds its
/// set itself: one holding the viewer's adds nothing (another layout's lends
/// its "All windows" marks), and of two holding another only the first does,
/// or a shared mark would paint twice. A chart still settling its layout (a
/// key without its scope) neither lends nor borrows: the set it holds is
/// about to be replaced.
pub fn mirror_owners<'a>(
    count: usize,
    viewer: usize,
    key: impl Fn(usize) -> Option<&'a DrawingKey> + Copy + 'a,
) -> impl Iterator<Item = usize> + 'a {
    let settling = move |chart| key(chart).is_some_and(|key| key.scope.is_none());
    let set = move |chart| key(chart).map(|key| (&key.market, key.scope));
    (0..count).filter(move |&owner| {
        let same_set = |other: usize| set(owner).is_some() && set(other) == set(owner);
        owner != viewer
            && !settling(owner)
            && !settling(viewer)
            && !same_set(viewer)
            && !(0..owner).any(|earlier| earlier != viewer && same_set(earlier))
    })
}

/// The scopes a tab's charts hold: one per chart at most.
type HeldScopes = SmallVec<[DrawingScope; 8]>;

/// See the module docs.
#[derive(Default)]
pub struct LentMarks {
    marks: Drawings<DrawingTool>,
    /// What `marks` was read for: the store's revision, the market and the
    /// scopes the tab's charts held then.
    read_for: Option<(u64, MarketKey, HeldScopes)>,
}

impl LentMarks {
    /// Read again what the market's other layouts lend a tab whose charts
    /// hold `held`, once the store or those keys moved; an idle frame
    /// compares and allocates nothing. A key without its scope (a chart
    /// still settling its layout) neither holds nor asks. The marks wear
    /// `ground`'s ink (the tab's flow chart's), as its own marks do.
    pub fn follow<'a>(
        &mut self,
        store: &MarketSettingsStore,
        held: impl IntoIterator<Item = &'a DrawingKey>,
        ground: &Drawings<DrawingTool>,
    ) {
        self.read(store, held);
        self.marks.follow_ground_of(ground);
    }

    fn read<'a>(
        &mut self,
        store: &MarketSettingsStore,
        held: impl IntoIterator<Item = &'a DrawingKey>,
    ) {
        let mut ready = held.into_iter().filter(|key| key.scope.is_some());
        let Some(first) = ready.next() else {
            if self.read_for.take().is_some() {
                self.marks = Drawings::default();
            }
            return;
        };
        let market = &first.market;
        let mut scopes: HeldScopes = std::iter::once(first)
            .chain(ready)
            .filter(|key| key.market == *market)
            .filter_map(|key| key.scope)
            .collect();
        scopes.sort_unstable();
        scopes.dedup();
        let revision = store.revision();
        let read = self.read_for.as_ref();
        if read.is_some_and(|(at, held, then)| *at == revision && held == market && *then == scopes)
        {
            return;
        }
        let workspace = scopes[0].workspace;
        let lends = |scope: DrawingScope| scope.workspace == workspace && !scopes.contains(&scope);
        let settings = store.settings();
        let lent: Vec<SavedDrawing> = settings
            .keys()
            .filter(|key| key.role == PaneRole::AllTimeframes && key.market == *market)
            .filter(|key| key.scope.is_some_and(lends))
            .flat_map(|key| settings.drawings(&key).to_vec())
            .filter(|entry| entry.all_windows && entry.all_timeframes())
            // A profile or VWAP folds this pane's bars; nothing folds a lent one.
            .filter(|entry| ![crate::frvp::TOOL_ID, crate::avwap::TOOL_ID].contains(&&*entry.tool))
            .collect();
        self.marks = Drawings::default();
        self.marks.adopt(crate::saved::live(&lent, first));
        self.read_for = Some((revision, market.clone(), scopes));
    }

    /// The marks as a chart paints another's: see [`SharedSource`].
    #[must_use]
    pub fn source(&self) -> SharedSource<'_> {
        SharedSource {
            drawings: &self.marks,
            content_editing: None,
        }
    }
}
