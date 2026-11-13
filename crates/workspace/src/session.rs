//! Layout-session decisions and read-only pane membership.
use crate::layout_document::{LayoutBook, LayoutError, LayoutId};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

#[derive(Debug, Clone, Copy, Default)]
struct Membership {
    layout: Option<LayoutId>,
    seeded: bool,
    generation: u64,
}

/// An O(1), read-only projection of the session's single membership value.
/// Like the chart consumer this session is single-threaded; it owns no runtime objects.
#[derive(Debug, Clone, Default)]
pub struct LayoutView(Option<Rc<Cell<Membership>>>);
impl LayoutView {
    #[must_use]
    pub fn layout(&self) -> Option<LayoutId> {
        self.0.as_ref().and_then(|state| state.get().layout)
    }
    #[must_use]
    pub fn seeded(&self) -> bool {
        self.0.as_ref().is_some_and(|state| state.get().seeded)
    }
}

/// Facts supplied by the chart, never a callback into its root.
#[derive(Debug, Clone, Copy, Default)]
pub struct PaneFacts {
    pub gesture_in_flight: bool,
}
impl PaneFacts {
    fn refusal(self) -> Option<LayoutError> {
        self.gesture_in_flight
            .then_some(LayoutError::GestureInFlight)
    }
}

/// Ordered host operations; membership and default changes occur at distinct boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutEffect {
    DetachIndicators,
    CommitMembership,
    MaterializeIndicators,
    RefreshLabel,
    CommitDefault,
    MarkDirty,
}
const SWAP_EFFECTS: &[LayoutEffect] = &[
    LayoutEffect::DetachIndicators,
    LayoutEffect::CommitMembership,
    LayoutEffect::MaterializeIndicators,
    LayoutEffect::RefreshLabel,
    LayoutEffect::CommitDefault,
    LayoutEffect::MarkDirty,
];
/// A validated selection. Outgoing indicators leave before this transition commits.
/// The shell can read but cannot retarget a validated selection.
/// ```compile_fail
/// use quantick_workspace::session::Selection;
/// fn retarget(selection: &mut Selection) { selection.pane = 99; }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pane: u64,
    from: LayoutId,
    to: LayoutId,
    generation: u64,
}
/// A selection whose membership effect has run, but whose default has not.
#[derive(Debug, PartialEq, Eq)]
pub struct CommittedSelection {
    selection: Selection,
    generation: u64,
    /// What the pane holds now: `to`, or its tab's copy of it.
    landed: LayoutId,
}
impl Selection {
    #[must_use]
    pub fn pane(self) -> u64 {
        self.pane
    }
    #[must_use]
    pub fn from(self) -> LayoutId {
        self.from
    }
    #[must_use]
    pub fn to(self) -> LayoutId {
        self.to
    }

    #[must_use]
    pub fn changed(self) -> bool {
        self.from != self.to
    }
    #[must_use]
    pub fn effects(self) -> &'static [LayoutEffect] {
        if self.changed() {
            SWAP_EFFECTS
        } else {
            &[LayoutEffect::CommitMembership]
        }
    }
}

/// What [`LayoutSession::own_copies`] moved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnedCopies {
    /// Whether any pane moved, so the book owes the file a change.
    pub changed: bool,
    /// Panes that joined their tab's copy holding another set than they show:
    /// the host puts the copy's set on them.
    pub reshow: Vec<u64>,
}

