//! Diagnostic access to the worker owner, without cloning its projections.
use super::OrderflowView;
use crate::orderflow_worker::BookCommand;
use crate::worker_progress::ProgressSnapshot;

impl OrderflowView {
    pub(crate) fn worker_progress(&self) -> ProgressSnapshot {
        self.worker.progress()
    }

    pub fn reset_summary_counters(&mut self) {
        self.worker.send(BookCommand::ResetSummaryCounters);
    }
}

#[cfg(test)]
impl OrderflowView {
    pub(crate) fn with_worker_for_test(
        symbol: &str,
        worker: crate::orderflow_worker::BookWorker,
    ) -> Self {
        let mut view = Self::new(symbol);
        view.worker = worker;
        view
    }
}
