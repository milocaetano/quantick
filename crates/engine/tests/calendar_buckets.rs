//! Day, week and calendar-month time bars: the one bucket law in UTC.
//!
//! Days start at 00:00 UTC, weeks on Monday 00:00 UTC, and a month is the
//! calendar month — never a fixed number of milliseconds. The live builder and
//! the venue-candle fold both read [`time_bucket_start`], so these goldens pin
//! the law for both.

use quantick_engine::time_bucket::{
    CALENDAR_MONTH_MS, DAY_MS, MAX_CALENDAR_MONTHS, WEEK_MS, calendar_months, time_bucket_end,
    time_bucket_start,
};
use quantick_engine::{
    BarBuilder, BarSpec, BarSpecError, MAX_TIME_INTERVAL_MS, Side, TimeBarBuilder, Trade,
    fmt_time_interval, golden,
};
use rust_decimal::Decimal;

const MONTH_TRADES: &str = include_str!("fixtures/calendar_month_trades.csv");
const MONTH_EXPECTED: &str = include_str!("fixtures/calendar_month_expected.csv");
const WEEK_TRADES: &str = include_str!("fixtures/calendar_week_trades.csv");
const WEEK_EXPECTED: &str = include_str!("fixtures/calendar_week_expected.csv");
const DAY_TRADES: &str = include_str!("fixtures/calendar_day_trades.csv");
const DAY_EXPECTED: &str = include_str!("fixtures/calendar_day_expected.csv");

/// 2024-01-01T00:00:00Z, a Monday.
const JAN_2024: i64 = 1_704_067_200_000;

/// The epoch millisecond of 00:00 UTC on `year-month-01`, counted the slow way
/// so the law under test is not also the oracle.
fn month_start(year: i64, month: i64) -> i64 {
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = |y: i64, m: i64| match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap(y) => 29,
        _ => 28,
    };
    let mut days = 0;
    if year >= 1970 {
        for y in 1970..year {
            days += if leap(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if leap(y) { 366 } else { 365 };
        }
    }
    for m in 1..month {
        days += days_in(year, m);
    }
    days * DAY_MS
}

