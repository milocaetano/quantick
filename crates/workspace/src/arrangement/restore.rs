//! Synchronous restore sequencing. The temporary stale-ID worklist is a
//! command batch, not a second editable live order.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreMode {
    StartupOrImport,
    Named,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreEffect {
    AdoptSettings,
    RestoreChrome,
    OpenSaved { index: usize, adopt: Option<TabId> },
    ArrangeSaved { index: usize, target: TabId },
    CloseStale { index: Option<usize> },
    SelectSaved { index: usize },
    RefreshLabel,
    Finish,
}
#[derive(Debug)]
pub struct RestoreStep {
    epoch: u64,
    sequence: usize,
    effect: RestoreEffect,
    previous_len: usize,
}
impl RestoreStep {
    #[must_use]
    pub const fn effect(&self) -> RestoreEffect {
        self.effect
    }
}
#[derive(Debug, Clone, Copy)]
enum Phase {
    Settings,
    ChromeBefore,
    Open(usize),
    Arrange(usize),
    Close(usize),
    ChromeAfter,
    Select,
    Label,
    Finish,
}
#[derive(Debug)]
pub(super) struct RestoreState {
    pub(super) epoch: u64,
    pub(super) sequence: usize,
    mode: RestoreMode,
    count: usize,
    active: usize,
    adopt: Option<TabId>,
    stale: Vec<TabId>,
    phase: Phase,
}
impl ArrangementLifecycle {
    /// The first saved market and number of documents are facts. Adoption and
    /// effect order are decided here; no saved runtime is constructed here.
    pub fn begin_restore(
        &mut self,
        mode: RestoreMode,
        count: usize,
        first_market: Option<(&str, &str)>,
        active: usize,
        facts: &impl Topology,
    ) -> Result<(), ArrangementError> {
        self.validate_current(facts)?;
        if count == 0 && mode == RestoreMode::Named {
            return Err(ArrangementError::MissingTab);
        }
        let first = facts.tab(0).ok_or(ArrangementError::MissingTab)?;
        let adopt = (mode == RestoreMode::StartupOrImport
            && count > 0
            && facts.len() == 1
            && first.id == TabId(0)
            && first_market == Some((first.feed, first.symbol)))
        .then_some(first.id);
        let stale = if adopt.is_some() {
            Vec::new()
        } else {
            (0..facts.len())
                .map(|index| identity(facts, index))
                .collect::<Result<Vec<_>, _>>()?
        };
        self.restore_epoch = self
            .restore_epoch
            .checked_add(1)
            .expect("restore epoch exhausted");
        // Replacing a restore invalidates every prior ordinary/restore plan.
        self.generation = self
            .generation
            .checked_add(1)
            .expect("arrangement generation exhausted");
        self.restore = Some(RestoreState {
            epoch: self.restore_epoch,
            sequence: 0,
            mode,
            count,
            active,
            adopt,
            stale,
            phase: if mode == RestoreMode::StartupOrImport {
                Phase::Settings
            } else {
                Phase::Open(0)
            },
        });
        Ok(())
    }
    pub fn restore_step(&self, facts: &impl Topology) -> Result<RestoreStep, ArrangementError> {
        self.validate_current(facts)?;
        let state = self
            .restore
            .as_ref()
            .ok_or(ArrangementError::StaleTransition)?;
        let effect = match state.phase {
            Phase::Settings => RestoreEffect::AdoptSettings,
            Phase::ChromeBefore | Phase::ChromeAfter => RestoreEffect::RestoreChrome,
            Phase::Open(index) => RestoreEffect::OpenSaved {
                index,
                adopt: if index == 0 { state.adopt } else { None },
            },
            Phase::Arrange(index) => RestoreEffect::ArrangeSaved {
                index,
                target: identity(
                    facts,
                    if index == 0 && state.adopt.is_some() {
                        0
                    } else {
                        facts.len() - 1
                    },
                )?,
            },
            Phase::Close(cursor) => {
                let index = if state.mode == RestoreMode::Named {
                    Some(0)
                } else {
                    (0..facts.len()).find(|index| {
                        facts
                            .tab(*index)
                            .is_some_and(|tab| tab.id == state.stale[cursor])
                    })
                };
                RestoreEffect::CloseStale { index }
            }
            Phase::Select => RestoreEffect::SelectSaved {
                index: state.active,
            },
            Phase::Label => RestoreEffect::RefreshLabel,
            Phase::Finish => RestoreEffect::Finish,
        };
        Ok(RestoreStep {
            epoch: state.epoch,
            sequence: state.sequence,
            effect,
            previous_len: facts.len(),
        })
    }
    /// Translate only this pending effect into a topology command. The host may
    /// refuse provider opening without invoking it, preserving the legacy path.
    pub fn plan_restore(
        &self,
        step: &RestoreStep,
        facts: &impl Topology,
    ) -> Result<Option<Transition>, ArrangementError> {
        self.validate_restore_step(step)?;
        let command = match step.effect {
            RestoreEffect::OpenSaved { adopt: None, .. } => Some(Command::Open),
            RestoreEffect::CloseStale { index: Some(index) } if facts.len() > 1 => {
                Some(Command::Close(index))
            }
            RestoreEffect::SelectSaved { index } => Some(Command::SelectClamped(index)),
            _ => None,
        };
        command
            .map(|command| {
                let mut plan = self.plan(command, facts)?;
                plan.restore_origin = Some((step.epoch, step.sequence));
                Ok(plan)
            })
            .transpose()
    }
    pub fn advance_restore(
        &mut self,
        step: RestoreStep,
        facts: &impl Topology,
    ) -> Result<(), ArrangementError> {
        self.validate_restore_step(&step)?;
        self.validate_current(facts)?;
        let length_ok = match step.effect {
            RestoreEffect::OpenSaved { adopt: None, .. } => {
                facts.len() == step.previous_len || facts.len() == step.previous_len + 1
            }
            RestoreEffect::CloseStale { index: Some(_) } if step.previous_len > 1 => {
                facts.len() + 1 == step.previous_len
            }
            RestoreEffect::SelectSaved { index } => {
                facts.len() == step.previous_len && self.active.index == index.min(facts.len() - 1)
            }
            _ => facts.len() == step.previous_len,
        };
        if !length_ok {
            return Err(ArrangementError::InvalidTopology);
        }
        let state = self
            .restore
            .as_mut()
            .ok_or(ArrangementError::StaleTransition)?;
        state.phase = match state.phase {
            Phase::Settings => Phase::ChromeBefore,
            Phase::ChromeBefore => {
                if state.count == 0 {
                    Phase::Finish
                } else {
                    Phase::Open(0)
                }
            }
            Phase::Open(index) => Phase::Arrange(index),
            Phase::Arrange(index) if index + 1 < state.count => Phase::Open(index + 1),
            Phase::Arrange(_) => {
                if state.stale.is_empty() {
                    Phase::Select
                } else {
                    Phase::Close(0)
                }
            }
            Phase::Close(cursor) if cursor + 1 < state.stale.len() => Phase::Close(cursor + 1),
            Phase::Close(_) => {
                if state.mode == RestoreMode::Named {
                    Phase::ChromeAfter
                } else {
                    Phase::Select
                }
            }
            Phase::ChromeAfter => Phase::Select,
            Phase::Select => Phase::Label,
            Phase::Label => Phase::Finish,
            Phase::Finish => {
                self.restore = None;
                return Ok(());
            }
        };
        state.sequence += 1;
        Ok(())
    }
    fn validate_restore_step(&self, step: &RestoreStep) -> Result<(), ArrangementError> {
        if self
            .restore
            .as_ref()
            .map(|state| (state.epoch, state.sequence))
            != Some((step.epoch, step.sequence))
        {
            return Err(ArrangementError::StaleTransition);
        }
        Ok(())
    }
}
