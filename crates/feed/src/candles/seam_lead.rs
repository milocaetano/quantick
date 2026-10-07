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
//! Two rules keep it honest. A candle that reaches the first trade is left
//! out, never split: its volume and extremes overlap prints the chart holds,
//! and counting them twice is worse than missing the seconds before the first
//! trade. And a stretch the candles held do not reach is no lead at all,
//! never a partial one quietly presented as the venue's record: the absence
//! comes back named, so the caller can log it or fetch what is missing.
//!
//! A daily base cannot say anything about the seam *day*, which the chart's
//! own trades overlap; that part comes from minutes — whole days before the
//! seam day from the daily base, then the seam day's minutes up to the first
//! trade.
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
    /// trade, folded into one bar stamped inside the bucket.
    Lead(Bar),
    /// Nothing to put in front: the bucket opens on the first trade, or the
    /// only candle before it overlaps the trade.
    Nothing,
    /// The stretch from `from_ms` to the first trade needs minutes, and no
    /// minute candles are held.
    MinutesNotHeld {
        /// Where the minutes would have to start.
        from_ms: i64,
    },
    /// The candles held do not cover the stretch from `from_ms` to the first
    /// trade, or one of them reaches past the trade: a lead from them would
    /// be partial or would count prints twice, so there is none.
    Uncovered {
        /// Where the stretch the candles do not cover starts.
        from_ms: i64,
    },
}

impl SeamLead {
    /// The lead bar, when there is one.
    #[must_use]
    pub fn into_bar(self) -> Option<Bar> {
        match self {
            Self::Lead(bar) => Some(bar),
            _ => None,
        }
    }

    /// A stable token for the structured log.
    #[must_use]
    pub const fn token(&self) -> &'static str {
        match self {
            Self::Lead(_) => "venue_lead",
            Self::Nothing => "nothing_before_first_trade",
            Self::MinutesNotHeld { .. } => "minutes_not_held",
            Self::Uncovered { .. } => "candles_do_not_cover",
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
/// from where the last of those days ended (a server-day candle can end a
/// few hours either side of 00:00 UTC) or from the seam day's start.
#[must_use]
pub fn seam_lead(
    days: Option<&[Bar]>,
    minutes: Option<&[Bar]>,
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
        let seam_day = time_bucket_start(first_trade_ms, DAY_MS);
        minutes_from = seam_day.max(bucket);
        if seam_day > bucket {
            if !reaches(days, DAY_MS, bucket, seam_day) {
                return SeamLead::Uncovered { from_ms: bucket };
            }
            for day in placed_in(days, bucket, seam_day) {
                if day.close_time >= first_trade_ms {
                    return SeamLead::Uncovered { from_ms: bucket };
                }
                minutes_from = day.close_time.saturating_add(1).max(bucket);
                absorb(&mut lead, day);
            }
        }
    }
    if minutes_from < first_trade_ms {
        let Some(minutes) = minutes else {
            return SeamLead::MinutesNotHeld {
                from_ms: minutes_from,
            };
        };
        let first_minute = time_bucket_start(first_trade_ms, OHLCV_BASE_INTERVAL_MS);
        if !reaches(minutes, OHLCV_BASE_INTERVAL_MS, minutes_from, first_minute) {
            return SeamLead::Uncovered {
                from_ms: minutes_from,
            };
        }
        let start = minutes.partition_point(|minute| minute.open_time < minutes_from);
        for minute in minutes[start..]
            .iter()
            .take_while(|minute| minute.close_time < first_trade_ms)
        {
            absorb(&mut lead, minute);
        }
    }
    lead.map_or(SeamLead::Nothing, |mut bar| {
        // Stamped inside the bucket, and before the first trade: a server-day
        // candle opening the evening before is the bucket's all the same.
        bar.open_time = bar.open_time.max(bucket);
        bar.close_time = bar
            .close_time
            .min(first_trade_ms.saturating_sub(1))
            .max(bar.open_time);
        SeamLead::Lead(bar)
    })
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
        let lead = seam_lead(None, Some(&held), FIRST_TRADE, 5 * MINUTE);
        let bucket = WED_7_OCT_2026 + 6 * HOUR + 10 * MINUTE;
        let expected: Vec<Bar> = held
            .iter()
            .filter(|bar| bar.open_time >= bucket && bar.close_time < FIRST_TRADE)
            .cloned()
            .collect();
        assert_eq!(expected.len(), 4, "06:10, 06:11, 06:12, 06:13");
        assert_eq!(lead, SeamLead::Lead(summary(&expected)));
        let bar = lead.into_bar().unwrap();
        assert_eq!(bar.open_time, bucket, "the bar opens on its bucket");
        assert!(bar.close_time < FIRST_TRADE, "and ends before the trade");
    }

