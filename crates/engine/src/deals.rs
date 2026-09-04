//! Deal bars — close a bar every `N` exchange deals, as the venue counts them.
//!
//! A print is not always one deal. MetaTrader folds every fill an aggressor
//! took at one price into a single tick; measured on B3's mini index on
//! 2026-09-03, 5 821 205 session deals arrived as 1 774 869 ticks. A tick bar
//! over that tape counts ticks, and ProfitChart's *Trades* periodicity counts
//! deals, so the two charts cut in different places. This builder counts what
//! the venue counts.
//!
//! # The join: prints on one side, a counter on the other
//!
//! MetaTrader exposes no deal count per tick, only the session's running
//! total (`SYMBOL_SESSION_DEALS`). The bridge samples it every poll and the
//! feed hands each reading here as a [`DealSample`]. A print is joined to the
//! latest sample **at or before its own timestamp** — the counter as the
//! venue had it when that print was read — and a bar closes on the first
//! print whose reading reaches the next multiple of `N`.
//!
//! Three consequences, all deliberate:
//!
//! - **Bars are the session's multiples of `N`**, never "N deals since the
//!   chart connected". A chart that connects at reading 2 300 411 closes its
//!   first bar at 2 302 000, exactly where a chart that ran since the open
//!   does — the same alignment ProfitChart shows.
//! - **A print before the first sample is uncounted.** It is reported through
//!   [`BarBuilder::uncounted_trades`] and folded into no bar: the venue never
//!   said how many deals it held, and guessing would put a price on a bar
//!   the market did not cut. The data-honesty rule.
//! - **Resolution is one sample.** Every print read in the same poll carries
//!   the same reading, so the boundary lands on the last print of the poll
//!   that crossed it. A bar can therefore overshoot by up to one poll's
//!   worth of deals, and never carries the overshoot into the next bar — the
//!   next boundary is the next multiple above the reading that closed it.
//!
//! # A hole in the readings
//!
//! A print that lies between two readings more than [`READING_MAX_AGE_MS`]
//! apart, and more than that past the earlier one, has no reading of its
//! own: the recorder was down (a restart), or the day's file has a gap.
//! Such prints are uncounted like the ones before the first reading, and
//! the bar forming when the readings stopped is closed as it stands — its
//! deals were counted, the next ones were not, and one bar spanning both
//! would put a cut where nothing was counted. Counting resumes with the
//! reading that ends the hole.
//!
//! Only a hole *between* readings counts as one. A print with no later
//! reading in hand yet — the live edge — is joined to the reading in force
//! however old it is, because a bridge fetches a round of ticks and reads
//! the counter *after* them: one round after a stall can span minutes of
//! tape under one valid reading, and declaring its prints uncounted would
//! throw away a morning. A counter that has really stopped shows as a
//! reading that no longer moves while prints keep coming; bars then wait,
//! and the application says so beside the chart.
//!
//! # Session rollover
//!
//! The counter restarts at the next session. A sample that reads *lower*
//! than the reading in force closes the forming bar — the exchange stopped
//! counting, which is a real end — and starts counting again from the new
//! reading. Samples that merely go backwards in time are ignored: the feed
//! promises monotonic samples and a late one carries no information the
//! builder can place.

use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarProgress, Trade};

/// How far past the newest reading a print may be and still be counted
/// against it. A live bridge samples every 20 ms and a recorded day holds
/// one reading per change, so four seconds without one is the counter
/// having stopped, not a quiet tape.
pub const READING_MAX_AGE_MS: i64 = 4_000;

/// One reading of the venue's session deal counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DealSample {
    /// When the reading was taken, in epoch milliseconds on the tape's own
    /// clock — the same clock the prints carry, or the join means nothing.
    pub time_ms: i64,
    /// Deals the venue had counted for the session at that instant.
    pub session_deals: u64,
}

