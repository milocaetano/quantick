//! The test-side barrier rule both workers share: wait for the worker's
//! answer, pumping parked commands meanwhile, and say which way it failed.
//!
//! A test is its own UI frame loop, so the wait also pumps whatever a full
//! queue parked — a parked barrier would otherwise wait on a queue nobody
//! refills. The bound is against a hung worker, not a budget: a loaded host
//! that takes a minute to drain a batch is still correct, and a test that
//! asserts on the pipeline before the barrier returns is the only wrong
//! answer. The old ten-second wait returned quietly when it passed, and a
//! test under CPU contention then failed some unrelated assertion a few
//! lines on.
use super::ObservedSender;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long a test waits for a worker to answer a barrier before calling
/// the worker hung.
pub(crate) const WORKER_REPLY_WAIT: Duration = Duration::from_secs(120);

/// Why a barrier was not answered: the two are different bugs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unanswered {
    /// The worker's end of the channel is gone: its thread ended.
    WorkerGone,
    /// [`WORKER_REPLY_WAIT`] passed with the worker still alive.
    Hung,
}

/// Wait for the reply to a command just sent on `commands`, pumping parked
/// commands meanwhile.
pub(crate) fn await_reply<C, T>(
    commands: &ObservedSender<C>,
    rx: &Receiver<T>,
) -> Result<T, Unanswered> {
    let deadline = Instant::now() + WORKER_REPLY_WAIT;
    while Instant::now() < deadline {
        commands.pump();
        match rx.recv_timeout(Duration::from_millis(5)) {
            Ok(reply) => return Ok(reply),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Unanswered::WorkerGone),
        }
    }
    Err(Unanswered::Hung)
}

/// The failure a flush reports, naming the worker and which way it failed.
pub(crate) fn unacknowledged(worker: &str, why: Unanswered) -> String {
    match why {
        Unanswered::WorkerGone => format!("the {worker} went away before acknowledging its flush"),
        Unanswered::Hung => {
            format!("the {worker} did not acknowledge its flush within {WORKER_REPLY_WAIT:?}")
        }
    }
}
