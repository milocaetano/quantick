//! The venue's record of the seam bucket before the chart's first trade.
//!
//! [`super::trim_to_seam`] drops every venue candle in the bucket the pane's
//! first engine bar opens in, because that bar covers the same window. But
//! that bar opens on the first trade the app retained, not on its bucket's
//! edge: on a daily chart opened at 06:14 the forming day holds six hours
//! less than the day did, and a week or a month misses whole days. The lead
//! is what the venue says about that missing stretch — every candle placed in
//! the seam bucket that ended before the first trade — folded into one bar
//! the chart puts in front of its first one.
//!
//! Three rules keep it honest. A candle that reaches the first trade is left
//! out, never split: its volume and extremes overlap prints the chart holds,
//! and counting them twice is worse than missing the seconds before the first
//! trade — but those seconds are then missing, and the lead says so
//! ([`SeamLead::covers_seam`]). A stretch the candles held do not reach is no
//! lead at all, never a partial one quietly presented as the venue's record:
//! the absence comes back named, so the caller can fetch what is missing. And
//! a minute with no candle is a minute nothing traded only where a complete
//! answer for that stretch says so ([`MinutesAnswer::Covers`]); an answer
//! known to be short vouches for nothing it lacks.
//!
//! A daily base cannot say anything about the seam *day*, which the chart's
//! own trades overlap; that part comes from minutes — whole days before the
//! seam day from the daily base, then minutes from where the last day before
//! the seam day ended up to the first trade. A server-day candle ends a few
//! hours either side of 00:00 UTC, so that hand-over is never assumed to be
//! midnight: west of UTC the day before already holds the seam day's first
//! hours, east of it the seam day's minutes start the evening before.
//!
//! Pure and deterministic, like the rest of [`super`].

use quantick_engine::Bar;
use quantick_engine::time_bucket::{DAY_MS, TimeBucketLaw, time_bucket_start};

use super::placement_ms;
use crate::OHLCV_BASE_INTERVAL_MS;

/// What the venue's candles say about the seam bucket before the first trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeamLead {
    /// The candles placed in the seam bucket that ended before the first
    /// trade, folded into one bar stamped on the bucket's start.
    Lead {
        /// The lead.
        bar: Bar,
        /// Whether the lead and the prints together hold the whole bucket up
        /// to the first trade: false when the part of the first trade's minute
        /// before it is left out, or the minutes came from an answer known to
        /// be short.
        whole: bool,
    },
    /// Nothing to put in front, and nothing missing: the bucket opens on the
    /// first trade, or the days before it reach the trade.
    Nothing,
    /// Nothing to put in front, but the stretch from `from_ms` to the first
    /// trade lies inside the minute the trade is in, which no candle can
    /// speak for without overlapping it.
    Unrecorded {
        /// Where the stretch no candle speaks for starts.
        from_ms: i64,
    },
    /// The stretch from `from_ms` to the first trade needs minutes, and no
    /// minute candles are held.
    MinutesNotHeld {
        /// Where the minutes would have to start.
        from_ms: i64,
    },
    /// The minutes held do not cover the stretch from `from_ms` to the first
    /// trade: a lead from them would be partial, so there is none. Minutes
    /// that do can be asked for.
    Uncovered {
        /// Where the stretch the minutes do not cover starts.
        from_ms: i64,
    },
    /// The daily candles held do not cover the whole days before the seam
    /// day, or one of them reaches past the first trade: minutes cannot mend
    /// that, so there is no lead.
    DaysDoNotCover {
        /// Where the stretch the days do not cover starts.
        from_ms: i64,
    },
}

impl SeamLead {
    /// The lead bar, when there is one.
    #[must_use]
    pub fn into_bar(self) -> Option<Bar> {
        match self {
            Self::Lead { bar, .. } => Some(bar),
            _ => None,
        }
    }

