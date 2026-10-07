//! The one bucket law every time bar obeys, live or folded from candles.
//!
//! A time interval is a millisecond count, as it has always been: that is what
//! [`crate::BarSpec::Time`] holds and what every saved workspace, config file
//! and control call already carries. The law reads three shapes out of it, all
//! in UTC:
//!
//! - **A calendar month** is a whole number `n` (1..=[`MAX_CALENDAR_MONTHS`])
//!   of [`CALENDAR_MONTH_MS`]. Months differ in length, so no fixed duration
//!   *is* a month; the mean Gregorian month stands in as its nominal size. That
//!   nominal value is not a whole number of days, so no `Nd` interval can be
//!   mistaken for it, and arithmetic that only needs a typical bar width (how
//!   far ahead an empty slot sits) still gets a sensible answer. The buckets
//!   themselves are calendar months, counted from January 1970: `3mo` cuts
//!   quarters, `12mo` calendar years.
//! - **A whole number of weeks** opens on Monday 00:00 UTC. The epoch was a
//!   Thursday, so this is the one fixed interval that is not epoch-aligned.
//! - **Anything else** — seconds to days — is epoch-aligned and floor-divided,
//!   which puts a day on 00:00 UTC and is exactly the rule time bars always
//!   followed.
//!
//! The live builder ([`crate::TimeBarBuilder`]) and the venue-candle fold read
//! the same two functions, so a chart's history and its forming bar cannot
//! disagree about where a day, a week or a month begins. Pure arithmetic: no
//! clock, no time zone database.

/// One UTC day.
pub const DAY_MS: i64 = 86_400_000;

/// One week. A bucket of whole weeks opens on Monday 00:00 UTC.
pub const WEEK_MS: i64 = 7 * DAY_MS;

/// The nominal size of one calendar month: the mean Gregorian month,
/// 365.2425 / 12 days.
///
/// An interval of `n` times this, for `n` in 1..=[`MAX_CALENDAR_MONTHS`], is
/// read as `n` calendar months rather than as a duration. It is a whole number
/// of seconds but not of minutes, hours or days, so it never collides with an
/// interval written in those units below a year.
pub const CALENDAR_MONTH_MS: i64 = 2_629_746_000;

/// The most calendar months one bar may span: a calendar year.
pub const MAX_CALENDAR_MONTHS: i64 = 12;

/// How far 1970-01-01 (a Thursday) sits after the Monday that opened its week.
const MONDAY_ANCHOR_MS: i64 = -3 * DAY_MS;

/// The number of calendar months `interval_ms` names, if it names any.
#[must_use]
pub fn calendar_months(interval_ms: i64) -> Option<i64> {
    (interval_ms > 0 && interval_ms % CALENDAR_MONTH_MS == 0)
        .then_some(interval_ms / CALENDAR_MONTH_MS)
        .filter(|months| (1..=MAX_CALENDAR_MONTHS).contains(months))
}

/// The start (epoch ms, UTC) of the `interval_ms` bucket holding `time_ms`.
///
/// A non-positive interval has no bucket and returns `time_ms` unchanged.
#[must_use]
pub fn time_bucket_start(time_ms: i64, interval_ms: i64) -> i64 {
    if interval_ms <= 0 {
        return time_ms;
    }
    if let Some(months) = calendar_months(interval_ms) {
        let index = month_index(time_ms.div_euclid(DAY_MS));
        return month_start_ms(index.div_euclid(months) * months);
    }
    if interval_ms % WEEK_MS == 0 {
        let offset = time_ms.saturating_sub(MONDAY_ANCHOR_MS);
        return offset.div_euclid(interval_ms) * interval_ms + MONDAY_ANCHOR_MS;
    }
    time_ms.div_euclid(interval_ms) * interval_ms
}

/// The start of the bucket after the one opening at `bucket_start` — where
/// that bucket ends, exclusive.
///
/// `bucket_start` is expected to be a value [`time_bucket_start`] returned. A
/// calendar month ends where the next month starts, so `1mo` from February
/// 2024 is 29 days and from March 31.
#[must_use]
pub fn time_bucket_end(bucket_start: i64, interval_ms: i64) -> i64 {
    match calendar_months(interval_ms) {
        Some(months) => {
            month_start_ms(month_index(bucket_start.div_euclid(DAY_MS)).saturating_add(months))
        }
        None => bucket_start.saturating_add(interval_ms.max(0)),
    }
}

/// Months since January 1970 of the civil date `days` after the epoch.
fn month_index(days: i64) -> i64 {
    let (year, month, _) = civil_from_days(days);
    (year - 1970) * 12 + (month - 1)
}

/// The epoch millisecond of 00:00 UTC on the first day of month `index`
/// (months since January 1970).
fn month_start_ms(index: i64) -> i64 {
    let year = 1970 + index.div_euclid(12);
    let month = index.rem_euclid(12) + 1;
    days_from_civil(year, month, 1).saturating_mul(DAY_MS)
}

/// `(year, month 1..=12, day 1..=31)` of the day `days` after 1970-01-01, in
/// the proleptic Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Days from 1970-01-01 to `year-month-day` (Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_civil_conversions_are_inverses() {
        for days in [-800_000, -1, 0, 1, 59, 60, 11_016, 19_782, 2_000_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn the_month_unit_collides_with_no_shorter_unit_below_a_year() {
        for months in 1..=MAX_CALENDAR_MONTHS {
            let ms = months * CALENDAR_MONTH_MS;
            assert_ne!(ms % DAY_MS, 0, "{months}mo is not a whole number of days");
            assert_ne!(
                ms % 3_600_000,
                0,
                "{months}mo is not a whole number of hours"
            );
        }
    }
}
