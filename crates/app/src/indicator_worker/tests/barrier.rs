//! The indicator worker's test-side barrier, on the shared rule
//! (`worker_progress::barrier`).
use super::{IndicatorWorker, WorkerCommand};
use crate::worker_progress::barrier;
use std::sync::mpsc::{Receiver, channel};

impl IndicatorWorker {
    /// Wait for a reply to a command just sent, pumping parked commands
    /// meanwhile. `None` when the worker is gone or hung
    /// ([`barrier::WORKER_REPLY_WAIT`]); a caller that must tell the two
    /// apart flushes instead.
    pub(crate) fn await_reply<T>(&self, rx: &Receiver<T>) -> Option<T> {
        barrier::await_reply(&self.commands, rx).ok()
    }

    /// Block until every command sent before this call has been applied and
    /// its events published. A flush the worker never acknowledged is this
    /// call's failure, named here.
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(WorkerCommand::Flush(ack_tx));
        if let Err(why) = barrier::await_reply(&self.commands, &ack_rx) {
            panic!("{}", barrier::unacknowledged("indicator worker", why));
        }
    }
}