/// Builds deal bars: one closed [`Bar`] per `N` deals of the venue's counter.
///
/// Feed samples with [`observe_deals`](BarBuilder::observe_deals) and prints
/// with [`push`](BarBuilder::push), each in time order. A sample may arrive
/// before or after the prints it stamps as long as no print older than it is
/// pushed afterwards — the live feed emits the reading ahead of the batch it
/// read, and a rebuild feeds every sample first.
#[derive(Debug, Clone)]
pub struct DealBarBuilder {
    n: u64,
    /// Samples not yet joined to a print, ascending by time.
    pending: VecDeque<DealSample>,
    /// The newest sample accepted, joined or not; the monotonicity guard.
    newest: Option<DealSample>,
    /// The reading in force for the print being pushed, and when it was
    /// taken.
    reading: Option<u64>,
    reading_time_ms: i64,
    /// The counter value that closes the forming bar.
    next_boundary: u64,
    current: Option<Bar>,
    uncounted: u64,
}

impl DealBarBuilder {
    /// Create a builder that closes a bar every `n` deals.
    ///
    /// # Panics
    ///
    /// Panics if `n == 0`: a bar of zero deals is meaningless, and silently
    /// coercing it would violate the data-honesty rule.
    #[must_use]
    pub fn new(n: u64) -> Self {
        assert!(n >= 1, "deal bar size N must be >= 1, got {n}");
        Self {
            n,
            pending: VecDeque::new(),
            newest: None,
            reading: None,
            reading_time_ms: 0,
            next_boundary: 0,
            current: None,
            uncounted: 0,
        }
    }

    /// The configured bar size (deals per bar).
    #[must_use]
    pub fn size(&self) -> u64 {
        self.n
    }

    /// The counter reading joined to the last print pushed, if any print
    /// has been counted yet.
    #[must_use]
    pub fn reading(&self) -> Option<u64> {
        self.reading
    }

    /// When the newest reading in hand was taken, joined to a print or not.
    #[must_use]
    pub fn newest_reading_time_ms(&self) -> Option<i64> {
        self.newest.map(|sample| sample.time_ms)
    }

    /// Whether a print at `time_ms` lies in a hole: the next reading in hand
    /// is more than [`READING_MAX_AGE_MS`] past the one in force, and so is
    /// the print. Never true at the live edge, where no next reading exists.
    fn in_hole(&self, time_ms: i64) -> bool {
        let Some(next) = self.pending.front() else {
            return false;
        };
        next.time_ms.saturating_sub(self.reading_time_ms) > READING_MAX_AGE_MS
            && time_ms.saturating_sub(self.reading_time_ms) > READING_MAX_AGE_MS
    }

    /// The first multiple of `n` strictly above `reading`.
    fn boundary_above(&self, reading: u64) -> u64 {
        (reading / self.n).saturating_add(1).saturating_mul(self.n)
    }

    /// Join every pending sample at or before `time_ms` into the reading.
    /// Returns a closed bar when a rollover ended the forming one.
    fn advance_to(&mut self, time_ms: i64) -> Option<Bar> {
        let mut closed = None;
        while let Some(sample) = self.pending.front().copied() {
            if sample.time_ms > time_ms {
                break;
            }
            self.pending.pop_front();
            self.reading_time_ms = sample.time_ms;
            match self.reading {
                Some(current) if sample.session_deals < current => {
                    // The venue restarted its count: a new session. The bar
                    // forming under the old count ends here.
                    if let Some(bar) = self.current.take() {
                        closed = Some(bar);
                    }
                    self.reading = Some(sample.session_deals);
                    self.next_boundary = self.boundary_above(sample.session_deals);
                }
                Some(_) => self.reading = Some(sample.session_deals),
                None => {
                    self.reading = Some(sample.session_deals);
                    self.next_boundary = self.boundary_above(sample.session_deals);
                }
            }
        }
        closed
    }
}

