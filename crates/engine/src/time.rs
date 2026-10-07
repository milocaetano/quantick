//! Time bars — the baseline reference, driven only by trade timestamps.
//!
//! Time bars close on fixed wall-clock intervals (1s, 1m, ...). They are not
//! the point of quantick — the alternative bars are — but they are the baseline
//! the alternative bars are compared against, on the chart and in research, and
//! they are nearly free once the builder skeleton exists.
//!
//! Crucially, the interval boundary is derived **only from trade timestamps**,
//! never from a wall clock: a trade at time `t` falls in the bucket
//! [`crate::time_bucket::time_bucket_start`] names — `floor(t / interval) * interval` for a fixed
//! interval, Monday 00:00 UTC for whole weeks and the calendar month for
//! months (see [`crate::time_bucket`]). Reading the host clock would make the
//! same fixture produce different bars on different runs — a determinism
//! violation.
//!
//! # Empty-interval policy: skip, don't fabricate
//!
//! An interval in which no trade occurred produces **no bar**. The alternative:
//! emitting an "empty" bar (volume 0, OHLC carried forward from the previous
//! close) would fabricate a price for a moment that had no trade — inferred data
//! presented as if it were sampled, which the data-honesty rule forbids. The
//! alternative bars never have empty bars either, so skipping keeps the baseline
//! consistent with them.
//!
//! The skip is not hidden: consecutive closed bars can be non-contiguous in
//! time (a bar's `open_time` bucket may be several intervals after the previous
//! bar's), and that gap is the honest record that no trades happened in between.

use rust_decimal::Decimal;

use crate::time_bucket::TimeBucketLaw;
use crate::{Bar, BarBuilder, BarProgress, Trade};

/// Builds time bars: one bar per `interval_ms` interval that contains trades.
///
/// Feed trades in non-decreasing timestamp order with
/// [`push`](BarBuilder::push); a bar closes when the first trade of a *later*
/// interval arrives. Intervals with no trades are skipped (see the [module
/// docs](self)).
#[derive(Debug, Clone)]
pub struct TimeBarBuilder {
    interval_ms: i64,
    /// The interval's shape, read once: a trade never re-derives it.
    law: TimeBucketLaw,
    bucket_start: i64,
    /// Where the forming bar's bucket ends, exclusive. A trade before it (and
    /// not before `bucket_start`) joins the forming bar on one comparison;
    /// only a trade past it asks the law where its bucket starts.
    bucket_end: i64,
    current: Option<Bar>,
}

impl TimeBarBuilder {
    /// Create a builder with the given interval, in milliseconds.
    ///
    /// # Panics
    ///
    /// Panics if `interval_ms <= 0`: a non-positive interval has no meaningful
    /// bucket boundary.
    #[must_use]
    pub fn new(interval_ms: i64) -> Self {
        assert!(
            interval_ms > 0,
            "time bar interval must be > 0 ms, got {interval_ms}"
        );
        let law =
            TimeBucketLaw::of(interval_ms).expect("a positive interval always has a bucket law");
        Self {
            interval_ms,
            law,
            bucket_start: 0,
            bucket_end: 0,
            current: None,
        }
    }

    /// The configured interval, in milliseconds.
    #[must_use]
    pub fn interval_ms(&self) -> i64 {
        self.interval_ms
    }
}

