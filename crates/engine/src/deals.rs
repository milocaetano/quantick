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
//! # Between readings
//!
//! A print is joined to the newest reading at or before it, however far
//! back that reading is. That is the one rule, and it is the same whether
//! the readings arrive just ahead of their prints (the live feed) or all at
//! once before the prints are replayed (a rebuild): the join looks only at
//! what is at or before the print, so the two orders cut identical bars —
//! the property a chart and its rebuilt twin are held to.
//!
//! The price of one rule is that a stretch with no readings — the
//! application was down, the day's file has a hole — folds into the bar
//! the last reading was forming, which closes on the first reading after
//! the stretch. Nothing is cut inside it, nothing is invented; the
//! application knows where its own recording stopped and is the one that
//! can say so. A counter that stands still while prints keep coming is the
//! same shape at the live edge: bars wait, and the application marks it.
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

/// How long the counter may stand still while prints keep coming before a
/// consumer calls it stale. A live bridge samples every 20 ms and a
/// recorded day holds one reading per change, so four seconds of prints
/// under one reading is the counter having stopped, not a quiet tape. The
/// builder itself keeps counting against the reading in force — bars wait
/// rather than close by estimate — and the application says what it sees.
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
    /// The reading in force for the print being pushed.
    reading: Option<u64>,
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

    /// One join rule, whatever the order: readings fed all at once before
    /// the prints (a rebuild) and readings fed just ahead of their prints
    /// (the live feed) cut the same bars — across a stretch with no
    /// readings, and across a round of prints that spans far longer than
    /// the readings' cadence.
    #[test]
    fn a_stretch_without_readings_cuts_the_same_bars_live_and_rebuilt() {
        let samples = vec![
            sample(100, 5_000_400),
            // Nothing for forty seconds, then the counter has moved past the
            // next boundary.
            sample(40_100, 5_001_005),
            sample(40_200, 5_001_009),
        ];
        let trades: Vec<Trade> = [100, 200, 10_000, 30_000, 40_100, 40_150, 40_200]
            .into_iter()
            .enumerate()
            .map(|(i, ts)| trade(i as u64 + 1, ts, "100"))
            .collect();
        let (rebuilt_builder, rebuilt) = run(&samples, &trades, 1_000);

        let mut live = DealBarBuilder::new(1_000);
        let mut cut = Vec::new();
        let mut next = 0;
        for t in &trades {
            while next < samples.len() && samples[next].time_ms <= t.timestamp_ms {
                live.observe_deals(samples[next]);
                next += 1;
            }
            cut.extend(live.push(t));
        }
        assert_eq!(cut, rebuilt);
        assert_eq!(live.uncounted_trades(), rebuilt_builder.uncounted_trades());
        // The stretch folded into the bar the first reading was forming,
        // which closed on the reading that ended it: nothing was cut inside
        // the stretch, and nothing was thrown away.
        assert_eq!(rebuilt.len(), 1);
        assert_eq!((rebuilt[0].open_time, rebuilt[0].close_time), (100, 40_100));
        assert_eq!(rebuilt[0].trade_count, 5);
        assert_eq!(rebuilt_builder.uncounted_trades(), 0);
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
