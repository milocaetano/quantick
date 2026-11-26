//! Deal bars: close a bar every `N` exchange deals, estimating per-print deals between venue readings.
//! Bars close on session multiples of `N`; prints with no usable reading or rate are uncounted, never guessed.
//! A reading falling by over half is a session rollover that closes the forming bar.

use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarBuilderDiagnostics, BarProgress, DealCounterInput, Trade};

/// How long a reading holds for the prints after it. Ten minutes under one reading means the
/// counter stopped or the app was away, so a print further behind is uncounted, not credited a
/// stale rate.
pub const READING_MAX_AGE_MS: i64 = 600_000;

/// One reading of the venue's session deal counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DealSample {
    /// When the reading was taken, epoch ms on the tape's own clock (the prints' clock).
    pub time_ms: i64,
    /// Deals the venue had counted for the session at that instant.
    pub session_deals: u64,
}

/// Builds deal bars: one closed [`Bar`] per `N` venue deals (module doc).
/// Feed samples and prints in time order; a sample may precede or follow the prints it stamps.
#[derive(Debug, Clone)]
pub struct DealBarBuilder {
    n: u64,
    /// Samples not yet joined to a print, ascending by time.
    pending: VecDeque<DealSample>,
    /// The newest sample accepted, joined or not; the monotonicity guard.
    newest: Option<DealSample>,
    /// The newest reading in force: the venue's exact total and when it was taken.
    reading: Option<DealSample>,
    /// Deals per contract over the last completed window; none until one completes.
    rate: Option<Decimal>,
    /// Contracts printed since the reading in force: the divisor of the next rate.
    window_volume: Decimal,
    /// The running total: the reading in force plus deals estimated since. Re-anchored per reading.
    total: Decimal,
    /// The counter value that closes the forming bar.
    next_boundary: u64,
    /// The running total when the forming bar opened; progress counts from here.
    bar_opened_at: Decimal,
    /// Whether a print was credited deals since the reading in force (a bar was forming).
    window_counted: bool,
    /// Whether a print since the reading fell beyond its max age: its deals are in the next
    /// delta but its contracts are not in the divisor, so that window sets no rate.
    window_had_uncounted: bool,
    /// A rollover print's bar whose estimate already reached the multiple closes on the next push.
    close_next: bool,
    /// After the forming bar closes, skip multiples an empty or uncounted stretch crossed.
    skip_catch_up: bool,
    current: Option<Bar>,
    uncounted: u64,
}

