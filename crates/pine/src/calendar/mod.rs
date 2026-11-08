//! Deterministic Gregorian calendar fields from supplied epoch milliseconds.
use quantick_indicators::IndicatorBar;
use std::{fmt, str::FromStr};

/// A validated fixed offset from UTC. The default is UTC; there is no host timezone.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CalendarTimezone {
    minutes: i64,
}

/// Unsupported timezone spelling or offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimezoneError;
impl fmt::Display for TimezoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("timezone must be UTC or UTC+HH:MM/UTC-HH:MM (00:00 through 23:59)")
    }
}
impl std::error::Error for TimezoneError {}
impl FromStr for CalendarTimezone {
    type Err = TimezoneError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "UTC" {
            return Ok(Self::default());
        }
        let b = value.as_bytes();
        if b.len() != 9
            || &b[..3] != b"UTC"
            || !matches!(b[3], b'+' | b'-')
            || b[6] != b':'
            || ![b[4], b[5], b[7], b[8]].iter().all(u8::is_ascii_digit)
        {
            return Err(TimezoneError);
        }
        let hours = i64::from(b[4] - b'0') * 10 + i64::from(b[5] - b'0');
        let minutes = i64::from(b[7] - b'0') * 10 + i64::from(b[8] - b'0');
        if hours > 23 || minutes > 59 {
            return Err(TimezoneError);
        }
        Ok(Self {
            minutes: (hours * 60 + minutes) * if b[3] == b'-' { -1 } else { 1 },
        })
    }
}

/// Calendar component, shared by the variable and function registrations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarField {
    /// Proleptic Gregorian year.
    Year,
    /// Month, January = 1.
    Month,
    /// Day of the month, starting at 1.
    DayOfMonth,
    /// Sunday = 1, Saturday = 7.
    DayOfWeek,
    /// Hour, 0 through 23.
    Hour,
    /// Minute, 0 through 59.
    Minute,
}

impl CalendarField {
    pub(crate) fn at(self, timestamp: i64, zone: CalendarTimezone) -> f64 {
        // Divide before adding the offset to avoid overflow at either i64 endpoint.
        let minutes = timestamp.div_euclid(60_000) + zone.minutes;
        let days = minutes.div_euclid(1440);
        let minute = minutes.rem_euclid(1440);
        if matches!(self, Self::Hour) {
            return (minute / 60) as f64;
        }
        if matches!(self, Self::Minute) {
            return (minute % 60) as f64;
        }
        if matches!(self, Self::DayOfWeek) {
            return (days + 4).rem_euclid(7) as f64 + 1.0;
        }
        // Decompose complete Gregorian 400-year eras, then March-based years.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        (match self {
            Self::Year => year,
            Self::Month => month,
            _ => day,
        }) as f64
    }
}

/// Open timestamp in epoch milliseconds, without consulting a clock.
pub(crate) fn open_time(bar: &IndicatorBar) -> f64 {
    bar.open_time as f64
}
/// Close timestamp in epoch milliseconds, without inferring an interval.
pub(crate) fn close_time(bar: &IndicatorBar) -> f64 {
    bar.close_time as f64
}

pub(crate) fn timestamp(value: f64) -> Result<i64, &'static str> {
    // Function arguments use floating-point Pine numbers; only exact integer ms are accepted.
    if !value.is_finite() || value.fract() != 0.0 || value.abs() > 9_007_199_254_740_991.0 {
        return Err(
            "calendar timestamp must be an exact integer in the safe epoch-millisecond range",
        );
    }
    Ok(value as i64)
}

pub(crate) fn validate_arguments(args: &[crate::ast::Arg]) -> Result<(), &'static str> {
    if !(1..=2).contains(&args.len()) || args.iter().any(|arg| arg.name.is_some()) {
        return Err("calendar calls require timestamp and optional timezone positional arguments");
    }
    Ok(())
}

pub(crate) fn call(
    field: CalendarField,
    value: crate::types::Value,
    zone: Option<crate::types::Value>,
    default: CalendarTimezone,
) -> Result<crate::types::Value, String> {
    use crate::types::Value;
    let zone = match zone {
        None => default,
        Some(Value::Str(zone)) => zone.parse().map_err(|e: TimezoneError| e.to_string())?,
        Some(_) => return Err("calendar timezone must be a string".into()),
    };
    if value.is_na() {
        return Ok(Value::Na);
    }
    let Value::Num(value) = value else {
        return Err("calendar timestamp must be epoch milliseconds".into());
    };
    Ok(Value::Num(field.at(timestamp(value)?, zone)))
}
