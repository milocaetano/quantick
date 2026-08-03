//! Folding venue candles up to a coarser interval.
//!
//! Every provider delivers candle history at one interval
//! ([`crate::feed::OHLCV_BASE_INTERVAL_MS`], a minute), and the time pane
//! shows whatever its header asks for. Folding locally is what makes changing
//! that free: a chip click is a different fold over bars already held, not a
//! round trip to a venue.
//!
//! Pure and deterministic — same bars in, same bars out, no clock and no
//! iteration-order dependence. That is what lets a chip click be tested
//! without a feed and re-run without drift.

use quantick_engine::Bar;

use crate::feed::OHLCV_BASE_INTERVAL_MS;

/// Whether `interval_ms` can be folded to from the base interval at all.
///
/// A whole number of base candles or nothing: 5m and 1h are exact unions of
/// minutes, 90s and 100ms are not, and a bucket built from a fraction of a
/// candle would be inventing where the missing part went. The sub-minute range
/// simply gets no prefix — an honest absence rather than an approximation.
#[must_use]
pub fn is_foldable(interval_ms: i64) -> bool {
    interval_ms >= OHLCV_BASE_INTERVAL_MS && interval_ms % OHLCV_BASE_INTERVAL_MS == 0
}

/// Fold `base` candles up to `interval_ms`, or return nothing when the
/// interval is not a whole number of base candles.
///
/// Bars are bucketed by the epoch-aligned window their `open_time` falls in,
/// which is the same alignment a venue uses for its own coarser candles. Each
/// bucket takes the first bar's open, the highest high, the lowest low and the
/// last bar's close; volumes and trade counts add up.
///
/// `base` is expected ascending by `open_time`, as every provider delivers it.
/// Buckets with nothing in them are skipped rather than emitted flat — the
/// engine's empty-interval rule, kept across the fold: a gap is the honest
/// record that nothing traded.
#[must_use]
pub fn fold(base: &[Bar], interval_ms: i64) -> Vec<Bar> {
    if !is_foldable(interval_ms) || base.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Bar> = Vec::with_capacity(
        base.len()
            / usize::try_from(interval_ms / OHLCV_BASE_INTERVAL_MS)
                .unwrap_or(1)
                .max(1)
            + 1,
    );
    let mut open_bucket: Option<i64> = None;
    for bar in base {
        let bucket = bucket_start(bar.open_time, interval_ms);
        match (open_bucket, out.last_mut()) {
            // Same bucket as the bar before it: merge in.
            (Some(current), Some(folded)) if current == bucket => {
                folded.high = folded.high.max(bar.high);
                folded.low = folded.low.min(bar.low);
                folded.close = bar.close;
                folded.close_time = bar.close_time;
                folded.buy_volume = folded.buy_volume.saturating_add(bar.buy_volume);
                folded.sell_volume = folded.sell_volume.saturating_add(bar.sell_volume);
                folded.trade_count = folded.trade_count.saturating_add(bar.trade_count);
            }
            // A new bucket starts a new bar, keeping the base candle's own
            // `open_time` rather than the window's start: the first minute
            // that traded is when this bar opened, and rounding it down to the
            // bucket would claim a price at a moment nothing printed. The
            // bucket is what groups; the stamp stays the market's.
            _ => {
                open_bucket = Some(bucket);
                out.push(bar.clone());
            }
        }
    }
    out
}