impl DealBarBuilder {
    /// Create a builder that closes a bar every `n` deals. Panics if `n == 0`.
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
            bar_opened_at: Decimal::ZERO,
            window_counted: false,
            window_had_uncounted: false,
            close_next: false,
            skip_catch_up: false,
            current: None,
            uncounted: 0,
        }
    }

    /// The configured bar size (deals per bar).
    #[must_use]
    pub fn size(&self) -> u64 {
        self.n
    }

    /// The venue's newest reading joined to the prints, once any print was counted.
    #[must_use]
    pub fn reading(&self) -> Option<u64> {
        self.reading.map(|sample| sample.session_deals)
    }

    /// Deals per contract over the last completed window, once one has.
    #[must_use]
    pub fn rate(&self) -> Option<Decimal> {
        self.rate
    }

    /// Hand this builder one venue reading; type-erased consumers use
    /// [`BarBuilder::deal_counter_input`].
    pub fn observe_deals(&mut self, sample: DealSample) {
        DealCounterInput::observe(self, sample);
    }

    #[must_use]
    pub fn uncounted_trades(&self) -> u64 {
        self.uncounted
    }

    /// The first multiple of `n` strictly above `reading`.
    fn boundary_above(&self, reading: u64) -> u64 {
        (reading / self.n).saturating_add(1).saturating_mul(self.n)
    }

    /// Join every pending sample strictly before `time_ms` into the reading. Returns a closed bar
    /// when a rollover ended the forming one.
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
                // Lower by more than half: the venue restarted its count, so the bar forming under the old
                // count ends here. A smaller dip is a late poll answer, not a restart. The rate carries over.
                Some(current) if deals.saturating_mul(2) < current => {
                    if let Some(bar) = self.current.take() {
                        closed = Some(bar);
                    }
                    self.close_next = false;
                    self.skip_catch_up = false;
                    self.next_boundary = self.boundary_above(deals);
                }
                // A smaller dip is a late poll answer, not a window: nothing changes.
                Some(current) if deals < current => continue,
                // An unchanged reading is a reconnect re-emitting: not a window, but the age the later prints are
                // judged by restarts here, or a long halt would leave every print uncounted.
                Some(current) if deals == current => {
                    self.reading = Some(sample);
                    continue;
                }
                Some(current) => {
                    // A completed window: its exact deals over its contracts is the next rate, unless a print of it
                    // fell beyond a reading's age (deals in the delta, contracts not: inflated) or had no volume.
                    if self.window_volume > Decimal::ZERO && !self.window_had_uncounted {
                        self.rate = Some(Decimal::from(deals - current) / self.window_volume);
                    }
                    // A multiple crossed with no print credited has no bar to close: the count moves on. Otherwise
                    // the boundary stands: a missed multiple closes on the next print, an early-closed one is not
                    // cut twice.
                    let counted_window = self.window_counted && !self.window_had_uncounted;
                    if !counted_window && deals >= self.next_boundary {
                        if self.current.is_none() {
                            self.next_boundary = self.boundary_above(deals);
                        } else {
                            // A bar forming from before the empty stretch closes at its multiple on the next print; the
                            // count then moves on rather than catching every multiple up.
                            self.skip_catch_up = true;
                        }
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

    /// Whether the running total has reached the boundary (estimate or reading crossed a multiple).
    fn crossed_boundary(&self) -> bool {
        self.total >= Decimal::from(self.next_boundary)
    }

    /// The boundary after a close is always the next multiple, or the bar count drifts below the
    /// venue's. A total two or more multiples ahead gives each multiple a bar, one per print.
    fn boundary_after_close(&mut self) -> u64 {
        if self.skip_catch_up {
            self.skip_catch_up = false;
            let floor: u64 = self
                .total
                .max(Decimal::ZERO)
                .trunc()
                .try_into()
                .unwrap_or(u64::MAX);
            return self.boundary_above(floor);
        }
        self.next_boundary.saturating_add(self.n)
    }
}

impl BarBuilder for DealBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        // A rollover, or a rollover print's bar that reached its multiple, can close a bar before this
        // print is placed; the print then opens the next. At most one bar closes per print.
        let mut rolled = self.advance_to(trade.timestamp_ms);
        if rolled.is_none() && self.close_next {
            self.close_next = false;
            rolled = self.current.take();
        }
        let Some(reading) = self.reading else {
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled;
        };
        // Counter silence: a print too far behind the reading has no count, and says so.
        if trade.timestamp_ms.saturating_sub(reading.time_ms) > READING_MAX_AGE_MS {
            self.uncounted = self.uncounted.saturating_add(1);
            self.window_had_uncounted = true;
            return rolled;
        }
        self.window_volume += trade.quantity;
        let Some(rate) = self.rate else {
            // No window has completed yet, so no rate: counted from the next reading.
            self.uncounted = self.uncounted.saturating_add(1);
            return rolled;
        };
        self.total += trade.quantity * rate;
        self.window_counted = true;
        if rolled.is_some() {
            self.current = Some(Bar::opened_by(trade));
            self.bar_opened_at = self.total - trade.quantity * rate;
            if self.crossed_boundary() {
                // This print's estimate reached the multiple: its bar closes on the next push.
                self.next_boundary = self.boundary_after_close();
                self.close_next = true;
            }
            return rolled;
        }
        match self.current.as_mut() {
            Some(bar) => bar.extend(trade),
            None => {
                self.current = Some(Bar::opened_by(trade));
                self.bar_opened_at = self.total - trade.quantity * rate;
            }
        }
        if self.crossed_boundary() {
            self.next_boundary = self.boundary_after_close();
            return self.current.take();
        }
        None
    }

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    fn progress(&self) -> Option<BarProgress> {
        self.reading?;
        // From where the forming bar opened, never past a bar: while catching up, the total stands
        // several bars ahead, and a countdown past its end would read as reached.
        let floor = if self.current.is_some() {
            self.bar_opened_at
        } else {
            Decimal::from(self.next_boundary.saturating_sub(self.n))
        };
        let target = Decimal::from(self.n);
        Some(BarProgress {
            done: (self.total - floor).clamp(Decimal::ZERO, target),
            target,
        })
    }

    fn deal_counter_input(&mut self) -> Option<&mut dyn DealCounterInput> {
        Some(self)
    }

    fn diagnostics(&self) -> BarBuilderDiagnostics {
        BarBuilderDiagnostics {
            uncounted_trades: self.uncounted,
            ..BarBuilderDiagnostics::default()
        }
    }
}

