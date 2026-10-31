//! The worker progress port, `quantick_backpressure::progress`, on the clock
//! this window installs.

use std::sync::Arc;
use std::time::Instant;

pub(crate) use quantick_backpressure::progress::*;

/// How long a test waits for a worker to answer a barrier before calling
/// the worker hung. A bound, not a budget: a loaded host that takes a
/// minute to drain a batch is still correct, and a test that asserts on
/// the pipeline before the barrier returns is the only wrong answer.
#[cfg(test)]
pub(crate) const WORKER_REPLY_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// Elapsed time since the worker started: bounded, nonblocking, and never a
/// wall-clock reading.
struct Monotonic(Instant);
impl ProgressClock for Monotonic {
    fn now_ns(&self) -> Option<u64> {
        self.0.elapsed().as_nanos().try_into().ok()
    }
}

/// A worker's progress on the monotonic clock.
pub(crate) fn monotonic() -> WorkerProgress {
    WorkerProgress::with_clock(Arc::new(Monotonic(Instant::now())))
}
