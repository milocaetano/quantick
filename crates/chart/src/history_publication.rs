//! A retained chart and a pending recut, independent of the caller's scheduler.

use super::state::{ChartState, HistoryRebuild};
use quantick_engine::Trade;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Small tapes can be recut in the caller; longer tapes need a runner.
pub const HISTORY_WORKER_TRADES: usize = 50_000;
/// Maximum live tail folded during one publication poll.
const LIVE_CATCHUP_TRADES: usize = 4096;

/// The scheduling boundary. The runner owns threads and result delivery.
pub trait HistoryRunner: Default {
    fn start(&mut self, rebuild: HistoryRebuild, cancelled: Arc<AtomicBool>) -> bool;
    fn finished(&mut self) -> Result<Option<ChartState>, HistoryRunnerStopped>;
    fn retire(&mut self, state: ChartState);
}

#[derive(Debug)]
pub struct HistoryRunnerStopped;

struct Active {
    series_revision: u64,
    source_len: usize,
    pages: Vec<Arc<Vec<Trade>>>,
    cancelled: Arc<AtomicBool>,
    result: Option<ChartState>,
    caught_up: usize,
    ready: bool,
    samples: usize,
    last_sample: Option<quantick_engine::DealSample>,
}

#[derive(Default)]
pub struct HistoryPublication<R: HistoryRunner> {
    runner: R,
    active: Option<Active>,
    queued: Vec<Arc<Vec<Trade>>>,
    page_len: Option<usize>,
    failures: u8,
    failed: bool,
}

impl<R: HistoryRunner> HistoryPublication<R> {
    pub fn pending(&self) -> bool {
        !self.failed && (self.active.is_some() || !self.queued.is_empty())
    }

    pub fn enqueue(&mut self, trades: Arc<Vec<Trade>>, page: bool) {
        self.failed = false;
        if page {
            self.page_len = Some(self.page_len.unwrap_or(0) + trades.len());
        }
        if !trades.is_empty() {
            self.queued.push(trades);
        }
    }

    pub fn take_page(&mut self) -> Option<usize> {
        if self.pending() {
            None
        } else {
            self.page_len.take()
        }
    }

    pub fn cancel(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancelled.store(true, Ordering::Relaxed);
            if let Some(result) = active.result {
                self.runner.retire(result);
            }
        }
        self.queued.clear();
        self.page_len = None;
        self.failures = 0;
        self.failed = false;
        // Each source gets its own result channel; late results cannot enter it.
        self.runner = R::default();
    }

    fn start(&mut self, displayed: &ChartState) {
        if self.failed || self.active.is_some() || self.queued.is_empty() {
            return;
        }
        let pages = std::mem::take(&mut self.queued);
        let cancelled = Arc::new(AtomicBool::new(false));
        let snapshot = displayed.history_rebuild(pages.clone());
        if !self.runner.start(snapshot, Arc::clone(&cancelled)) {
            self.queued = pages;
            self.runner = R::default();
            self.record_failure();
            return;
        }
        self.active = Some(Active {
            series_revision: displayed.series_revision(),
            source_len: displayed.trades().len(),
            pages,
            cancelled,
            result: None,
            caught_up: displayed.trades().len(),
            ready: false,
            samples: displayed.deal_samples().len(),
            last_sample: displayed.deal_samples().last().copied(),
        });
    }

    /// Only a complete recut with an ordered live tail can replace the display.
    pub fn poll(&mut self, displayed: &ChartState) -> bool {
        if self.failed {
            return true;
        }
        self.start(displayed);
        let Some(active) = self.active.as_mut() else {
            return self.queued.is_empty();
        };
        let readings_changed = active.samples > displayed.deal_samples().len()
            || (active.samples > 0
                && displayed.deal_samples().get(active.samples - 1).copied() != active.last_sample)
            || displayed
                .deal_samples()
                .get(active.samples)
                .is_some_and(|sample| {
                    displayed
                        .trades()
                        .get(active.source_len.saturating_sub(1))
                        .is_some_and(|trade| sample.time_ms <= trade.timestamp_ms)
                });
        if active.series_revision != displayed.series_revision() || readings_changed {
            let active = self.active.take().expect("active recut");
            active.cancelled.store(true, Ordering::Relaxed);
            if let Some(result) = active.result {
                self.runner.retire(result);
            }
            let mut pages = active.pages;
            pages.append(&mut self.queued);
            self.queued = pages;
            self.runner = R::default();
            self.start(displayed);
            return false;
        }
        if active.result.is_none() {
            match self.runner.finished() {
                Ok(Some(result)) => active.result = Some(result),
                Ok(None) => return false,
                Err(HistoryRunnerStopped) => {
                    let active = self.active.take().expect("active recut");
                    let mut pages = active.pages;
                    pages.append(&mut self.queued);
                    self.queued = pages;
                    self.runner = R::default();
                    self.record_failure();
                    return false;
                }
            }
        }
        if !self.queued.is_empty() {
            let active = self.active.take().expect("completed recut");
            if let Some(result) = active.result {
                self.runner.retire(result);
            }
            let mut pages = active.pages;
            pages.append(&mut self.queued);
            self.queued = pages;
            self.start(displayed);
            return false;
        }
        let candidate = active.result.as_mut().expect("recut completed");
        // Samples may arrive while the worker runs. Readings are ingested
        // before their tail, as on the ordinary chart path.
        candidate.observe_deals_batch(&displayed.deal_samples()[active.samples..]);
        active.samples = displayed.deal_samples().len();
        active.last_sample = displayed.deal_samples().last().copied();
        let end = (active.caught_up + LIVE_CATCHUP_TRADES).min(displayed.trades().len());
        for trade in displayed.trades().range(active.caught_up..end) {
            candidate.ingest_live(trade);
        }
        active.caught_up = end;
        if end != displayed.trades().len() {
            return false;
        }
        debug_assert!(active.source_len <= end);
        active.ready = true;
        true
    }

    /// Take a ready result only when every pane in the caller's publication is ready.
    pub fn take_ready(&mut self, displayed: &ChartState) -> Option<ChartState> {
        if !self.queued.is_empty() || !self.active.as_ref().is_some_and(|active| active.ready) {
            return None;
        }
        let mut result = self.active.take()?.result?;
        result.advance_history_revision(displayed);
        self.failures = 0;
        Some(result)
    }

    pub fn retire(&mut self, state: ChartState) {
        self.runner.retire(state);
    }

    fn record_failure(&mut self) {
        self.failures += 1;
        self.failed = self.failures >= 2;
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// Retry retained pages without fetching or losing them.
    pub fn retry(&mut self) -> bool {
        if !self.failed {
            return false;
        }
        self.failed = false;
        self.failures = 0;
        true
    }
}

#[cfg(test)]
#[path = "history_publication_tests.rs"]
mod tests;

impl<R: HistoryRunner> Drop for HistoryPublication<R> {
    fn drop(&mut self) {
        self.cancel();
    }
}
