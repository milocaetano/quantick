//! Display timezone: a fixed UTC offset applied only when rendering trade times.
//!
//! The engine works purely in UTC epoch-milliseconds and never sees this — the
//! determinism rule is untouched. Choosing a timezone is a pure presentation
//! concern: pick an offset and the time axis relabels. Offsets are stored in
//! whole minutes so fractional zones (India UTC+05:30, Nepal UTC+05:45) stay
//! exact rather than being silently rounded (data-honesty rule).

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
}
