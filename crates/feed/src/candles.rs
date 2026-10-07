//! Venue candles on the chart's side of the port: folding them up to a
//! coarser interval, putting an older slice in front of the ones held, and
//! trimming them to the seam where the chart's own bars begin.
//!
//! [`fold`] groups **by time**: every provider delivers candle history at a
//! base interval ([`crate::OHLCV_BASE_INTERVAL_MS`], a minute, or
//! [`crate::OHLCV_DAILY_INTERVAL_MS`] for a pane cut at a day or longer) and
//! the time pane shows whatever its header asks for, so folding locally is
//! what makes changing that free — a chip click is a different fold over bars
//! already held, not a round trip to a venue. The buckets are the engine's own
//! ([`quantick_engine::time_bucket`]): a day at 00:00 UTC, a week from Monday,
//! a calendar month — the law the forming bar on the same pane is cut by, so
//! history and live meet at the seam. The row merge lives in [`merge_into`],
//! for the reason `Bar::extend` is public in the engine: the summary of a run
//! of bars is a fact about those bars, and a second implementation of it is a
//! second answer waiting to drift.
//!
//! What this module deliberately does **not** offer: folding bars by count
//! for display. One bar is one candle at every zoom (`crate::viewport`) —
//! a trader must be able to trust that every candle on screen is exactly one
//! bar of the rule they configured, and a fold on the way to the screen would
//! break that somewhere.
//!
//! Pure and deterministic — same bars in, same bars out, no clock and no
//! iteration-order dependence. That is what lets a chip click be tested
//! without a feed and re-run without drift.

use quantick_engine::Bar;
use quantick_engine::time_bucket::{
    DAY_MS, TimeBucketLaw, WEEK_MS, calendar_months, time_bucket_start,
};

mod seam_lead;
pub use seam_lead::{SeamLead, seam_lead};

/// Whether `interval_ms` can be folded to from `base_interval_ms` candles.
///
/// A whole number of base candles or nothing: 5m and 1h are exact unions of
/// minutes, 90s and 100ms are not, and a bucket built from a fraction of a
/// candle would be inventing where the missing part went. The sub-minute range
/// simply gets no prefix — an honest absence rather than an approximation.
/// Weeks and calendar months open on a day boundary, so any base that tiles a
/// day folds to them; a daily base folds to nothing shorter than a day.
#[must_use]
pub fn is_foldable(base_interval_ms: i64, interval_ms: i64) -> bool {
    if base_interval_ms <= 0 {
        return false;
    }
    let tiles_a_day = base_interval_ms <= DAY_MS && DAY_MS % base_interval_ms == 0;
    if calendar_months(interval_ms).is_some() || (interval_ms > 0 && interval_ms % WEEK_MS == 0) {
        return tiles_a_day;
    }
    interval_ms >= base_interval_ms && interval_ms % base_interval_ms == 0
}

/// The instant that places a venue candle in a bucket: the middle of the span
/// it covers.
///
/// For a candle aligned to the bucket law — every minute, and a daily candle
/// opening at 00:00 UTC — this is the bucket its `open_time` names, so nothing
/// moves. It matters for a daily candle cut at a server's own midnight
/// (MetaTrader's D1 on a server three hours ahead of UTC opens at 21:00 the
/// day before): the candle lands in the UTC day holding most of it rather
/// than the one its first hours fall in, and [`fold`] stamps it inside that
/// day.
#[must_use]
pub fn placement_ms(bar: &Bar) -> i64 {
    let span = bar.close_time.saturating_sub(bar.open_time).max(0);
    bar.open_time.saturating_add(span / 2)
}

