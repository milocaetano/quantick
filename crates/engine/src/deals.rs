//! Deal bars — close a bar every `N` exchange deals, as the venue counts them.
//!
//! A print is not always one deal. MetaTrader folds every fill an aggressor
//! took at one price into a single tick; measured on B3's mini index on
//! 2026-09-03, 5 821 205 session deals arrived as 1 774 869 ticks. A tick bar
//! over that tape counts ticks, and ProfitChart's *Trades* periodicity counts
//! deals, so the two charts cut in different places. This builder counts what
//! the venue counts.
//!
//! # The counter, and what it resolves
//!
//! MetaTrader exposes no deal count per tick, only the session's running
//! total (`SYMBOL_SESSION_DEALS`) — and the terminal refreshes that total
//! about every 31 seconds (measured over a whole B3 session on 2026-09-04:
//! 592 readings, median interval 31.2 s, 1 500 to 10 000 deals apart). The
//! bridge forwards each new reading as a [`DealSample`] on the tape's own
//! clock. Between two readings the venue's count is unknown print by print;
//! what is known exactly is the total at each reading.
//!
//! # The estimate
//!
//! Each print between two readings is credited an **estimated** number of
//! deals: its contracts times the *rate* — deals per contract — of the last
//! completed reading window. The running total is re-anchored to the exact
//! reading every time one arrives, so the day's total, and with it the
//! number of bars, is the venue's; only where inside a window each bar
//! closes is an estimate, off by the difference between two consecutive
//! windows' rates. A bar closes on the first print whose estimated total
//! reaches the next multiple of `N` — the session's multiples, so a chart
//! that connects at reading 2 300 411 closes its first bar at 2 302 000, as
//! one that ran since the open does. A reading that reaches a multiple the
//! estimate had not closes the forming bar on the next print; a multiple the
//! estimate closed early is not closed twice.
//!
//! The estimate uses only what came *before* the print — the last completed
//! window's rate and the newest reading strictly before it — so readings fed
//! just ahead of their prints (the live feed) and readings fed all at once
//! before the prints (a rebuild) cut identical bars: the property a chart
//! and its rebuilt twin are held to, and what lets a change of `N` recut the
//! whole day from the recorded readings.
//!
//! Prints before the first reading, or before the first completed window
//! (no rate yet), or further behind the newest reading than
//! [`READING_MAX_AGE_MS`] — last night's reading under this morning's
//! prints — are **uncounted**: reported through
//! [`BarBuilder::uncounted_trades`], folded into no bar. The venue never said
//! how many deals they held, and guessing would put a price on a bar the
//! market did not cut.
//!
//! # Session rollover
//!
//! A reading lower than the one in force by more than a bar's worth of deals
//! closes the forming bar — the exchange restarted its count, which is a
//! real end — and counts on from the new reading. A smaller dip is the
//! terminal answering a poll late and changes nothing. Samples that merely
//! go backwards in time are ignored: the feed promises monotonic samples and
//! a late one carries no information the builder can place.

use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarProgress, Trade};

/// How long a reading holds for the prints after it. The terminal refreshes
/// the counter about every 31 seconds, so ten minutes of prints under one
/// reading is the counter having stopped, or the application having been
/// away — last night's reading under this morning's prints — not a slow
/// terminal. A print further behind its reading than this is uncounted
/// rather than credited a stale rate.
pub const READING_MAX_AGE_MS: i64 = 600_000;

/// One reading of the venue's session deal counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DealSample {
    /// When the reading was taken, in epoch milliseconds on the tape's own
    /// clock — the same clock the prints carry, or the join means nothing.
    pub time_ms: i64,
    /// Deals the venue had counted for the session at that instant.
    pub session_deals: u64,
}

