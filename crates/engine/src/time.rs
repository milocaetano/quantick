//! Time bars — the baseline reference, driven only by trade timestamps.
//!
//! Time bars close on fixed wall-clock intervals (1s, 1m, ...). They are not
//! the point of quantick — the alternative bars are — but they are the baseline
//! the alternative bars are compared against, on the chart and in research, and
//! they are nearly free once the builder skeleton exists.
//!
//! Crucially, the interval boundary is derived **only from trade timestamps**,
//! never from a wall clock: a trade at time `t` falls in the bucket starting at
//! `floor(t / interval) * interval`. Reading the host clock would make the same
//! fixture produce different bars on different runs — a determinism violation.
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

use crate::threshold::{extend_bar, open_bar};
use crate::{Bar, BarBuilder, Trade};

/// Builds time bars: one bar per `interval_ms` interval that contains trades.
///
/// Feed trades in non-decreasing timestamp order with
/// [`push`](BarBuilder::push); a bar closes when the first trade of a *later*
/// interval arrives. Intervals with no trades are skipped (see the [module
/// docs](self)).
#[derive(Debug, Clone)]
pub struct TimeBarBuilder {
    interval_ms: i64,
    bucket_start: i64,
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
        Self {
            interval_ms,
            bucket_start: 0,
            current: None,
        }
    }

    /// The configured interval, in milliseconds.
    #[must_use]
    pub fn interval_ms(&self) -> i64 {
        self.interval_ms
    }

    /// The start (epoch ms) of the interval a trade at `timestamp_ms` belongs
    /// to. Uses Euclidean division so it floors correctly for any timestamp.
    fn bucket_of(&self, timestamp_ms: i64) -> i64 {
        timestamp_ms.div_euclid(self.interval_ms) * self.interval_ms
    }
}

impl BarBuilder for TimeBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        let bucket = self.bucket_of(trade.timestamp_ms);
        match &mut self.current {
            // First trade: open the first bar; nothing closes yet.
            None => {
                self.current = Some(open_bar(trade));
                self.bucket_start = bucket;
                None
            }
            // Same interval: fold the trade into the forming bar.
            Some(bar) if bucket == self.bucket_start => {
                extend_bar(bar, trade);
                None
            }
            // A later interval: close the current bar and open a fresh one for
            // this trade. Any intervening empty intervals are simply skipped.
            Some(_) => {
                let closed = self.current.take();
                self.current = Some(open_bar(trade));
                self.bucket_start = bucket;
                closed
            }
        }
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
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
}