/// Fold `base` candles, each `base_interval_ms` long, up to `interval_ms`, or
/// return nothing when the interval is not a whole number of base candles.
///
/// Bars are bucketed by the engine's bucket law at their [`placement_ms`] —
/// the epoch-aligned window for a fixed interval, Monday 00:00 UTC for weeks
/// and the calendar month for months — which is the same alignment a venue
/// uses for its own coarser candles. Each bucket takes the first bar's open,
/// the highest high, the lowest low and the last bar's close; volumes and
/// trade counts add up.
///
/// `base` is expected ascending by `open_time`, as every provider delivers it.
/// Buckets with nothing in them are skipped rather than emitted flat — the
/// engine's empty-interval rule, kept across the fold: a gap is the honest
/// record that nothing traded.
///
/// A folded bar's stamps stay inside the bucket it was placed in. A candle
/// cut at a server's midnight reaches across the UTC boundary, and keeping its
/// raw `open_time` would stamp a UTC day's bar on the evening before — where
/// the crosshair and every slot lookup would put it beside an intraday pane's
/// previous day. The clamp moves only a stamp that lay outside the bucket; the
/// feed says once per block that its days are the server's (the
/// `MT5_RATES_SERVER_DAY` event), so the re-cut is labelled, not silent.
///
/// A bar folded from daily candles is stamped on its UTC bucket — opening at
/// its start and closing on its last millisecond, the span a candle covers —
/// because a daily candle's own stamps are the server's midnights, not the
/// moment anything traded: a B3 day would otherwise open at 03:00 and a month
/// on its first trading day.
#[must_use]
pub fn fold(base: &[Bar], base_interval_ms: i64, interval_ms: i64) -> Vec<Bar> {
    if !is_foldable(base_interval_ms, interval_ms) || base.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Bar> = Vec::with_capacity(
        base.len()
            / usize::try_from(interval_ms / base_interval_ms)
                .unwrap_or(1)
                .max(1)
            + 1,
    );
    let Some(law) = TimeBucketLaw::of(interval_ms) else {
        return Vec::new();
    };
    let mut open_bucket: Option<(i64, i64)> = None;
    for bar in base {
        let bucket = law.start(placement_ms(bar));
        match (open_bucket, out.last_mut()) {
            // Same bucket as the bar before it: merge in.
            (Some((current, end)), Some(folded)) if current == bucket => {
                merge_into(folded, bar);
                folded.close_time = folded.close_time.min(end - 1);
            }
            // A new bucket starts a new bar, keeping the base candle's own
            // `open_time` rather than the window's start: the first minute
            // that traded is when this bar opened, and rounding it down to the
            // bucket would claim a price at a moment nothing printed. The
            // bucket is what groups; the stamp stays the market's — clamped
            // into the bucket only where the candle reached outside it.
            _ => {
                let end = law.end(bucket);
                open_bucket = Some((bucket, end));
                let mut opened = bar.clone();
                opened.open_time = opened.open_time.clamp(bucket, end - 1);
                opened.close_time = opened.close_time.clamp(opened.open_time, end - 1);
                out.push(opened);
            }
        }
    }
    if base_interval_ms >= DAY_MS {
        for folded in &mut out {
            folded.open_time = law.start(folded.open_time);
            folded.close_time = law.end(folded.open_time) - 1;
        }
    }
    out
}

/// Whether the oldest of `base` candles opens after the `interval_ms` bucket
/// it folds into: the start of the record cuts into that bucket, so the
/// folded bar knowingly holds only part of it.
#[must_use]
pub fn starts_inside_bucket(base: &[Bar], base_interval_ms: i64, interval_ms: i64) -> bool {
    let (Some(first), Some(law)) = (base.first(), TimeBucketLaw::of(interval_ms)) else {
        return false;
    };
    let placed = placement_ms(first);
    law.start(placed) < time_bucket_start(placed, base_interval_ms)
}

/// Merge `bar` into the bar it is being folded with — [`Bar::absorb`], the
/// engine's one summary of a run of bars. Exact arithmetic on the numbers
/// already held: a folded bar states nothing the bars in it did not.
///
/// `bar` must come after `folded` in time, which every caller guarantees by
/// walking an ascending series.
pub fn merge_into(folded: &mut Bar, bar: &Bar) {
    folded.absorb(bar);
}

/// The start of the `interval_ms` window containing `time_ms` — the engine's
/// bucket law ([`time_bucket_start`]), so a fold and the live builder agree on
/// every boundary.
///
/// Floor-divided, so a negative timestamp (a fixture before 1970, never a real
/// market) lands in the window below it rather than rounding toward zero into
/// the wrong bucket.
#[must_use]
pub fn bucket_start(time_ms: i64, interval_ms: i64) -> i64 {
    time_bucket_start(time_ms, interval_ms)
}

