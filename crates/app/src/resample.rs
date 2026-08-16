//! Folding bars into coarser bars.
//!
//! Two callers, one fold. [`fold`] groups **by time**: every provider delivers
//! candle history at one interval ([`crate::feed::OHLCV_BASE_INTERVAL_MS`], a
//! minute) and the time pane shows whatever its header asks for, so folding
//! locally is what makes changing that free — a chip click is a different fold
//! over bars already held, not a round trip to a venue. [`for_each_group`]
//! groups **by count**, for the chart squeezed past the zoom where a bar can be
//! drawn on its own: several bars share one candle rather than each getting a
//! sliver of a pixel.
//!
//! Both go through [`merge_into`], for the reason `Bar::extend` is public in
//! the engine: the summary of a run of bars is a fact about those bars, and a
//! second implementation of it is a second answer waiting to drift.
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
            (Some(current), Some(folded)) if current == bucket => merge_into(folded, bar),
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

/// Merge `bar` into the bar it is being folded with.
///
/// The summary of a run of bars, and the only one: the run keeps the first
/// bar's open and stamp, takes the extremes, ends on the last bar's close and
/// stamp, and adds the volumes and trade counts up. Exact arithmetic on the
/// numbers already held — a folded bar states nothing the bars in it did not.
///
/// `bar` must come after `folded` in time, which both callers guarantee by
/// walking an ascending series.
pub fn merge_into(folded: &mut Bar, bar: &Bar) {
    folded.high = folded.high.max(bar.high);
    folded.low = folded.low.min(bar.low);
    folded.close = bar.close;
    folded.close_time = bar.close_time;
    folded.buy_volume = folded.buy_volume.saturating_add(bar.buy_volume);
    folded.sell_volume = folded.sell_volume.saturating_add(bar.sell_volume);
    folded.trade_count = folded.trade_count.saturating_add(bar.trade_count);
}

/// Walk `bars` in runs of `per_slot`, calling `paint` once per run with the
/// index of the run's first bar and the bar the run is drawn as.
///
/// `first_index` is the series index of the first bar handed in, so runs line
/// up with the same absolute grouping every frame (bar *n* belongs to run
/// `n / per_slot`) rather than with wherever the window happens to start.
///
/// **Test-only.** The chart draws its groups from
/// [`crate::bar_groups::BarGroups`], which folds each bar once and keeps the
/// answer between frames — re-folding a window of tens of thousands of bars at
/// sixty hertz is the whole frame budget. This is the straight-line version
/// that cache is checked against: two ways to *walk* the bars, one way to fold
/// them (both call [`merge_into`]), so the oracle can differ about when work
/// happens and never about what the answer is.
#[cfg(test)]
pub fn for_each_group<'a>(
    bars: impl Iterator<Item = &'a Bar>,
    first_index: usize,
    per_slot: usize,
    mut paint: impl FnMut(usize, &Bar),
) {
    if per_slot <= 1 {
        for (offset, bar) in bars.enumerate() {
            paint(first_index + offset, bar);
        }
        return;
    }
    let mut run: Option<(usize, Bar)> = None;
    for (offset, bar) in bars.enumerate() {
        let index = first_index + offset;
        match &mut run {
            Some((first, folded)) if *first / per_slot == index / per_slot => {
                merge_into(folded, bar);
            }
            _ => {
                if let Some((first, folded)) = run.take() {
                    paint(first, &folded);
                }
                run = Some((index, bar.clone()));
            }
        }
    }
    if let Some((first, folded)) = run {
        paint(first, &folded);
    }
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

    /// Collect what `for_each_group` paints: `(first bar of the run, the bar
    /// the run is drawn as)`.
    fn groups(bars: &[Bar], first_index: usize, per_slot: usize) -> Vec<(usize, Bar)> {
        let mut out = Vec::new();
        for_each_group(bars.iter(), first_index, per_slot, |index, bar| {
            out.push((index, bar.clone()));
        });
        out
    }

    /// One bar per candle passes straight through, in order — the path the
    /// chart is on at every zoom a trader works at.
    #[test]
    fn ungrouped_bars_pass_through_untouched() {
        let bars: Vec<Bar> = (0..5).map(|m| candle(m, m)).collect();
        for per_slot in [0_usize, 1] {
            let painted = groups(&bars, 10, per_slot);
            assert_eq!(painted.len(), 5);
            for (offset, (index, bar)) in painted.iter().enumerate() {
                assert_eq!(*index, 10 + offset);
                assert_eq!(bar, &bars[offset]);
            }
        }
    }

    /// Grouped, a run is drawn as its exact fold — and the runs line up with
    /// absolute indices, not with where the window happens to start, so a
    /// candle keeps standing for the same bars as the view moves.
    #[test]
    fn a_run_is_drawn_as_the_exact_fold_of_its_bars() {
        let bars: Vec<Bar> = (0..8).map(|m| candle(m, m)).collect();
        // Handed in starting at bar 4, in groups of four: 4..8 is one whole
        // group, and its first index is 4.
        let painted = groups(&bars[4..], 4, 4);
        assert_eq!(painted.len(), 1);
        let (index, folded) = &painted[0];
        assert_eq!(*index, 4);
        assert_eq!(folded.open, bars[4].open, "the first bar's open");
        assert_eq!(folded.open_time, bars[4].open_time);
        assert_eq!(folded.close, bars[7].close, "the last bar's close");
        assert_eq!(folded.close_time, bars[7].close_time);
        assert_eq!(folded.high, bars[7].high, "the highest high");
        assert_eq!(folded.low, bars[4].low, "the lowest low");
        assert_eq!(folded.buy_volume, Decimal::from(8), "volumes add up");
        assert_eq!(folded.sell_volume, Decimal::from(12));
        assert_eq!(folded.trade_count, 28);
    }

    /// A window rarely ends on a group boundary; the tail is drawn from the
    /// bars there are rather than held back until the group fills.
    #[test]
    fn a_partial_run_at_the_end_is_still_drawn() {
        let bars: Vec<Bar> = (0..6).map(|m| candle(m, m)).collect();
        let painted = groups(&bars, 0, 4);
        assert_eq!(painted.len(), 2);
        assert_eq!(painted[0].0, 0);
        assert_eq!(painted[1].0, 4, "the tail starts its own run");
        assert_eq!(painted[1].1.close, bars[5].close);
        assert!(for_each_group_is_empty_for_no_bars());
    }

    fn for_each_group_is_empty_for_no_bars() -> bool {
        let mut painted = 0;
        for_each_group([].iter(), 0, 4, |_, _| painted += 1);
        painted == 0
    }

    /// Grouping by count and grouping by time are the same fold, so a run of
    /// bars comes to the same candle either way.
    #[test]
    fn the_count_fold_and_the_time_fold_agree() {
        let bars: Vec<Bar> = (0..5).map(|m| candle(m, m)).collect();
        let by_time = fold(&bars, 5 * OHLCV_BASE_INTERVAL_MS);
        let by_count = groups(&bars, 0, 5);
        assert_eq!(by_time.len(), 1);
        assert_eq!(by_count.len(), 1);
        assert_eq!(by_time[0], by_count[0].1);
    }
}
