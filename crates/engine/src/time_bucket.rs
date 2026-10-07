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

/// The shape an interval's buckets take, read once from its millisecond count.
///
/// Everything the law says about one interval follows from which of the three
/// shapes it is, so a caller that cuts many timestamps by the same interval —
/// the live builder, a fold, a drawing walking empty slots — resolves it once
/// and asks the shape, rather than re-reading the count every time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeBucketLaw {
    /// Epoch-aligned windows of this many milliseconds: seconds to days.
    Fixed(i64),
    /// Windows of this many milliseconds, a whole number of weeks, opening on
    /// Monday 00:00 UTC.
    Weeks(i64),
    /// This many calendar months, counted from January 1970.
    Months(i64),
}

impl TimeBucketLaw {
    /// The shape `interval_ms` names; `None` for a non-positive interval,
    /// which has no buckets.
    #[must_use]
    pub fn of(interval_ms: i64) -> Option<Self> {
        if interval_ms <= 0 {
            None
        } else if let Some(months) = calendar_months(interval_ms) {
            Some(Self::Months(months))
        } else if interval_ms % WEEK_MS == 0 {
            Some(Self::Weeks(interval_ms))
        } else {
            Some(Self::Fixed(interval_ms))
        }
    }

    /// Whether the buckets follow the calendar — Monday weeks or months —
    /// rather than plain epoch-aligned windows.
    #[must_use]
    pub fn is_calendar(self) -> bool {
        !matches!(self, Self::Fixed(_))
    }

    /// The ordinal of the bucket holding `time_ms`: consecutive buckets have
    /// consecutive ordinals, so two ordinals subtract to a bucket count.
    #[must_use]
    pub fn index(self, time_ms: i64) -> i64 {
        match self {
            Self::Fixed(length) => time_ms.div_euclid(length),
            Self::Weeks(length) => time_ms.saturating_sub(MONDAY_ANCHOR_MS).div_euclid(length),
            Self::Months(months) => month_index(time_ms.div_euclid(DAY_MS)).div_euclid(months),
        }
    }

    /// The start (epoch ms, UTC) of the bucket with ordinal `index`.
    #[must_use]
    pub fn start_of(self, index: i64) -> i64 {
        match self {
            Self::Fixed(length) => index.saturating_mul(length),
            Self::Weeks(length) => index
                .saturating_mul(length)
                .saturating_add(MONDAY_ANCHOR_MS),
            Self::Months(months) => month_start_ms(index.saturating_mul(months)),
        }
    }

    /// The start of the bucket holding `time_ms`.
    #[must_use]
    pub fn start(self, time_ms: i64) -> i64 {
        self.start_of(self.index(time_ms))
    }

    /// Where the bucket opening at `bucket_start` ends, exclusive: the start
    /// of the next one. A calendar month ends where the next month starts, so
    /// `1mo` from February 2024 is 29 days and from March 31.
    #[must_use]
    pub fn end(self, bucket_start: i64) -> i64 {
        match self {
            Self::Fixed(length) | Self::Weeks(length) => bucket_start.saturating_add(length),
            Self::Months(_) => self.step(bucket_start, 1),
        }
    }

    /// The start of the bucket `buckets` after the one holding `time_ms` —
    /// before it, for a negative count.
    #[must_use]
    pub fn step(self, time_ms: i64, buckets: i64) -> i64 {
        self.start_of(self.index(time_ms).saturating_add(buckets))
    }
}

/// The start (epoch ms, UTC) of the `interval_ms` bucket holding `time_ms`.
///
/// A non-positive interval has no bucket and returns `time_ms` unchanged.
#[must_use]
pub fn time_bucket_start(time_ms: i64, interval_ms: i64) -> i64 {
    TimeBucketLaw::of(interval_ms).map_or(time_ms, |law| law.start(time_ms))
}

/// The start of the bucket after the one opening at `bucket_start` — where
/// that bucket ends, exclusive.
///
/// `bucket_start` is expected to be a value [`time_bucket_start`] returned.
/// See [`TimeBucketLaw::end`].
#[must_use]
pub fn time_bucket_end(bucket_start: i64, interval_ms: i64) -> i64 {
    TimeBucketLaw::of(interval_ms).map_or(bucket_start, |law| law.end(bucket_start))
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

    const FEB_2024_MS: i64 = 19_754 * DAY_MS;
    const MAR_2024_MS: i64 = 19_783 * DAY_MS;

    #[test]
    fn a_month_steps_to_the_next_calendar_month_in_both_directions() {
        let law = TimeBucketLaw::of(CALENDAR_MONTH_MS).unwrap();
        assert_eq!(law, TimeBucketLaw::Months(1));
        assert!(law.is_calendar());
        let mid_feb = FEB_2024_MS + 10 * DAY_MS + 5;
        assert_eq!(law.start(mid_feb), FEB_2024_MS);
        assert_eq!(law.end(FEB_2024_MS), MAR_2024_MS, "a leap February");
        assert_eq!(law.step(mid_feb, 1), MAR_2024_MS);
        assert_eq!(law.step(MAR_2024_MS, -1), FEB_2024_MS);
        assert_eq!(law.step(FEB_2024_MS, 0), FEB_2024_MS);
        assert_eq!(law.index(MAR_2024_MS) - law.index(mid_feb), 1);
        let week = TimeBucketLaw::of(WEEK_MS).unwrap();
        assert!(week.is_calendar());
        // 1970-01-05 was the first Monday after the epoch.
        assert_eq!(week.step(0, 1), 4 * DAY_MS);
        assert_eq!(week.step(0, -1), -10 * DAY_MS);
        assert!(!TimeBucketLaw::of(DAY_MS).unwrap().is_calendar());
        assert_eq!(TimeBucketLaw::of(0), None);
    }

    /// The resolved law and the free functions are one law: every shape,
    /// across the epoch, a leap day and a negative timestamp.
    #[test]
    fn the_resolved_law_agrees_with_the_free_functions() {
        let intervals = [1_000, 60_000, DAY_MS, 2 * DAY_MS, WEEK_MS, 2 * WEEK_MS]
            .into_iter()
            .chain((1..=MAX_CALENDAR_MONTHS).map(|months| months * CALENDAR_MONTH_MS));
        for interval in intervals {
            let law = TimeBucketLaw::of(interval).unwrap();
            for time in (-40..800).map(|day| day * DAY_MS / 3 + FEB_2024_MS * (day % 2)) {
                let start = time_bucket_start(time, interval);
                assert_eq!(law.start(time), start, "{interval} at {time}");
                assert_eq!(law.end(start), time_bucket_end(start, interval));
                assert!(
                    start <= time && time < law.end(start),
                    "{interval} at {time}"
                );
            }
        }
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
