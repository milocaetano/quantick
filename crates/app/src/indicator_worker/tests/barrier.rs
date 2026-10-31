//! The test-side barrier: wait for the indicator worker, and say when it
//! never answers.
//!
//! A test is its own UI frame loop, so waiting here also pumps whatever a
//! full queue parked — a parked barrier would otherwise wait on a queue
//! nobody refills. The wait is a bound against a hung worker, not a budget:
//! a loaded host that takes a minute to drain a batch is still correct, and
//! a test that asserts on the pipeline before the barrier returns is the
//! only wrong answer. The old ten-second wait returned quietly when it
//! passed, and a test under CPU contention then failed some unrelated
//! assertion a few lines on.
use super::{IndicatorWorker, WorkerCommand};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// How long a test waits for a worker to answer a barrier before calling
/// the worker hung. Shared with the book worker's barrier.
pub(crate) const WORKER_REPLY_WAIT: Duration = Duration::from_secs(120);

impl IndicatorWorker {
    /// Wait for a reply to a command just sent, pumping parked commands
    /// meanwhile. `None` when the worker is gone, or after
    /// [`WORKER_REPLY_WAIT`].
    pub(crate) fn await_reply<T>(&self, rx: &Receiver<T>) -> Option<T> {
        let deadline = Instant::now() + WORKER_REPLY_WAIT;
        while Instant::now() < deadline {
            self.commands.pump();
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(reply) => return Some(reply),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
        None
    }

    /// Block until every command sent before this call has been applied and
    /// its events published. A flush the worker never acknowledged is this
    /// call's failure, named here.
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(WorkerCommand::Flush(ack_tx));
        self.await_reply(&ack_rx)
            .expect("the indicator worker acknowledged its flush within WORKER_REPLY_WAIT");
    }
}