impl BarBuilder for TimeBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        let time = trade.timestamp_ms;
        // Same interval: fold the trade into the forming bar. The hot path,
        // and one comparison against the bucket already resolved.
        if let Some(bar) = &mut self.current
            && (self.bucket_start..self.bucket_end).contains(&time)
        {
            bar.extend(trade);
            return None;
        }
        // The first trade, or one in another interval: open a fresh bar for it
        // and close the one forming, if any. Intervening empty intervals are
        // simply skipped.
        self.bucket_start = self.law.start(time);
        self.bucket_end = self.law.end(self.bucket_start);
        self.current.replace(Bar::opened_by(trade))
    }

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    /// Elapsed time *of the trades seen*, against the interval — never a wall
    /// clock, which would make the same fixture report differently on two runs.
    /// A quiet stretch therefore freezes the readout instead of running it out:
    /// the bar closes on the first trade of a later interval, so with no trades
    /// there is nothing yet to close it.
    ///
    /// The target is this bucket's own length, which for a calendar month is
    /// the month's days rather than the nominal interval.
    fn progress(&self) -> Option<BarProgress> {
        let bar = self.current.as_ref()?;
        let length = self.bucket_end - self.bucket_start;
        let elapsed = bar
            .close_time
            .saturating_sub(self.bucket_start)
            .clamp(0, length);
        Some(BarProgress {
            done: Decimal::from(elapsed),
            target: Decimal::from(length),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    fn trade(ts: i64, price: &str, side: Side) -> Trade {
        Trade {
            agg_id: 0,
            timestamp_ms: ts,
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str("1.0").unwrap(),
            side,
        }
    }

    #[test]
    #[should_panic(expected = "time bar interval must be > 0 ms")]
    fn rejects_non_positive_interval() {
        let _ = TimeBarBuilder::new(0);
    }

    #[test]
    fn trades_in_the_same_interval_share_a_bar() {
        let mut b = TimeBarBuilder::new(1000);
        assert!(b.push(&trade(1000, "100.0", Side::Buy)).is_none());
        assert!(b.push(&trade(1999, "101.0", Side::Buy)).is_none());
        assert_eq!(b.partial().unwrap().trade_count, 2);
    }

    #[test]
    fn a_later_interval_closes_the_previous_bar() {
        let mut b = TimeBarBuilder::new(1000);
        assert!(b.push(&trade(1500, "100.0", Side::Buy)).is_none());
        let closed = b
            .push(&trade(2500, "101.0", Side::Buy))
            .expect("new interval closes");
        assert_eq!(closed.open_time, 1500);
        assert_eq!(closed.close_time, 1500);
    }

    #[test]
    fn empty_intervals_are_skipped_not_emitted() {
        // Trade at 1500 (bucket 1000), then a jump to 4200 (bucket 4000):
        // buckets 2000 and 3000 are empty and must not produce bars.
        let mut b = TimeBarBuilder::new(1000);
        assert!(b.push(&trade(1500, "100.0", Side::Buy)).is_none());
        let closed = b
            .push(&trade(4200, "103.0", Side::Buy))
            .expect("closes bucket 1000");
        assert_eq!(closed.open_time, 1500, "only the non-empty bucket closed");
        // The forming bar jumps straight to bucket 4000 — no empty bars between.
        assert_eq!(b.partial().unwrap().open_time, 4200);
    }

    /// The bucket resolved once and cached must cut exactly where the law
    /// does for every trade: same trades in, same bars out, whichever shape.
    #[test]
    fn the_cached_bucket_cuts_where_the_law_does() {
        use crate::time_bucket::{CALENDAR_MONTH_MS, DAY_MS, WEEK_MS, time_bucket_start};
        let times: Vec<i64> = (0..400).map(|i| -5 * DAY_MS + i * 37 * 3_600_000).collect();
        for interval in [
            60_000,
            DAY_MS,
            WEEK_MS,
            CALENDAR_MONTH_MS,
            3 * CALENDAR_MONTH_MS,
        ] {
            let mut b = TimeBarBuilder::new(interval);
            let closed: Vec<i64> = times
                .iter()
                .filter_map(|&ts| b.push(&trade(ts, "100.0", Side::Buy)))
                .map(|bar| time_bucket_start(bar.open_time, interval))
                .collect();
            let mut expected: Vec<i64> = times
                .iter()
                .map(|&ts| time_bucket_start(ts, interval))
                .collect();
            expected.dedup();
            expected.pop(); // the forming bar has not closed
            assert_eq!(closed, expected, "{interval}");
        }
    }

    /// The countdown follows the trades, not a wall clock — the same fixture
    /// must report the same progress on every run.
    #[test]
    fn progress_is_elapsed_trade_time_inside_the_interval() {
        let mut b = TimeBarBuilder::new(1000);
        assert!(b.progress().is_none(), "no bar, nothing to report");
        b.push(&trade(1200, "100.0", Side::Buy));
        let p = b.progress().expect("a time bar runs toward its interval");
        assert_eq!(
            (p.done, p.target),
            (Decimal::from(200), Decimal::from(1000))
        );
        // A later trade in the same bucket moves it; a quiet stretch does not.
        b.push(&trade(1900, "101.0", Side::Buy));
        assert_eq!(b.progress().unwrap().done, Decimal::from(900));
        // The next interval opens a fresh bar and a fresh countdown.
        assert!(b.push(&trade(2100, "101.0", Side::Buy)).is_some());
        assert_eq!(b.progress().unwrap().done, Decimal::from(100));
    }
}
