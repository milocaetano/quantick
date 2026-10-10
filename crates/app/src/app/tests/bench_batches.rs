//! Batch statistics shared by the control-plane capture benches.

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

/// The best of a bench's batches, each a whole real batch: the one with the
/// lowest median (what the always-on budget guards judge) and the one with
/// the lowest p99 (what the ignored strict tail guards judge). A noisy
/// neighbour can only make a batch look slower, never faster, so the best
/// batch is the honest reading of the capture's own cost.
#[derive(Clone, Copy, Debug)]
pub(super) struct BestBatches {
    pub(super) lowest_median: BatchReading,
    pub(super) lowest_p99: BatchReading,
    pub(super) batches: usize,
}

impl BestBatches {
    pub(super) fn of(readings: &[BatchReading]) -> Self {
        Self {
            lowest_median: *readings.iter().min_by_key(|r| r.median_us).unwrap(),
            lowest_p99: *readings.iter().min_by_key(|r| r.p99_us).unwrap(),
            batches: readings.len(),
        }
    }
}
