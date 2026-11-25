#![cfg(test)]

use super::FAKE_NOW_UNIX_MS;
use crate::JournalAccess;
use crate::annotate::AnnotateAccess;
use crate::notify::NotifyAccess;
use quantick_control::wire::{ActorContext, ActorKind, WireU64};
use quantick_control_host::journal::NewEvent;
use std::time::Duration;

/// A fake access: a fixed number of notifications it allows, then the wait
/// it reports, and every event it journaled.
pub(crate) struct FakeAccess {
    pub budget: usize,
    pub retry_after: Duration,
    pub events: Vec<NewEvent>,
    /// The actor and client a replayed run recorded, when the call replays one.
    pub replayed: Option<(ActorKind, String)>,
}

impl FakeAccess {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            retry_after: Duration::from_millis(1_500),
            events: Vec::new(),
            replayed: None,
        }
    }
}

impl Default for FakeAccess {
    fn default() -> Self {
        Self::with_budget(0)
    }
}

impl JournalAccess for FakeAccess {
    fn record_event(&mut self, event: NewEvent) -> WireU64 {
        self.events.push(event);
        WireU64::new(self.events.len() as u64)
    }
    fn now_unix_ms(&self) -> i64 {
        FAKE_NOW_UNIX_MS
    }
}

impl AnnotateAccess for FakeAccess {
    fn recorded_actor(&self) -> Option<(ActorKind, &str)> {
        self.replayed
            .as_ref()
            .map(|(kind, client)| (*kind, client.as_str()))
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}

impl NotifyAccess for FakeAccess {
    fn allow_notification(&mut self, _actor: &ActorContext) -> Result<(), Duration> {
        if self.budget == 0 {
            return Err(self.retry_after);
        }
        self.budget -= 1;
        Ok(())
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}