    /// A one-minute bucket's only candle is the one the trade sits inside.
    #[test]
    fn a_candle_reaching_the_first_trade_is_never_counted() {
        let held = minutes(WED_7_OCT_2026, FIRST_TRADE + HOUR);
        assert_eq!(
            seam_lead(None, Some(&held), FIRST_TRADE, MINUTE),
            SeamLead::Nothing
        );
        let on_the_edge = WED_7_OCT_2026 + 6 * HOUR;
        assert_eq!(
            seam_lead(None, Some(&held), on_the_edge, HOUR),
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
            seam_lead(None, Some(&late_start), FIRST_TRADE, 5 * MINUTE),
            SeamLead::Uncovered {
                from_ms: WED_7_OCT_2026 + 6 * HOUR + 10 * MINUTE
            }
        );
        let early_end = minutes(WED_7_OCT_2026, WED_7_OCT_2026 + 6 * HOUR + 12 * MINUTE);
        assert!(matches!(
            seam_lead(None, Some(&early_end), FIRST_TRADE, 5 * MINUTE),
            SeamLead::Uncovered { .. }
        ));
        assert!(matches!(
            seam_lead(None, Some(&[]), FIRST_TRADE, 5 * MINUTE),
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
        let lead = seam_lead(Some(&daily), Some(&held), FIRST_TRADE, DAY_MS);
        let today: Vec<Bar> = held
            .iter()
            .filter(|bar| bar.open_time >= WED_7_OCT_2026 && bar.close_time < FIRST_TRADE)
            .cloned()
            .collect();
        assert_eq!(today.len(), 6 * 60 + 14);
        assert_eq!(lead, SeamLead::Lead(summary(&today)));
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
            let lead = seam_lead(Some(&daily), Some(&held), FIRST_TRADE, interval);
            assert_eq!(lead, SeamLead::Lead(summary(&parts)), "{interval}");
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
        let lead = seam_lead(Some(&east), Some(&held), FIRST_TRADE, WEEK_MS)
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
            seam_lead(Some(&west), Some(&held), FIRST_TRADE, WEEK_MS),
            SeamLead::Uncovered { from_ms: monday }
        );
    }

    /// A daily base that has not reached the seam day was fetched before it:
    /// its last candle is a day still forming, not a whole one.
    #[test]
    fn a_daily_base_that_stops_short_of_the_seam_day_is_no_lead() {
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let stale = days(WED_7_OCT_2026 - DAY_MS, 30, false);
        assert!(matches!(
            seam_lead(Some(&stale), Some(&held), FIRST_TRADE, WEEK_MS),
            SeamLead::Uncovered { .. }
        ));
    }

    #[test]
    fn the_lead_is_deterministic() {
        let daily = days(WED_7_OCT_2026, 60, true);
        let held = minutes(WED_7_OCT_2026 - DAY_MS, FIRST_TRADE + HOUR);
        let once = seam_lead(Some(&daily), Some(&held), FIRST_TRADE, CALENDAR_MONTH_MS);
        let twice = seam_lead(Some(&daily), Some(&held), FIRST_TRADE, CALENDAR_MONTH_MS);
        assert_eq!(once, twice);
    }
}