    /// Whether the seam bar, with this lead in front, holds its whole bucket
    /// up to the first trade. False is a bar that knowingly omits a stretch,
    /// to be labelled partial.
    #[must_use]
    pub const fn covers_seam(&self) -> bool {
        matches!(self, Self::Nothing | Self::Lead { whole: true, .. })
    }

    /// Where the minutes this lead lacks start, when minutes could mend it.
    #[must_use]
    pub const fn wants_minutes_from(&self) -> Option<i64> {
        match self {
            Self::MinutesNotHeld { from_ms } | Self::Uncovered { from_ms } => Some(*from_ms),
            _ => None,
        }
    }

    /// A stable token for the structured log.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::Lead { whole: true, .. } => "venue_lead",
            Self::Lead { whole: false, .. } => "venue_lead_partly_covered",
            Self::Nothing => "nothing_before_first_trade",
            Self::Unrecorded { .. } => "inside_the_first_trade_s_minute",
            Self::MinutesNotHeld { .. } => "minutes_not_held",
            Self::Uncovered { .. } => "candles_do_not_cover",
            Self::DaysDoNotCover { .. } => "days_do_not_cover",
        }
    }
}

/// What the answer that brought a run of minute candles can vouch for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinutesAnswer {
    /// Nothing beyond the candles: they speak for what they reach, end to
    /// end.
    Candles,
    /// A complete answer for `from_ms..=until_ms`: a minute it lacks there is
    /// a minute nothing traded.
    Covers {
        /// The oldest millisecond the answer covers.
        from_ms: i64,
        /// The newest millisecond the answer covers, inclusive.
        until_ms: i64,
    },
    /// An answer known to be short: what it holds is real, but a hole in it
    /// may be a minute the venue did not send.
    Short,
}

/// Minute candles, ascending by `open_time`, and what their answer vouches
/// for.
#[derive(Debug, Clone, Copy)]
pub struct Minutes<'a> {
    /// The candles.
    pub candles: &'a [Bar],
    /// What the answer that brought them can vouch for.
    pub answer: MinutesAnswer,
}

impl<'a> Minutes<'a> {
    /// Candles whose answer vouches for nothing beyond themselves.
    #[must_use]
    pub const fn candles(candles: &'a [Bar]) -> Self {
        Self {
            candles,
            answer: MinutesAnswer::Candles,
        }
    }

    /// Whether these minutes speak for `from..to`: a complete answer for the
    /// stretch does, holes and all; otherwise the candles have to reach both
    /// ends.
    fn speak_for(&self, from: i64, to: i64) -> bool {
        match self.answer {
            MinutesAnswer::Covers { from_ms, until_ms } if from_ms <= from && until_ms >= to => {
                true
            }
            _ => reaches(self.candles, OHLCV_BASE_INTERVAL_MS, from, to),
        }
    }
}

