//! Lifecycle, delivery and durable readback contracts.
use crate::input::{AlertInput, BarIdentity, ResetReason, Unavailable};
use crate::spec::{AlertId, AlertSpec, MarketRef};
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

/// Empty scaffold. Child implementation supplies storage and evaluation;
/// command/readback requests truthfully report unavailable until then.
#[derive(Debug, Default)]
pub struct AlertBook {
    _reserved: (),
}

impl AlertBook {
    pub fn evaluate(&mut self, _input: &AlertInput<'_>, _out: &mut impl AlertEventSink) {}

    pub fn command(
        &mut self,
        _command: AlertCommand,
        _utc_ms: i64,
    ) -> Result<CommandResult, AlertError> {
        Err(AlertError::Unavailable)
    }

    pub fn validate(&self, _spec: &AlertSpec) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }

    pub fn reset(&mut self, _market: &MarketRef, _reason: ResetReason, _bar: BarIdentity) {}

    pub fn restore(&mut self, _document: AlertDocument, _utc_ms: i64) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }

    pub fn document(&self) -> Result<AlertDocument, AlertError> {
        Err(AlertError::Unavailable)
    }

    pub fn views(&self) -> Result<Vec<AlertView>, AlertError> {
        Err(AlertError::Unavailable)
    }

    pub fn history(&self, _after: u64, _limit: usize) -> Result<Vec<AlertEvent>, AlertError> {
        Err(AlertError::Unavailable)
    }
}
