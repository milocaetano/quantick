//! The autostart supervisor's lifecycle, as a pure state machine.
//!
//! The supervisor outlives any single bridge: it watches a healthy one, gives
//! someone else's bridge a grace period to dial in whenever silence begins,
//! launches its own when the silence lasts, backs off after a bridge that
//! would not stay up, and gives up after a run of consecutive failures. Every
//! one of those decisions depends only on what it has seen so far and on one
//! observation — is a bridge feeding us right now? — so they live here, apart
//! from the sleeps and the child process, where a test can walk them.
//!
//! The driver in [`super::supervise`] owns the clock and the process: it asks
//! [`Autostart`] what to do, does it (sleeping, launching), and reports back
//! through [`Autostart::poll`] or [`Autostart::bridge_down`].

/// What the supervisor does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    /// A bridge is feeding us: sleep one watch interval, then poll again.
    /// `announce` is true on the first poll of a watching episode, which is
    /// the one that logs.
    Watch { announce: bool },
    /// Silence just began: wait out the grace period, then poll again.
    Grace,
    /// Launch a bridge; `attempt` counts from 1 within the failure budget.
    Launch { attempt: u32 },
    /// The bridge we started is down: sleep the retry gap, then poll again.
    Backoff,
    /// The failure budget is spent; stop and say so.
    GiveUp,
}

/// The supervisor's memory between wake-ups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Autostart {
    /// Consecutive failures, not attempts: a bridge that connects resets it.
    failures: u32,
    /// The budget [`Action::GiveUp`] is measured against.
    max_attempts: u32,
    /// Whether the next silence gets a grace period. The grace belongs to
    /// every *episode* of silence, not just the first: someone else's bridge
    /// reconnects on its own schedule, and losing a session arms it again.
    grace_pending: bool,
    /// Whether this watching episode already logged that it is watching.
    watching_logged: bool,
}

impl Autostart {
    /// A fresh supervisor: no failures, and the first silence gets its grace.
    pub(super) fn new(max_attempts: u32) -> Self {
        Self {
            failures: 0,
            max_attempts,
            grace_pending: true,
            watching_logged: false,
        }
    }

    /// Step on a fresh look at whether a bridge is feeding us.
    pub(super) fn poll(&mut self, connected: bool) -> Action {
        if self.failures >= self.max_attempts {
            return Action::GiveUp;
        }
        if connected {
            // Somebody is feeding us — our own bridge, one started by hand, or
            // an Expert Advisor. Watch rather than return: when that one goes
            // away, this is what brings the chart back.
            let announce = !self.watching_logged;
            self.watching_logged = true;
            self.failures = 0;
            self.grace_pending = true;
            return Action::Watch { announce };
        }
        self.watching_logged = false;
        if self.grace_pending {
            self.grace_pending = false;
            return Action::Grace;
        }
        Action::Launch {
            attempt: self.failures + 1,
        }
    }

    /// Step on a launch that ended: the bridge could not be started, or it
    /// started and exited. Either way it is one more consecutive failure.
    pub(super) fn bridge_down(&mut self) -> Action {
        self.failures += 1;
        Action::Backoff
    }
}

#[cfg(test)]
mod autostart_tests {
    use super::*;

    #[test]
    fn silence_gets_a_grace_period_before_the_first_launch() {
        let mut sup = Autostart::new(5);
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 1 });
    }

    #[test]
    fn a_bridge_that_arrives_during_the_grace_is_watched_not_raced() {
        let mut sup = Autostart::new(5);
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(true), Action::Watch { announce: true });
    }

    #[test]
    fn watching_announces_once_per_episode() {
        let mut sup = Autostart::new(5);
        assert_eq!(sup.poll(true), Action::Watch { announce: true });
        assert_eq!(sup.poll(true), Action::Watch { announce: false });
        assert_eq!(sup.poll(true), Action::Watch { announce: false });
        // The bridge goes away and comes back: a new episode announces again.
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(true), Action::Watch { announce: true });
    }

    #[test]
    fn a_failed_launch_backs_off_and_retries_without_a_second_grace() {
        let mut sup = Autostart::new(5);
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 1 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 2 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 3 });
    }

    #[test]
    fn consecutive_failures_spend_the_budget_and_give_up() {
        let mut sup = Autostart::new(2);
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 1 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 2 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        // Spent: even a bridge that shows up now is not watched — the loop
        // has ended, as it always did.
        assert_eq!(sup.poll(true), Action::GiveUp);
        assert_eq!(sup.poll(false), Action::GiveUp);
    }

    #[test]
    fn a_bridge_that_connects_resets_the_budget_and_rearms_the_grace() {
        let mut sup = Autostart::new(2);
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 1 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        // The terminal came back and our retry connected.
        assert_eq!(sup.poll(true), Action::Watch { announce: true });
        // Lost again at lunchtime: grace first, then a full budget.
        assert_eq!(sup.poll(false), Action::Grace);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 1 });
        assert_eq!(sup.bridge_down(), Action::Backoff);
        assert_eq!(sup.poll(false), Action::Launch { attempt: 2 });
    }

    #[test]
    fn a_zero_budget_gives_up_at_once() {
        assert_eq!(Autostart::new(0).poll(false), Action::GiveUp);
    }
}