/// The lead of the `interval_ms` bucket holding `first_trade_ms`, from
/// `days` (daily candles, placed by [`placement_ms`]) and `minutes`, each
/// ascending by `open_time`.
///
/// Without `days` the whole stretch comes from minutes — the case of every
/// intraday pane, and of a day pane folding minutes. With them, whole days
/// before the seam day come from the daily base and minutes cover the rest,
/// from where the last day before the seam day ended.
#[must_use]
pub fn seam_lead(
    days: Option<&[Bar]>,
    minutes: Option<Minutes<'_>>,
    first_trade_ms: i64,
    interval_ms: i64,
) -> SeamLead {
    let Some(law) = TimeBucketLaw::of(interval_ms) else {
        return SeamLead::Nothing;
    };
    let bucket = law.start(first_trade_ms);
    let mut lead: Option<Bar> = None;
    let mut minutes_from = bucket;
    if let Some(days) = days {
        match days_before_the_seam_day(days, bucket, first_trade_ms) {
            Ok((whole_days, from)) => {
                for day in whole_days {
                    absorb(&mut lead, day);
                }
                minutes_from = from;
            }
            Err(from_ms) => return SeamLead::DaysDoNotCover { from_ms },
        }
    }
    let first_minute = time_bucket_start(first_trade_ms, OHLCV_BASE_INTERVAL_MS);
    // What no candle can speak for: the first trade's own minute before it.
    let unrecorded_from = first_minute.max(minutes_from);
    let mut short = false;
    if minutes_from < first_minute {
        let Some(minutes) = minutes else {
            return SeamLead::MinutesNotHeld {
                from_ms: minutes_from,
            };
        };
        if !minutes.speak_for(minutes_from, first_minute) {
            return SeamLead::Uncovered {
                from_ms: minutes_from,
            };
        }
        short = minutes.answer == MinutesAnswer::Short;
        let candles = minutes.candles;
        let start = candles.partition_point(|minute| minute.open_time < minutes_from);
        for minute in candles[start..]
            .iter()
            .take_while(|minute| minute.close_time < first_trade_ms)
        {
            absorb(&mut lead, minute);
        }
    }
    let omitted = unrecorded_from < first_trade_ms;
    match lead {
        Some(mut bar) => {
            // Stamped on the bucket's start, as every candle-sourced bar is,
            // and before the first trade: a server-day candle opening the
            // evening before is the bucket's all the same.
            bar.open_time = bucket;
            bar.close_time = bar
                .close_time
                .min(first_trade_ms.saturating_sub(1))
                .max(bar.open_time);
            SeamLead::Lead {
                bar,
                whole: !omitted && !short,
            }
        }
        None if omitted => SeamLead::Unrecorded {
            from_ms: unrecorded_from,
        },
        None => SeamLead::Nothing,
    }
}

/// The whole days of the bucket before the seam day, and where the minutes
/// take over: right after the last day held before the seam day ends, which
/// for a server-day candle is not midnight UTC. `Err` names where the days
/// fail to cover the bucket, or reach the first trade.
fn days_before_the_seam_day(
    days: &[Bar],
    bucket: i64,
    first_trade_ms: i64,
) -> Result<(&[Bar], i64), i64> {
    let seam_day = time_bucket_start(first_trade_ms, DAY_MS);
    if seam_day > bucket && !reaches(days, DAY_MS, bucket, seam_day) {
        return Err(bucket);
    }
    let whole_days = placed_in(days, bucket, seam_day);
    if whole_days
        .iter()
        .any(|day| day.close_time >= first_trade_ms)
    {
        return Err(bucket);
    }
    // The day the minutes follow: the last whole day of the bucket, or — on a
    // daily pane, whose bucket is the seam day — the day before it, which a
    // fold put in the bar before this one.
    let handed_over = whole_days
        .last()
        .or_else(|| placed_in(days, seam_day - DAY_MS, seam_day).last());
    let from = handed_over.map_or(seam_day.max(bucket), |day| day.close_time.saturating_add(1));
    Ok((whole_days, from))
}

fn absorb(lead: &mut Option<Bar>, candle: &Bar) {
    match lead {
        Some(bar) => bar.absorb(candle),
        None => *lead = Some(candle.clone()),
    }
}

/// The candles placed in `[from, to)`.
fn placed_in(candles: &[Bar], from: i64, to: i64) -> &[Bar] {
    let start = candles.partition_point(|bar| placement_ms(bar) < from);
    let end = candles.partition_point(|bar| placement_ms(bar) < to);
    &candles[start..end.max(start)]
}

/// Whether `candles`, each `unit_ms` long, reach back to `from` and forward
/// to `to`: the first is placed in a `unit_ms` window at or before `from`,
/// and the last in the window ending at `to` or later. A base fetched before
/// the stretch ended, or one that starts inside it, cannot speak for it.
fn reaches(candles: &[Bar], unit_ms: i64, from: i64, to: i64) -> bool {
    let window = |bar: &Bar| time_bucket_start(placement_ms(bar), unit_ms);
    match (candles.first(), candles.last()) {
        (Some(first), Some(last)) => {
            window(first) <= from && window(last).saturating_add(unit_ms) >= to
        }
        _ => false,
    }
}