impl DealCounterInput for DealBarBuilder {
    fn observe(&mut self, sample: DealSample) {
        if let Some(newest) = self.newest
            && sample.time_ms < newest.time_ms
        {
            return;
        }
        self.newest = Some(sample);
        self.pending.push_back(sample);
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

    /// The shared fixture: readings at the terminal's cadence, ten-contract prints, 2 000 per bar.
    /// Window 1 has no rate; reading 1 003 000 gives 75 deals/contract. Window 2 closes bars A and B
    /// (1 004 000, 1 006 000); window 3 leaves a bar forming at 1 007 500.
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
        // Bar A: 1 500 estimated deals over 1 003 000 reach 1 004 000 on the second print.
        assert_eq!((bars[0].open_time, bars[0].close_time), (30_100, 30_200));
        assert_eq!(bars[0].trade_count, 2);
        assert_eq!(bars[0].open, Decimal::from(107));
        assert_eq!(bars[0].close, Decimal::from(108));
        // Bar B: the next two, reaching 1 006 000 on the fourth.
        assert_eq!((bars[1].open_time, bars[1].close_time), (30_300, 30_400));
        assert_eq!(bars[1].trade_count, 2);
        // Window 3 forming: 1 500 estimated deals past 1 006 000, toward 1 008 000.
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

    /// The live order (each reading just ahead of its prints) cuts where the rebuild order does
    /// and leaves the same prints uncounted.
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

    /// Another `N` over the same readings and prints recuts the day.
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

    /// Connected with the counter at 2 300 411: the first bar closes at 2 302 000.
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

    /// A reading that reaches a multiple the estimate had not closes the forming bar on the next
    /// print; one closed early is not closed twice.
    #[test]
    fn a_reading_corrects_the_estimate_without_cutting_twice() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 10_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 10_500)); // rate 50 per contract
        // Window 2 estimates 5 contracts at 250: total 10 750, no bar.
        assert!(b.push(&trade_of(2, 30_100, "100", "5")).is_none());
        // The reading says 11 200: 11 000 was crossed, so the next print closes the bar.
        b.observe_deals(sample(60_099, 11_200));
        let late = b
            .push(&trade_of(3, 60_100, "100", "1"))
            .expect("the missed multiple closes on the next print");
        assert_eq!(late.trade_count, 2);
        // Rate 140: the estimate closes 12 000 on the third print, early.
        b.push(&trade_of(4, 60_200, "100", "2"));
        let early = b
            .push(&trade_of(5, 60_300, "100", "3"))
            .expect("the estimate reaches 12 000");
        assert_eq!(early.trade_count, 2);
        // The reading says 11 900: 12 000 stays closed and is not cut again.
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

    /// A stretch with no readings past what a reading holds for is uncounted in both orders; cutting
    /// resumes at the next completed window.
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
        // Window 1 has no rate; window 2 counts 50 per contract (closes 5 002 000); the prints at 700 000
        // and 900 000 are beyond the reading's age. Multiples crossed during the stall get no bar.
        assert_eq!(rebuilt_builder.uncounted_trades(), 4);
        assert_eq!(rebuilt.len(), 1, "{rebuilt:?}");
        assert_eq!(
            (rebuilt[0].open_time, rebuilt[0].close_time),
            (30_100, 30_200)
        );
        assert_eq!(
            rebuilt_builder
                .partial()
                .map(|bar| (bar.open_time, bar.trade_count)),
            Some((1_230_100, 2))
        );
    }

    /// Yesterday's last reading under today's first prints counts none of them; the session's first
    /// reading starts the day clean instead of closing the morning as one bar.
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
        // The session's first reading is lower: a rollover ending a bar of last night's one print.
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

    /// A reading re-anchoring the total two multiples past the boundary gives each multiple its bar,
    /// one per print, so the bar count stays the venue's over `N`.
    #[test]
    fn a_reading_two_multiples_ahead_gives_each_its_bar() {
        let mut b = DealBarBuilder::new(500);
        b.observe_deals(sample(99, 1_000_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 1_000_750)); // rate 75
        // 100 contracts estimated at 7 500; the venue counted 10 000, so the reading lands four
        // multiples past the boundary.
        let mut closed = 0;
        for i in 0..100_u64 {
            let print = trade_of(2 + i, 30_100 + i as i64 * 10, "100", "1");
            closed += usize::from(b.push(&print).is_some());
        }
        assert_eq!(
            closed, 15,
            "7 500 estimated deals close fifteen bars at 500"
        );
        b.observe_deals(sample(60_099, 1_010_750));
        // Five multiples stand ahead (1 008 500 ... 1 010 500): one bar per print, the sixth reaches
        // 1 011 000, the seventh stays forming.
        let mut after = 0;
        for i in 0..6_u64 {
            let print = trade_of(200 + i, 60_100 + i as i64 * 100, "100", "1");
            after += usize::from(b.push(&print).is_some());
        }
        assert_eq!(after, 6, "each multiple the reading crossed gets its bar");
        assert!(b.push(&trade_of(300, 61_000, "100", "1")).is_none());
        assert_eq!(b.progress().map(|p| p.target), Some(Decimal::from(500)));
    }

    /// A count still below `N` can restart too: a drop by more than half is a restart.
    #[test]
    fn a_restart_below_n_is_still_a_restart() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(99, 1_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 1_500)); // rate 50
        assert!(b.push(&trade_of(2, 30_100, "100", "1")).is_none());
        b.observe_deals(sample(60_099, 0)); // the restart
        let ended = b
            .push(&trade_of(3, 60_100, "100", "1"))
            .expect("the restart ends the bar");
        assert_eq!(ended.trade_count, 1);
        assert_eq!(b.reading(), Some(0));
    }

    /// An unchanged reading re-emitted after a halt past a reading's age counts as taken now.
    #[test]
    fn a_repeated_reading_after_a_halt_renews_the_age() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 100));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 200)); // rate 10
        b.push(&trade_of(2, 30_100, "100", "1"));
        // A halt: fifteen minutes later the reconnect re-emits 200.
        let back = 30_099 + 15 * 60_000;
        b.observe_deals(sample(back, 200));
        assert!(b.push(&trade_of(3, back + 1, "100", "1")).is_none());
        assert_eq!(b.uncounted_trades(), 1, "window 1's print only");
        assert_eq!(b.partial().map(|bar| bar.trade_count), Some(2));
    }

    /// While multiples are caught up one print at a time, the countdown never passes a bar.
    #[test]
    fn progress_never_runs_past_a_bar() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(99, 5_000_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 5_000_100)); // rate 10
        b.push(&trade_of(2, 30_100, "100", "10")); // +100
        b.observe_deals(sample(60_099, 5_009_000)); // four multiples ahead
        for i in 0..3 {
            b.push(&trade_of(3 + i, 60_100 + i as i64 * 100, "100", "1"));
            let progress = b.progress().expect("a countdown");
            assert!(
                progress.done <= progress.target,
                "{} of {}",
                progress.done,
                progress.target
            );
        }
    }

    /// A reading past the boundary with a bar forming from before an empty window closes it at its
    /// multiple on the next print; the count then moves past the multiples that window crossed.
    #[test]
    fn a_forming_bar_closes_at_its_multiple_after_an_empty_window() {
        let mut b = DealBarBuilder::new(1_000);
        b.observe_deals(sample(99, 10_000));
        for i in 0..5 {
            b.push(&trade_of(1 + i, 100 + i as i64 * 100, "100", "10"));
        }
        b.observe_deals(sample(30_099, 10_500)); // rate 10
        assert!(b.push(&trade_of(10, 30_100, "100", "5")).is_none());
        assert!(b.push(&trade_of(11, 30_200, "100", "5")).is_none()); // total 10 600, forming
        b.observe_deals(sample(60_099, 10_900));
        b.observe_deals(sample(90_099, 11_200)); // an empty window crossed 11 000
        let closed = b
            .push(&trade_of(12, 90_100, "100", "1"))
            .expect("the forming bar closes at 11 000");
        assert_eq!(closed.trade_count, 3);
        assert_eq!(
            b.progress().map(|p| p.target),
            Some(Decimal::from(1_000)),
            "and the count moves on toward 12 000"
        );
        assert!(b.push(&trade_of(13, 90_200, "100", "1")).is_none());
    }

    /// A restart clears stretch state: the new session catches up one multiple per print.
    #[test]
    fn a_restart_clears_the_skipped_catch_up() {
        let mut b = DealBarBuilder::new(100);
        b.observe_deals(sample(99, 1_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 1_050)); // rate 5
        b.push(&trade_of(2, 30_100, "100", "1")); // forming, total 1 055
        b.observe_deals(sample(60_099, 1_120)); // an empty window crossed 1 100
        b.observe_deals(sample(90_099, 3)); // the restart, before any print
        let ended = b
            .push(&trade_of(3, 90_100, "100", "1"))
            .expect("the old bar ends");
        assert_eq!(ended.trade_count, 1);
        b.observe_deals(sample(120_099, 400)); // four multiples ahead of the estimate
        let mut closed = 0;
        for i in 0..4 {
            closed += usize::from(
                b.push(&trade_of(4 + i, 120_100 + i as i64 * 100, "100", "1"))
                    .is_some(),
            );
        }
        assert_eq!(closed, 4, "each multiple the reading crossed gets its bar");
    }

    /// A reconnect re-emitting an unchanged reading is not a window and sets no zero rate.
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

    /// A window with a print beyond a reading's age sets no rate (its deals are in the delta, its
    /// contracts are not); the window keeps the rate it had.
    #[test]
    fn an_uncounted_stretch_does_not_inflate_the_rate() {
        let mut b = DealBarBuilder::new(1_000_000);
        b.observe_deals(sample(99, 1_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 2_000)); // rate 100
        b.push(&trade_of(2, 30_100, "100", "1"));
        assert_eq!(b.rate(), Some(Decimal::from(100)));
        // Twelve minutes of silence, a stale print, then a reading covering the silence.
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

    /// A reading reaching the next multiple right after a close still gives it a bar on the next
    /// print: "nothing forming" means no print was credited.
    #[test]
    fn a_multiple_reached_right_after_a_close_gets_its_bar() {
        let (samples, mut trades) = fixture();
        // Keep window 2's first two prints (bar A closes), then a reading past 1 006 000 with nothing
        // forming.
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

    /// The print opening the bar after a rollover can reach the first multiple itself: its bar
    /// closes on the next push, never one print late.
    #[test]
    fn the_first_print_after_a_rollover_can_close_its_own_bar() {
        let mut b = DealBarBuilder::new(2_000);
        b.observe_deals(sample(99, 1_000_000));
        b.push(&trade_of(1, 100, "100", "10"));
        b.observe_deals(sample(30_099, 1_000_750)); // rate 75
        b.push(&trade_of(2, 30_100, "100", "1"));
        // The restart at 1 950: ten contracts are 750, past 2 000.
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

    /// A reading lower by less than half is a late poll answer, not a restart: no bar ends. A drop
    /// by more than half is the restart.
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
        b.observe_deals(sample(32_099, 4_996_000)); // a refresh behind: ignored too
        assert!(
            b.push(&trade(30, 32_100, "102")).is_none(),
            "a late poll thousands behind ends nothing"
        );
        assert_eq!(b.reading(), Some(5_000_500));
        assert_eq!(b.partial().map(|bar| bar.trade_count), Some(3));
        b.observe_deals(sample(60_099, 3)); // the restart
        let ended = b
            .push(&trade(4, 60_100, "103"))
            .expect("the restart ends the bar");
        assert_eq!(ended.trade_count, 3);
        assert_eq!(b.reading(), Some(3));
        assert_eq!(b.rate(), Some(Decimal::from(100)), "the rate carries over");
    }

    #[test]
    fn print_only_builders_expose_no_deal_counter_port() {
        let mut tick = crate::TickBarBuilder::new(2);
        assert!(tick.deal_counter_input().is_none());
        assert!(tick.push(&trade(1, 1, "100")).is_none());
        assert_eq!(tick.diagnostics().uncounted_trades, 0);
        assert!(
            tick.push(&trade(2, 2, "100")).is_some(),
            "a tick bar counts ticks"
        );
    }
}
