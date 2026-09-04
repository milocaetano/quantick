//! The venue's deal counter, turned from a stamp on every tick into the
//! sample series the engine's deal bars join prints against.
//!
//! MetaTrader folds several exchange deals into one tick and keeps no count
//! per tick; what it does keep is the session's running total
//! (`SYMBOL_SESSION_DEALS`). The bridge reads that total once per poll and
//! stamps it on every tick the poll fetched (`deals` in `PROTOCOL.md`). This
//! module reduces those stamps to one [`DealSample`] per *change* — the first
//! tick carrying a new reading, dated at the last tick the previous reading
//! covered — on the tape's own clock, so the engine can join a print to the
//! reading in force when it was read.
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
    /// A new reading, waiting for its round to end so it can be dated at
    /// the round's last tick — the last print the reading is known to
    /// cover, since the bridge reads the counter after it fetched the round.
    pending: Option<PendingReading>,
    /// How many ticks carried a stamp, and how many of those became samples.
    pub stats: DealSampleStats,
}

/// A reading first carried by a round whose end is not known yet.
#[derive(Debug, Clone, Copy)]
struct PendingReading {
    deals: u64,
    /// The round, by the send time every tick of it shares.
    round_sent_ms: i64,
    /// The newest tick of the round so far, on the bridge's clock.
    last_tick_ms: i64,
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
            pending: None,
            stats: DealSampleStats::default(),
        }
    }

    /// The sample this tick contributes, if its stamp is a new reading.
    ///
    /// Ticks without a stamp — history pages, an older bridge — contribute
    /// nothing and are not counted as stamped.
    /// The sample this tick contributes, if a stamp became a new reading.
    ///
    /// Ticks without a stamp — history pages, an older bridge — contribute
    /// nothing and are not counted as stamped. A reading is dated at the
    /// last tick of the round that first carried it: the bridge reads the
    /// counter after it fetched the round, so the reading covers every tick
    /// of that round, and the engine — joining a print to the reading
    /// strictly before it — must see those prints under it, not credited
    /// again on top of a total that already holds them. The round's end is
    /// known when a tick of the next round arrives (ticks share `sent_ms`
    /// within a round), which is when the sample is emitted — ahead of that
    /// tick, as the stream sends it. A tick without `sent_ms` (an older
    /// bridge) has no round and is dated at its own time.
    pub fn observe(&mut self, tick: &Tick) -> Option<DealSample> {
        let deals = tick.deals?;
        self.stats.stamped += 1;
        let mut emitted = None;
        if let Some(pending) = self.pending {
            if tick.sent_ms == Some(pending.round_sent_ms) {
                self.pending = Some(PendingReading {
                    last_tick_ms: tick.time_ms,
                    ..pending
                });
            } else {
                self.pending = None;
                emitted = Some(self.emit(pending.deals, pending.last_tick_ms));
            }
        }
        if self.pending.is_none() && self.last != Some(deals) {
            match tick.sent_ms {
                Some(round_sent_ms) => {
                    self.pending = Some(PendingReading {
                        deals,
                        round_sent_ms,
                        last_tick_ms: tick.time_ms,
                    });
                }
                None => emitted = Some(self.emit(deals, tick.time_ms)),
            }
        }
        emitted
    }

    /// Turn a reading into the sample the engine joins prints against.
    fn emit(&mut self, deals: u64, taken_ms: i64) -> DealSample {
        if self.last.is_some_and(|last| deals < last) {
            self.stats.regressions += 1;
        }
        self.last = Some(deals);
        self.stats.samples += 1;
        DealSample {
            time_ms: taken_ms.saturating_sub(self.offset_ms),
            session_deals: deals,
        }
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
        tick_in_round(seq, time_ms, deals, None)
    }

    fn tick_in_round(seq: u64, time_ms: i64, deals: Option<u64>, sent_ms: Option<i64>) -> Tick {
        Tick {
            seq,
            time_ms,
            sent_ms,
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
        // Round 1 (sent at 1 500) first carries 2 000 000; the reading is
        // dated at the round's last tick, once round 2 begins.
        assert!(
            sampler
                .observe(&tick_in_round(1, 1_000, Some(2_000_000), Some(1_500)))
                .is_none()
        );
        assert!(
            sampler
                .observe(&tick_in_round(2, 1_005, Some(2_000_000), Some(1_500)))
                .is_none()
        );
        assert!(
            sampler
                .observe(&tick_in_round(3, 1_010, Some(2_000_000), Some(1_500)))
                .is_none()
        );
        let first = sampler.observe(&tick_in_round(4, 1_020, Some(2_000_000), Some(1_520)));
        assert_eq!(
            first,
            Some(DealSample {
                time_ms: 1_010 + 10_800_000,
                session_deals: 2_000_000
            })
        );
        // Round 3 carries a new reading; round 4's first tick dates it.
        assert!(
            sampler
                .observe(&tick_in_round(5, 1_040, Some(2_000_007), Some(1_540)))
                .is_none()
        );
        let next = sampler.observe(&tick_in_round(6, 1_060, Some(2_000_007), Some(1_560)));
        assert_eq!(
            next.map(|s| (s.time_ms, s.session_deals)),
            Some((1_040 + 10_800_000, 2_000_007))
        );
        assert_eq!(sampler.stats.stamped, 6);
        assert_eq!(sampler.stats.samples, 2);
        assert_eq!(sampler.reading(), Some(2_000_007));
    }

    /// An older bridge stamps no round: a reading is dated at its own tick.
    #[test]
    fn a_tick_with_no_round_dates_the_reading_at_itself() {
        let mut sampler = DealSampler::new(0);
        let first = sampler.observe(&tick(1, 1_000, Some(5)));
        assert_eq!(first.map(|s| s.time_ms), Some(1_000));
        assert!(sampler.observe(&tick(2, 1_005, Some(5))).is_none());
        let next = sampler.observe(&tick(3, 1_010, Some(6)));
        assert_eq!(next.map(|s| s.time_ms), Some(1_010));
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