#[cfg(test)]
mod seam_lead_tests {
    use super::*;
    use quantick_engine::time_bucket::{CALENDAR_MONTH_MS, WEEK_MS};
    use rust_decimal::Decimal;

    const MINUTE: i64 = OHLCV_BASE_INTERVAL_MS;
    const HOUR: i64 = 60 * MINUTE;
    /// Wednesday 2026-10-07T00:00:00Z.
    const WED_7_OCT_2026: i64 = 1_791_331_200_000;
    /// The first trade the chart retained: 06:14:03 that Wednesday.
    const FIRST_TRADE: i64 = WED_7_OCT_2026 + 6 * HOUR + 14 * MINUTE + 3_000;

    /// A candle `length_ms` long opening at `open_time`, its prices and
    /// volumes derived from `seed` so a merge is visible.
    fn candle(open_time: i64, length_ms: i64, seed: i64) -> Bar {
        Bar {
            open_time,
            close_time: open_time + length_ms - 1,
            open: Decimal::from(100 + seed),
            high: Decimal::from(110 + seed),
            low: Decimal::from(90 - seed),
            close: Decimal::from(105 + seed),
            buy_volume: Decimal::from(2),
            sell_volume: Decimal::from(3),
            trade_count: 7,
        }
    }

    /// Minute candles from `from` (inclusive) to `to` (exclusive).
    fn minutes(from: i64, to: i64) -> Vec<Bar> {
        (0..(to - from) / MINUTE)
            .map(|index| candle(from + index * MINUTE, MINUTE, index % 9))
            .collect()
    }

    /// Daily candles for the `count` days before `until`, the last of them
    /// being `until`'s own day when `with_today`.
    fn days(until: i64, count: i64, with_today: bool) -> Vec<Bar> {
        let end = if with_today { 1 } else { 0 };
        (-count..end)
            .map(|day| candle(until + day * DAY_MS, DAY_MS, day.rem_euclid(9)))
            .collect()
    }

    /// A lead missing the first trade's minute before the trade.
    fn partial_lead(bar: Bar) -> SeamLead {
        SeamLead::Lead { bar, whole: false }
    }

    fn summary(candles: &[Bar]) -> Bar {
        let mut lead = candles[0].clone();
        for candle in &candles[1..] {
            lead.absorb(candle);
        }
        lead
    }

    /// The 5m seam of the BTC launch: the venue's 06:10 to 06:13 minutes go
    /// in front of the bar the first trade opened, and 06:14 — which the
    /// trade sits inside — stays out.
    #[test]
    fn an_intraday_lead_holds_the_bucket_s_minutes_before_the_first_trade() {
        let held = minutes(WED_7_OCT_2026, FIRST_TRADE + HOUR);
        let lead = seam_lead(None, Some(Minutes::candles(&held)), FIRST_TRADE, 5 * MINUTE);
        let bucket = WED_7_OCT_2026 + 6 * HOUR + 10 * MINUTE;
        let expected: Vec<Bar> = held
            .iter()
            .filter(|bar| bar.open_time >= bucket && bar.close_time < FIRST_TRADE)
            .cloned()
            .collect();
        assert_eq!(expected.len(), 4, "06:10, 06:11, 06:12, 06:13");
        assert_eq!(lead, partial_lead(summary(&expected)));
        let bar = lead.into_bar().unwrap();
        assert_eq!(bar.open_time, bucket, "the bar opens on its bucket");
        assert!(bar.close_time < FIRST_TRADE, "and ends before the trade");
    }

    /// A one-minute bucket's only candle is the one the trade sits inside:
    /// nothing goes in front, and the seconds before the trade are named.
    #[test]
    fn a_candle_reaching_the_first_trade_is_never_counted() {
        let held = minutes(WED_7_OCT_2026, FIRST_TRADE + HOUR);
        let unrecorded = seam_lead(None, Some(Minutes::candles(&held)), FIRST_TRADE, MINUTE);
        assert_eq!(
            unrecorded,
            SeamLead::Unrecorded {
                from_ms: FIRST_TRADE - 3_000
            }
        );
        assert!(!unrecorded.covers_seam(), "the bar misses three seconds");
        let on_the_edge = WED_7_OCT_2026 + 6 * HOUR;
        assert_eq!(
            seam_lead(None, Some(Minutes::candles(&held)), on_the_edge, HOUR),
            SeamLead::Nothing,
            "a bucket opening on the first trade has nothing before it"
        );
    }

