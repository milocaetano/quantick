//! Getting the trader's attention: the per-connection budget and the access
//! adapter the notify handlers in `quantick_control_handlers::notify` run
//! through. The budget is `quantick_control_host::rate::TokenBucket`; the
//! handlers are told only whether a call fits and how long until one would.
//!
//! Rate class: a human or an agent asking for attention. Never per trade,
//! never per frame.

pub(crate) use quantick_control_schema::notify::*;

use std::time::Duration;

use quantick_control::wire::ActorContext;
use quantick_control_handlers::notify::NotifyAccess;

use super::{gateway::ControlAccess, journal::NewEvent};

impl NotifyAccess for ControlAccess {
    fn allow_notification(&mut self, actor: &ActorContext) -> Result<(), Duration> {
        ControlAccess::allow_notification(self, actor)
    }

    /// Journaled at the moment it is recorded, by the window's clock.
    fn record_event(&mut self, event: NewEvent) {
        ControlAccess::append_event(self, event);
    }
}
