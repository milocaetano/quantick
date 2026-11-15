//! How long a live print waits inside this process before paper trading sees it.
//!
//! The figure is the drain's receipt stamp (`received_at_ms`, read once when
//! the window starts draining the feed queue) subtracted from the moment the
//! paper-trading stage runs for that print. It is the intake half of the
//! tape delay: the venue and the wire are reported elsewhere, and this says
//! whether the window itself is falling behind a busy tape.
//!
//! The owner is told the time — both stamps arrive as arguments — so the
//! window keeps the clock and this module stays headless and deterministic.
//! Only the newest [`INTAKE_LAG_WINDOW`] samples are kept: the readout
//! describes the present tape, not the whole session, and memory stays fixed.

/// How many of the newest prints the percentiles cover.
pub const INTAKE_LAG_WINDOW: usize = 1_024;

/// A bounded window of per-print intake lags, in milliseconds.
#[derive(Clone, Debug, Default)]
pub struct IntakeLag {
    /// Ring storage; grows to [`INTAKE_LAG_WINDOW`] and is then overwritten.
    samples: Vec<i64>,
    /// Where the next sample lands once the ring is full.
    next: usize,
    /// Samples whose stamps ran backwards and were recorded as zero.
    clamped: u64,
}

/// The window summarised; nearest-rank percentiles over the kept samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntakeLagSummary {
    pub p50_ms: i64,
    pub p99_ms: i64,
    pub max_ms: i64,
    /// Samples the percentiles cover, at most [`INTAKE_LAG_WINDOW`].
    pub samples: u64,
    /// Recorded samples, since the last [`IntakeLag::clear`], whose stage
    /// stamp preceded the receipt stamp (a wall-clock step back) and were
    /// counted as zero lag rather than dropped.
    pub clamped: u64,
}

impl IntakeLag {
    /// Record one print: received at `received_at_ms`, reached paper trading
    /// at `now_ms`. A stage stamp before the receipt stamp records zero and
    /// is counted in [`IntakeLagSummary::clamped`].
    pub fn record(&mut self, received_at_ms: i64, now_ms: i64) {
        let lag = now_ms.saturating_sub(received_at_ms);
        let lag = if lag < 0 {
            self.clamped = self.clamped.saturating_add(1);
            0
        } else {
            lag
        };
        if self.samples.len() < INTAKE_LAG_WINDOW {
            self.samples.push(lag);
        } else {
            self.samples[self.next] = lag;
            self.next = (self.next + 1) % INTAKE_LAG_WINDOW;
        }
    }

    /// Forget every sample; a new feed session starts its own window.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The window's percentiles, or `None` before the first sample.
    #[must_use]
    pub fn summary(&self) -> Option<IntakeLagSummary> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        Some(IntakeLagSummary {
            p50_ms: nearest_rank(&sorted, 50),
            p99_ms: nearest_rank(&sorted, 99),
            max_ms: sorted[sorted.len() - 1],
            samples: sorted.len() as u64,
            clamped: self.clamped,
        })
    }
}

/// The nearest-rank `percent`th value of a non-empty, ascending slice.
fn nearest_rank(sorted: &[i64], percent: usize) -> i64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_window_has_no_summary() {
        assert_eq!(IntakeLag::default().summary(), None);
    }

    #[test]
    fn one_sample_is_every_percentile() {
        let mut lag = IntakeLag::default();
        lag.record(1_000, 1_007);
        assert_eq!(
            lag.summary(),
            Some(IntakeLagSummary {
                p50_ms: 7,
                p99_ms: 7,
                max_ms: 7,
                samples: 1,
                clamped: 0,
            })
        );
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mut lag = IntakeLag::default();
        // Lags 1..=100 in reverse order: insertion order must not matter.
        for ms in (1..=100).rev() {
            lag.record(0, ms);
        }
        let summary = lag.summary().expect("samples were recorded");
        assert_eq!(
            (summary.p50_ms, summary.p99_ms, summary.max_ms),
            (50, 99, 100)
        );
        assert_eq!(summary.samples, 100);
    }

    #[test]
    fn the_window_keeps_only_the_newest_samples() {
        let mut lag = IntakeLag::default();
        lag.record(0, 9_999);
        for _ in 0..INTAKE_LAG_WINDOW {
            lag.record(0, 3);
        }
        let summary = lag.summary().expect("samples were recorded");
        assert_eq!(summary.samples, INTAKE_LAG_WINDOW as u64);
        assert_eq!(summary.max_ms, 3, "the oldest outlier was evicted");
    }

    #[test]
    fn a_backwards_stamp_records_zero_and_is_counted() {
        let mut lag = IntakeLag::default();
        lag.record(2_000, 1_990);
        lag.record(2_000, 2_004);
        let summary = lag.summary().expect("samples were recorded");
        assert_eq!((summary.p50_ms, summary.max_ms), (0, 4));
        assert_eq!(summary.clamped, 1);
    }

    #[test]
    fn clear_starts_a_fresh_window() {
        let mut lag = IntakeLag::default();
        lag.record(0, 1_990);
        lag.record(5, 0);
        lag.clear();
        assert_eq!(lag.summary(), None);
        lag.record(0, 2);
        assert_eq!(lag.summary().map(|s| (s.max_ms, s.clamped)), Some((2, 0)));
    }
}