/// Put an older slice of venue candles in front of the ones already held,
/// keeping the base ascending by `open_time` and free of duplicates.
///
/// The fast path is the one progressive loading actually produces: the slice
/// is strictly older than everything held, so it is spliced in front and the
/// order is already right. The merge below exists for the case the port
/// permits but no provider aims for — a window that overlaps what is held,
/// through a venue re-reporting a bucket at a boundary. There the candle
/// already on screen wins: it is the one the trader has been reading, and a
/// bar that redraws itself for no visible reason is worse than a bar fetched
/// a second apart from an identical twin.
pub fn merge_older_candles(base: &mut Vec<Bar>, older: Vec<Bar>) {
    let disjoint = match (older.last(), base.first()) {
        (Some(newest_incoming), Some(oldest_held)) => {
            newest_incoming.open_time < oldest_held.open_time
        }
        _ => true,
    };
    if disjoint {
        base.splice(0..0, older);
        return;
    }
    let mut merged: std::collections::BTreeMap<i64, Bar> =
        older.into_iter().map(|bar| (bar.open_time, bar)).collect();
    for bar in base.drain(..) {
        merged.insert(bar.open_time, bar);
    }
    *base = merged.into_values().collect();
}

/// Drop venue bars that overlap the trade-derived series.
///
/// The two series meet at a seam, and the composed chart is only searchable if
/// `open_time` never decreases across it. A venue candle covering the same
/// window as the first engine bar would sit *after* it in time while sitting
/// before it in slot order, so every venue bucket from that one on is dropped:
/// what the app cut from prints is the better record of that window anyway.
///
/// With no engine bars yet the whole prefix stands — there is nothing to
/// overlap.
pub fn trim_to_seam(
    mut folded: Vec<Bar>,
    first_engine_bar: Option<&Bar>,
    partial: Option<&Bar>,
    interval_ms: i64,
) -> Vec<Bar> {
    let Some(seam) = seam_bucket_ms(first_engine_bar, partial, interval_ms) else {
        return folded;
    };
    folded.retain(|bar| before_seam(bar, seam, interval_ms));
    folded
}

/// The same trim over a block the caller only has on loan.
///
/// Two functions rather than one taking a `Cow`, because they pay for
/// different things and both paths are on the diet. The owning one above trims
/// a vector [`fold`] just built and is about to drop — a `retain` there
/// copies nothing at all. This one is handed the venue's whole base, which the
/// tab keeps, so it copies out only the bars that survive rather than cloning a
/// week of minutes in order to throw most of them away.
pub fn trim_borrowed_to_seam(
    base: &[Bar],
    first_engine_bar: Option<&Bar>,
    partial: Option<&Bar>,
    interval_ms: i64,
) -> Vec<Bar> {
    let Some(seam) = seam_bucket_ms(first_engine_bar, partial, interval_ms) else {
        return base.to_vec();
    };
    base.iter()
        .filter(|bar| before_seam(bar, seam, interval_ms))
        .cloned()
        .collect()
}

/// Whether a venue candle sits wholly before the seam bucket — the one test
/// both trims apply. By the bucket the candle is placed in, not its stamp: a
/// server-day candle opening the evening before still covers the seam's own
/// day.
fn before_seam(bar: &Bar, seam: i64, interval_ms: i64) -> bool {
    bucket_start(placement_ms(bar), interval_ms) < seam
}

/// Where the venue's candles have to stop for the pane's own bars to begin, or
/// `None` when there are no bars yet and the whole block stands.
///
/// Buckets, not stamps. A venue candle's `open_time` is its bucket start; an
/// engine bar's is its *first trade*, which sits strictly inside the bucket.
/// Comparing the two raw would keep the venue candle covering the same window
/// and put a later-closing bar in an earlier slot.
///
/// One owner for the rule, so the two trims above cannot drift apart about
/// where the seam is.
fn seam_bucket_ms(
    first_engine_bar: Option<&Bar>,
    partial: Option<&Bar>,
    interval_ms: i64,
) -> Option<i64> {
    let first = first_engine_bar.or(partial)?;
    Some(bucket_start(first.open_time, interval_ms))
}

