//! Sole physical tab order and runtime effects for the arrangement model.
use crate::tab::Tab;
use quantick_workspace::arrangement::{
    ArrangementLifecycle, Command, Effect, TabFacts, TabId, Topology, Transition,
};
use std::ops::Index;

struct TabEntry {
    id: TabId,
    runtime: Tab,
}
struct Entries<'a>(&'a [TabEntry]);
impl Topology for Entries<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn tab(&self, index: usize) -> Option<TabFacts<'_>> {
        self.0.get(index).map(|entry| TabFacts {
            id: entry.id,
            feed: &entry.runtime.feed_id,
            symbol: &entry.runtime.symbol,
        })
    }
}

/// Mutable borrows contain runtimes only. Entry identity and physical order
/// never escape, even through indexing or whole-value runtime replacement.
pub(crate) struct ArrangementHost {
    lifecycle: ArrangementLifecycle,
    runtimes: Vec<TabEntry>,
}
impl ArrangementHost {
    pub(super) fn new(id: TabId, runtime: Tab) -> Self {
        Self {
            lifecycle: ArrangementLifecycle::new(id),
            runtimes: vec![TabEntry { id, runtime }],
        }
    }
    #[cfg(test)]
    pub(super) fn into_single_runtime(self) -> Tab {
        assert_eq!(
            self.runtimes.len(),
            1,
            "fixture consumes a single-tab owner"
        );
        self.runtimes.into_iter().next().unwrap().runtime
    }
    #[cfg(test)]
    pub(super) fn with_fixture_identity(self, id: u64) -> Self {
        Self::new(TabId(id), self.into_single_runtime())
    }
    pub(crate) fn len(&self) -> usize {
        self.runtimes.len()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.runtimes.is_empty()
    }
    pub(crate) fn active_index(&self) -> usize {
        self.lifecycle.active_index()
    }
    pub(crate) fn active_id(&self) -> u64 {
        self.lifecycle.active_id().0
    }
    pub(crate) fn id_at(&self, index: usize) -> u64 {
        self.runtimes[index].id.0
    }
    pub(crate) fn get(&self, index: usize) -> Option<&Tab> {
        self.runtimes.get(index).map(|entry| &entry.runtime)
    }
    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.runtimes.get_mut(index).map(|entry| &mut entry.runtime)
    }
    pub(crate) fn runtime_mut(&mut self, index: usize) -> &mut Tab {
        &mut self.runtimes[index].runtime
    }
    #[cfg(test)]
    pub(crate) fn last(&self) -> Option<&Tab> {
        self.runtimes.last().map(|entry| &entry.runtime)
    }
    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &Tab> + ExactSizeIterator {
        self.runtimes.iter().map(|entry| &entry.runtime)
    }
    pub(crate) fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = &mut Tab> + ExactSizeIterator {
        self.runtimes.iter_mut().map(|entry| &mut entry.runtime)
    }
    pub(crate) fn iter_with_ids(
        &self,
    ) -> impl DoubleEndedIterator<Item = (u64, &Tab)> + ExactSizeIterator {
        self.runtimes
            .iter()
            .map(|entry| (entry.id.0, &entry.runtime))
    }
    pub(crate) fn iter_with_ids_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (u64, &mut Tab)> + ExactSizeIterator {
        self.runtimes
            .iter_mut()
            .map(|entry| (entry.id.0, &mut entry.runtime))
    }
    pub(crate) fn position(&self, id: u64) -> Option<usize> {
        self.runtimes.iter().position(|entry| entry.id.0 == id)
    }
    pub(crate) fn by_id(&self, id: u64) -> Option<&Tab> {
        self.position(id).map(|index| &self.runtimes[index].runtime)
    }
    pub(crate) fn by_id_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.position(id)
            .map(|index| &mut self.runtimes[index].runtime)
    }
    pub(super) fn select(&mut self, index: usize) {
        if let Ok(plan) = self
            .lifecycle
            .plan(Command::Select(index), &Entries(&self.runtimes))
        {
            self.lifecycle
                .commit(plan, &Entries(&self.runtimes))
                .expect("selection does not alter topology");
        }
    }
    pub(super) fn cycle(&mut self, delta: isize) {
        if self.len() < 2 {
            return;
        }
        let plan = self
            .lifecycle
            .plan(Command::Cycle(delta), &Entries(&self.runtimes))
            .expect("valid arrangement");
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("selection does not alter topology");
    }
    pub(super) fn begin_restore(
        &mut self,
        mode: quantick_workspace::arrangement::RestoreMode,
        count: usize,
        first: Option<(&str, &str)>,
        active: usize,
    ) -> Result<(), quantick_workspace::arrangement::ArrangementError> {
        self.lifecycle
            .begin_restore(mode, count, first, active, &Entries(&self.runtimes))
    }
    pub(super) fn restore_step(&self) -> quantick_workspace::arrangement::RestoreStep {
        self.lifecycle
            .restore_step(&Entries(&self.runtimes))
            .expect("current restore")
    }
    pub(super) fn plan_restore(
        &self,
        step: &quantick_workspace::arrangement::RestoreStep,
    ) -> Option<Transition> {
        self.lifecycle
            .plan_restore(step, &Entries(&self.runtimes))
            .expect("current restore effect")
    }
    pub(super) fn advance_restore(&mut self, step: quantick_workspace::arrangement::RestoreStep) {
        self.lifecycle
            .advance_restore(step, &Entries(&self.runtimes))
            .expect("restore effect landed");
    }
    pub(super) fn validate_transition(
        &self,
        plan: &Transition,
    ) -> Result<(), quantick_workspace::arrangement::ArrangementError> {
        self.lifecycle
            .validate_transition(plan, &Entries(&self.runtimes))
    }
    pub(super) fn commit_selection(&mut self, plan: Transition) {
        self.lifecycle
            .validate_transition(&plan, &Entries(&self.runtimes))
            .expect("current selection");
        assert!(matches!(plan.effect(), Effect::Select));
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("selection landed");
    }
    pub(super) fn plan_open(&self) -> Transition {
        self.lifecycle
            .plan(Command::Open, &Entries(&self.runtimes))
            .expect("tab identities available")
    }
    pub(super) fn append(&mut self, plan: Transition, runtime: Tab) {
        self.lifecycle
            .validate_transition(&plan, &Entries(&self.runtimes))
            .expect("current opening plan");
        let Effect::Append { id } = plan.effect() else {
            panic!("opening needs append effect")
        };
        self.runtimes.push(TabEntry { id, runtime });
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("append landed exactly once");
    }
    pub(super) fn plan_close(
        &self,
        index: usize,
    ) -> Result<Transition, quantick_workspace::arrangement::ArrangementError> {
        self.lifecycle
            .plan(Command::Close(index), &Entries(&self.runtimes))
    }
    /// Remove the planned tab and hand it back closed; sibling owners forget it.
    pub(super) fn close_planned(
        &mut self,
        plan: Transition,
    ) -> Result<ClosedTab, quantick_workspace::arrangement::ArrangementError> {
        self.lifecycle
            .validate_transition(&plan, &Entries(&self.runtimes))?;
        let Effect::Remove { index, id } = plan.effect() else {
            return Err(quantick_workspace::arrangement::ArrangementError::InvalidTopology);
        };
        let mut runtime = self.runtimes.remove(index).runtime;
        runtime.close();
        tracing::info!(target: "quantick::app", schema_version = 1_u8, event_code = "TAB_CLOSED", tab = id.0, feed = %runtime.feed_id, symbol = %runtime.symbol, tabs = self.runtimes.len(), action = "drop_feed_and_workers", "closing a market tab");
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("removal landed exactly once");
        Ok(ClosedTab { id: id.0, runtime })
    }
}

/// A tab the host has removed and closed; dropping it releases its workers.
pub(super) struct ClosedTab {
    pub(super) id: u64,
    pub(super) runtime: Tab,
}
impl Index<usize> for ArrangementHost {
    type Output = Tab;
    fn index(&self, index: usize) -> &Self::Output {
        &self.runtimes[index].runtime
    }
}
