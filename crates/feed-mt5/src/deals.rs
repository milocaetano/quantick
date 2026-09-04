//! The venue's deal counter, turned from a stamp on every tick into the
//! sample series the engine's deal bars join prints against.
//!
//! MetaTrader folds several exchange deals into one tick and keeps no count
//! per tick; what it does keep is the session's running total
//! (`SYMBOL_SESSION_DEALS`). The bridge reads that total once per poll and
//! stamps it on every tick the poll fetched (`deals` in `PROTOCOL.md`). This
//! module reduces those stamps to one [`DealSample`] per *change* — the first
//! tick carrying a new reading — on the tape's own clock, so the engine can
//! join a print to the reading in force when it was read.
//!
//! One sample per change rather than per tick because the reading is a fact
//! about the poll, not the print: forty ticks fetched together carry the
//! same number forty times, and forty identical samples would only make the
//! series forty times heavier for a consumer that retains it all day.

use quantick_engine::DealSample;

use crate::protocol::Tick;

/// Reduces the per-tick `deals` stamps of one session to a sample series.
#[derive(Debug, Clone)]
pub struct DealSampler {
    /// `server_utc_offset_s × 1000`, the same correction the tick mapper
    /// applies, so a sample and the print it stamps share a clock.
    offset_ms: i64,
    /// The last reading turned into a sample; the dedupe.
    last: Option<u64>,
    /// How many ticks carried a stamp, and how many of those became samples.
    pub stats: DealSampleStats,
}

/// Counters for the health view: how the stamps reduced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DealSampleStats {
    /// Ticks that carried a `deals` stamp.
    pub stamped: u64,
    /// Stamps that changed the reading and became a sample.
    pub samples: u64,
    /// Stamps lower than the reading in force — a session rollover, or a
    /// terminal that answered out of order. Forwarded, never dropped: the
    /// engine decides what a lower reading means (see its deal builder).
    pub regressions: u64,
}

impl DealSampler {
    /// A sampler for a session whose ticks are stamped in server time
    /// `server_utc_offset_s` seconds ahead of UTC.
    #[must_use]
    pub fn new(server_utc_offset_s: i64) -> Self {
        Self {
            // Saturating for the same reason as the tick mapper: the offset is
            // declared by any process that dials the port.
            offset_ms: server_utc_offset_s.saturating_mul(1000),
            last: None,
            stats: DealSampleStats::default(),
        }
    }

    /// The sample this tick contributes, if its stamp is a new reading.
    ///
    /// Ticks without a stamp — history pages, an older bridge — contribute
    /// nothing and are not counted as stamped.
    pub fn observe(&mut self, tick: &Tick) -> Option<DealSample> {
        let deals = tick.deals?;
        self.stats.stamped += 1;
        if self.last == Some(deals) {
            return None;
        }
        if self.last.is_some_and(|last| deals < last) {
            self.stats.regressions += 1;
        }
        self.last = Some(deals);
        self.stats.samples += 1;
        Some(DealSample {
            time_ms: tick.time_ms.saturating_sub(self.offset_ms),
            session_deals: deals,
        })
    }

    /// The reading in force after the last stamped tick, if any.
    #[must_use]
    pub fn reading(&self) -> Option<u64> {
        self.last
    }

    /// Follow the bridge's clock as the tick mapper does: a heartbeat may
    /// restate the server offset, and a sample and the print it stamps must
    /// keep sharing one clock or the join lands them hours apart.
    pub fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        self.offset_ms = offset_s.saturating_mul(1000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick(seq: u64, time_ms: i64, deals: Option<u64>) -> Tick {
        Tick {
            seq,
            time_ms,
            sent_ms: None,
            bid: "0".into(),
            ask: "0".into(),
            last: "100".into(),
            volume: 1,
            flags: 1080,
            deals,
        }
    }

    #[test]
    fn one_sample_per_change_on_the_tapes_own_clock() {
        let mut sampler = DealSampler::new(-10_800);
        let first = sampler.observe(&tick(1, 1_000, Some(2_000_000)));
        assert_eq!(
            first,
            Some(DealSample {
                time_ms: 1_000 + 10_800_000,
                session_deals: 2_000_000
            })
        );
        assert!(sampler.observe(&tick(2, 1_005, Some(2_000_000))).is_none());
        assert!(sampler.observe(&tick(3, 1_010, Some(2_000_000))).is_none());
        let next = sampler.observe(&tick(4, 1_020, Some(2_000_007)));
        assert_eq!(next.map(|s| s.session_deals), Some(2_000_007));
        assert_eq!(sampler.stats.stamped, 4);
        assert_eq!(sampler.stats.samples, 2);
        assert_eq!(sampler.reading(), Some(2_000_007));
    }

    #[test]
    fn the_offset_follows_the_heartbeat() {
        let mut sampler = DealSampler::new(-10_800);
        sampler.set_server_utc_offset_s(-7_200);
        let sample = sampler.observe(&tick(1, 1_000, Some(5))).unwrap();
        assert_eq!(sample.time_ms, 1_000 + 7_200_000);
    }

    #[test]
    fn an_unstamped_tick_is_neither_a_sample_nor_counted() {
        let mut sampler = DealSampler::new(0);
        assert!(sampler.observe(&tick(1, 1, None)).is_none());
        assert_eq!(sampler.stats, DealSampleStats::default());
        assert_eq!(sampler.reading(), None);
    }

    #[test]
    fn a_lower_reading_is_forwarded_and_counted_as_a_regression() {
        let mut sampler = DealSampler::new(0);
        sampler.observe(&tick(1, 1, Some(5_000_000)));
        let rollover = sampler.observe(&tick(2, 2, Some(3)));
        assert_eq!(rollover.map(|s| s.session_deals), Some(3));
        assert_eq!(sampler.stats.regressions, 1);
    }
}