    /// A base that starts inside the stretch, or ends before the trade,
    /// cannot speak for it: no lead, and the stretch it misses is named.
    #[test]
    fn candles_that_do_not_cover_the_stretch_are_no_lead() {
        let late_start = minutes(WED_7_OCT_2026 + 6 * HOUR + 12 * MINUTE, FIRST_TRADE + HOUR);
        assert_eq!(
            seam_lead(
                None,
                Some(Minutes::candles(&late_start)),
                FIRST_TRADE,
                5 * MINUTE
            ),
            SeamLead::Uncovered {
                from_ms: WED_7_OCT_2026 + 6 * HOUR + 10 * MINUTE
            }
        );
        let early_end = minutes(WED_7_OCT_2026, WED_7_OCT_2026 + 6 * HOUR + 12 * MINUTE);
        assert!(matches!(
            seam_lead(
                None,
                Some(Minutes::candles(&early_end)),
                FIRST_TRADE,
                5 * MINUTE
            ),
            SeamLead::Uncovered { .. }
        ));
        assert!(matches!(
            seam_lead(None, Some(Minutes::candles(&[])), FIRST_TRADE, 5 * MINUTE),
            SeamLead::Uncovered { .. }
        ));
    }

    /// The forming day: the daily base cannot speak for the seam day, so the
    /// whole lead is that day's minutes, from 00:00 to the first trade.
    #[test]
    fn a_daily_lead_takes_the_seam_day_from_minutes() {
        let daily = days(WED_7_OCT_2026, 30, true);
        let held = minutes(WED_7_OCT_2026 - 2 * DAY_MS, FIRST_TRADE + HOUR);
        assert_eq!(
            seam_lead(Some(&daily), None, FIRST_TRADE, DAY_MS),
            SeamLead::MinutesNotHeld {
                from_ms: WED_7_OCT_2026
            },
            "the day's own candle overlaps the trades"
        );
        let lead = seam_lead(
            Some(&daily),
            Some(Minutes::candles(&held)),
            FIRST_TRADE,
            DAY_MS,
        );
        let today: Vec<Bar> = held
            .iter()
            .filter(|bar| bar.open_time >= WED_7_OCT_2026 && bar.close_time < FIRST_TRADE)
            .cloned()
            .collect();
        assert_eq!(today.len(), 6 * 60 + 14);
        assert_eq!(lead, partial_lead(summary(&today)));
    }

    /// The forming week and month: whole days from the daily base, then the
    /// seam day's minutes — Monday and Tuesday, then Wednesday to 06:14; the
    /// first to the sixth of October, then the seventh.
    #[test]
    fn weekly_and_monthly_leads_join_whole_days_to_the_seam_day_s_minutes() {
        let daily = days(WED_7_OCT_2026, 60, true);
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let today: Vec<Bar> = held
            .iter()
            .filter(|bar| bar.open_time >= WED_7_OCT_2026 && bar.close_time < FIRST_TRADE)
            .cloned()
            .collect();
        for (interval, whole_days) in [(WEEK_MS, 2), (CALENDAR_MONTH_MS, 6)] {
            let from = WED_7_OCT_2026 - whole_days * DAY_MS;
            let mut parts: Vec<Bar> = daily
                .iter()
                .filter(|bar| bar.open_time >= from && bar.open_time < WED_7_OCT_2026)
                .cloned()
                .collect();
            assert_eq!(parts.len() as i64, whole_days, "{interval}");
            parts.extend(today.iter().cloned());
            let lead = seam_lead(
                Some(&daily),
                Some(Minutes::candles(&held)),
                FIRST_TRADE,
                interval,
            );
            assert_eq!(lead, partial_lead(summary(&parts)), "{interval}");
            assert_eq!(lead.into_bar().unwrap().open_time, from, "{interval}");
        }
    }

