//! Getting the trader's attention: the per-connection budget and the access
//! adapter the notify handlers in `quantick_control_handlers::notify` run
//! through. The budget stays here because it reads the monotonic clock; the
//! handlers are told only whether a call fits and how long until one would.
//!
//! Rate class: a human or an agent asking for attention. Never per trade,
//! never per frame.

pub(crate) use quantick_control_schema::notify::*;

use std::time::{Duration, Instant};

use quantick_control::{
    limits::{CONTROL_NOTIFICATION_BURST, CONTROL_NOTIFICATION_RATE_PER_MINUTE},
    wire::ActorContext,
};
use quantick_control_handlers::notify::NotifyAccess;

use crate::metrics;

use super::{gateway::ControlAccess, journal::NewEvent};

/// Per-client notification budget: stricter than the ordinary request limit
/// because the cost of exceeding it is a trader who cannot work, not a queue
/// that fills.
pub(crate) struct NotificationLimiter {
    available_token_nanos: u128,
    last_refill: Instant,
}

impl NotificationLimiter {
    const ONE_TOKEN_NANOS: u128 = 1_000_000_000;
    /// The budget is stated per minute and the clock ticks in nanoseconds,
    /// so every refill divides by the seconds in a minute.
    const SECONDS_PER_MINUTE: u128 = 60;

    pub fn new() -> Self {
        Self {
            available_token_nanos: u128::from(CONTROL_NOTIFICATION_BURST) * Self::ONE_TOKEN_NANOS,
            last_refill: Instant::now(),
        }
    }

    /// Whether one more notification fits, refilling by elapsed time first.
    pub fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let capacity = u128::from(CONTROL_NOTIFICATION_BURST) * Self::ONE_TOKEN_NANOS;
        let refill = elapsed
            .as_nanos()
            .saturating_mul(u128::from(CONTROL_NOTIFICATION_RATE_PER_MINUTE))
            / Self::SECONDS_PER_MINUTE;
        self.available_token_nanos = self
            .available_token_nanos
            .saturating_add(refill)
            .min(capacity);
        if self.available_token_nanos < Self::ONE_TOKEN_NANOS {
            return false;
        }
        self.available_token_nanos -= Self::ONE_TOKEN_NANOS;
        true
    }

    /// How long until one more notification would be allowed.
    pub fn retry_after(&self) -> Duration {
        if self.available_token_nanos >= Self::ONE_TOKEN_NANOS {
            return Duration::ZERO;
        }
        let missing = Self::ONE_TOKEN_NANOS - self.available_token_nanos;
        let nanos = missing.saturating_mul(Self::SECONDS_PER_MINUTE)
            / u128::from(CONTROL_NOTIFICATION_RATE_PER_MINUTE).max(1);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

impl NotifyAccess for ControlAccess {
    fn allow_notification(&mut self, actor: &ActorContext) -> Result<(), Duration> {
        ControlAccess::allow_notification(self, actor)
    }

    /// Journaled at the moment it is recorded, by the window's clock.
    fn record_event(&mut self, event: NewEvent) {
        self.journal_mut().record(event, metrics::wall_clock_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The budget's arithmetic, pinned on a clock the test owns: two at
    /// once, then one every ten seconds, and the wait it reports between.
    #[test]
    fn the_notification_budget_refills_one_every_ten_seconds() {
        let started = Instant::now();
        let mut limiter = NotificationLimiter {
            available_token_nanos: u128::from(CONTROL_NOTIFICATION_BURST)
                * NotificationLimiter::ONE_TOKEN_NANOS,
            last_refill: started,
        };
        assert!(limiter.allow(started));
        assert!(limiter.allow(started));
        assert!(!limiter.allow(started));
        assert_eq!(limiter.retry_after(), Duration::from_secs(10));
        assert!(!limiter.allow(started + Duration::from_secs(5)));
        assert_eq!(limiter.retry_after(), Duration::from_secs(5));
        assert!(limiter.allow(started + Duration::from_secs(10)));
        assert_eq!(limiter.retry_after(), Duration::from_secs(10));
    }
}
