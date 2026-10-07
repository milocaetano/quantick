//! Sole physical tab order and runtime effects for the arrangement model.
use super::{
    ArrangementError, ArrangementLifecycle, Command, Effect, RestoreMode, RestoreStep, TabFacts,
    TabId, Topology, Transition,
};
use std::ops::Index;

/// What the arrangement needs from a tab runtime: its market facts and the
/// one settle step before it is dropped.
pub trait TabRuntime {
    fn feed(&self) -> &str;
    fn symbol(&self) -> &str;
    /// Everything the runtime must settle before it is dropped.
    fn close(&mut self);
}

struct TabEntry<R> {
    id: TabId,
    runtime: R,
}
struct Entries<'a, R>(&'a [TabEntry<R>]);
impl<R: TabRuntime> Topology for Entries<'_, R> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn tab(&self, index: usize) -> Option<TabFacts<'_>> {
        self.0.get(index).map(|entry| TabFacts {
            id: entry.id,
            feed: entry.runtime.feed(),
            symbol: entry.runtime.symbol(),
        })
    }
}

/// Mutable borrows contain runtimes only. Entry identity and physical order
/// never escape, even through indexing or whole-value runtime replacement.
pub struct Arrangement<R: TabRuntime> {
    lifecycle: ArrangementLifecycle,
    runtimes: Vec<TabEntry<R>>,
}
impl<R: TabRuntime> Arrangement<R> {
    pub fn new(id: TabId, runtime: R) -> Self {
        Self {
            lifecycle: ArrangementLifecycle::new(id),
            runtimes: vec![TabEntry { id, runtime }],
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn into_single_runtime(self) -> R {
        assert_eq!(
            self.runtimes.len(),
            1,
            "fixture consumes a single-tab owner"
        );
        self.runtimes.into_iter().next().unwrap().runtime
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_fixture_identity(self, id: u64) -> Self {
        Self::new(TabId(id), self.into_single_runtime())
    }
    pub fn len(&self) -> usize {
        self.runtimes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.runtimes.is_empty()
    }
    pub fn active_index(&self) -> usize {
        self.lifecycle.active_index()
    }
    pub fn active_id(&self) -> u64 {
        self.lifecycle.active_id().0
    }
    pub fn id_at(&self, index: usize) -> u64 {
        self.runtimes[index].id.0
    }
    pub fn get(&self, index: usize) -> Option<&R> {
        self.runtimes.get(index).map(|entry| &entry.runtime)
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut R> {
        self.runtimes.get_mut(index).map(|entry| &mut entry.runtime)
    }
    pub fn runtime_mut(&mut self, index: usize) -> &mut R {
        &mut self.runtimes[index].runtime
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn last(&self) -> Option<&R> {
        self.runtimes.last().map(|entry| &entry.runtime)
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &R> + ExactSizeIterator {
        self.runtimes.iter().map(|entry| &entry.runtime)
    }
    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = &mut R> + ExactSizeIterator {
        self.runtimes.iter_mut().map(|entry| &mut entry.runtime)
    }
    pub fn iter_with_ids(&self) -> impl DoubleEndedIterator<Item = (u64, &R)> + ExactSizeIterator {
        self.runtimes
            .iter()
            .map(|entry| (entry.id.0, &entry.runtime))
    }
    pub fn iter_with_ids_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (u64, &mut R)> + ExactSizeIterator {
        self.runtimes
            .iter_mut()
            .map(|entry| (entry.id.0, &mut entry.runtime))
    }
    pub fn position(&self, id: u64) -> Option<usize> {
        self.runtimes.iter().position(|entry| entry.id.0 == id)
    }
    pub fn by_id(&self, id: u64) -> Option<&R> {
        self.position(id).map(|index| &self.runtimes[index].runtime)
    }
    pub fn by_id_mut(&mut self, id: u64) -> Option<&mut R> {
        self.position(id)
            .map(|index| &mut self.runtimes[index].runtime)
    }
    pub fn select(&mut self, index: usize) {
        if let Ok(plan) = self
            .lifecycle
            .plan(Command::Select(index), &Entries(&self.runtimes))
        {
            self.lifecycle
                .commit(plan, &Entries(&self.runtimes))
                .expect("selection does not alter topology");
        }
    }
    pub fn cycle(&mut self, delta: isize) {
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
    pub fn begin_restore(
        &mut self,
        mode: RestoreMode,
        count: usize,
        first: Option<(&str, &str)>,
        active: usize,
    ) -> Result<(), ArrangementError> {
        self.lifecycle
            .begin_restore(mode, count, first, active, &Entries(&self.runtimes))
    }
    pub fn restore_step(&self) -> RestoreStep {
        self.lifecycle
            .restore_step(&Entries(&self.runtimes))
            .expect("current restore")
    }
    pub fn plan_restore(&self, step: &RestoreStep) -> Option<Transition> {
        self.lifecycle
            .plan_restore(step, &Entries(&self.runtimes))
            .expect("current restore effect")
    }
    pub fn advance_restore(&mut self, step: RestoreStep) {
        self.lifecycle
            .advance_restore(step, &Entries(&self.runtimes))
            .expect("restore effect landed");
    }
    pub fn validate_transition(&self, plan: &Transition) -> Result<(), ArrangementError> {
        self.lifecycle
            .validate_transition(plan, &Entries(&self.runtimes))
    }
    pub fn commit_selection(&mut self, plan: Transition) {
        self.lifecycle
            .validate_transition(&plan, &Entries(&self.runtimes))
            .expect("current selection");
        assert!(matches!(plan.effect(), Effect::Select));
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("selection landed");
    }
    pub fn plan_open(&self) -> Transition {
        self.lifecycle
            .plan(Command::Open, &Entries(&self.runtimes))
            .expect("tab identities available")
    }
    pub fn append(&mut self, plan: Transition, runtime: R) {
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
    pub fn plan_close(&self, index: usize) -> Result<Transition, ArrangementError> {
        self.lifecycle
            .plan(Command::Close(index), &Entries(&self.runtimes))
    }
    /// Remove the planned tab and hand it back closed; sibling owners forget it.
    pub fn close_planned(&mut self, plan: Transition) -> Result<ClosedTab<R>, ArrangementError> {
        self.lifecycle
            .validate_transition(&plan, &Entries(&self.runtimes))?;
        let Effect::Remove { index, id } = plan.effect() else {
            return Err(ArrangementError::InvalidTopology);
        };
        let mut runtime = self.runtimes.remove(index).runtime;
        runtime.close();
        tracing::info!(target: "quantick::app", schema_version = 1_u8, event_code = "TAB_CLOSED", tab = id.0, feed = %runtime.feed(), symbol = %runtime.symbol(), tabs = self.runtimes.len(), action = "drop_feed_and_workers", "closing a market tab");
        self.lifecycle
            .commit(plan, &Entries(&self.runtimes))
            .expect("removal landed exactly once");
        Ok(ClosedTab { id: id.0, runtime })
    }
}

/// A tab the host has removed and closed; dropping it releases its workers.
pub struct ClosedTab<R> {
    pub id: u64,
    pub runtime: R,
}
impl<R: TabRuntime> Index<usize> for Arrangement<R> {
    type Output = R;
    fn index(&self, index: usize) -> &Self::Output {
        &self.runtimes[index].runtime
    }
}

#[cfg(test)]
mod tests;
