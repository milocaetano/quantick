//! The book worker's test-side barrier, on the shared rule
//! (`worker_progress::barrier`).
use super::{BookCommand, BookWorker};
use crate::worker_progress::barrier;
use std::sync::mpsc::channel;

impl BookWorker {
    /// Block until every command sent before this call has been applied and
    /// published. A flush the worker never acknowledged is this call's
    /// failure, named here.
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(BookCommand::Flush(ack_tx));
        if let Err(why) = barrier::await_reply(&self.commands, &ack_rx) {
            panic!("{}", barrier::unacknowledged("book worker", why));
        }
    }
}
