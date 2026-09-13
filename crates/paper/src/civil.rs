//! Civil time: the display offset and the date law every trade surface
//! shares.
//!
//! Two halves that used to live in `app` beside the widgets that draw them.
//! [`TzOffset`] is the display timezone — a fixed offset applied only when a
//! UTC epoch-millisecond timestamp is shown. The rest is the civil-date law:
//! which day a trade closed on, where that day starts and ends, and how a
//! journal file is named from venue time. The report cuts on it, the journal
//! names files by it and the ledger stamps rows with it, so it lives with
//! them in this crate rather than in the UI that happens to paint the dates.
//!
//! Pure integer arithmetic. No clock is read here: "today" is the caller's
//! question, answered with a timestamp it already holds.
//!
//! The offset only ever relabels: the engine works in UTC epoch
//! milliseconds and never sees it, so the determinism rule is untouched.
//! Offsets are whole minutes, so fractional zones (India UTC+05:30, Nepal
//! UTC+05:45) stay exact rather than being silently rounded (data honesty).

/// A fixed offset from UTC for displaying times, in whole minutes east of UTC
/// (negative for zones west of UTC, e.g. `-180` for UTC−03:00).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TzOffset {
    minutes: i32,
}

impl TzOffset {
    /// An offset of `minutes` east of UTC.
    #[must_use]
    pub const fn new(minutes: i32) -> Self {
        Self { minutes }
    }

    /// The offset in whole minutes east of UTC — how a saved workspace records
    /// it, in the unit [`Self::new`] takes back.
    #[must_use]
    pub const fn minutes(self) -> i32 {
        self.minutes
    }

    /// The offset in milliseconds, to add to a UTC epoch-ms timestamp before
    /// extracting the local time-of-day.
    #[must_use]
    pub const fn offset_ms(self) -> i64 {
        self.minutes as i64 * 60_000
    }

    /// A short label like `UTC`, `UTC-03:00` or `UTC+05:30`.
    #[must_use]
    pub fn label(self) -> String {
        if self.minutes == 0 {
            return "UTC".to_string();
        }
        let sign = if self.minutes < 0 { '-' } else { '+' };
        let abs = self.minutes.unsigned_abs();
        let (h, m) = (abs / 60, abs % 60);
        format!("UTC{sign}{h:02}:{m:02}")
    }

    /// Every standard fixed UTC offset in use, ascending from UTC−12:00 to
    /// UTC+14:00, including the fractional (30- and 45-minute) zones. Used to
    /// populate the timezone selector.
    pub const ALL: [TzOffset; 38] = [
        TzOffset::new(-720), // -12:00
        TzOffset::new(-660), // -11:00
        TzOffset::new(-600), // -10:00
        TzOffset::new(-570), // -09:30
        TzOffset::new(-540), // -09:00
        TzOffset::new(-480), // -08:00
        TzOffset::new(-420), // -07:00
        TzOffset::new(-360), // -06:00
        TzOffset::new(-300), // -05:00
        TzOffset::new(-240), // -04:00
        TzOffset::new(-210), // -03:30
        TzOffset::new(-180), // -03:00
        TzOffset::new(-120), // -02:00
        TzOffset::new(-60),  // -01:00
        TzOffset::new(0),    //  00:00 (UTC)
        TzOffset::new(60),   // +01:00
        TzOffset::new(120),  // +02:00
        TzOffset::new(180),  // +03:00
        TzOffset::new(210),  // +03:30
        TzOffset::new(240),  // +04:00
        TzOffset::new(270),  // +04:30
        TzOffset::new(300),  // +05:00
        TzOffset::new(330),  // +05:30
        TzOffset::new(345),  // +05:45
        TzOffset::new(360),  // +06:00
        TzOffset::new(390),  // +06:30
        TzOffset::new(420),  // +07:00
        TzOffset::new(480),  // +08:00
        TzOffset::new(525),  // +08:45
        TzOffset::new(540),  // +09:00
        TzOffset::new(570),  // +09:30
        TzOffset::new(600),  // +10:00
        TzOffset::new(630),  // +10:30
        TzOffset::new(660),  // +11:00
        TzOffset::new(720),  // +12:00
        TzOffset::new(765),  // +12:45
        TzOffset::new(780),  // +13:00
        TzOffset::new(840),  // +14:00
    ];
}

impl Default for TzOffset {
    /// UTC−03:00 — the chart's home zone.
    fn default() -> Self {
        Self { minutes: -180 }
    }
}

/// Milliseconds in a civil day. Civil days here are exactly 24 h: the
/// display timezone is a fixed offset (the workspace has no DST table),
/// so there is no shorter or longer day to model — and inventing one
/// would be a guess, not a fact.
pub const DAY_MS: i64 = 86_400_000;