impl BarBuilder for DealBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        // A rollover can close a bar before this print is placed; that bar
        // is returned and the print opens the next one. At most one bar
        // closes per print either way, because a rollover resets the
        // boundary above the new reading.
        let rolled = self.advance_to(trade.timestamp_ms);
        let Some(reading) = self.reading else {
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled;
        };
        if self.in_hole(trade.timestamp_ms) {
            // No reading covers this print: uncounted, and the bar that was
            // forming under the last reading ends where the readings did.
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled.or_else(|| self.current.take());
        }
        if rolled.is_some() {
            self.current = Some(Bar::opened_by(trade));
            return rolled;
        }
        match self.current.as_mut() {
            Some(bar) => bar.extend(trade),
            None => self.current = Some(Bar::opened_by(trade)),
        }
        if reading >= self.next_boundary {
            self.next_boundary = self.boundary_above(reading);
            return self.current.take();
        }
        None
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    fn progress(&self) -> Option<BarProgress> {
        let reading = self.reading?;
        let floor = self.next_boundary.saturating_sub(self.n);
        Some(BarProgress {
            done: Decimal::from(reading.saturating_sub(floor)),
            target: Decimal::from(self.n),
        })
    }

    fn observe_deals(&mut self, sample: DealSample) {
        if let Some(newest) = self.newest
            && sample.time_ms < newest.time_ms
        {
            return;
        }
        self.newest = Some(sample);
        self.pending.push_back(sample);
    }

    fn uncounted_trades(&self) -> u64 {
        self.uncounted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    use std::str::FromStr as _;

    fn trade(agg_id: u64, ts: i64, price: &str) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: ts,
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    fn sample(time_ms: i64, session_deals: u64) -> DealSample {
        DealSample {
            time_ms,
            session_deals,
        }
    }

    /// The fixture every test below reads: a chart that connects with the
    /// counter at 1 990, deals per bar 2 000. Prints at 50 and 60 predate the
    /// first sample; the poll at 300 crosses 2 000; the poll at 900 jumps
    /// clean over 4 000.
    fn fixture() -> (Vec<DealSample>, Vec<Trade>) {
        let samples = vec![
            sample(100, 1_990),
            sample(300, 2_003),
            sample(500, 2_010),
            sample(900, 4_100),
        ];
        let trades = vec![
            trade(1, 50, "100"),
            trade(2, 60, "101"),
            trade(3, 100, "102"),
            trade(4, 200, "103"),
            trade(5, 300, "104"),
            trade(6, 400, "105"),
            trade(7, 500, "106"),
            trade(8, 900, "107"),
            trade(9, 1_000, "108"),
        ];
        (samples, trades)
    }

    /// Samples first, then prints — the rebuild order.
    fn run(samples: &[DealSample], trades: &[Trade], n: u64) -> (DealBarBuilder, Vec<Bar>) {
        let mut b = DealBarBuilder::new(n);
        for s in samples {
            b.observe_deals(*s);
        }
        let bars = trades.iter().filter_map(|t| b.push(t)).collect();
        (b, bars)
    }

    #[test]
    #[should_panic(expected = "deal bar size N must be >= 1")]
    fn rejects_zero_size() {
        let _ = DealBarBuilder::new(0);
    }

    #[test]
    fn golden_cuts_at_the_sessions_multiples_of_n() {
        let (samples, trades) = fixture();
        let (b, bars) = run(&samples, &trades, 2_000);

        assert_eq!(
            bars.len(),
            2,
            "two boundaries were crossed: 2 000 and 4 000"
        );
        // Bar A: prints at 100, 200, 300 — closed by the reading 2 003.
        assert_eq!((bars[0].open_time, bars[0].close_time), (100, 300));
        assert_eq!(bars[0].trade_count, 3);
        assert_eq!(bars[0].open, Decimal::from(102));
        assert_eq!(bars[0].close, Decimal::from(104));
        // Bar B: prints at 400, 500, 900 — closed by the jump to 4 100, which
        // overshoots 4 000 by one poll and is not carried forward.
        assert_eq!((bars[1].open_time, bars[1].close_time), (400, 900));
        assert_eq!(bars[1].trade_count, 3);
        // The print at 1 000 opens bar C; the next boundary is 6 000.
        let partial = b.partial().expect("bar C is forming");
        assert_eq!((partial.open_time, partial.trade_count), (1_000, 1));
        assert_eq!(b.reading(), Some(4_100));
        let progress = b.progress().expect("a deal bar runs toward a fixed count");
        assert_eq!(
            (progress.done, progress.target),
            (Decimal::from(100), Decimal::from(2_000))
        );
    }

    #[test]
    fn prints_before_the_first_sample_are_uncounted_not_guessed() {
        let (samples, trades) = fixture();
        let (b, bars) = run(&samples, &trades, 2_000);
        assert_eq!(b.uncounted_trades(), 2, "the prints at 50 and 60");
        let counted: u64 = bars.iter().map(|bar| bar.trade_count).sum::<u64>()
            + b.partial().map_or(0, |bar| bar.trade_count);
        assert_eq!(counted + b.uncounted_trades(), trades.len() as u64);
    }

    #[test]
    fn before_any_sample_there_is_no_countdown_to_report() {
        let mut b = DealBarBuilder::new(10);
        assert!(b.progress().is_none());
        assert!(b.push(&trade(1, 5, "100")).is_none());
        assert!(b.partial().is_none(), "an uncounted print forms no bar");
        assert!(b.progress().is_none());
    }

    #[test]
    fn same_input_same_bars() {
        let (samples, trades) = fixture();
        let (_, first) = run(&samples, &trades, 2_000);
        let (_, second) = run(&samples, &trades, 2_000);
        assert_eq!(first, second);
    }

    /// The live order — each poll's reading arrives just ahead of its prints
    /// — cuts exactly where the rebuild order does.
    #[test]
    fn interleaved_samples_cut_where_a_rebuild_cuts() {
        let (samples, trades) = fixture();
        let (_, rebuilt) = run(&samples, &trades, 2_000);

        let mut b = DealBarBuilder::new(2_000);
        let mut live = Vec::new();
        let mut next_sample = 0;
        for t in &trades {
            while next_sample < samples.len() && samples[next_sample].time_ms <= t.timestamp_ms {
                b.observe_deals(samples[next_sample]);
                next_sample += 1;
            }
            live.extend(b.push(t));
        }
        assert_eq!(live, rebuilt);
    }

    #[test]
    fn a_chart_that_connects_late_still_cuts_at_the_sessions_multiples() {
        // Connected with the counter at 2 300 411: the first bar must close
        // at 2 302 000, not 2 000 deals after connecting.
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(10, 2_300_411));
        b.observe_deals(sample(20, 2_301_999));
        b.observe_deals(sample(30, 2_302_000));
        assert!(b.push(&trade(1, 10, "100")).is_none());
        assert!(b.push(&trade(2, 20, "100")).is_none());
        let bar = b
            .push(&trade(3, 30, "100"))
            .expect("closes on the multiple");
        assert_eq!(bar.trade_count, 3);
    }

    /// A restart or a hole in a recorded day: the prints between two readings
    /// far apart are uncounted, the bar that was forming ends where the
    /// readings did, and counting resumes with the reading that ends the
    /// hole. Known only once the later reading is in hand — the rebuild
    /// order.
    #[test]
    fn a_hole_between_readings_leaves_its_prints_uncounted() {
        let mut b = DealBarBuilder::new(1_000);
        let quiet_at = 200 + READING_MAX_AGE_MS + 1;
        b.observe_deals(sample(100, 5_000_400));
        b.observe_deals(sample(20_000, 5_001_005));
        assert!(b.push(&trade(1, 100, "100")).is_none());
        assert!(b.push(&trade(2, 200, "101")).is_none());
        let ended = b
            .push(&trade(3, quiet_at, "102"))
            .expect("the forming bar ends");
        assert_eq!(
            (ended.open_time, ended.close_time, ended.trade_count),
            (100, 200, 2)
        );
        assert!(b.push(&trade(4, quiet_at + 50, "103")).is_none());
        assert!(b.partial().is_none(), "nothing forms inside the hole");
        assert_eq!(b.uncounted_trades(), 2);
        // The reading that ends the hole is past the next boundary: the first
        // counted print opens a bar that closes on it.
        let resumed = b
            .push(&trade(5, 20_000, "104"))
            .expect("closes at 5 001 000");
        assert_eq!(resumed.trade_count, 1);
        assert_eq!(b.uncounted_trades(), 2);
        assert_eq!(b.newest_reading_time_ms(), Some(20_000));
    }

    /// At the live edge there is no later reading to measure a hole against:
    /// a round of ticks fetched after a stall can span minutes under one
    /// valid reading taken after them, and every one of its prints counts.
    #[test]
    fn the_live_edge_counts_every_print_against_the_reading_in_force() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(100, 5_000_400));
        assert!(b.push(&trade(1, 100, "100")).is_none());
        let much_later = 100 + 10 * READING_MAX_AGE_MS;
        assert!(b.push(&trade(2, much_later, "101")).is_none());
        assert_eq!(b.uncounted_trades(), 0);
        assert_eq!(b.partial().map(|bar| bar.trade_count), Some(2));
        // The next reading arrives 40 s later and crosses the boundary: the
        // bar closes on the print that carries it, nothing was thrown away.
        b.observe_deals(sample(much_later + 10, 5_001_002));
        let closed = b
            .push(&trade(3, much_later + 10, "102"))
            .expect("closes at 5 001 000");
        assert_eq!(closed.trade_count, 3);
    }

    /// A rollover that lands inside a hole: the old session's bar ends on the
    /// rollover, and the print itself is uncounted rather than opening a bar
    /// nothing covers.
    #[test]
    fn a_rollover_inside_a_hole_ends_the_bar_and_counts_the_print_as_uncounted() {
        let mut b = DealBarBuilder::new(1_000);
        let rollover_at = 100 + READING_MAX_AGE_MS + 1;
        b.observe_deals(sample(100, 5_000_400));
        b.observe_deals(sample(rollover_at, 3));
        b.observe_deals(sample(rollover_at + 3 * READING_MAX_AGE_MS, 4));
        assert!(b.push(&trade(1, 100, "100")).is_none());
        // Past the rollover sample by more than the limit, inside the hole
        // before the next reading.
        let inside = rollover_at + READING_MAX_AGE_MS + 1;
        let ended = b
            .push(&trade(2, inside, "101"))
            .expect("the old session's bar ends");
        assert_eq!(ended.trade_count, 1);
        assert!(b.partial().is_none(), "the print opened nothing");
        assert_eq!(b.uncounted_trades(), 1);
    }

    #[test]
    fn a_sample_going_back_in_time_is_ignored() {
        let mut b = DealBarBuilder::new(5);
        b.observe_deals(sample(100, 3));
        b.observe_deals(sample(50, 9)); // late and out of order: dropped
        assert!(b.push(&trade(1, 100, "100")).is_none());
        assert_eq!(b.reading(), Some(3));
    }

    #[test]
    fn a_counter_that_restarts_closes_the_forming_bar_and_counts_on() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(100, 5_000_400));
        assert!(b.push(&trade(1, 100, "100")).is_none());
        assert!(b.push(&trade(2, 200, "101")).is_none());
        // Next session: the venue counts from 3 again.
        b.observe_deals(sample(1_000, 3));
        let ended = b
            .push(&trade(3, 1_000, "200"))
            .expect("the old session's bar ends");
        assert_eq!(
            (ended.open_time, ended.close_time, ended.trade_count),
            (100, 200, 2)
        );
        let forming = b
            .partial()
            .expect("the print at 1 000 opens the new session's bar");
        assert_eq!((forming.open_time, forming.trade_count), (1_000, 1));
        assert_eq!(b.reading(), Some(3));
        b.observe_deals(sample(2_000, 1_000));
        assert!(
            b.push(&trade(4, 2_000, "201")).is_some(),
            "closes at the new session's 1 000"
        );
    }

    #[test]
    fn the_default_builders_ignore_samples_and_count_nothing_uncounted() {
        let mut tick = crate::TickBarBuilder::new(2);
        tick.observe_deals(sample(1, 1));
        assert!(tick.push(&trade(1, 1, "100")).is_none());
        assert_eq!(tick.uncounted_trades(), 0);
    }
}