/// Whether the chart can reach further back for venue candles, and when it
/// cannot, why — the tab answers it from what it holds and asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OlderCandles {
    /// Another span can be asked for.
    Available,
    /// This feed publishes no candle history at all.
    FeedServesNone,
    /// Nothing on this chart is cut by time, so no venue candle was ever
    /// wanted — the prefix follows what a pane *shows*, not which pane it is.
    NoChartCutByTime,
    /// A request is out; the answer is what to wait for.
    Fetching,
    /// The opening span has not landed yet. There is nothing to reach back
    /// *from* until it does.
    NotArrivedYet,
    /// A reach-back came back complete with nothing older in it. That is the
    /// venue's record, or the provider's, and it is the one reason here that
    /// had to be learned by asking.
    RecordStartsHere,
}

impl OlderCandles {
    /// Whether the control is live.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// What to tell the trader hovering a control this state disabled.
    /// `None` when it is not disabled.
    #[must_use]
    pub const fn why_not(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::FeedServesNone => Some("this feed publishes no candle history"),
            Self::NoChartCutByTime => Some(
                "no chart here is cut by time, so there are no venue candles \
                 to extend — switch on the venue lead-in to put them in front \
                 of a chart cut by trades",
            ),
            Self::Fetching => Some("a request is already out; this is what it is fetching"),
            Self::NotArrivedYet => Some("the first span has not arrived yet"),
            Self::RecordStartsHere => Some("this is as far back as the venue's record goes"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OHLCV_BASE_INTERVAL_MS;
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
        assert!(
            is_foldable(OHLCV_BASE_INTERVAL_MS, 60_000),
            "the base interval folds to itself"
        );
        assert!(is_foldable(OHLCV_BASE_INTERVAL_MS, 300_000), "5m");
        assert!(is_foldable(OHLCV_BASE_INTERVAL_MS, 3_600_000), "1h");
        assert!(
            !is_foldable(OHLCV_BASE_INTERVAL_MS, 90_000),
            "90s is not a whole number of minutes"
        );
        assert!(
            !is_foldable(OHLCV_BASE_INTERVAL_MS, 1_000),
            "and nothing below the base folds"
        );
        assert!(!is_foldable(OHLCV_BASE_INTERVAL_MS, 0));
        assert!(!is_foldable(OHLCV_BASE_INTERVAL_MS, -60_000));

        assert!(
            fold(&[candle(0, 0)], OHLCV_BASE_INTERVAL_MS, 90_000).is_empty(),
            "an interval that cannot be folded to gets no bars, not approximate ones"
        );
    }

    #[test]
    fn five_minutes_takes_first_open_last_close_and_the_extremes() {
        // Minutes 0..5 into one bucket, with the extremes in the middle.
        let base: Vec<Bar> = (0..5).map(|m| candle(m, m)).collect();

        let folded = fold(&base, OHLCV_BASE_INTERVAL_MS, 5 * OHLCV_BASE_INTERVAL_MS);

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

        let folded = fold(&base, OHLCV_BASE_INTERVAL_MS, 5 * OHLCV_BASE_INTERVAL_MS);

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

        let folded = fold(&base, OHLCV_BASE_INTERVAL_MS, 5 * OHLCV_BASE_INTERVAL_MS);

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
        assert_eq!(
            fold(&base, OHLCV_BASE_INTERVAL_MS, OHLCV_BASE_INTERVAL_MS),
            base
        );
        assert!(fold(&[], OHLCV_BASE_INTERVAL_MS, 5 * OHLCV_BASE_INTERVAL_MS).is_empty());
    }

    #[test]
    fn the_fold_is_deterministic() {
        let base: Vec<Bar> = (0..37).map(|m| candle(m, m % 7)).collect();
        let once = fold(&base, OHLCV_BASE_INTERVAL_MS, 15 * OHLCV_BASE_INTERVAL_MS);
        let twice = fold(&base, OHLCV_BASE_INTERVAL_MS, 15 * OHLCV_BASE_INTERVAL_MS);
        assert_eq!(once, twice, "same bars in, same bars out");
    }

    use quantick_engine::time_bucket::CALENDAR_MONTH_MS;

    /// 2024-01-29T00:00:00Z, a Monday.
    const MON_29_JAN_2024: i64 = 1_706_486_400_000;

    /// One daily candle `day` days after Monday 2024-01-29, opening `shift_ms`
    /// away from 00:00 UTC.
    fn daily(day: i64, shift_ms: i64) -> Bar {
        let open_time = MON_29_JAN_2024 + day * DAY_MS + shift_ms;
        Bar {
            open_time,
            close_time: open_time + DAY_MS - 1,
            ..candle(0, day)
        }
    }

    #[test]
    fn a_daily_base_folds_to_days_weeks_and_months_but_nothing_shorter() {
        assert!(is_foldable(DAY_MS, DAY_MS));
        assert!(is_foldable(DAY_MS, 2 * DAY_MS));
        assert!(is_foldable(DAY_MS, WEEK_MS));
        assert!(is_foldable(DAY_MS, CALENDAR_MONTH_MS));
        assert!(!is_foldable(DAY_MS, 3_600_000), "a day holds no hour");
        assert!(is_foldable(OHLCV_BASE_INTERVAL_MS, CALENDAR_MONTH_MS));
        assert!(is_foldable(OHLCV_BASE_INTERVAL_MS, WEEK_MS));
        assert!(
            !is_foldable(5 * 3_600_000, WEEK_MS),
            "five-hour candles do not tile the Monday a week opens on"
        );
        assert!(!is_foldable(0, DAY_MS));
    }

    #[test]
    fn daily_candles_fold_to_monday_weeks_and_calendar_months() {
        // Monday 29 January to Sunday 3 March 2024: five whole weeks.
        let base: Vec<Bar> = (0..35).map(|day| daily(day, 0)).collect();

        let weeks = fold(&base, DAY_MS, WEEK_MS);
        assert_eq!(weeks.len(), 5);
        for (index, week) in weeks.iter().enumerate() {
            let monday = MON_29_JAN_2024 + i64::try_from(index).unwrap() * WEEK_MS;
            assert_eq!(week.open_time, monday, "week {index} opens on its Monday");
            assert_eq!(week.close_time, monday + WEEK_MS - 1);
        }

        let months = fold(&base, DAY_MS, CALENDAR_MONTH_MS);
        let days_in: Vec<i64> = months
            .iter()
            .map(|month| (month.close_time + 1 - month.open_time) / DAY_MS)
            .collect();
        assert_eq!(
            days_in,
            [31, 29, 31],
            "each month stamped whole, a leap February among them"
        );
        assert!(
            starts_inside_bucket(&base, DAY_MS, CALENDAR_MONTH_MS),
            "and the record held only January's last three days"
        );
        assert_eq!(
            months[1].open_time,
            MON_29_JAN_2024 + 3 * DAY_MS,
            "1 February"
        );
    }

    /// A MetaTrader D1 candle is cut at the server's midnight. On a server two
    /// hours ahead of UTC, Monday's candle opens at 22:00 on Sunday — and
    /// still belongs to Monday's week, where most of it lies.
    #[test]
    fn a_server_day_candle_lands_in_the_utc_day_holding_most_of_it() {
        let two_hours = 2 * 3_600_000;
        let base: Vec<Bar> = (6..9).map(|day| daily(day, -two_hours)).collect();

        let weeks = fold(&base, DAY_MS, WEEK_MS);
        assert_eq!(weeks.len(), 2, "Sunday's candle, then Monday and Tuesday's");
        assert_eq!(
            bucket_start(placement_ms(&weeks[1]), WEEK_MS),
            MON_29_JAN_2024 + WEEK_MS
        );

        // The pane's own bars begin on Tuesday: Monday's server-day candle
        // stays, Tuesday's goes, though both open the evening before.
        let first_engine = Bar {
            open_time: MON_29_JAN_2024 + 8 * DAY_MS + 3_600_000,
            close_time: MON_29_JAN_2024 + 8 * DAY_MS + 3_600_000,
            ..candle(0, 0)
        };
        let days = trim_to_seam(
            fold(&base, DAY_MS, DAY_MS),
            Some(&first_engine),
            None,
            DAY_MS,
        );
        assert_eq!(days.len(), 2);
        assert_eq!(
            days[1].open_time,
            MON_29_JAN_2024 + 7 * DAY_MS,
            "stamped inside the UTC day it was placed in"
        );
    }

    /// A server-day candle folds to a bar stamped inside the UTC bucket it was
    /// placed in, east of UTC and west of it, so a slot lookup or the
    /// crosshair finds it on the same day an intraday pane draws.
    #[test]
    fn a_folded_server_day_candle_is_stamped_inside_its_utc_bucket() {
        let hours = 3_600_000;
        for offset in [-3 * hours, 5 * hours] {
            let base: Vec<Bar> = (6..9).map(|day| daily(day, offset)).collect();
            for interval in [DAY_MS, WEEK_MS, CALENDAR_MONTH_MS] {
                for bar in fold(&base, DAY_MS, interval) {
                    let bucket = bucket_start(placement_ms(&bar), interval);
                    let end = quantick_engine::time_bucket::time_bucket_end(bucket, interval);
                    assert!(
                        bucket <= bar.open_time && bar.open_time <= bar.close_time,
                        "{offset} {interval}: {bar:?}"
                    );
                    assert!(bar.close_time < end, "{offset} {interval}: {bar:?}");
                    assert_eq!(bucket_start(bar.open_time, interval), bucket);
                }
            }
            let days = fold(&base, DAY_MS, DAY_MS);
            assert_eq!(days.len(), 3, "one bar per server day");
            for (day, bar) in (6..9).zip(&days) {
                let midnight = MON_29_JAN_2024 + day * DAY_MS;
                assert_eq!(
                    (bar.open_time, bar.close_time),
                    (midnight, midnight + DAY_MS - 1),
                    "{offset}: the UTC day, not the server's"
                );
            }
            assert_eq!(
                days.iter()
                    .map(|bar| bucket_start(bar.open_time, DAY_MS))
                    .collect::<Vec<_>>(),
                (6..9)
                    .map(|day| MON_29_JAN_2024 + day * DAY_MS)
                    .collect::<Vec<_>>(),
                "{offset}"
            );
        }
    }

    /// MetaTrader on a server three hours behind UTC: its days open at 03:00
    /// UTC, and the month's first trading day is the 5th. The folded day,
    /// week and month open on their UTC bucket and close on its last
    /// millisecond, whatever the server's offset.
    #[test]
    fn bars_folded_from_server_days_are_stamped_on_their_utc_bucket() {
        let three = 3 * 3_600_000;
        let base: Vec<Bar> = (7..19).map(|day| daily(day, three)).collect();
        let week = &fold(&base, DAY_MS, WEEK_MS)[0];
        let monday = MON_29_JAN_2024 + WEEK_MS;
        assert_eq!(
            (week.open_time, week.close_time),
            (monday, monday + WEEK_MS - 1)
        );
        let month = &fold(&base, DAY_MS, CALENDAR_MONTH_MS)[0];
        let first_of_february = MON_29_JAN_2024 + 3 * DAY_MS;
        assert_eq!(month.open_time, first_of_february, "not the 5th at 03:00");
        assert_eq!(
            month.close_time,
            first_of_february + 29 * DAY_MS - 1,
            "29 February's last millisecond"
        );
        let minutes: Vec<Bar> = (3..9).map(|minute| candle(minute, 0)).collect();
        assert_eq!(
            fold(&minutes, OHLCV_BASE_INTERVAL_MS, 5 * 60_000)[0].open_time,
            3 * 60_000,
            "minutes keep the first traded minute"
        );
    }

    /// The record's start cuts into its first bucket: a five-year daily
    /// answer opening on a Friday, a minute base opening at 07:45.
    #[test]
    fn a_record_that_starts_inside_its_first_bucket_is_named() {
        let from_friday: Vec<Bar> = (4..20).map(|day| daily(day, 0)).collect();
        assert!(starts_inside_bucket(&from_friday, DAY_MS, WEEK_MS));
        assert!(!starts_inside_bucket(&from_friday, DAY_MS, DAY_MS));
        let from_monday: Vec<Bar> = (0..20).map(|day| daily(day, 0)).collect();
        assert!(!starts_inside_bucket(&from_monday, DAY_MS, WEEK_MS));
        assert!(starts_inside_bucket(
            &from_monday,
            DAY_MS,
            CALENDAR_MONTH_MS
        ));
        let from_07_45: Vec<Bar> = (465..500).map(|minute| candle(minute, 0)).collect();
        assert!(starts_inside_bucket(
            &from_07_45,
            OHLCV_BASE_INTERVAL_MS,
            DAY_MS
        ));
        assert!(!starts_inside_bucket(
            &from_07_45,
            OHLCV_BASE_INTERVAL_MS,
            5 * 60_000
        ));
        assert!(!starts_inside_bucket(&[], DAY_MS, WEEK_MS));
    }

    /// Both trims place a candle by the same rule, so the unfolded base and
    /// the folded one stop at the same seam.
    #[test]
    fn both_trims_cut_a_server_day_candle_at_the_same_seam() {
        let base: Vec<Bar> = (6..9).map(|day| daily(day, -2 * 3_600_000)).collect();
        let first_engine = Bar {
            open_time: MON_29_JAN_2024 + 8 * DAY_MS + 3_600_000,
            close_time: MON_29_JAN_2024 + 8 * DAY_MS + 3_600_000,
            ..candle(0, 0)
        };
        let borrowed = trim_borrowed_to_seam(&base, Some(&first_engine), None, DAY_MS);
        let owned = trim_to_seam(
            fold(&base, DAY_MS, DAY_MS),
            Some(&first_engine),
            None,
            DAY_MS,
        );
        assert_eq!(
            borrowed.len(),
            2,
            "Tuesday's candle opens on Monday evening"
        );
        assert_eq!(fold(&borrowed, DAY_MS, DAY_MS), owned);
    }

    /// The seam rule: history folded from venue candles and bars the engine
    /// cuts from trades at the same instants fall into the same buckets, with
    /// the same prices, for a day, a week and a calendar month.
    #[test]
    fn the_fold_and_the_live_builder_agree_on_every_calendar_bucket() {
        use quantick_engine::{BarBuilder, Side, TimeBarBuilder, Trade};
        // Every six hours from 25 January to 5 March 2024.
        let start = MON_29_JAN_2024 - 4 * DAY_MS;
        let minutes: Vec<i64> = (0..160).map(|step| start + step * 6 * 3_600_000).collect();
        for interval in [DAY_MS, WEEK_MS, CALENDAR_MONTH_MS] {
            let base: Vec<Bar> = minutes
                .iter()
                .enumerate()
                .map(|(seed, &open_time)| Bar {
                    open_time,
                    close_time: open_time + OHLCV_BASE_INTERVAL_MS - 1,
                    ..candle(0, i64::try_from(seed).unwrap())
                })
                .collect();
            let folded = fold(&base, OHLCV_BASE_INTERVAL_MS, interval);

            let mut builder = TimeBarBuilder::new(interval);
            let mut built = Vec::new();
            for bar in &base {
                for price in [bar.open, bar.close] {
                    let trade = Trade {
                        agg_id: 0,
                        timestamp_ms: bar.open_time + 1_000,
                        price,
                        quantity: Decimal::ONE,
                        side: Side::Buy,
                    };
                    built.extend(builder.push(&trade));
                }
            }
            built.extend(builder.partial().cloned());

            let buckets = |bars: &[Bar]| -> Vec<i64> {
                bars.iter()
                    .map(|bar| bucket_start(bar.open_time, interval))
                    .collect()
            };
            assert_eq!(buckets(&folded), buckets(&built), "{interval}");
            for (venue, live) in folded.iter().zip(&built) {
                assert_eq!(
                    (venue.open, venue.close),
                    (live.open, live.close),
                    "{interval}"
                );
            }
        }
    }
}