/// Days in one calendar week.
pub const WEEK_DAYS: i64 = 7;

/// Civil UTC date-time from epoch milliseconds: `(year, month, day, hour,
/// minute, second)`. Civil-from-days per Howard Hinnant's algorithm; no
/// clock, no chrono.
pub fn civil_utc(timestamp_ms: i64) -> (i64, i64, i64, i64, i64, i64) {
    let seconds = timestamp_ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let time_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    (
        year,
        month,
        day,
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60,
    )
}

/// `(year, month, day)` from a day number counted from 1970-01-01 —
/// Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The exact inverse of [`civil_from_days`] — Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_offset = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_offset + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// One civil date in the display timezone: the unit the ledger stamps, the
/// calendar paints and the range filter cuts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDate {
    /// Day number counted from 1970-01-01. Kept as the single field so
    /// ordering, differences and range tests are plain integer work, and
    /// an impossible date (month 13, day 32) cannot be constructed.
    day_number: i64,
}

impl CivilDate {
    /// The civil date a venue timestamp falls on, in the display timezone.
    pub fn from_ms(timestamp_ms: i64, tz: TzOffset) -> Self {
        let local = timestamp_ms.saturating_add(tz.offset_ms());
        Self {
            day_number: local.div_euclid(DAY_MS),
        }
    }

    /// The date `year-month-day`, normalising out-of-range months and days
    /// the way the civil algorithm does (month 13 is January of the next
    /// year) — the calendar's month paging relies on it.
    pub fn from_ymd(year: i64, month: i64, day: i64) -> Self {
        Self {
            day_number: days_from_civil(year, month, day),
        }
    }

    /// The date with this day number, counted from 1970-01-01 — the inverse
    /// of [`Self::day_number`], for an index keyed by it.
    #[must_use]
    pub const fn from_day_number(day_number: i64) -> Self {
        Self { day_number }
    }

    /// `(year, month, day)`.
    pub fn ymd(self) -> (i64, i64, i64) {
        civil_from_days(self.day_number)
    }

    /// Day number from 1970-01-01 — the sort key and the day index's key.
    pub fn day_number(self) -> i64 {
        self.day_number
    }

    /// This date `count` days later (negative walks back).
    pub fn offset_days(self, count: i64) -> Self {
        Self {
            day_number: self.day_number.saturating_add(count),
        }
    }

    /// First venue timestamp that belongs to this civil date, inclusive.
    pub fn start_ms(self, tz: TzOffset) -> i64 {
        self.day_number
            .saturating_mul(DAY_MS)
            .saturating_sub(tz.offset_ms())
    }

    /// First venue timestamp *after* this civil date — the exclusive end,
    /// so a trade printed at 23:59:59.999 local is inside and the next
    /// day's 00:00:00.000 is not.
    pub fn end_ms(self, tz: TzOffset) -> i64 {
        self.offset_days(1).start_ms(tz)
    }

    /// `2026-08-17` — the unambiguous stamp every surface prints.
    pub fn iso(self) -> String {
        let (year, month, day) = self.ymd();
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// `17 Aug` — the compact stamp for a row that already sits under a
    /// year-qualified day header. Short on purpose: the characters it does
    /// not spend are characters the exit reason beside it gets to keep.
    pub fn short(self) -> String {
        let (_, month, day) = self.ymd();
        format!("{day:02} {}", month_abbr(month))
    }

    /// `Mon 17 Aug 2026` — the ledger's day header.
    pub fn long(self) -> String {
        let (year, month, day) = self.ymd();
        format!(
            "{} {day:02} {} {year:04}",
            weekday_abbr(self.weekday()),
            month_abbr(month)
        )
    }

    /// Weekday, 0 = Monday … 6 = Sunday. 1970-01-01 was a Thursday, which
    /// is why the epoch day number is shifted by three before the modulo.
    pub fn weekday(self) -> i64 {
        (self.day_number + 3).rem_euclid(WEEK_DAYS)
    }

    /// The first day of this date's month — where a month grid starts.
    pub fn month_start(self) -> Self {
        let (year, month, _) = self.ymd();
        Self::from_ymd(year, month, 1)
    }

    /// The month `count` months later (negative walks back), clamped to
    /// the first of the month: paging never lands on the 31st of a month
    /// that has 30 days.
    pub fn offset_months(self, count: i64) -> Self {
        let (year, month, _) = self.ymd();
        let zero_based = (year * 12 + month - 1).saturating_add(count);
        Self::from_ymd(zero_based.div_euclid(12), zero_based.rem_euclid(12) + 1, 1)
    }

    /// Whether this date shares a month with `other`.
    pub fn same_month(self, other: Self) -> bool {
        let (year, month, _) = self.ymd();
        let (other_year, other_month, _) = other.ymd();
        (year, month) == (other_year, other_month)
    }
}

/// `YYYY-MM-DD HH:MM` in the display timezone — the equity curve's hover
/// stamp, on the same clock as every trade row beneath it.
///
/// Here rather than beside the report's formatters because it is civil-date
/// arithmetic, and this module is where that law lives.
pub fn fmt_offset_minute(timestamp_ms: i64, tz: TzOffset) -> String {
    let (year, month, day, hour, minute, _) =
        civil_utc(timestamp_ms.saturating_add(tz.offset_ms()));
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// `Jan`…`Dec`; anything outside 1..=12 is a bug upstream, and `???` says
/// so rather than panicking inside a paint.
fn month_abbr(month: i64) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    usize::try_from(month - 1)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("???")
}

/// `Mon`…`Sun` from [`CivilDate::weekday`].
pub fn weekday_abbr(weekday: i64) -> &'static str {
    const NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    usize::try_from(weekday)
        .ok()
        .and_then(|index| NAMES.get(index))
        .copied()
        .unwrap_or("???")
}

