//! The worker progress port, `quantick_backpressure::progress`, on the clock
//! this window installs.

use std::sync::Arc;
use std::time::Instant;

pub(crate) use quantick_backpressure::progress::*;

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
