//! The bridge listener's lifecycle, as a pure state machine.
//!
//! The feed listens for the bridge on one port. The listener task can end
//! three ways, and each decides what the feed does next:
//!
//! - **it could not bind** — the port is taken, usually by a tab being torn
//!   down or another instance about to close. That is a moment, not a
//!   verdict: the feed answers commands for a retry gap and binds again.
//!   The user hears about it once per distinct error, not once per retry,
//!   and any status from a later listener proves the port opened, so the
//!   same error repeating after that deserves a fresh report;
//! - **it finished cleanly** (shutdown) or **it panicked** — nothing will
//!   listen again, and the feed spends the rest of its life answering
//!   commands empty so no loader spins against a dead feed.
//!
//! The driver in `metatrader.rs` owns the task, the retry sleep and the
//! channels; [`Listener`] decides.

/// How the listener task ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListenerExit {
    /// The listen address would not bind; `message` is the error's text.
    BindFailed { message: String },
    /// The task returned without error: shutdown.
    Finished,
    /// The task panicked.
    Panicked,
}

/// What the driver does after the listener ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AfterExit {
    /// Answer commands for the retry gap, then bind again. `report` is true
    /// when this error is news to the user.
    RetryBind { report: bool },
    /// Nothing will listen again: answer commands until the UI goes away.
    ServeUntilGone,
}

/// The listener's memory across restarts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Listener {
    /// The bind error last reported to the user, so a port that stays taken
    /// produces one attention card rather than one per retry.
    reported_bind_error: Option<String>,
}

impl Listener {
    /// A status arrived: the listener is up, so a bind failure that later
    /// repeats deserves a fresh report.
    pub(super) fn status_seen(&mut self) {
        self.reported_bind_error = None;
    }

    /// Step on the listener task ending.
    pub(super) fn exited(&mut self, exit: ListenerExit) -> AfterExit {
        match exit {
            ListenerExit::BindFailed { message } => {
                let report = self.reported_bind_error.as_deref() != Some(message.as_str());
                if report {
                    self.reported_bind_error = Some(message);
                }
                AfterExit::RetryBind { report }
            }
            ListenerExit::Finished | ListenerExit::Panicked => AfterExit::ServeUntilGone,
        }
    }
}

#[cfg(test)]
mod listener_tests {
    use super::*;

    fn bind(message: &str) -> ListenerExit {
        ListenerExit::BindFailed {
            message: message.to_owned(),
        }
    }

    #[test]
    fn a_port_that_stays_taken_is_reported_once_and_retried_every_time() {
        let mut listener = Listener::default();
        assert_eq!(
            listener.exited(bind("in use")),
            AfterExit::RetryBind { report: true }
        );
        assert_eq!(
            listener.exited(bind("in use")),
            AfterExit::RetryBind { report: false }
        );
        assert_eq!(
            listener.exited(bind("in use")),
            AfterExit::RetryBind { report: false }
        );
    }

    #[test]
    fn a_different_bind_error_is_reported_again() {
        let mut listener = Listener::default();
        let _ = listener.exited(bind("in use"));
        assert_eq!(
            listener.exited(bind("access denied")),
            AfterExit::RetryBind { report: true }
        );
    }

    #[test]
    fn a_status_between_failures_rearms_the_report() {
        let mut listener = Listener::default();
        let _ = listener.exited(bind("in use"));
        listener.status_seen();
        assert_eq!(
            listener.exited(bind("in use")),
            AfterExit::RetryBind { report: true }
        );
    }

    #[test]
    fn a_listener_that_finished_or_panicked_is_never_restarted() {
        let mut listener = Listener::default();
        assert_eq!(
            listener.exited(ListenerExit::Finished),
            AfterExit::ServeUntilGone
        );
        assert_eq!(
            listener.exited(ListenerExit::Panicked),
            AfterExit::ServeUntilGone
        );
    }
}