/// An inclusive span of civil days.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateRange {
    pub start: CivilDate,
    pub end: CivilDate,
}

impl DateRange {
    /// The span between two days, in either click order.
    pub fn new(a: CivilDate, b: CivilDate) -> Self {
        if a <= b {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    /// Whether a venue timestamp falls inside — start inclusive, end
    /// inclusive to the last millisecond of its civil day.
    pub fn contains_ms(self, timestamp_ms: i64, tz: TzOffset) -> bool {
        timestamp_ms >= self.start.start_ms(tz) && timestamp_ms < self.end.end_ms(tz)
    }

    /// How many civil days the span covers, both ends counted.
    pub fn days(self) -> i64 {
        self.end.day_number() - self.start.day_number() + 1
    }

    /// `2026-08-17` for a single day, `2026-08-12 to 2026-08-17` for a
    /// span — what the report's support line reads out loud.
    ///
    /// Spelled with a word, not an arrow: this string is rendered in the
    /// proportional UI font, whose fallback has no `→` and draws a tofu
    /// box instead. The arrow survives where the text is monospace (the
    /// trade list's `ENTRY → EXIT`, a ledger row's round trip); here it
    /// would be a missing glyph in the one label naming the filter.
    pub fn label(self) -> String {
        if self.start == self.end {
            self.start.iso()
        } else {
            format!("{} to {}", self.start.iso(), self.end.iso())
        }
    }
}

/// Read a `YYYY-MM-DD` date, refusing anything that is not one. The
/// parse must round-trip: `2026-02-30` normalises to March 2nd inside the
/// civil algorithm, and silently answering a question nobody asked is
/// exactly the guess this refuses to make.
pub fn parse_iso_date(text: &str) -> Option<CivilDate> {
    let mut parts = text.trim().split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next()?.parse().ok()?;
    let day: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let date = CivilDate::from_ymd(year, month, day);
    (date.ymd() == (year, month, day)).then_some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_formats_sign_hours_and_minutes() {
        assert_eq!(TzOffset::new(0).label(), "UTC");
        assert_eq!(TzOffset::new(-180).label(), "UTC-03:00");
        assert_eq!(TzOffset::new(330).label(), "UTC+05:30");
        assert_eq!(TzOffset::new(345).label(), "UTC+05:45");
        assert_eq!(TzOffset::new(840).label(), "UTC+14:00");
    }

    #[test]
    fn offset_ms_scales_minutes() {
        assert_eq!(TzOffset::new(-180).offset_ms(), -10_800_000);
        assert_eq!(TzOffset::new(0).offset_ms(), 0);
        assert_eq!(TzOffset::new(330).offset_ms(), 19_800_000);
    }

    #[test]
    fn default_is_utc_minus_three() {
        assert_eq!(TzOffset::default(), TzOffset::new(-180));
        assert_eq!(TzOffset::default().label(), "UTC-03:00");
    }

    #[test]
    fn all_offsets_are_sorted_and_include_utc_and_home() {
        for pair in TzOffset::ALL.windows(2) {
            assert!(pair[0].minutes < pair[1].minutes, "ALL must be ascending");
        }
        assert!(TzOffset::ALL.contains(&TzOffset::new(0)), "UTC present");
        assert!(
            TzOffset::ALL.contains(&TzOffset::default()),
            "home zone present"
        );
    }

    fn utc() -> TzOffset {
        TzOffset::new(0)
    }

    /// The trader's own timezone — every off-by-one this module could have
    /// shows up here first.
    fn sao_paulo() -> TzOffset {
        TzOffset::new(-180)
    }

    /// Travelled here with `fmt_offset_minute`: the curve's hover stamp
    /// and the ledger's rows must read on one clock, and that clock is
    /// this module's.
    #[test]
    fn display_stamps_read_on_the_display_clock() {
        assert_eq!(
            fmt_offset_minute(1_773_666_068_000, utc()),
            "2026-03-16 13:01"
        );
        assert_eq!(
            fmt_offset_minute(1_773_666_068_000, sao_paulo()),
            "2026-03-16 10:01",
            "the curve's stamp reads on the display clock"
        );
    }

    #[test]
    fn civil_days_round_trip_through_the_epoch_and_back() {
        for day in [-100_000_i64, -1, 0, 1, 19_952, 100_000] {
            let (year, month, date) = civil_from_days(day);
            assert_eq!(
                days_from_civil(year, month, date),
                day,
                "{year:04}-{month:02}-{date:02} must come back to day {day}"
            );
        }
    }

    #[test]
    fn a_date_knows_its_own_midnight_bounds() {
        let date = CivilDate::from_ymd(2026, 8, 17);
        assert_eq!(date.iso(), "2026-08-17");
        assert_eq!(date.end_ms(utc()) - date.start_ms(utc()), DAY_MS);
        // The last millisecond of the day is inside; the next is not.
        let range = DateRange {
            start: date,
            end: date,
        };
        assert!(range.contains_ms(date.end_ms(utc()) - 1, utc()));
        assert!(!range.contains_ms(date.end_ms(utc()), utc()));
        assert!(range.contains_ms(date.start_ms(utc()), utc()));
        assert!(!range.contains_ms(date.start_ms(utc()) - 1, utc()));
    }

    #[test]
    fn a_late_utc_print_belongs_to_the_previous_day_in_sao_paulo() {
        // 2026-08-18T01:30:00Z is 2026-08-17T22:30 local at UTC-03:00.
        let timestamp = CivilDate::from_ymd(2026, 8, 18).start_ms(utc()) + 90 * 60_000;
        assert_eq!(CivilDate::from_ms(timestamp, utc()).iso(), "2026-08-18");
        assert_eq!(
            CivilDate::from_ms(timestamp, sao_paulo()).iso(),
            "2026-08-17",
            "the calendar must highlight the day the trader saw on the clock"
        );
    }

    #[test]
    fn month_paging_never_lands_on_a_day_the_month_does_not_have() {
        let january_31 = CivilDate::from_ymd(2026, 1, 31);
        assert_eq!(january_31.offset_months(1).iso(), "2026-02-01");
        assert_eq!(january_31.offset_months(-1).iso(), "2025-12-01");
        assert_eq!(january_31.offset_months(12).iso(), "2027-01-01");
        assert_eq!(january_31.offset_months(-13).iso(), "2024-12-01");
    }

    #[test]
    fn weekdays_start_on_monday() {
        // 2026-08-17 is a Monday.
        assert_eq!(CivilDate::from_ymd(2026, 8, 17).weekday(), 0);
        assert_eq!(CivilDate::from_ymd(2026, 8, 23).weekday(), 6);
        assert_eq!(CivilDate::from_ymd(1970, 1, 1).weekday(), 3, "a Thursday");
        assert_eq!(CivilDate::from_ymd(1969, 12, 28).weekday(), 6, "a Sunday");
    }

    #[test]
    fn a_range_contains_every_millisecond_of_both_end_days() {
        let range = DateRange::new(
            CivilDate::from_ymd(2026, 8, 12),
            CivilDate::from_ymd(2026, 8, 17),
        );
        assert!(range.contains_ms(range.start.start_ms(sao_paulo()), sao_paulo()));
        assert!(range.contains_ms(range.end.end_ms(sao_paulo()) - 1, sao_paulo()));
        assert!(!range.contains_ms(range.start.start_ms(sao_paulo()) - 1, sao_paulo()));
        assert!(!range.contains_ms(range.end.end_ms(sao_paulo()), sao_paulo()));
    }

    #[test]
    fn dates_print_the_way_every_surface_reads_them() {
        let date = CivilDate::from_ymd(2026, 8, 17);
        assert_eq!(date.iso(), "2026-08-17");
        assert_eq!(date.short(), "17 Aug");
        assert_eq!(date.long(), "Mon 17 Aug 2026");
    }
}