    /// Server-day candles: east of UTC the last whole day ends before
    /// midnight and the minutes start there; west of it a day reaching past
    /// the first trade overlaps it, and the lead is refused rather than
    /// counting prints twice.
    #[test]
    fn server_day_candles_hand_over_to_minutes_where_they_end() {
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let east: Vec<Bar> = days(WED_7_OCT_2026, 10, true)
            .into_iter()
            .map(|bar| Bar {
                open_time: bar.open_time - 3 * HOUR,
                close_time: bar.close_time - 3 * HOUR,
                ..bar
            })
            .collect();
        let lead = seam_lead(
            Some(&east),
            Some(Minutes::candles(&held)),
            FIRST_TRADE,
            WEEK_MS,
        )
        .into_bar()
        .expect("a lead");
        let monday = WED_7_OCT_2026 - 2 * DAY_MS;
        assert_eq!(lead.open_time, monday, "clamped into the week");
        let minutes_in: u64 = held
            .iter()
            .filter(|bar| {
                bar.open_time >= WED_7_OCT_2026 - 3 * HOUR && bar.close_time < FIRST_TRADE
            })
            .map(|bar| bar.trade_count)
            .sum();
        assert_eq!(lead.trade_count, 2 * 7 + minutes_in, "no hour twice");

        let west: Vec<Bar> = days(WED_7_OCT_2026, 10, true)
            .into_iter()
            .map(|bar| Bar {
                open_time: bar.open_time + 8 * HOUR,
                close_time: bar.close_time + 8 * HOUR,
                ..bar
            })
            .collect();
        assert_eq!(
            seam_lead(
                Some(&west),
                Some(Minutes::candles(&held)),
                FIRST_TRADE,
                WEEK_MS
            ),
            SeamLead::DaysDoNotCover { from_ms: monday }
        );
    }

    /// A daily base that has not reached the seam day was fetched before it:
    /// its last candle is a day still forming, not a whole one.
    #[test]
    fn a_daily_base_that_stops_short_of_the_seam_day_is_no_lead() {
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let stale = days(WED_7_OCT_2026 - DAY_MS, 30, false);
        assert!(matches!(
            seam_lead(
                Some(&stale),
                Some(Minutes::candles(&held)),
                FIRST_TRADE,
                WEEK_MS
            ),
            SeamLead::DaysDoNotCover { .. }
        ));
    }

    /// A daily pane on server-day candles: the minutes start where the day
    /// before the seam day ended, never at midnight. West of UTC (05:00 UTC
    /// server midnight) the day before already holds 00:00 to 05:00, so the
    /// minutes start at 05:00; east of it (21:00) the seam day's minutes
    /// start the evening before, which no other bar holds.
    #[test]
    fn a_daily_lead_starts_where_the_day_before_ended() {
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let shifted = |hours: i64| -> Vec<Bar> {
            days(WED_7_OCT_2026, 10, true)
                .into_iter()
                .map(|bar| Bar {
                    open_time: bar.open_time + hours * HOUR,
                    close_time: bar.close_time + hours * HOUR,
                    ..bar
                })
                .collect()
        };
        let traded_from = |from: i64| -> u64 {
            held.iter()
                .filter(|bar| bar.open_time >= from && bar.close_time < FIRST_TRADE)
                .map(|bar| bar.trade_count)
                .sum()
        };
        for (hours, minutes_from) in [
            (5, WED_7_OCT_2026 + 5 * HOUR),
            (-3, WED_7_OCT_2026 - 3 * HOUR),
            (0, WED_7_OCT_2026),
        ] {
            let lead = seam_lead(
                Some(&shifted(hours)),
                Some(Minutes::candles(&held)),
                FIRST_TRADE,
                DAY_MS,
            )
            .into_bar()
            .expect("a lead");
            assert_eq!(lead.trade_count, traded_from(minutes_from), "{hours}h");
            assert_eq!(lead.open_time, WED_7_OCT_2026, "{hours}h: the bucket");
        }
    }

