//! A pane's share of its market's drawings.
//!
//! A mark shown on all timeframes is kept once per market, under
//! [`PaneRole::AllTimeframes`], and every chart of that market shows it at
//! whatever bars it is cut to: market time and price place it, never a bar
//! index. A mark kept to its chart stays under its pane role, pinned to the
//! bar rule it was written at ([`SavedDrawing::bars`]), and shows only on a
//! chart of that role cut that way.
//!
//! A file from before either rule is read as written and never rewritten to
//! fit: an entry with no pinned bars shows at every bar rule of its role, and
//! a shared entry still under a role reaches every chart of its market. Such
//! an entry moves to the market's set only when a pane writes an edit to it.
use super::{DrawingKey, MarketKey, MarketSettings, PaneRole, settled};
use crate::layout_document::SavedDrawing;
use std::collections::BTreeSet;

/// What a pane's write changed, so the caller brings exactly the panes that
/// show it up to date.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PaneWrite {
    /// The market's all-timeframes marks, which every chart of it shows.
    pub all_timeframes: bool,
    /// The marks kept to the writer's pane role.
    pub own: bool,
}

impl PaneWrite {
    /// Both halves: a change the caller cannot narrow, such as a legacy slot
    /// claimed by a pane.
    pub const ALL: Self = Self {
        all_timeframes: true,
        own: true,
    };

    #[must_use]
    pub const fn changed(self) -> bool {
        self.all_timeframes || self.own
    }

    /// Whether a pane holding `other` shows what this write by a pane holding
    /// `writer` changed.
    #[must_use]
    pub fn reaches(self, writer: &DrawingKey, other: &DrawingKey) -> bool {
        (self.all_timeframes && writer.market == other.market) || (self.own && writer == other)
    }
}

/// Every set the panes at `keys` show: their own, and each market's
/// all-timeframes set once.
pub fn shown_sets<'a>(keys: impl IntoIterator<Item = &'a DrawingKey>) -> BTreeSet<DrawingKey> {
    keys.into_iter()
        .flat_map(|key| [key.clone(), DrawingKey::all_timeframes(key.market.clone())])
        .collect()
}

/// A role whose sets an older build wrote shared marks into.
fn held_shared_before(role: &PaneRole) -> bool {
    matches!(role, PaneRole::Flow | PaneRole::Context { .. })
}

/// Whether a pane of the entry's role cut at `bars` shows it as its own mark.
fn own_at(entry: &SavedDrawing, bars: &str) -> bool {
    !entry.all_timeframes() && entry.bars.as_deref().is_none_or(|pinned| pinned == bars)
}

fn same_marks<'a>(held: impl Iterator<Item = &'a SavedDrawing>, written: &[SavedDrawing]) -> bool {
    held.map(settled).eq(written.iter().map(settled))
}

impl MarketSettings {
    /// What a pane holding `key`, cut at `bars`, shows: its market's
    /// all-timeframes marks, then its role's marks pinned to `bars` or to
    /// none. Entries of a tool this build lacks are in it; the caller skips
    /// them, and [`Self::store_pane`] keeps them.
    #[must_use]
    pub fn pane_drawings(&self, key: &DrawingKey, bars: &str) -> Vec<SavedDrawing> {
        let mut shown: Vec<SavedDrawing> = self.all_timeframes_of(&key.market).cloned().collect();
        if key.role != PaneRole::AllTimeframes {
            let own = self
                .drawings(key)
                .iter()
                .filter(|entry| own_at(entry, bars));
            shown.extend(own.cloned());
        }
        shown
    }

    /// Whether a pane holding `key` shows the same marks of its own cut at
    /// `from` as at `to`: none of them is pinned to either. A chart whose key
    /// stays when it is cut to other bars (the flow chart) then only moves its
    /// bars, and keeps its selection and undo.
    #[must_use]
    pub fn same_marks_at(&self, key: &DrawingKey, from: &str, to: &str) -> bool {
        let mut entries = self.drawings(key).iter();
        entries.all(|entry| own_at(entry, from) == own_at(entry, to))
    }

    /// Write back what a pane holding `key`, cut at `bars`, holds: marks for
    /// all timeframes to the market's set, the rest to its role. Only a half
    /// that changed is written (see [`super::settled`]: a re-anchor is not an
    /// edit), and what the pane never showed stays: a tool this build lacks,
    /// a mark pinned to other bars.
    ///
    /// `items` carry their pins already; the caller pins a mark kept to this
    /// chart to `bars` unless it predates pinning.
    pub fn store_pane(
        &mut self,
        key: &DrawingKey,
        bars: &str,
        items: Vec<SavedDrawing>,
        known_tool: impl Fn(&str) -> bool,
    ) -> PaneWrite {
        let (wide, own): (Vec<_>, Vec<_>) =
            items.into_iter().partition(SavedDrawing::all_timeframes);
        let mut write = PaneWrite::default();
        let mine = |entry: &SavedDrawing| known_tool(&entry.tool) && own_at(entry, bars);
        let held = self.drawings(key).iter().filter(|entry| mine(entry));
        if key.role != PaneRole::AllTimeframes && !same_marks(held, &own) {
            let kept = self.drawings(key).iter().filter(|entry| !mine(entry));
            let next = own.into_iter().chain(kept.cloned()).collect();
            self.put(key, next);
            write.own = true;
        }
        let held = self.all_timeframes_of(&key.market);
        if !same_marks(held.filter(|entry| known_tool(&entry.tool)), &wide) {
            self.retire_shared_before(&key.market, &known_tool);
            let set = DrawingKey::all_timeframes(key.market.clone());
            self.replace(&set, wide, known_tool);
            write.all_timeframes = true;
        }
        write
    }

    /// The marks every chart of `market` shows: its all-timeframes set, then
    /// the shared marks an older build kept under a role.
    fn all_timeframes_of<'a>(
        &'a self,
        market: &'a MarketKey,
    ) -> impl Iterator<Item = &'a SavedDrawing> + 'a {
        let set = DrawingKey::all_timeframes(market.clone());
        let before = self
            .drawings
            .iter()
            .filter(move |old| old.is_market(market) && held_shared_before(&old.role))
            .flat_map(|old| old.items.iter().filter(|entry| entry.all_timeframes()));
        self.drawings(&set).iter().chain(before)
    }

    /// Take the shared marks an older build kept under `market`'s roles out of
    /// them: the pane that just wrote the market's set held them, so they are
    /// in what it wrote.
    fn retire_shared_before(&mut self, market: &MarketKey, known_tool: &impl Fn(&str) -> bool) {
        let retired = |entry: &SavedDrawing| known_tool(&entry.tool) && entry.all_timeframes();
        self.drawings.retain_mut(|set| {
            if !set.is_market(market) || !held_shared_before(&set.role) {
                return true;
            }
            let before = set.items.len();
            set.items.retain(|entry| !retired(entry));
            // A set only this emptied goes; one the file held empty stays.
            set.items.len() == before || !set.items.is_empty()
        });
    }

    /// Set `key`'s entries to `items`; an empty set leaves no entry.
    fn put(&mut self, key: &DrawingKey, items: Vec<SavedDrawing>) {
        match self.drawings.iter().position(|set| set.is(key)) {
            Some(index) if items.is_empty() => {
                self.drawings.remove(index);
            }
            Some(index) => self.drawings[index].items = items,
            None if items.is_empty() => {}
            None => self.insert(key, items),
        }
    }
}