/// The start of the `interval_ms` window containing `time_ms`.
///
/// Epoch-aligned and floor-divided, so a negative timestamp (a fixture before
/// 1970, never a real market) lands in the window below it rather than
/// rounding toward zero into the wrong bucket.
#[must_use]
pub fn bucket_start(time_ms: i64, interval_ms: i64) -> i64 {
    if interval_ms <= 0 {
        return time_ms;
    }
    time_ms.div_euclid(interval_ms) * interval_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    /// One base candle: minute `minute`, prices derived from `seed` so a merge
    /// is visible in the result.
    fn candle(minute: i64, seed: i64) -> Bar {
        let open_time = minute * OHLCV_BASE_INTERVAL_MS;
        Bar {
            open_time,
            close_time: open_time + OHLCV_BASE_INTERVAL_MS - 1,
            open: Decimal::from(100 + seed),
            high: Decimal::from(110 + seed),
            low: Decimal::from(90 + seed),
            close: Decimal::from(105 + seed),
            buy_volume: Decimal::from(2),
            sell_volume: Decimal::from(3),
            trade_count: 7,
        }
    }

    #[test]
    fn only_whole_multiples_of_the_base_interval_fold() {
        assert!(is_foldable(60_000), "the base interval folds to itself");
        assert!(is_foldable(300_000), "5m");
        assert!(is_foldable(3_600_000), "1h");
        assert!(!is_foldable(90_000), "90s is not a whole number of minutes");
        assert!(!is_foldable(1_000), "and nothing below the base folds");
        assert!(!is_foldable(0));
        assert!(!is_foldable(-60_000));

        assert!(
            fold(&[candle(0, 0)], 90_000).is_empty(),
            "an interval that cannot be folded to gets no bars, not approximate ones"
        );
    }

    #[test]
    fn five_minutes_takes_first_open_last_close_and_the_extremes() {
        // Minutes 0..5 into one bucket, with the extremes in the middle.
        let base: Vec<Bar> = (0..5).map(|m| candle(m, m)).collect();

        let folded = fold(&base, 5 * OHLCV_BASE_INTERVAL_MS);

        assert_eq!(folded.len(), 1);
        let bar = &folded[0];
        assert_eq!(bar.open_time, 0, "the bucket opens where its first bar did");
        assert_eq!(
            bar.close_time, base[4].close_time,
            "and closes where the last did"
        );
        assert_eq!(bar.open, base[0].open);
        assert_eq!(bar.close, base[4].close);
        assert_eq!(
            bar.high,
            base.iter().map(|b| b.high).max().expect("bars"),
            "the highest high survives the fold"
        );
        assert_eq!(
            bar.low,
            base.iter().map(|b| b.low).min().expect("bars"),
            "and the lowest low"
        );
        assert_eq!(bar.buy_volume, Decimal::from(10), "volumes add up");
        assert_eq!(bar.sell_volume, Decimal::from(15));
        assert_eq!(bar.trade_count, 35);
    }

    #[test]
    fn buckets_are_epoch_aligned_not_first_bar_aligned() {
        // Starting at minute 7: the 5m windows are [5,10) and [10,15), so the
        // first bucket holds three bars, not five.
        let base: Vec<Bar> = (7..13).map(|m| candle(m, 0)).collect();

        let folded = fold(&base, 5 * OHLCV_BASE_INTERVAL_MS);

        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].open_time, 7 * OHLCV_BASE_INTERVAL_MS);
        assert_eq!(
            bucket_start(folded[0].open_time, 5 * OHLCV_BASE_INTERVAL_MS),
            5 * OHLCV_BASE_INTERVAL_MS,
            "the first bucket is the venue's own [5m,10m) window"
        );
        assert_eq!(folded[1].open_time, 10 * OHLCV_BASE_INTERVAL_MS);
    }

    #[test]
    fn an_empty_window_is_skipped_rather_than_carried_forward() {
        // Nothing traded between minute 1 and minute 20.
        let base = vec![candle(0, 0), candle(1, 1), candle(20, 2)];

        let folded = fold(&base, 5 * OHLCV_BASE_INTERVAL_MS);

        assert_eq!(
            folded.len(),
            2,
            "two buckets held bars; the three between them are gaps, not flat candles"
        );
        assert_eq!(folded[0].open_time, 0);
        assert_eq!(folded[1].open_time, 20 * OHLCV_BASE_INTERVAL_MS);
    }

    #[test]
    fn folding_to_the_base_interval_returns_what_it_was_given() {
        let base: Vec<Bar> = (0..4).map(|m| candle(m, m)).collect();
        assert_eq!(fold(&base, OHLCV_BASE_INTERVAL_MS), base);
        assert!(fold(&[], 5 * OHLCV_BASE_INTERVAL_MS).is_empty());
    }

    #[test]
    fn the_fold_is_deterministic() {
        let base: Vec<Bar> = (0..37).map(|m| candle(m, m % 7)).collect();
        let once = fold(&base, 15 * OHLCV_BASE_INTERVAL_MS);
        let twice = fold(&base, 15 * OHLCV_BASE_INTERVAL_MS);
        assert_eq!(once, twice, "same bars in, same bars out");
    }
}
