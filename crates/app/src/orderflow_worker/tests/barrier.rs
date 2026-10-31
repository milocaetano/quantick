//! The test-side barrier: wait for the book worker, and say when it never
//! answers. The rule and the bound are the indicator worker's
//! (`indicator_worker::barrier`).
use super::{BookCommand, BookWorker};
pub(crate) use crate::indicator_worker::barrier::WORKER_REPLY_WAIT;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

impl BookWorker {
    /// Block until every command sent before this call has been applied and
    /// published. A flush the worker never acknowledged is this call's
    /// failure, named here. A test is its own frame loop: parked commands
    /// are pumped meanwhile, or a parked barrier would wait on a queue
    /// nobody refills.
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(BookCommand::Flush(ack_tx));
        let deadline = Instant::now() + WORKER_REPLY_WAIT;
        while Instant::now() < deadline {
            self.commands.pump();
            match ack_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(()) => return,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("the book worker went away before acknowledging its flush")
                }
            }
        }
        panic!("the book worker did not acknowledge its flush within {WORKER_REPLY_WAIT:?}");
    }
}
