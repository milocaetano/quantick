//! Deterministic tab identity and selection decisions over borrowed host facts.
//!
//! The host owns the sole physical order. Only validated transition effects may
//! alter it; runtime-only borrows cannot change identity or order. No runtime,
//! clock, worker or UI type is stored here.

mod restore;
pub use restore::{RestoreEffect, RestoreMode, RestoreStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, Copy)]
pub struct TabFacts<'a> {
    pub id: TabId,
    pub feed: &'a str,
    pub symbol: &'a str,
}

/// Borrowed identity/market facts, never runtime objects or a copied order.
pub trait Topology {
    fn len(&self) -> usize;
    fn tab(&self, index: usize) -> Option<TabFacts<'_>>;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Open,
    Select(usize),
    SelectClamped(usize),
    Cycle(isize),
    Close(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Select,
    Append { id: TabId },
    Remove { index: usize, id: TabId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrangementError {
    InvalidTopology,
    MissingTab,
    LastTab,
    StaleTransition,
    IdentityExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveSelection {
    id: TabId,
    index: usize,
}

/// A private transition token cannot be changed to address another tab.
///
/// ```compile_fail
/// use quantick_workspace::arrangement::Transition;
/// fn retarget(plan: &mut Transition) { plan.next.index = 99; }
/// ```
#[derive(Debug)]
pub struct Transition {
    generation: u64,
    previous_len: usize,
    next: ActiveSelection,
    effect: Effect,
    restore_origin: Option<(u64, usize)>,
}
impl Transition {
    #[must_use]
    pub const fn effect(&self) -> Effect {
        self.effect
    }
}

#[derive(Debug)]
pub struct ArrangementLifecycle {
    active: ActiveSelection,
    next_id: Option<u64>,
    generation: u64,
    restore: Option<restore::RestoreState>,
    restore_epoch: u64,
}

impl ArrangementLifecycle {
    /// Register the host's single boot entry. Subsequent insertion is planned.
    #[must_use]
    pub const fn new(first: TabId) -> Self {
        Self {
            active: ActiveSelection {
                id: first,
                index: 0,
            },
            next_id: first.0.checked_add(1),
            generation: 0,
            restore: None,
            restore_epoch: 0,
        }
    }
    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active.index
    }
    #[must_use]
    pub const fn active_id(&self) -> TabId {
        self.active.id
    }

    pub fn plan(
        &self,
        command: Command,
        facts: &impl Topology,
    ) -> Result<Transition, ArrangementError> {
        self.validate_current(facts)?;
        let count = facts.len();
        let (effect, index, id) = match command {
            Command::Open => {
                let id = TabId(self.next_id.ok_or(ArrangementError::IdentityExhausted)?);
                (Effect::Append { id }, count, id)
            }
            Command::Select(index) => (Effect::Select, index, identity(facts, index)?),
            Command::SelectClamped(index) => {
                let index = index.min(count - 1);
                (Effect::Select, index, identity(facts, index)?)
            }
            Command::Cycle(delta) => {
                // Match the existing positional wrap, with wide arithmetic so
                // even a caller-supplied extreme delta remains deterministic.
                let index =
                    (self.active.index as i128 + delta as i128).rem_euclid(count as i128) as usize;
                (Effect::Select, index, identity(facts, index)?)
            }
            Command::Close(removed) => {
                let id = identity(facts, removed)?;
                if count == 1 {
                    return Err(ArrangementError::LastTab);
                }
                let index = self.active.index.min(count - 2);
                let previous_index = if index >= removed { index + 1 } else { index };
                (
                    Effect::Remove { index: removed, id },
                    index,
                    identity(facts, previous_index)?,
                )
            }
        };
        Ok(Transition {
            generation: self.generation,
            previous_len: count,
            next: ActiveSelection { id, index },
            effect,
            restore_origin: None,
        })
    }

    /// Recheck a token before any physical effect. The host never mutates its
    /// private entry vector with a stale or differently targeted plan.
    pub fn validate_transition(
        &self,
        plan: &Transition,
        facts: &impl Topology,
    ) -> Result<(), ArrangementError> {
        self.validate_current(facts)?;
        if plan.generation != self.generation || plan.previous_len != facts.len() {
            return Err(ArrangementError::StaleTransition);
        }
        if let Some(origin) = plan.restore_origin
            && self
                .restore
                .as_ref()
                .map(|state| (state.epoch, state.sequence))
                != Some(origin)
        {
            return Err(ArrangementError::StaleTransition);
        }
        match plan.effect {
            Effect::Remove { index, id } if identity(facts, index)? != id => {
                Err(ArrangementError::InvalidTopology)
            }
            Effect::Append { id } if self.next_id != Some(id.0) => {
                Err(ArrangementError::InvalidTopology)
            }
            Effect::Select if identity(facts, plan.next.index)? != plan.next.id => {
                Err(ArrangementError::InvalidTopology)
            }
            _ => Ok(()),
        }
    }

    /// Land an already performed typed host effect. An unlanded/stale effect
    /// cannot publish a new selection or consume the next identity.
    pub fn commit(
        &mut self,
        plan: Transition,
        facts: &impl Topology,
    ) -> Result<(), ArrangementError> {
        if plan.generation != self.generation {
            return Err(ArrangementError::StaleTransition);
        }
        if let Some(origin) = plan.restore_origin
            && self
                .restore
                .as_ref()
                .map(|state| (state.epoch, state.sequence))
                != Some(origin)
        {
            return Err(ArrangementError::StaleTransition);
        }
        validate_unique(facts)?;
        let expected_len = match plan.effect {
            Effect::Select => plan.previous_len,
            Effect::Append { .. } => plan.previous_len + 1,
            Effect::Remove { .. } => plan.previous_len - 1,
        };
        if facts.len() != expected_len || identity(facts, plan.next.index)? != plan.next.id {
            return Err(ArrangementError::InvalidTopology);
        }
        match plan.effect {
            Effect::Append { id } => {
                if self.next_id != Some(id.0) || identity(facts, facts.len() - 1)? != id {
                    return Err(ArrangementError::InvalidTopology);
                }
                self.next_id = id.0.checked_add(1);
            }
            Effect::Remove { id, .. } => {
                if (0..facts.len()).any(|index| facts.tab(index).is_some_and(|tab| tab.id == id)) {
                    return Err(ArrangementError::InvalidTopology);
                }
            }
            Effect::Select => {}
        }
        if plan.restore_origin.is_none() {
            self.restore = None;
        }
        self.active = plan.next;
        self.generation = self
            .generation
            .checked_add(1)
            .expect("arrangement transition generation exhausted");
        Ok(())
    }

    fn validate_current(&self, facts: &impl Topology) -> Result<(), ArrangementError> {
        validate_unique(facts)?;
        if identity(facts, self.active.index)? != self.active.id {
            return Err(ArrangementError::InvalidTopology);
        }
        if let Some(next) = self.next_id
            && (0..facts.len()).any(|index| facts.tab(index).is_some_and(|tab| tab.id.0 >= next))
        {
            return Err(ArrangementError::InvalidTopology);
        }
        Ok(())
    }
}

fn identity(facts: &impl Topology, index: usize) -> Result<TabId, ArrangementError> {
    facts
        .tab(index)
        .map(|tab| tab.id)
        .ok_or(ArrangementError::MissingTab)
}
fn validate_unique(facts: &impl Topology) -> Result<(), ArrangementError> {
    if facts.is_empty() {
        return Err(ArrangementError::InvalidTopology);
    }
    for index in 0..facts.len() {
        let id = identity(facts, index)?;
        for earlier in 0..index {
            if identity(facts, earlier)? == id {
                return Err(ArrangementError::InvalidTopology);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
