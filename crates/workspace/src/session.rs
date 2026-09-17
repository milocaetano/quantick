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
    pub strategy_armed: bool,
    pub gesture_in_flight: bool,
}
impl PaneFacts {
    fn refusal(self) -> Option<LayoutError> {
        if self.strategy_armed {
            Some(LayoutError::StrategyArmed)
        } else if self.gesture_in_flight {
            Some(LayoutError::GestureInFlight)
        } else {
            None
        }
    }
}

/// Ordered host operations; membership and default changes occur at distinct boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutEffect {
    LeaveGestures,
    PersistChangedDrawings,
    StoreOutgoingDrawings,
    DetachIndicators,
    CommitMembership,
    MaterializeIndicators,
    RestoreDrawings,
    RefreshLabel,
    CommitDefault,
    MarkDirty,
}
const SWAP_EFFECTS: &[LayoutEffect] = &[
    LayoutEffect::LeaveGestures,
    LayoutEffect::PersistChangedDrawings,
    LayoutEffect::StoreOutgoingDrawings,
    LayoutEffect::DetachIndicators,
    LayoutEffect::CommitMembership,
    LayoutEffect::MaterializeIndicators,
    LayoutEffect::RestoreDrawings,
    LayoutEffect::RefreshLabel,
    LayoutEffect::CommitDefault,
    LayoutEffect::MarkDirty,
];
/// A validated selection. Outgoing effects precede committing this transition.
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

/// The headless owner of the document and all registered pane memberships.
#[derive(Debug)]
pub struct LayoutSession {
    book: LayoutBook,
    panes: BTreeMap<u64, Rc<Cell<Membership>>>,
    next_generation: u64,
}
impl LayoutSession {
    #[must_use]
    pub fn new(book: LayoutBook) -> Self {
        Self {
            book,
            panes: BTreeMap::new(),
            next_generation: 0,
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
    pub fn store_drawings(
        &mut self,
        id: LayoutId,
        key: &crate::layout_document::DrawingKey,
        items: Vec<crate::layout_document::SavedDrawing>,
        known_tool: impl Fn(&str) -> bool,
    ) {
        if let Some(layout) = self.book.get_mut(id) {
            layout.replace_drawings(key, items, known_tool);
        }
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
            value.layout = None;
            value.seeded = false;
            state.set(value);
        }
    }
    pub fn replace_book(&mut self, book: LayoutBook) {
        let panes = std::mem::take(&mut self.panes);
        for state in panes.into_values() {
            let mut value = state.get();
            value.layout = None;
            value.seeded = false;
            state.set(value);
        }
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
    /// Commit at the named membership effect, after outgoing drawings and slots leave.
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
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .expect("layout generation exhausted");
        value.generation = self.next_generation;
        value.layout = Some(selection.to);
        if selection.changed() {
            value.seeded = true;
        }
        state.set(value);
        Ok(CommittedSelection {
            selection,
            generation: value.generation,
        })
    }
    /// Last-pick default changes only after incoming indicators and drawings are restored.
    pub fn finish_selection(&mut self, committed: CommittedSelection) -> Result<(), LayoutError> {
        let selection = committed.selection;
        let state = self
            .panes
            .get(&selection.pane)
            .ok_or(LayoutError::Unknown)?
            .get();
        if state.generation != committed.generation || state.layout != Some(selection.to) {
            return Err(LayoutError::Unknown);
        }
        self.book.get(selection.to).ok_or(LayoutError::Unknown)?;
        if selection.changed() {
            self.book.switch(selection.to)?;
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
        for (pane, facts) in ordered_facts {
            if self
                .panes
                .get(&pane)
                .is_some_and(|state| state.get().layout == Some(id))
            {
                if !seen.insert(pane) {
                    return Err(LayoutError::Unknown);
                }
                if let Some(error) = facts.refusal() {
                    return Err(error);
                }
                selections.push(self.select(pane, to, facts)?);
            }
        }
        if seen.len()
            != self
                .panes
                .values()
                .filter(|state| state.get().layout == Some(id))
                .count()
        {
            return Err(LayoutError::Unknown);
        }
        Ok(selections)
    }
    pub fn finish_delete(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        if self
            .panes
            .values()
            .any(|state| state.get().layout == Some(id))
        {
            return Err(LayoutError::Unknown);
        }
        self.book.delete(id)
    }
}

#[cfg(test)]
mod tests;