fn trade(ts: i64) -> Trade {
    Trade {
        agg_id: 0,
        timestamp_ms: ts,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[test]
fn monthly_bars_match_golden() {
    golden::assert_golden(
        || TimeBarBuilder::new(CALENDAR_MONTH_MS),
        MONTH_TRADES,
        MONTH_EXPECTED,
    );
}

#[test]
fn weekly_bars_match_golden() {
    golden::assert_golden(|| TimeBarBuilder::new(WEEK_MS), WEEK_TRADES, WEEK_EXPECTED);
}

#[test]
fn daily_bars_match_golden() {
    golden::assert_golden(|| TimeBarBuilder::new(DAY_MS), DAY_TRADES, DAY_EXPECTED);
}

#[test]
fn the_parsed_spec_builds_the_same_calendar_bars() {
    for (spec, trades, expected) in [
        ("time:1mo", MONTH_TRADES, MONTH_EXPECTED),
        ("time:1w", WEEK_TRADES, WEEK_EXPECTED),
        ("time:1d", DAY_TRADES, DAY_EXPECTED),
    ] {
        let spec = BarSpec::parse(spec).unwrap();
        let trades = quantick_engine::fixture::parse_trades(trades).unwrap();
        let expected = quantick_engine::fixture::parse_bars(expected).unwrap();
        let bars = golden::replay(&mut *spec.build(), &trades);
        assert_eq!(golden::diff_bars(&expected, &bars), None, "{spec:?}");
    }
}

/// Every month of a common and a leap year starts on its first day and is as
/// long as that month is.
#[test]
fn every_month_is_as_long_as_the_calendar_says() {
    for year in [2023, 2024] {
        for month in 1..=12 {
            let start = month_start(year, month);
            let next = if month == 12 {
                month_start(year + 1, 1)
            } else {
                month_start(year, month + 1)
            };
            for ts in [start, start + DAY_MS / 2, next - 1] {
                assert_eq!(
                    time_bucket_start(ts, CALENDAR_MONTH_MS),
                    start,
                    "{year}-{month:02} at {ts}"
                );
            }
            assert_eq!(
                time_bucket_end(start, CALENDAR_MONTH_MS),
                next,
                "{year}-{month:02} ends where the next month starts"
            );
        }
    }
    let february =
        |year| time_bucket_end(month_start(year, 2), CALENDAR_MONTH_MS) - month_start(year, 2);
    assert_eq!(february(2023), 28 * DAY_MS);
    assert_eq!(february(2024), 29 * DAY_MS, "2024 is a leap year");
    assert_eq!(february(2000), 29 * DAY_MS, "a 400th year is a leap year");
    assert_eq!(february(1900), 28 * DAY_MS, "a 100th year is not");
}

#[test]
fn several_months_group_from_january() {
    let may = month_start(2024, 5) + 14 * DAY_MS;
    assert_eq!(
        time_bucket_start(may, 3 * CALENDAR_MONTH_MS),
        month_start(2024, 4),
        "quarters run Jan/Apr/Jul/Oct"
    );
    assert_eq!(
        time_bucket_end(month_start(2024, 4), 3 * CALENDAR_MONTH_MS),
        month_start(2024, 7)
    );
    let july = month_start(2024, 7) + 3 * DAY_MS;
    assert_eq!(
        time_bucket_start(july, 12 * CALENDAR_MONTH_MS),
        JAN_2024,
        "twelve months is the calendar year"
    );
    assert_eq!(
        time_bucket_end(JAN_2024, 12 * CALENDAR_MONTH_MS),
        month_start(2025, 1)
    );
}

#[test]
fn weeks_start_on_monday_utc() {
    // 2024-01-01 is a Monday: the whole week up to Sunday's last millisecond
    // shares its bucket, and the next Monday opens a new one.
    for offset in [0, DAY_MS, 6 * DAY_MS, WEEK_MS - 1] {
        assert_eq!(time_bucket_start(JAN_2024 + offset, WEEK_MS), JAN_2024);
    }
    assert_eq!(
        time_bucket_start(JAN_2024 + WEEK_MS, WEEK_MS),
        JAN_2024 + WEEK_MS
    );
    assert_eq!(time_bucket_end(JAN_2024, WEEK_MS), JAN_2024 + WEEK_MS);
    // The epoch was a Thursday, so its week opened on Monday 1969-12-29.
    assert_eq!(time_bucket_start(0, WEEK_MS), -3 * DAY_MS);
    // Two weeks keep the Monday anchor too.
    let start = time_bucket_start(JAN_2024, 2 * WEEK_MS);
    assert_eq!(
        (start - JAN_2024).rem_euclid(WEEK_MS),
        0,
        "a two-week bucket still opens on a Monday"
    );
}

#[test]
fn days_start_at_midnight_utc() {
    let leap_day = month_start(2024, 2) + 28 * DAY_MS;
    assert_eq!(time_bucket_start(leap_day + DAY_MS - 1, DAY_MS), leap_day);
    assert_eq!(time_bucket_start(leap_day, DAY_MS), leap_day);
    assert_eq!(time_bucket_end(leap_day, DAY_MS), leap_day + DAY_MS);
    // Several days stay epoch-aligned, as every fixed interval always has.
    assert_eq!(time_bucket_start(3 * DAY_MS + 5, 2 * DAY_MS), 2 * DAY_MS);
}

/// Before 1970 the law floors, it does not round toward zero.
#[test]
fn timestamps_before_the_epoch_floor_into_the_bucket_below() {
    assert_eq!(time_bucket_start(-1, DAY_MS), -DAY_MS);
    assert_eq!(time_bucket_start(-1, WEEK_MS), -3 * DAY_MS);
    assert_eq!(
        time_bucket_start(-1, CALENDAR_MONTH_MS),
        month_start(1969, 12)
    );
    assert_eq!(
        time_bucket_start(month_start(1900, 3) - 1, CALENDAR_MONTH_MS),
        month_start(1900, 2)
    );
    assert_eq!(time_bucket_end(month_start(1969, 12), CALENDAR_MONTH_MS), 0);
}

/// Sub-day intervals keep the epoch-aligned floor they always had.
#[test]
fn intraday_intervals_are_unchanged() {
    assert_eq!(time_bucket_start(1_999, 1_000), 1_000);
    assert_eq!(time_bucket_start(-1, 60_000), -60_000);
    assert_eq!(time_bucket_start(7_200_001, 3_600_000), 7_200_000);
    assert_eq!(time_bucket_end(7_200_000, 3_600_000), 10_800_000);
}

#[test]
fn a_month_reports_its_own_length_as_progress() {
    let mut builder = TimeBarBuilder::new(CALENDAR_MONTH_MS);
    let feb = month_start(2024, 2);
    builder.push(&trade(feb + DAY_MS));
    let progress = builder.progress().expect("a forming month");
    assert_eq!(progress.done, Decimal::from(DAY_MS));
    assert_eq!(progress.target, Decimal::from(29 * DAY_MS));
}

#[test]
fn calendar_months_are_recognised_only_inside_their_range() {
    assert_eq!(calendar_months(CALENDAR_MONTH_MS), Some(1));
    assert_eq!(
        calendar_months(MAX_CALENDAR_MONTHS * CALENDAR_MONTH_MS),
        Some(MAX_CALENDAR_MONTHS)
    );
    assert_eq!(
        calendar_months((MAX_CALENDAR_MONTHS + 1) * CALENDAR_MONTH_MS),
        None
    );
    assert_eq!(
        calendar_months(30 * DAY_MS),
        None,
        "thirty days is not a month"
    );
    assert_eq!(calendar_months(CALENDAR_MONTH_MS + 1), None);
    assert_eq!(calendar_months(0), None);
}

#[test]
fn day_week_and_month_intervals_parse_and_round_trip() {
    for (text, ms) in [
        ("1d", DAY_MS),
        ("2d", 2 * DAY_MS),
        ("1w", WEEK_MS),
        ("2w", 2 * WEEK_MS),
        ("1mo", CALENDAR_MONTH_MS),
        ("3mo", 3 * CALENDAR_MONTH_MS),
        ("12mo", 12 * CALENDAR_MONTH_MS),
    ] {
        assert_eq!(
            BarSpec::parse(&format!("time:{text}")),
            Ok(BarSpec::Time(ms))
        );
        assert_eq!(fmt_time_interval(ms), text);
        assert_eq!(BarSpec::Time(ms).to_config_string(), format!("time:{text}"));
    }
    // Older spellings of the same values still read, and write back in the
    // largest unit that holds them exactly.
    assert_eq!(BarSpec::parse("time:24h"), Ok(BarSpec::Time(DAY_MS)));
    assert_eq!(BarSpec::parse("time:7d"), Ok(BarSpec::Time(WEEK_MS)));
    assert_eq!(BarSpec::parse("time:86400000"), Ok(BarSpec::Time(DAY_MS)));
    assert_eq!(fmt_time_interval(DAY_MS), "1d");
    assert_eq!(fmt_time_interval(36 * 3_600_000), "36h");
    // Minutes keep their letter; a month is spelled out.
    assert_eq!(BarSpec::parse("time:1m"), Ok(BarSpec::Time(60_000)));
}

#[test]
fn intervals_past_the_calendar_range_are_refused() {
    for bad in ["time:13mo", "time:0mo", "time:5w", "time:29d", "time:30d"] {
        assert!(
            matches!(
                BarSpec::parse(bad),
                Err(BarSpecError::IntervalOutOfRange { .. } | BarSpecError::NotAnInterval { .. })
            ),
            "{bad} parsed"
        );
    }
    assert_eq!(
        MAX_TIME_INTERVAL_MS,
        4 * WEEK_MS,
        "the longest fixed interval"
    );
    assert_eq!(
        BarSpec::parse(&format!("time:{}", MAX_TIME_INTERVAL_MS + DAY_MS)),
        Err(BarSpecError::IntervalOutOfRange {
            ms: MAX_TIME_INTERVAL_MS + DAY_MS,
            param: (MAX_TIME_INTERVAL_MS + DAY_MS).to_string(),
        })
    );
}