    /// B3 on MetaTrader, launched before the open: the minutes end on the
    /// previous session and the first trade comes at 12:00:36. Read from the
    /// candles alone the stretch is uncovered; a complete answer for it says
    /// nothing traded there, and the week's lead is its whole days.
    #[test]
    fn a_complete_answer_vouches_for_the_minutes_nothing_traded_in() {
        let first_trade = WED_7_OCT_2026 + 12 * HOUR + 36_000;
        let daily = days(WED_7_OCT_2026, 30, true);
        let previous_session = minutes(
            WED_7_OCT_2026 - DAY_MS + 13 * HOUR,
            WED_7_OCT_2026 - 3 * HOUR,
        );
        let alone = Minutes::candles(&previous_session);
        assert_eq!(
            seam_lead(Some(&daily), Some(alone), first_trade, WEEK_MS),
            SeamLead::Uncovered {
                from_ms: WED_7_OCT_2026
            }
        );
        let vouched = Minutes {
            candles: &previous_session,
            answer: MinutesAnswer::Covers {
                from_ms: WED_7_OCT_2026,
                until_ms: first_trade,
            },
        };
        let monday = WED_7_OCT_2026 - 2 * DAY_MS;
        let whole_days: Vec<Bar> = daily
            .iter()
            .filter(|bar| bar.open_time >= monday && bar.open_time < WED_7_OCT_2026)
            .cloned()
            .collect();
        let mut expected = summary(&whole_days);
        expected.close_time = WED_7_OCT_2026 - 1;
        assert_eq!(
            seam_lead(Some(&daily), Some(vouched), first_trade, WEEK_MS),
            partial_lead(expected),
            "the seconds of 12:00 before the trade are still missing"
        );
        assert_eq!(
            seam_lead(Some(&daily), Some(vouched), first_trade, DAY_MS),
            SeamLead::Unrecorded {
                from_ms: WED_7_OCT_2026 + 12 * HOUR
            }
        );
        let too_short = Minutes {
            answer: MinutesAnswer::Covers {
                from_ms: WED_7_OCT_2026 + HOUR,
                until_ms: first_trade,
            },
            ..vouched
        };
        assert!(matches!(
            seam_lead(Some(&daily), Some(too_short), first_trade, WEEK_MS),
            SeamLead::Uncovered { .. }
        ));
    }

    /// A lead whole to the first trade covers the seam; one from an answer
    /// known to be short does not, whatever its candles reach.
    #[test]
    fn a_short_answer_never_makes_a_whole_lead() {
        let on_a_minute = WED_7_OCT_2026 + 6 * HOUR + 14 * MINUTE;
        let held = minutes(WED_7_OCT_2026, on_a_minute + HOUR);
        let whole = seam_lead(None, Some(Minutes::candles(&held)), on_a_minute, 5 * MINUTE);
        assert!(whole.covers_seam(), "{whole:?}");
        let short = Minutes {
            candles: &held,
            answer: MinutesAnswer::Short,
        };
        let partial = seam_lead(None, Some(short), on_a_minute, 5 * MINUTE);
        assert!(matches!(partial, SeamLead::Lead { whole: false, .. }));
        assert!(!partial.covers_seam());
    }

    #[test]
    fn the_lead_is_deterministic() {
        let daily = days(WED_7_OCT_2026, 60, true);
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let once = seam_lead(
            Some(&daily),
            Some(Minutes::candles(&held)),
            FIRST_TRADE,
            CALENDAR_MONTH_MS,
        );
        let twice = seam_lead(
            Some(&daily),
            Some(Minutes::candles(&held)),
            FIRST_TRADE,
            CALENDAR_MONTH_MS,
        );
        assert_eq!(once, twice);
    }
}