/// Builds deal bars: one closed [`Bar`] per `N` deals of the venue's counter,
/// the deals between readings estimated per print. See the module doc.
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
    /// The newest reading in force: the venue's exact total, and when it was
    /// taken on the tape's clock.
    reading: Option<DealSample>,
    /// Deals per contract over the last completed window between two
    /// readings; none until a window has completed.
    rate: Option<Decimal>,
    /// Contracts printed since the reading in force — the divisor of the
    /// next rate.
    window_volume: Decimal,
    /// The running total: the reading in force plus the deals estimated for
    /// the prints since it. Re-anchored to every reading.
    total: Decimal,
    /// The counter value that closes the forming bar.
    next_boundary: u64,
    /// Whether a print was credited deals since the reading in force —
    /// the difference between "nothing was forming" and "a bar just closed"
    /// when a reading reaches the boundary.
    window_counted: bool,
    /// Whether a print since the reading in force fell beyond what a
    /// reading holds for: its deals are in the next delta, its contracts
    /// are not in the divisor, so that window sets no rate.
    window_had_uncounted: bool,
    /// The bar opened by a rollover print whose own estimate already
    /// reached the multiple closes on the next push, before that print.
    close_next: bool,
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
            rate: None,
            window_volume: Decimal::ZERO,
            total: Decimal::ZERO,
            next_boundary: 0,
            window_counted: false,
            window_had_uncounted: false,
            close_next: false,
            current: None,
            uncounted: 0,
        }
    }

    /// The configured bar size (deals per bar).
    #[must_use]
    pub fn size(&self) -> u64 {
        self.n
    }

    /// The venue's newest reading joined to the prints, if any print has
    /// been counted yet.
    #[must_use]
    pub fn reading(&self) -> Option<u64> {
        self.reading.map(|sample| sample.session_deals)
    }

    /// Deals per contract over the last completed window, once one has.
    #[must_use]
    pub fn rate(&self) -> Option<Decimal> {
        self.rate
    }

    /// The first multiple of `n` strictly above `reading`.
    fn boundary_above(&self, reading: u64) -> u64 {
        (reading / self.n).saturating_add(1).saturating_mul(self.n)
    }

    /// Join every pending sample strictly before `time_ms` into the
    /// reading. Returns a closed bar when a rollover ended the forming one.
    /// Join every pending sample strictly before `time_ms` into the
    /// reading. Returns a closed bar when a rollover ended the forming one.
    fn advance_to(&mut self, time_ms: i64) -> Option<Bar> {
        let mut closed = None;
        while let Some(sample) = self.pending.front().copied() {
            if sample.time_ms >= time_ms {
                break;
            }
            self.pending.pop_front();
            let deals = sample.session_deals;
            match self.reading.map(|r| r.session_deals) {
                None => {
                    self.next_boundary = self.boundary_above(deals);
                }
                // Lower by more than a bar's worth of deals: the venue
                // restarted its count, a new session, and the bar forming
                // under the old count ends here. The rate carries over — a
                // new session's deals per contract are no different.
                Some(current) if deals < current.saturating_sub(self.n) => {
                    if let Some(bar) = self.current.take() {
                        closed = Some(bar);
                    }
                    self.close_next = false;
                    self.next_boundary = self.boundary_above(deals);
                }
                // A smaller dip is the terminal answering a poll late —
                // polls are milliseconds apart, a bar is minutes — and an
                // unchanged reading is a reconnect re-emitting what it
                // found. Neither is a window: nothing changes.
                Some(current) if deals <= current => continue,
                Some(current) => {
                    // A completed window: its exact deals over its contracts
                    // is the rate the next window's prints are estimated at
                    // — unless a print of the window fell beyond what a
                    // reading holds for, since its deals are in the delta
                    // and its contracts are not, and the rate would be
                    // inflated; that window keeps the rate it had. A window
                    // with prints of no volume — a quoted tape — too.
                    if self.window_volume > Decimal::ZERO && !self.window_had_uncounted {
                        self.rate = Some(Decimal::from(deals - current) / self.window_volume);
                    }
                    // A multiple crossed while no print was credited — the
                    // window's prints uncounted, or none — has no bar to
                    // close: the count moves on. Otherwise the boundary
                    // stands: a multiple the estimate missed is below the
                    // re-anchored total and closes on the next print; one it
                    // closed early is above it and is not cut twice — the
                    // next window fills up to it.
                    if !self.window_counted && deals >= self.next_boundary {
                        self.next_boundary = self.boundary_above(deals);
                    }
                }
            }
            self.reading = Some(sample);
            self.total = Decimal::from(deals);
            self.window_volume = Decimal::ZERO;
            self.window_counted = false;
            self.window_had_uncounted = false;
        }
        closed
    }

    /// Whether the running total has reached the boundary — the estimate
    /// crossed a multiple, or a reading did.
    fn crossed_boundary(&self) -> bool {
        self.total >= Decimal::from(self.next_boundary)
    }

    /// The running total as a count, for the next multiple above it.
    fn total_floor(&self) -> u64 {
        self.total
            .max(Decimal::ZERO)
            .trunc()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

impl BarBuilder for DealBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        // A rollover can close a bar before this print is placed; that bar
        // is returned and the print opens the next one. So can a bar a
        // rollover print opened whose own estimate reached the multiple. At
        // most one bar closes per print either way.
        let mut rolled = self.advance_to(trade.timestamp_ms);
        if rolled.is_none() && self.close_next {
            self.close_next = false;
            rolled = self.current.take();
        }
        let Some(reading) = self.reading else {
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled;
        };
        // The counter's silence: a print further behind the reading in
        // force than a reading holds has no count, and says so, rather than
        // being credited a rate from another day.
        if trade.timestamp_ms.saturating_sub(reading.time_ms) > READING_MAX_AGE_MS {
            self.uncounted = self.uncounted.saturating_add(1);
            self.window_had_uncounted = true;
            return rolled;
        }
        self.window_volume += trade.quantity;
        let Some(rate) = self.rate else {
            // No window has completed yet: nothing says how many deals a
            // contract is worth on this tape. Counted from the next reading.
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled;
        };
        self.total += trade.quantity * rate;
        self.window_counted = true;
        if rolled.is_some() {
            self.current = Some(Bar::opened_by(trade));
            if self.crossed_boundary() {
                // This print's own estimate reached the multiple: its bar
                // closes on the next push, before the print after it.
                self.next_boundary = self.boundary_above(self.total_floor());
                self.close_next = true;
            }
            return rolled;
        }
        match self.current.as_mut() {
            Some(bar) => bar.extend(trade),
            None => self.current = Some(Bar::opened_by(trade)),
        }
        if self.crossed_boundary() {
            // Overshoot — an estimate past the multiple, a reading past it —
            // is not carried into the next bar: the next boundary is the
            // next multiple above where the total stands.
            self.next_boundary = self.boundary_above(self.total_floor());
            return self.current.take();
        }
        None
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    fn progress(&self) -> Option<BarProgress> {
        self.reading?;
        let floor = self.next_boundary.saturating_sub(self.n);
        Some(BarProgress {
            done: (self.total - Decimal::from(floor)).max(Decimal::ZERO),
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
        trade_of(agg_id, ts, price, "1")
    }

    fn trade_of(agg_id: u64, ts: i64, price: &str, quantity: &str) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: ts,
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str(quantity).unwrap(),
            side: Side::Buy,
        }
    }

    fn sample(time_ms: i64, session_deals: u64) -> DealSample {
        DealSample {
            time_ms,
            session_deals,
        }
    }

    /// The fixture every test below reads: readings at the terminal's
    /// cadence, prints of ten contracts between them, deals per bar 2 000.
    ///
    /// - Prints at 50 and 60 predate the first reading: uncounted.
    /// - Window 1 (reading 1 000 000 at 99, prints at 100..=400): no rate
    ///   yet, so its four prints are uncounted too; it completes at the
    ///   reading 1 003 000 at 30 099 — 3 000 deals over 40 contracts, a
    ///   rate of 75 deals per contract.
    /// - Window 2 (prints at 30 100..=30 400): each print is worth 750
    ///   estimated deals; the total reaches 1 004 000 on the second print
    ///   (bar A) and 1 006 000 on the fourth (bar B), where the reading
    ///   1 006 000 at 60 099 then re-anchors it exactly.
    /// - Window 3 (prints at 60 100, 60 200): the rate is still 75, the
    ///   total stands at 1 007 500 with a bar forming.
    fn fixture() -> (Vec<DealSample>, Vec<Trade>) {
        let samples = vec![
            sample(99, 1_000_000),
            sample(30_099, 1_003_000),
            sample(60_099, 1_006_000),
        ];
        let mut trades = vec![trade_of(1, 50, "100", "10"), trade_of(2, 60, "101", "10")];
        let mut id = 3;
        for window in [100, 30_100, 60_100] {
            for k in 0..4 {
                if window == 60_100 && k >= 2 {
                    break;
                }
                trades.push(trade_of(
                    id,
                    window + k * 100,
                    &(100 + id).to_string(),
                    "10",
                ));
                id += 1;
            }
        }
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

    /// Each reading just ahead of the prints it precedes — the live order.
    fn run_live(samples: &[DealSample], trades: &[Trade], n: u64) -> (DealBarBuilder, Vec<Bar>) {
        let mut b = DealBarBuilder::new(n);
        let mut bars = Vec::new();
        let mut next = 0;
        for t in trades {
            while next < samples.len() && samples[next].time_ms <= t.timestamp_ms {
                b.observe_deals(samples[next]);
                next += 1;
            }
            bars.extend(b.push(t));
        }
        while next < samples.len() {
            b.observe_deals(samples[next]);
            next += 1;
        }
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
            "two multiples crossed: 1 004 000 and 1 006 000"
        );
        // Bar A: the first two prints of window 2 — 1 500 estimated deals on
        // top of 1 003 000 reach 1 004 000 on the second.
        assert_eq!((bars[0].open_time, bars[0].close_time), (30_100, 30_200));
        assert_eq!(bars[0].trade_count, 2);
        assert_eq!(bars[0].open, Decimal::from(107));
        assert_eq!(bars[0].close, Decimal::from(108));
        // Bar B: the next two, reaching 1 006 000 on the fourth.
        assert_eq!((bars[1].open_time, bars[1].close_time), (30_300, 30_400));
        assert_eq!(bars[1].trade_count, 2);
        // Window 3 is forming: two prints, 1 500 estimated deals past the
        // re-anchored 1 006 000, toward 1 008 000.
        let partial = b.partial().expect("a bar is forming");
        assert_eq!((partial.open_time, partial.trade_count), (60_100, 2));
        assert_eq!(b.reading(), Some(1_006_000));
        assert_eq!(b.rate(), Some(Decimal::from(75)));
        let progress = b.progress().expect("a deal bar runs toward a fixed count");
        assert_eq!(
            (progress.done, progress.target),
            (Decimal::from(1_500), Decimal::from(2_000))
        );
    }

    #[test]
    fn prints_before_the_first_completed_window_are_uncounted_not_guessed() {
        let (samples, trades) = fixture();
        let (b, bars) = run(&samples, &trades, 2_000);
        assert_eq!(
            b.uncounted_trades(),
            6,
            "the two before the first reading, and window 1's four"
        );
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

    /// The live order — each reading arrives just ahead of its prints —
    /// cuts exactly where the rebuild order does, and leaves the same
    /// prints uncounted.
    #[test]
    fn interleaved_samples_cut_where_a_rebuild_cuts() {
        let (samples, trades) = fixture();
        let (rebuilt_builder, rebuilt) = run(&samples, &trades, 2_000);
        let (live_builder, live) = run_live(&samples, &trades, 2_000);
        assert_eq!(live, rebuilt);
        assert_eq!(
            live_builder.uncounted_trades(),
            rebuilt_builder.uncounted_trades()
        );
        assert_eq!(live_builder.partial(), rebuilt_builder.partial());
    }

    /// Another `N` over the same readings and prints recuts the day: what a
    /// change of the bar size on the chart does, from the recorded readings.
    #[test]
    fn another_n_recuts_the_same_day() {
        let (samples, trades) = fixture();
        let (_, at_2000) = run(&samples, &trades, 2_000);
        let (_, at_1000) = run(&samples, &trades, 1_000);
        assert_eq!(at_2000.len(), 2);
        assert_eq!(at_1000.len(), 4, "twice the multiples, twice the bars");
        let counts: Vec<u64> = at_1000.iter().map(|bar| bar.trade_count).collect();
        assert_eq!(counts, [2, 1, 1, 2]);
    }

    /// Connected with the counter at 2 300 411: the first bar must close at
    /// 2 302 000, not 2 000 deals after connecting.
    #[test]
    fn a_chart_that_connects_late_still_cuts_at_the_sessions_multiples() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(9, 2_300_411));
        // Window 1: one print of ten contracts, no rate yet — uncounted.
        assert!(b.push(&trade_of(1, 10, "100", "10")).is_none());
        // Its reading: 1 000 deals over 10 contracts, a rate of 100.
        b.observe_deals(sample(19, 2_301_411));
        // Six contracts: 600 estimated, total 2 302 011 — past the multiple.
        let bar = b
            .push(&trade_of(2, 20, "100", "6"))
            .expect("closes on the session's multiple");
        assert_eq!(bar.trade_count, 1);
        assert_eq!(b.progress().map(|p| p.done), Some(Decimal::from(11)));
    }

    /// A reading that reaches a multiple the estimate had not closes the
    /// forming bar on the next print; one the estimate closed early is not
    /// closed twice — the day's bar count is the venue's total over `N`.
    #[test]
    fn a_reading_corrects_the_estimate_without_cutting_twice() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 10_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 10_500)); // rate 50 per contract
        // Window 2 estimates 5 contracts at 250: total 10 750, no bar.
        assert!(b.push(&trade_of(2, 30_100, "100", "5")).is_none());
        // The reading says 11 200: the multiple 11 000 was crossed. The next
        // print closes the bar, and the total re-anchors.
        b.observe_deals(sample(60_099, 11_200));
        let late = b
            .push(&trade_of(3, 60_100, "100", "1"))
            .expect("the missed multiple closes on the next print");
        assert_eq!(late.trade_count, 2);
        // Window 3's rate is 700 over 5 contracts = 140: the print that
        // closed 11 000 counted 140 (total 11 340), two contracts are 280
        // (11 620) and three more are 420 — the estimate closes 12 000 on
        // the third print, early.
        b.push(&trade_of(4, 60_200, "100", "2"));
        let early = b
            .push(&trade_of(5, 60_300, "100", "3"))
            .expect("the estimate reaches 12 000");
        assert_eq!(early.trade_count, 2);
        // The reading says 11 900: 12 000 was not crossed after all. The bar
        // that closed stays closed and 12 000 is not cut again — the next
        // window fills toward 13 000 from the re-anchored 11 900.
        b.observe_deals(sample(90_099, 11_900));
        assert!(b.push(&trade_of(6, 90_100, "100", "1")).is_none());
        b.observe_deals(sample(120_099, 12_050));
        assert!(
            b.push(&trade_of(7, 120_100, "100", "1")).is_none(),
            "12 000 was cut already; the reading past it cuts nothing"
        );
        assert!(b.partial().is_some_and(|bar| bar.trade_count == 2));
        assert_eq!(b.progress().map(|p| p.target), Some(Decimal::from(1_000)));
    }

    /// A stretch with no readings — quantick was down, the day's file has a
    /// hole — is uncounted once it runs past what a reading holds for, in
    /// both orders; cutting resumes at the next completed window.
    #[test]
    fn a_stretch_without_readings_cuts_the_same_bars_live_and_rebuilt() {
        let samples = vec![
            sample(99, 5_000_400),
            sample(30_099, 5_001_400),
            // Nothing for twenty minutes, then the counter is back.
            sample(1_230_099, 5_050_000),
            sample(1_260_099, 5_050_100),
        ];
        let trades: Vec<Trade> = [
            100, 200, 30_100, 30_200, 700_000, 900_000, 1_230_100, 1_260_100,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, ts)| trade_of(i as u64 + 1, ts, "100", "10"))
        .collect();
        let (rebuilt_builder, rebuilt) = run(&samples, &trades, 1_000);
        let (live_builder, live) = run_live(&samples, &trades, 1_000);
        assert_eq!(live, rebuilt);
        assert_eq!(
            live_builder.uncounted_trades(),
            rebuilt_builder.uncounted_trades()
        );
        // Window 1's two prints have no rate; window 2's two are counted at
        // 50 per contract (500 each, closing 5 002 000 on the second); the
        // prints at 700 000 and 900 000 are beyond what the reading at
        // 30 099 holds for; the print at 1 230 100 is credited the rate the
        // stretch's window gives and closes a bar of its own.
        assert_eq!(rebuilt_builder.uncounted_trades(), 4);
        assert_eq!(rebuilt.len(), 2, "{rebuilt:?}");
        assert_eq!(
            (rebuilt[0].open_time, rebuilt[0].close_time),
            (30_100, 30_200)
        );
    }

    /// Yesterday's last reading under today's first prints — a chart left
    /// open overnight, or a reload that kept its readings — counts none of
    /// them, and the session's first reading starts the day clean instead
    /// of closing the whole morning as one bar.
    #[test]
    fn a_reading_from_last_night_counts_no_print_this_morning() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(99, 5_000_000));
        b.push(&trade(1, 100, "100"));
        b.observe_deals(sample(30_099, 5_000_500)); // rate 500 per contract
        assert!(b.push(&trade(2, 30_100, "100")).is_none());
        let morning = 100 + 14 * 3_600_000;
        assert!(b.push(&trade(3, morning, "101")).is_none());
        assert!(b.push(&trade(4, morning + 100, "102")).is_none());
        assert_eq!(
            b.uncounted_trades(),
            3,
            "window 1's print, and the two of this morning"
        );
        // The session's first reading is lower: a rollover. The bar it ends
        // holds last night's one counted print, nothing of this morning.
        b.observe_deals(sample(morning + 199, 120_000));
        let ended = b
            .push(&trade(5, morning + 200, "103"))
            .expect("last night's bar ends");
        assert_eq!(
            (ended.open_time, ended.close_time, ended.trade_count),
            (30_100, 30_100, 1)
        );
    }

    #[test]
    fn a_sample_going_back_in_time_is_ignored() {
        let mut b = DealBarBuilder::new(5);
        b.observe_deals(sample(99, 3));
        b.observe_deals(sample(50, 9)); // late and out of order: dropped
        assert!(b.push(&trade(1, 100, "100")).is_none());
        assert_eq!(b.reading(), Some(3));
    }

    /// A reconnect re-emits the reading it finds: an unchanged reading at
    /// a later time is not a window, and sets no rate of zero.
    #[test]
    fn a_repeated_reading_is_not_a_window() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 100));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 200)); // rate 10
        assert_eq!(b.rate(), None, "a rate is known at the next print");
        b.push(&trade_of(2, 30_100, "100", "1"));
        assert_eq!(b.rate(), Some(Decimal::from(10)));
        b.observe_deals(sample(45_099, 200)); // the same reading, re-emitted
        b.push(&trade_of(3, 45_100, "100", "1"));
        assert_eq!(b.rate(), Some(Decimal::from(10)), "unchanged: not a window");
        assert_eq!(b.reading(), Some(200));
    }

    /// A window with a print beyond what a reading holds for sets no rate:
    /// that print's deals are in the delta and its contracts are not, and a
    /// rate from them would be inflated. The window keeps the rate it had.
    #[test]
    fn an_uncounted_stretch_does_not_inflate_the_rate() {
        let mut b = DealBarBuilder::new(1_000_000);
        b.observe_deals(sample(99, 1_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 2_000)); // rate 100
        b.push(&trade_of(2, 30_100, "100", "1"));
        assert_eq!(b.rate(), Some(Decimal::from(100)));
        // Twelve minutes of silence, then a print the reading no longer
        // holds for, then a reading whose delta covers the silence.
        b.push(&trade_of(3, 750_000, "100", "50"));
        assert_eq!(b.uncounted_trades(), 2, "the first print, and this one");
        b.observe_deals(sample(760_099, 60_000));
        b.push(&trade_of(4, 760_100, "100", "1"));
        assert_eq!(
            b.rate(),
            Some(Decimal::from(100)),
            "not 58 000 per contract"
        );
    }

    /// A reading that reaches the next multiple right after a print closed
    /// a bar still gives that multiple its bar, on the next print: "nothing
    /// forming" means no print was credited, not a bar just closed.
    #[test]
    fn a_multiple_reached_right_after_a_close_gets_its_bar() {
        let (samples, mut trades) = fixture();
        // Keep window 2's first two prints only — bar A closes on the second,
        // the boundary moves to 1 006 000 — and put a reading past 1 006 000
        // right after that close, with nothing forming.
        trades.truncate(8);
        let mut samples = samples;
        samples[2] = sample(60_099, 1_006_200);
        let (mut b, bars) = run(&samples, &trades, 2_000);
        assert_eq!(bars.len(), 1);
        let bar = b
            .push(&trade_of(20, 60_100, "100", "10"))
            .expect("1 006 000, reached by the reading, gets its bar");
        assert_eq!(bar.trade_count, 1);
    }

    /// The print that opens the bar after a rollover can itself reach the
    /// new session's first multiple: its bar closes on the next push, before
    /// the print after it, never one print late.
    #[test]
    fn the_first_print_after_a_rollover_can_close_its_own_bar() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(99, 1_000_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 1_000_750)); // rate 75
        b.push(&trade_of(2, 30_100, "100", "1"));
        // The restart, at 1 950: ten contracts are 750, past 2 000.
        b.observe_deals(sample(60_099, 1_950));
        let ended = b
            .push(&trade_of(3, 60_100, "100", "10"))
            .expect("the restart ends the old bar");
        assert_eq!(
            ended.trade_count, 1,
            "the print credited a rate; the first had none"
        );
        let own = b
            .push(&trade_of(4, 60_200, "100", "1"))
            .expect("the rollover print's own bar closes next");
        assert_eq!((own.open_time, own.trade_count), (60_100, 1));
        assert_eq!(
            b.partial().map(|bar| (bar.open_time, bar.trade_count)),
            Some((60_200, 1))
        );
    }

    /// A reading a little lower than the one in force is the terminal
    /// answering a poll late, not a session restart: no bar ends, none is
    /// cut. A drop of more than a bar's worth of deals is the restart.
    #[test]
    fn a_small_dip_is_a_late_poll_and_a_large_one_a_restart() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 5_000_400));
        b.push(&trade(1, 100, "100"));
        b.observe_deals(sample(30_099, 5_000_500)); // rate 100
        assert!(b.push(&trade(2, 30_100, "101")).is_none());
        b.observe_deals(sample(31_099, 5_000_450)); // a dip: ignored
        assert!(
            b.push(&trade(3, 31_100, "102")).is_none(),
            "a dip ends nothing"
        );
        assert_eq!(b.reading(), Some(5_000_500));
        assert_eq!(b.partial().map(|bar| bar.trade_count), Some(2));
        b.observe_deals(sample(60_099, 3)); // the restart
        let ended = b
            .push(&trade(4, 60_100, "103"))
            .expect("the restart ends the bar");
        assert_eq!(ended.trade_count, 2);
        assert_eq!(b.reading(), Some(3));
        assert_eq!(b.rate(), Some(Decimal::from(100)), "the rate carries over");
    }

    #[test]
    fn the_default_builders_ignore_samples_and_count_nothing_uncounted() {
        let mut tick = crate::TickBarBuilder::new(2);
        tick.observe_deals(sample(1, 1));
        assert!(tick.push(&trade(1, 1, "100")).is_none());
        assert_eq!(tick.uncounted_trades(), 0);
        assert!(
            tick.push(&trade(2, 2, "100")).is_some(),
            "a tick bar counts ticks"
        );
    }
}
