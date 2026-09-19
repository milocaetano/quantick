//! The command side of a streaming venue's feed loop, as a pure decision.
//!
//! Binance and Hyperliquid run the same loop shape: a reconnecting trade stream
//! the venue crate owns, plus two side tasks the loop starts and stops on the
//! UI's behalf — a depth capture and a candle fetch. Each side task is a slot
//! that is either free or busy, and every UI command is answered by looking at
//! those two slots. That decision is the part both venues share, so it lives
//! here once and is tested without a runtime: the loop *observes* whether a
//! task is still running (a `JoinHandle` question only the driver can ask) and
//! hands the answer in; [`plan_command`] says what to do; the driver does it.
//!
//! What differs per venue — how history is paged, what the log lines carry,
//! how a depth task is started — stays in the venue's own driver.

use super::FeedCommand;

/// Whether a side task the loop started is still at work.
///
/// Observed by the driver from its task handle rather than tracked here: a
/// task can end by itself (a depth loop that gave up, a fetch that finished),
/// and only the handle knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// No task, or one that already ended.
    Free,
    /// A task is running and owns the slot.
    Busy,
}

impl Slot {
    /// The slot a possibly-absent task occupies, given whether it finished.
    pub(crate) fn observe(task_finished: Option<bool>) -> Self {
        match task_finished {
            Some(false) => Self::Busy,
            Some(true) | None => Self::Free,
        }
    }
}

/// What the driver does with one UI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandPlan {
    /// Page older trades back from the earliest one held.
    LoadOlder { count: usize },
    /// Start a candle fetch in the free slot.
    StartOhlcv {
        span_ms: i64,
        slice_ms: Option<i64>,
        before_ms: Option<i64>,
    },
    /// Answer a candle request with `Refused`: one fetch at a time, and the
    /// one in flight already answers.
    RefuseOhlcv {
        span_ms: i64,
        before_ms: Option<i64>,
    },
    /// A capture is already running; leave it alone.
    KeepBook { initial_generation: u64 },
    /// Reap whatever occupies the depth slot (logged with `stop_reason`),
    /// then start a capture at `initial_generation`.
    ReplaceBook {
        initial_generation: u64,
        stop_reason: &'static str,
    },
    /// Stop the capture, if any.
    StopBook { reason: &'static str },
    /// A command a live venue has no use for (replay transport).
    Ignore,
    /// The UI dropped its command sender: the loop ends.
    Shutdown,
}

/// Decide what one received command means, given the two side-task slots.
///
/// `None` is the closed command channel.
pub(crate) fn plan_command(cmd: Option<FeedCommand>, book: Slot, ohlcv: Slot) -> CommandPlan {
    match cmd {
        None => CommandPlan::Shutdown,
        Some(FeedCommand::LoadOlder { count }) => CommandPlan::LoadOlder { count },
        Some(FeedCommand::FetchOhlcv {
            span_ms,
            slice_ms,
            before_ms,
        }) => match ohlcv {
            Slot::Busy => CommandPlan::RefuseOhlcv { span_ms, before_ms },
            Slot::Free => CommandPlan::StartOhlcv {
                span_ms,
                slice_ms,
                before_ms,
            },
        },
        Some(FeedCommand::SetBookCapture {
            enabled: true,
            initial_generation,
        }) => match book {
            Slot::Busy => CommandPlan::KeepBook { initial_generation },
            // Reap a task that ended by itself before replacing it.
            Slot::Free => CommandPlan::ReplaceBook {
                initial_generation,
                stop_reason: "finished_before_enable",
            },
        },
        Some(FeedCommand::SetBookCapture { enabled: false, .. }) => {
            CommandPlan::StopBook { reason: "disabled" }
        }
        Some(FeedCommand::RestartBookCapture { initial_generation }) => CommandPlan::ReplaceBook {
            initial_generation,
            stop_reason: "restart",
        },
        // Transport commands belong to a recorded session; a live venue has no
        // playhead to move. Ignored rather than refused — the UI only shows the
        // transport while a replay is the source.
        Some(FeedCommand::Replay(_)) => CommandPlan::Ignore,
    }
}

