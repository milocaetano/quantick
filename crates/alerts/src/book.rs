//! Lifecycle, delivery and durable readback contracts.
use crate::input::{AlertInput, BarIdentity, ResetReason, Unavailable};
use crate::spec::{AlertId, AlertSpec, Expiration, ExpirationChoice, MarketRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertError {
    Unavailable,
    UnknownId,
    StaleRevision {
        actual: u64,
    },
    Limit {
        resource: &'static str,
        maximum: usize,
    },
    Invalid {
        field: String,
        reason: String,
    },
    UnsupportedVersion(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlertCommand {
    Create(AlertSpec),
    Edit {
        expected_revision: u64,
        spec: AlertSpec,
    },
    Pause {
        id: AlertId,
        expected_revision: u64,
    },
    Resume {
        id: AlertId,
        expected_revision: u64,
    },
    Rearm {
        id: AlertId,
        expected_revision: u64,
    },
    Delete {
        id: AlertId,
        expected_revision: u64,
    },
    ClearHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandResult {
    pub id: Option<AlertId>,
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertStatus {
    Monitoring,
    Paused,
    Completed,
    Expired,
    Suspended(Unavailable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliveryOutcome {
    Pending,
    Delivered,
    Suppressed,
    Failed,
}

/// An immutable event snapshot survives edits and deletion of the definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub sequence: u64,
    pub definition_revision: u64,
    pub spec: AlertSpec,
    pub utc_ms: i64,
    pub session_ms: u64,
    pub bar_epoch: u64,
    pub bar_sequence: u64,
    pub price: f64,
    pub delivery: DeliveryOutcome,
}

/// Delivery is bounded by the consumer. `false` is an explicit refusal;
/// the core must record it, never retry a notification implicitly.
pub trait AlertEventSink {
    fn try_push(&mut self, event: AlertEvent) -> bool;
}

/// A preallocated queue for a frame/consumer; it never grows past capacity.
#[derive(Debug)]
pub struct BoundedEvents {
    events: Vec<AlertEvent>,
    capacity: usize,
}

impl BoundedEvents {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.min(crate::spec::MAX_ALERTS);
        Self {
            events: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn drain(&mut self) -> impl Iterator<Item = AlertEvent> + '_ {
        self.events.drain(..)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl AlertEventSink for BoundedEvents {
    fn try_push(&mut self, event: AlertEvent) -> bool {
        if self.events.len() == self.capacity {
            return false;
        }
        self.events.push(event);
        true
    }
}

/// Durable duplicate suppression is separate from transient crossing baselines.
/// Baselines are always reseeded after restoration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedAlert {
    pub spec: AlertSpec,
    pub revision: u64,
    pub enabled: bool,
    pub completed: bool,
    pub last_fired_utc_ms: Option<i64>,
    pub last_fired_session_ms: Option<u64>,
    pub last_bar_epoch: Option<u64>,
    pub last_bar_sequence: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertDocument {
    pub version: u32,
    pub next_id: u64,
    pub next_event_sequence: u64,
    pub definitions: Vec<SavedAlert>,
    pub history: Vec<AlertEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertView {
    pub spec: AlertSpec,
    pub revision: u64,
    pub status: AlertStatus,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EditorView {
    pub draft: AlertSpec,
    pub errors: Vec<AlertError>,
    pub summary: String,
}

#[path = "book_editor.rs"]
mod editor;
#[path = "book_lifecycle.rs"]
mod lifecycle;
#[path = "book_predicate.rs"]
mod predicate;
use crate::input::ObservationKind;
use crate::spec::{FORMAT_VERSION, Frequency, MAX_ALERTS, MAX_CONDITIONS, MAX_HISTORY};
use std::collections::VecDeque;
#[derive(Debug)]
struct Entry {
    saved: SavedAlert,
    baselines: [predicate::Baseline; MAX_CONDITIONS],
    status: AlertStatus,
    epoch: Option<u64>,
    last_close: Option<BarIdentity>,
    reseed: bool,
    minute_anchor: Option<(u64, u64)>,
}
impl Entry {
    fn new(saved: SavedAlert) -> Self {
        Self {
            saved,
            baselines: [predicate::Baseline::default(); MAX_CONDITIONS],
            status: AlertStatus::Suspended(Unavailable::SourceInactive),
            epoch: None,
            last_close: None,
            reseed: false,
            minute_anchor: None,
        }
    }
    fn clear(&mut self) {
        self.baselines.fill(predicate::Baseline::default());
        self.last_close = None;
        self.reseed = true;
    }
    fn status(&self, utc_ms: i64) -> AlertStatus {
        if self.saved.completed {
            AlertStatus::Completed
        } else if matches!(self.saved.spec.expiration,Expiration::At{utc_ms:cutoff,..} if utc_ms>=cutoff)
        {
            AlertStatus::Expired
        } else if !self.saved.enabled {
            AlertStatus::Paused
        } else {
            self.status
        }
    }
}
/// Bounded deterministic alert runtime. All clocks and geometry are injected.
#[derive(Debug)]
pub struct AlertBook {
    entries: Vec<Entry>,
    log: VecDeque<AlertEvent>,
    next_id: u64,
    next_event_sequence: u64,
    utc_ms: i64,
}
impl Default for AlertBook {
    fn default() -> Self {
        Self {
            entries: Vec::with_capacity(MAX_ALERTS),
            log: VecDeque::with_capacity(MAX_HISTORY),
            next_id: 1,
            next_event_sequence: 1,
            utc_ms: 0,
        }
    }
}
impl AlertBook {
    pub(crate) fn drawing_targets<'a>(
        &'a self,
        market: &'a MarketRef,
    ) -> impl Iterator<Item = &'a crate::DurableDrawingRef> {
        self.entries
            .iter()
            .filter(move |entry| entry.saved.spec.market == *market)
            .flat_map(|entry| &entry.saved.spec.conditions)
            .filter_map(|condition| match &condition.target {
                crate::TargetRef::Drawing(target) => Some(target),
                crate::TargetRef::Value(_) => None,
            })
    }
    pub fn evaluate(&mut self, input: &AlertInput<'_>, out: &mut impl AlertEventSink) {
        self.utc_ms = input.utc_ms;
        for entry in &mut self.entries {
            if entry.saved.spec.market != *input.market {
                continue;
            }
            if matches!(
                entry.status(input.utc_ms),
                AlertStatus::Completed | AlertStatus::Expired | AlertStatus::Paused
            ) {
                continue;
            }
            let availability = input.availability.and_then(|()| {
                if input.price.is_finite() && input.coordinate.is_finite() {
                    Ok(())
                } else {
                    Err(Unavailable::InvalidGeometry)
                }
            });
            if let Err(reason) = availability {
                entry.clear();
                entry.status = AlertStatus::Suspended(reason);
                continue;
            }
            if entry.epoch.is_some_and(|epoch| epoch != input.bar.epoch) {
                entry.clear();
            }
            entry.epoch = Some(input.bar.epoch);
            let mut geometry_changed = false;
            let mut geometry_error = None;
            for (condition, state) in entry.saved.spec.conditions.iter().zip(&entry.baselines) {
                match predicate::preflight(condition, input, state) {
                    Ok(changed) => geometry_changed |= changed,
                    Err(reason) => geometry_error = Some(reason),
                }
            }
            if let Some(reason) = geometry_error {
                entry.clear();
                entry.status = AlertStatus::Suspended(reason);
                continue;
            }
            if geometry_changed {
                entry.clear();
            }
            entry.status = AlertStatus::Monitoring;
            if input.kind == ObservationKind::Lifecycle {
                continue;
            }
            // A restored/new epoch has no comparable monotonic timestamp. Wait
            // a fresh interval, anchored once, rather than trusting UTC jumps.
            if entry.saved.last_fired_utc_ms.is_some()
                && entry
                    .minute_anchor
                    .is_none_or(|(epoch, _)| epoch != input.bar.epoch)
            {
                entry.minute_anchor = Some((input.bar.epoch, input.session_ms));
            }
            if entry.saved.spec.frequency == Frequency::PerBarClose {
                if input.kind != ObservationKind::CommittedClose {
                    continue;
                }
                if entry.last_close == Some(input.bar) {
                    continue;
                }
                entry.last_close = Some(input.bar);
            }
            let mut qualifies = true;
            let mut unavailable = None;
            for (condition, state) in entry
                .saved
                .spec
                .conditions
                .iter()
                .zip(entry.baselines.iter_mut())
            {
                match predicate::judge(condition, input, state) {
                    Ok(result) => qualifies &= result,
                    Err(reason) => {
                        unavailable = Some(reason);
                        qualifies = false;
                    }
                }
            }
            if let Some(reason) = unavailable {
                entry.clear();
                entry.status = AlertStatus::Suspended(reason);
                continue;
            }
            entry.status = AlertStatus::Monitoring;
            if entry.reseed {
                entry.reseed = false;
                continue;
            }
            let saved = &mut entry.saved;
            let cadence = match saved.spec.frequency {
                Frequency::Once => true,
                Frequency::PerBar | Frequency::PerBarClose => {
                    saved.last_bar_epoch != Some(input.bar.epoch)
                        || saved.last_bar_sequence != Some(input.bar.sequence)
                }
                Frequency::PerMinute => entry.minute_anchor.is_none_or(|(_, last)| {
                    input
                        .session_ms
                        .checked_sub(last)
                        .is_some_and(|delta| delta >= 60_000)
                }),
            };
            if !qualifies || !cadence {
                continue;
            }
            entry.minute_anchor = Some((input.bar.epoch, input.session_ms));
            saved.last_bar_epoch = Some(input.bar.epoch);
            saved.last_bar_sequence = Some(input.bar.sequence);
            saved.last_fired_utc_ms = Some(input.utc_ms);
            saved.last_fired_session_ms = Some(input.session_ms);
            saved.completed = saved.spec.frequency == Frequency::Once;
            let mut event = AlertEvent {
                sequence: self.next_event_sequence,
                definition_revision: saved.revision,
                spec: saved.spec.clone(),
                utc_ms: input.utc_ms,
                session_ms: input.session_ms,
                bar_epoch: input.bar.epoch,
                bar_sequence: input.bar.sequence,
                price: input.price,
                delivery: DeliveryOutcome::Pending,
            };
            self.next_event_sequence = self.next_event_sequence.saturating_add(1);
            if !out.try_push(event.clone()) {
                event.delivery = DeliveryOutcome::Suppressed;
            }
            if self.log.len() == MAX_HISTORY {
                self.log.pop_front();
            }
            self.log.push_back(event);
        }
    }
    pub fn tick(&mut self, utc_ms: i64, _session_ms: u64) {
        self.utc_ms = utc_ms;
    }
    pub fn validate(&self, spec: &AlertSpec) -> Result<(), AlertError> {
        editor::validate(spec)
    }
    pub fn editor(&self, draft: AlertSpec) -> Result<EditorView, AlertError> {
        let errors = self.validate(&draft).err().into_iter().collect();
        Ok(EditorView {
            summary: editor::summary(&draft),
            draft,
            errors,
        })
    }
    pub fn record_delivery(
        &mut self,
        sequence: u64,
        outcome: DeliveryOutcome,
    ) -> Result<(), AlertError> {
        let event = self
            .log
            .iter_mut()
            .find(|event| event.sequence == sequence)
            .ok_or(AlertError::UnknownId)?;
        event.delivery = outcome;
        Ok(())
    }
    pub fn reset(&mut self, market: &MarketRef, reason: ResetReason, bar: BarIdentity) {
        for entry in &mut self.entries {
            if entry.saved.spec.market == *market {
                entry.clear();
                entry.epoch = Some(bar.epoch);
                entry.status = AlertStatus::Suspended(match reason {
                    ResetReason::Hydration => Unavailable::Hydrating,
                    _ => Unavailable::SourceInactive,
                });
            }
        }
    }
    pub fn document(&self) -> Result<AlertDocument, AlertError> {
        Ok(AlertDocument {
            version: FORMAT_VERSION,
            next_id: self.next_id,
            next_event_sequence: self.next_event_sequence,
            definitions: self
                .entries
                .iter()
                .map(|entry| entry.saved.clone())
                .collect(),
            history: self.log.iter().cloned().collect(),
        })
    }
    pub fn views(&self) -> Result<Vec<AlertView>, AlertError> {
        Ok(self
            .entries
            .iter()
            .map(|entry| AlertView {
                spec: entry.saved.spec.clone(),
                revision: entry.saved.revision,
                status: entry.status(self.utc_ms),
                summary: editor::summary(&entry.saved.spec),
            })
            .collect())
    }
    pub fn history(&self, after: u64, limit: usize) -> Result<Vec<AlertEvent>, AlertError> {
        Ok(self
            .log
            .iter()
            .filter(|event| event.sequence > after)
            .take(limit.min(MAX_HISTORY))
            .cloned()
            .collect())
    }
}