/// The headless owner of the document and all registered pane memberships.
#[derive(Debug)]
pub struct LayoutSession {
    book: LayoutBook,
    panes: BTreeMap<u64, Rc<Cell<Membership>>>,
    next_generation: u64,
    /// The tab each pane stands in: set at [`Self::register_in`], rebuilt by
    /// [`Self::own_copies`].
    tab_of: BTreeMap<u64, u64>,
    /// The tab each private copy belongs to. Not saved: a desk read back
    /// hands each copy to the tab whose pane holds it.
    owner: BTreeMap<LayoutId, u64>,
    /// `(pane, strip layout)` to the copy of it the pane last left, kept so a
    /// switch back finds its tab's set.
    kept: BTreeMap<(u64, LayoutId), LayoutId>,
}
impl LayoutSession {
    #[must_use]
    pub fn new(book: LayoutBook) -> Self {
        Self {
            book,
            panes: BTreeMap::new(),
            next_generation: 0,
            tab_of: BTreeMap::new(),
            owner: BTreeMap::new(),
            kept: BTreeMap::new(),
        }
    }
    #[must_use]
    pub fn book(&self) -> &LayoutBook {
        &self.book
    }
    #[must_use]
    pub fn resolve_layout(&self, named: Option<LayoutId>) -> LayoutId {
        named
            .filter(|id| self.book.get(*id).is_some())
            .unwrap_or(self.book.active_id())
    }
    #[must_use]
    pub fn shows(&self, view: &LayoutView, layout: LayoutId) -> bool {
        view.layout() == Some(layout)
    }
    /// Whether `view` shows the strip layout `origin`, itself or a copy of it.
    #[must_use]
    pub fn shows_origin(&self, view: &LayoutView, origin: LayoutId) -> bool {
        view.layout()
            .is_some_and(|id| self.book.origin_of(id) == origin)
    }
    /// How many indicators `pane` shows, or would show, on strip layout `origin`.
    #[must_use]
    pub fn shown_count(&self, pane: u64, origin: LayoutId) -> usize {
        let held = self.panes.get(&pane).and_then(|state| state.get().layout);
        let shown = held
            .filter(|held| self.book.is_private(*held) && self.book.origin_of(*held) == origin)
            .unwrap_or_else(|| self.landing(pane, origin));
        self.book
            .get(shown)
            .map_or(0, |layout| layout.indicators.len())
    }
    pub fn matching<'a, T: 'a>(
        &'a self,
        layout: LayoutId,
        order: impl Iterator<Item = (T, &'a LayoutView)> + 'a,
    ) -> impl Iterator<Item = T> + 'a {
        order.filter_map(move |(target, view)| self.shows(view, layout).then_some(target))
    }
    /// Document payload access never exposes membership, names or identities.
    pub fn indicators_mut(
        &mut self,
        id: LayoutId,
    ) -> Option<&mut Vec<crate::indicator_document::SavedIndicator>> {
        self.book.get_mut(id).map(|layout| &mut layout.indicators)
    }
    /// [`Self::register`] a pane of `tab`, so a selection knows its tab at once.
    pub fn register_in(&mut self, tab: u64, pane: u64, named: Option<LayoutId>) -> LayoutView {
        self.tab_of.insert(pane, tab);
        self.register(pane, named)
    }
    /// Register once; repeated registration returns the same read-only value.
    pub fn register(&mut self, pane: u64, named: Option<LayoutId>) -> LayoutView {
        let next_generation = &mut self.next_generation;
        LayoutView(Some(
            self.panes
                .entry(pane)
                .or_insert_with(|| {
                    *next_generation = next_generation
                        .checked_add(1)
                        .expect("layout generation exhausted");
                    Rc::new(Cell::new(Membership {
                        layout: named,
                        seeded: false,
                        generation: *next_generation,
                    }))
                })
                .clone(),
        ))
    }
    /// Explicit removal tombstones every retained view. A later registration is distinct.
    pub fn remove_pane(&mut self, pane: u64) {
        if let Some(state) = self.panes.remove(&pane) {
            let mut value = state.get();
            let held = value.layout;
            value.layout = None;
            value.seeded = false;
            state.set(value);
            if let Some(held) = held {
                self.drop_if_unheld(held);
            }
        }
        let left: Vec<LayoutId> = self
            .kept
            .iter()
            .filter_map(|((owner, _), copy)| (*owner == pane).then_some(*copy))
            .collect();
        self.kept.retain(|(owner, _), _| *owner != pane);
        for copy in left {
            self.drop_if_unheld(copy);
        }
        // The tab's last pane: its copies go with it.
        if let Some(tab) = self.tab_of.remove(&pane)
            && !self.tab_of.values().any(|other| *other == tab)
        {
            let copies: Vec<LayoutId> = self.copies_of(tab).collect();
            for copy in copies {
                self.drop_if_unheld(copy);
            }
        }
    }
    /// The private copies `tab` owns.
    fn copies_of(&self, tab: u64) -> impl Iterator<Item = LayoutId> + '_ {
        self.owner
            .iter()
            .filter_map(move |(copy, owner)| (*owner == tab).then_some(*copy))
    }
    /// The copy of the strip layout `origin` that `tab` owns, if any.
    fn tab_copy(&self, tab: u64, origin: LayoutId) -> Option<LayoutId> {
        self.copies_of(tab)
            .find(|copy| self.book.origin_of(*copy) == origin)
    }
    /// The layout `pane` lands on for the strip layout `to`: the copy of it the
    /// pane holds or last left, else its tab's, else `to` itself.
    fn landing(&self, pane: u64, to: LayoutId) -> LayoutId {
        let Some(tab) = self.tab_of.get(&pane).copied() else {
            return to;
        };
        let held = self.panes.get(&pane).and_then(|state| state.get().layout);
        let own = |copy: &LayoutId| self.owner.get(copy) == Some(&tab);
        held.filter(|held| own(held) && self.book.origin_of(*held) == to)
            .or_else(|| self.kept.get(&(pane, to)).copied().filter(own))
            .or_else(|| self.tab_copy(tab, to))
            .unwrap_or(to)
    }
    /// A private copy no pane holds leaves the book, unless it is the only
    /// copy of its strip layout that a standing tab owns, or the one a pane of
    /// that tab left: those keep their set while the tab shows another layout.
    fn drop_if_unheld(&mut self, layout: LayoutId) {
        let held = self
            .panes
            .values()
            .any(|state| state.get().layout == Some(layout));
        if held || !self.book.is_private(layout) {
            return;
        }
        if let Some(tab) = self.owner.get(&layout).copied() {
            let standing = self.tab_of.values().any(|other| *other == tab);
            let origin = self.book.origin_of(layout);
            let twin = self
                .copies_of(tab)
                .any(|copy| copy != layout && self.book.origin_of(copy) == origin);
            let left = self
                .kept
                .iter()
                .any(|((pane, _), copy)| *copy == layout && self.panes.contains_key(pane));
            if standing && (!twin || left) {
                return;
            }
        }
        self.book.remove_private(layout);
        self.owner.remove(&layout);
        self.kept.retain(|_, copy| *copy != layout);
    }
    /// Drop every private copy that no pane holds and `named` does not name: a
    /// copy a desk read from disk no longer refers to. Answers whether any went.
    pub fn prune_private(&mut self, named: impl IntoIterator<Item = LayoutId>) -> bool {
        let mut keep: std::collections::BTreeSet<LayoutId> = named.into_iter().collect();
        keep.extend(self.panes.values().filter_map(|state| state.get().layout));
        let pruned = self.book.retain_private(|id| keep.contains(&id));
        let book = &self.book;
        self.owner.retain(|copy, _| book.is_private(*copy));
        self.kept.retain(|_, copy| book.is_private(*copy));
        pruned
    }
    /// The layout a seeded pane holds.
    fn held(&self, pane: u64) -> Option<LayoutId> {
        let value = self.panes.get(&pane)?.get();
        value.layout.filter(|_| value.seeded)
    }
    /// Put a pane on `copy`; the copy it leaves may go, see [`Self::drop_if_unheld`].
    fn repoint(&mut self, pane: u64, copy: LayoutId) {
        let Some(state) = self.panes.get(&pane) else {
            return;
        };
        let mut value = state.get();
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        let left = value.layout.replace(copy);
        value.generation = self.next_generation;
        state.set(value);
        if let Some(left) = left.filter(|left| *left != copy) {
            self.drop_if_unheld(left);
        }
    }
    /// A new copy of `layout` for `tab`.
    fn copy_for(&mut self, tab: u64, layout: LayoutId) -> Option<LayoutId> {
        let copy = self.book.make_private_copy(layout)?;
        self.owner.insert(copy, tab);
        Some(copy)
    }
    /// Put every seeded pane, given as `(tab, pane)` for every pane there is,
    /// in pane order, on its tab's copy of its strip layout: the panes of one
    /// tab on one strip layout share that copy, so an edit mirrors across
    /// them, and no copy is ever another tab's. A pane put back on a strip
    /// layout its tab holds a copy of rejoins that copy; twin copies merge,
    /// see [`Self::merge_twins`]. Slots and entries keep their positions.
    pub fn own_copies(&mut self, panes: impl IntoIterator<Item = (u64, u64)>) -> OwnedCopies {
        self.tab_of.clear();
        let mut seeded = Vec::new();
        for (tab, pane) in panes {
            self.tab_of.insert(pane, tab);
            if let Some(layout) = self.held(pane) {
                seeded.push((tab, pane, layout));
            }
        }
        let book = &self.book;
        self.owner.retain(|copy, _| book.is_private(*copy));
        for &(tab, _, layout) in &seeded {
            if self.book.is_private(layout) {
                self.owner.entry(layout).or_insert(tab);
            }
        }
        let mut owned = self.merge_twins(&seeded);
        for (tab, pane, _) in seeded {
            let Some(layout) = self.held(pane) else {
                continue;
            };
            if self.owner.get(&layout) == Some(&tab) {
                continue;
            }
            let found = self.tab_copy(tab, self.book.origin_of(layout));
            // Another tab's copy holds that tab's edits: cut from the strip.
            let source = self.book.strip_layout(layout);
            let Some(copy) = found.or_else(|| self.copy_for(tab, source)) else {
                continue;
            };
            let set = |id: LayoutId| self.book.get(id).map(|layout| &layout.indicators);
            if set(copy) != set(layout) {
                owned.reshow.push(pane);
            }
            self.repoint(pane, copy);
            owned.changed = true;
        }
        owned
    }
    /// Twin copies of one strip layout that one tab owns, as a stored file can
    /// bring back, merge into the one its first pane in order holds, else the
    /// first one a pane of it left: one set per strip layout per tab. Panes on
    /// a twin move to the survivor and are told to show its set; a pane that
    /// left a twin finds the survivor on its way back. The twins leave.
    fn merge_twins(&mut self, seeded: &[(u64, u64, LayoutId)]) -> OwnedCopies {
        let own = |tab: u64, copy: LayoutId| (self.owner.get(&copy) == Some(&tab)).then_some(copy);
        let held = seeded
            .iter()
            .filter_map(|&(tab, _, layout)| Some((tab, own(tab, layout)?)));
        let left = seeded.iter().flat_map(|&(tab, pane, _)| {
            (self.kept.iter())
                .filter(move |((by, _), _)| *by == pane)
                .filter_map(move |(_, copy)| Some((tab, own(tab, *copy)?)))
        });
        let mut first: BTreeMap<(u64, LayoutId), LayoutId> = BTreeMap::new();
        let mut merged: BTreeMap<LayoutId, LayoutId> = BTreeMap::new();
        for (tab, copy) in held.chain(left) {
            let survivor = *first
                .entry((tab, self.book.origin_of(copy)))
                .or_insert(copy);
            if survivor != copy {
                merged.insert(copy, survivor);
            }
        }
        let mut owned = OwnedCopies::default();
        if merged.is_empty() {
            return owned;
        }
        for copy in self.kept.values_mut() {
            if let Some(survivor) = merged.get(copy) {
                *copy = *survivor;
            }
        }
        for &(tab, pane, layout) in seeded {
            let Some(survivor) = merged.get(&layout).copied() else {
                continue;
            };
            if self.owner.get(&layout) != Some(&tab) {
                continue;
            }
            let set = |id| self.book.get(id).map(|layout| &layout.indicators);
            if set(layout) != set(survivor) {
                owned.reshow.push(pane);
            }
            self.repoint(pane, survivor);
        }
        for twin in merged.into_keys() {
            self.drop_if_unheld(twin);
        }
        owned.changed = true;
        owned
    }
    /// Before a recipe lands on one tab's panes, given as `(pane, set)` in
    /// pane order: the set each pane shows. The panes on one copy show one
    /// set, the one the first of them is given; a recipe never parts a copy.
    /// Call after [`Self::own_copies`].
    #[must_use]
    pub fn shared_sets<'a>(
        &self,
        wanted: impl IntoIterator<Item = (u64, &'a [crate::indicator_document::SavedIndicator])>,
    ) -> Vec<(u64, &'a [crate::indicator_document::SavedIndicator])> {
        let mut decided: Vec<(LayoutId, &'a [_])> = Vec::new();
        let mut shown = Vec::new();
        for (pane, set) in wanted {
            let first = self.held(pane).map(|layout| {
                match decided.iter().find(|(copy, _)| *copy == layout) {
                    Some((_, first)) => *first,
                    None => {
                        decided.push((layout, set));
                        set
                    }
                }
            });
            shown.push((pane, first.unwrap_or(set)));
        }
        shown
    }
    pub fn replace_book(&mut self, book: LayoutBook) {
        let panes = std::mem::take(&mut self.panes);
        for state in panes.into_values() {
            let mut value = state.get();
            value.layout = None;
            value.seeded = false;
            state.set(value);
        }
        self.tab_of.clear();
        self.owner.clear();
        self.kept.clear();
        self.book = book;
    }
    #[must_use]
    pub fn layout(&self, pane: u64) -> LayoutId {
        self.panes
            .get(&pane)
            .and_then(|state| state.get().layout)
            .filter(|id| self.book.get(*id).is_some())
            .unwrap_or(self.book.active_id())
    }
    /// Seed precedence is named, own tab's seeded flow, focused seeded pane, default.
    pub fn seed(&mut self, pane: u64, flow: Option<u64>, focused: Option<u64>) -> Option<LayoutId> {
        let state = self.panes.get(&pane)?;
        if state.get().seeded {
            return None;
        }
        let valid = |id| self.book.get(id).is_some();
        let seeded_layout = |id: u64| {
            self.panes
                .get(&id)
                .map(|state| state.get())
                .filter(|state| state.seeded)
                .and_then(|state| state.layout)
                .filter(|id| valid(*id))
        };
        let layout = state
            .get()
            .layout
            .filter(|id| valid(*id))
            .or_else(|| flow.and_then(seeded_layout))
            .or_else(|| focused.and_then(seeded_layout))
            .unwrap_or(self.book.active_id());
        let mut value = state.get();
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        value.generation = self.next_generation;
        value.layout = Some(layout);
        value.seeded = true;
        state.set(value);
        Some(layout)
    }
    pub fn select(
        &self,
        pane: u64,
        to: LayoutId,
        facts: PaneFacts,
    ) -> Result<Selection, LayoutError> {
        let state = self.panes.get(&pane).ok_or(LayoutError::Unknown)?.get();
        self.book.get(to).ok_or(LayoutError::Unknown)?;
        let tab = self.tab_of.get(&pane);
        if self.book.is_private(to) && state.layout != Some(to) && self.owner.get(&to) != tab {
            // A copy is named by no strip row; another tab's would carry this
            // pane's edits to that tab.
            return Err(LayoutError::Unknown);
        }
        // A strip layout the pane's tab holds a copy of lands on that copy.
        let to = self.landing(pane, to);
        let from = self.layout(pane);
        if from != to
            && let Some(error) = facts.refusal()
        {
            return Err(error);
        }
        Ok(Selection {
            pane,
            from,
            to,
            generation: state.generation,
        })
    }
    /// Commit at the named membership effect, after outgoing slots leave.
    pub fn commit_selection(
        &mut self,
        selection: Selection,
    ) -> Result<CommittedSelection, LayoutError> {
        self.book.get(selection.to).ok_or(LayoutError::Unknown)?;
        let state = self
            .panes
            .get(&selection.pane)
            .ok_or(LayoutError::Unknown)?;
        let mut value = state.get();
        if value.generation != selection.generation || self.layout(selection.pane) != selection.from
        {
            return Err(LayoutError::Unknown);
        }
        let tab = self.tab_of.get(&selection.pane).copied();
        // A pane of a known tab never holds a strip layout another tab reads.
        let landed = match tab {
            Some(tab) if !self.book.is_private(selection.to) => {
                self.copy_for(tab, selection.to).unwrap_or(selection.to)
            }
            _ => selection.to,
        };
        let Some(state) = self.panes.get(&selection.pane) else {
            return Err(LayoutError::Unknown);
        };
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        value.generation = self.next_generation;
        value.layout = Some(landed);
        if selection.changed() {
            value.seeded = true;
        }
        state.set(value);
        let generation = value.generation;
        if selection.changed() {
            let from = selection.from;
            if tab.is_some() && tab == self.owner.get(&from).copied() {
                self.kept
                    .insert((selection.pane, self.book.origin_of(from)), from);
            }
            self.drop_if_unheld(from);
        }
        Ok(CommittedSelection {
            selection,
            generation,
            landed,
        })
    }
    /// Last-pick default changes only after incoming indicators are restored.
    pub fn finish_selection(&mut self, committed: CommittedSelection) -> Result<(), LayoutError> {
        let selection = committed.selection;
        let state = self
            .panes
            .get(&selection.pane)
            .ok_or(LayoutError::Unknown)?
            .get();
        if state.generation != committed.generation || state.layout != Some(committed.landed) {
            return Err(LayoutError::Unknown);
        }
        self.book.get(selection.to).ok_or(LayoutError::Unknown)?;
        if selection.changed() {
            self.book.switch(self.book.origin_of(selection.to))?;
        }
        Ok(())
    }
    pub fn create(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        self.book.create(name)
    }
    pub fn create_for(
        &mut self,
        pane: u64,
        name: Option<&str>,
        facts: PaneFacts,
    ) -> Result<LayoutId, LayoutError> {
        if !self.panes.contains_key(&pane) {
            return Err(LayoutError::Unknown);
        }
        if let Some(error) = facts.refusal() {
            return Err(error);
        }
        self.book.create(name)
    }
    pub fn rename(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        self.book.rename(id, name)
    }
    /// The supplied order is physical pane order; the session alone selects membership targets.
    pub fn targets<'a>(
        &'a self,
        id: LayoutId,
        order: impl Iterator<Item = u64> + 'a,
    ) -> impl Iterator<Item = u64> + 'a {
        order.filter(move |pane| {
            self.panes
                .get(pane)
                .is_some_and(|state| state.get().layout == Some(id))
        })
    }
    pub fn plan_delete(
        &self,
        id: LayoutId,
        ordered_facts: impl Iterator<Item = (u64, PaneFacts)>,
    ) -> Result<Vec<Selection>, LayoutError> {
        let index = self.book.index_of(id).ok_or(LayoutError::Unknown)?;
        if self.book.layouts().len() == 1 {
            return Err(LayoutError::Last);
        }
        let to = self
            .book
            .at(index.saturating_sub(1))
            .filter(|layout| layout.id != id)
            .or_else(|| self.book.at(index + 1))
            .ok_or(LayoutError::Last)?
            .id;
        let mut selections = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let on = |state: &Rc<Cell<Membership>>| {
            state
                .get()
                .layout
                .is_some_and(|layout| self.book.origin_of(layout) == id)
        };
        for (pane, facts) in ordered_facts {
            if self.panes.get(&pane).is_some_and(on) {
                if !seen.insert(pane) {
                    return Err(LayoutError::Unknown);
                }
                if let Some(error) = facts.refusal() {
                    return Err(error);
                }
                selections.push(self.select(pane, to, facts)?);
            }
        }
        if seen.len() != self.panes.values().filter(|state| on(state)).count() {
            return Err(LayoutError::Unknown);
        }
        Ok(selections)
    }
    /// Delete strip layout `id` once no pane shows it, and the copies of it
    /// the tabs own with it.
    pub fn finish_delete(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        let book = &self.book;
        if self.panes.values().any(|state| {
            state
                .get()
                .layout
                .is_some_and(|layout| book.origin_of(layout) == id)
        }) {
            return Err(LayoutError::Unknown);
        }
        self.book.delete(id)?;
        let copies: Vec<LayoutId> = self
            .owner
            .keys()
            .copied()
            .filter(|copy| self.book.origin_of(*copy) == id)
            .collect();
        for copy in copies {
            self.book.remove_private(copy);
            self.owner.remove(&copy);
            self.kept.retain(|_, kept| *kept != copy);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