/// Whether a candle reply frees the fetch slot.
///
/// Only the closing slice does. A run still walking backwards through the span
/// is one fetch, however many replies it makes, and letting a second one start
/// beside it would spend the same rate budget twice.
pub(crate) fn ohlcv_reply_frees_slot(slice: super::OhlcvSlice) -> bool {
    slice.is_last()
}

/// The drivers' one effect primitive: send a message, turning a closed channel
/// (the UI is gone) into the loop's `Break`.
pub(crate) async fn send_or_break<T>(
    channel: &tokio::sync::mpsc::Sender<T>,
    message: T,
) -> std::ops::ControlFlow<()> {
    if channel.send(message).await.is_err() {
        std::ops::ControlFlow::Break(())
    } else {
        std::ops::ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod venue_loop_tests {
    use super::*;
    use crate::OhlcvSlice;

    fn fetch(before_ms: Option<i64>) -> FeedCommand {
        FeedCommand::FetchOhlcv {
            span_ms: 60_000,
            slice_ms: Some(10_000),
            before_ms,
        }
    }

    #[test]
    fn a_slot_is_busy_only_while_its_task_runs() {
        assert_eq!(Slot::observe(None), Slot::Free);
        assert_eq!(Slot::observe(Some(true)), Slot::Free);
        assert_eq!(Slot::observe(Some(false)), Slot::Busy);
    }

    #[test]
    fn a_closed_command_channel_shuts_the_loop_down() {
        assert_eq!(
            plan_command(None, Slot::Busy, Slot::Busy),
            CommandPlan::Shutdown
        );
    }

    #[test]
    fn a_free_fetch_slot_starts_the_fetch_and_a_busy_one_refuses() {
        assert_eq!(
            plan_command(Some(fetch(Some(5))), Slot::Free, Slot::Free),
            CommandPlan::StartOhlcv {
                span_ms: 60_000,
                slice_ms: Some(10_000),
                before_ms: Some(5),
            }
        );
        assert_eq!(
            plan_command(Some(fetch(None)), Slot::Free, Slot::Busy),
            CommandPlan::RefuseOhlcv {
                span_ms: 60_000,
                before_ms: None,
            }
        );
    }

    #[test]
    fn enabling_a_running_capture_keeps_it_and_a_stopped_one_is_replaced() {
        let enable = || FeedCommand::SetBookCapture {
            enabled: true,
            initial_generation: 7,
        };
        assert_eq!(
            plan_command(Some(enable()), Slot::Busy, Slot::Free),
            CommandPlan::KeepBook {
                initial_generation: 7
            }
        );
        assert_eq!(
            plan_command(Some(enable()), Slot::Free, Slot::Free),
            CommandPlan::ReplaceBook {
                initial_generation: 7,
                stop_reason: "finished_before_enable",
            }
        );
    }

    #[test]
    fn disabling_stops_and_restarting_always_replaces() {
        let disable = FeedCommand::SetBookCapture {
            enabled: false,
            initial_generation: 3,
        };
        assert_eq!(
            plan_command(Some(disable), Slot::Busy, Slot::Free),
            CommandPlan::StopBook { reason: "disabled" }
        );
        for book in [Slot::Free, Slot::Busy] {
            assert_eq!(
                plan_command(
                    Some(FeedCommand::RestartBookCapture {
                        initial_generation: 9
                    }),
                    book,
                    Slot::Free,
                ),
                CommandPlan::ReplaceBook {
                    initial_generation: 9,
                    stop_reason: "restart",
                }
            );
        }
    }

    #[test]
    fn history_paging_passes_through_and_replay_transport_is_ignored() {
        assert_eq!(
            plan_command(
                Some(FeedCommand::LoadOlder { count: 500 }),
                Slot::Busy,
                Slot::Busy
            ),
            CommandPlan::LoadOlder { count: 500 }
        );
        assert_eq!(
            plan_command(
                Some(FeedCommand::Replay(crate::ReplayControl::Pause)),
                Slot::Free,
                Slot::Free
            ),
            CommandPlan::Ignore
        );
    }

    #[test]
    fn only_the_closing_candle_reply_frees_the_fetch_slot() {
        assert!(!ohlcv_reply_frees_slot(OhlcvSlice::More));
        assert!(ohlcv_reply_frees_slot(OhlcvSlice::Refused));
        assert!(ohlcv_reply_frees_slot(OhlcvSlice::Last { complete: false }));
    }
}
