//! Batch statistics shared by the control-plane capture benches, and the
//! counted heap work their always-on guards judge.

use crate::work_meter;

/// One measured batch, in microseconds.
#[derive(Clone, Copy, Debug)]
pub(super) struct BatchReading {
    pub(super) median_us: u64,
    pub(super) p99_us: u64,
    pub(super) worst_us: u64,
}

impl BatchReading {
    pub(super) fn of(mut elapsed_us: Vec<u64>) -> Self {
        elapsed_us.sort_unstable();
        let p99_index = (elapsed_us.len() * 99).div_ceil(100).saturating_sub(1);
        Self {
            median_us: elapsed_us[elapsed_us.len() / 2],
            p99_us: elapsed_us[p99_index],
            worst_us: *elapsed_us.last().unwrap(),
        }
    }
}

/// The heap work of one capture, counted on the capturing thread.
///
/// Unlike a stopwatch, this reading cannot move with the host's speed or
/// load, so it can fail only when the capture itself does more work.
#[derive(Clone, Copy, Debug)]
pub(super) struct CaptureHeap {
    pub(super) allocs: u64,
    pub(super) alloc_bytes: u64,
}

impl CaptureHeap {
    /// The fewest allocations and the fewest bytes any of `captures` runs
    /// of `capture` needed, each judged on its own.
    pub(super) fn least_of(captures: usize, mut capture: impl FnMut()) -> Self {
        let mut least = Self {
            allocs: u64::MAX,
            alloc_bytes: u64::MAX,
        };
        for _ in 0..captures {
            let before = work_meter::tally();
            capture();
            let spent = work_meter::tally().since(before);
            least.allocs = least.allocs.min(spent.allocs);
            least.alloc_bytes = least.alloc_bytes.min(spent.alloc_bytes);
        }
        least
    }

    /// Fail when this capture exceeds either ceiling, naming the path.
    pub(super) fn assert_within(self, path: &str, max_allocs: u64, max_alloc_bytes: u64) {
        let Self {
            allocs,
            alloc_bytes,
        } = self;
        assert!(
            allocs <= max_allocs && alloc_bytes <= max_alloc_bytes,
            "{path} does {allocs} allocations of {alloc_bytes} bytes; the ceilings are {max_allocs} and {max_alloc_bytes}"
        );
    }
}

/// The best of a bench's batches, a whole real batch: the one with the lowest
/// p99 (what the ignored strict tail guards judge). A noisy neighbour can only
/// make a batch look slower, never faster, so the best batch is the honest
/// reading of the capture's own cost.
#[derive(Clone, Copy, Debug)]
pub(super) struct BestBatches {
    pub(super) lowest_p99: BatchReading,
    pub(super) batches: usize,
}

impl BestBatches {
    pub(super) fn of(readings: &[BatchReading]) -> Self {
        Self {
            lowest_p99: *readings.iter().min_by_key(|r| r.p99_us).unwrap(),
            batches: readings.len(),
        }
    }
}
