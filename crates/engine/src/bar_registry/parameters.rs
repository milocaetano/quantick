//! Neutral parameter contracts, shared by text, controls and configuration.
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::time_bucket::{
    CALENDAR_MONTH_MS, DAY_MS, MAX_CALENDAR_MONTHS, WEEK_MS, calendar_months,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberKind {
    Count,
    Decimal,
    Duration,
}

/// A numeric editor's domain and step, in the parameter's own units.
#[derive(Debug, Clone, Copy)]
pub struct NumberEditor {
    pub label: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub decimals: Option<usize>,
    pub hover: &'static str,
    pub presets: &'static [(&'static str, i64)],
}

#[derive(Debug, Clone, Copy)]
pub struct ParameterDescriptor {
    pub name: &'static str,
    pub unit: &'static str,
    pub kind: NumberKind,
    pub default: Decimal,
    /// The smallest value the rule is defined for, where that is more than
    /// any positive value of its kind — a Renko brick needs two ticks.
    pub minimum: Option<Decimal>,
    pub editor: NumberEditor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputRequirements {
    pub traded_volume: bool,
    pub deal_counter: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ChoiceDescriptor {
    pub id: &'static str,
    pub hover: &'static str,
    pub requirements: InputRequirements,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarConfigurationError {
    NotKindParameter { text: String },
    UnknownKind { kind: String },
    UnknownParameter { parameter: String },
    InvalidCount { kind: String, parameter: String },
    InvalidNumber { kind: String, parameter: String },
    BelowMinimum { kind: String, minimum: String },
    UnknownChoice { choice: String },
    InvalidInterval { parameter: String },
    IntervalOutOfRange { ms: i64, parameter: String },
    DuplicateKind { kind: String },
    LegacyKindUnavailable { kind: String },
    InvalidDefinition { kind: String, reason: &'static str },
}

impl std::fmt::Display for BarConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotKindParameter { text } => {
                write!(f, "'{text}' is not a kind:parameter bar spec")
            }
            Self::UnknownKind { kind } => write!(f, "unknown bar kind '{kind}'"),
            Self::UnknownParameter { parameter } => {
                write!(f, "unknown bar parameter '{parameter}'")
            }
            Self::InvalidCount { kind, parameter } => write!(
                f,
                "{kind} bars need a positive whole number, got '{parameter}'"
            ),
            Self::InvalidNumber { kind, parameter } => {
                write!(f, "{kind} bars need a positive number, got '{parameter}'")
            }
            Self::BelowMinimum { kind, minimum } => {
                write!(f, "{kind} bars need at least {minimum}")
            }
            Self::UnknownChoice { choice } => write!(f, "unknown bar parameter choice '{choice}'"),
            Self::InvalidInterval { parameter } => write!(
                f,
                "'{parameter}' is not a time interval, like '1m' or '30s'"
            ),
            Self::IntervalOutOfRange { parameter, .. } => write!(
                f,
                "time interval '{parameter}' is outside {}..={} and is not 1mo..={}mo",
                fmt_time_interval(MIN_TIME_INTERVAL_MS),
                fmt_time_interval(MAX_TIME_INTERVAL_MS),
                MAX_CALENDAR_MONTHS,
            ),
            Self::DuplicateKind { kind } => write!(f, "duplicate bar registration '{kind}'"),
            Self::LegacyKindUnavailable { kind } => {
                write!(f, "bar kind '{kind}' has no legacy BarSpec representation")
            }
            Self::InvalidDefinition { kind, reason } => {
                write!(f, "invalid bar definition '{kind}': {reason}")
            }
        }
    }
}
impl std::error::Error for BarConfigurationError {}

pub const MIN_TIME_INTERVAL_MS: i64 = 100;
/// The longest *fixed* interval: four weeks. Longer bars are calendar months
/// (see [`crate::time_bucket`]), which [`is_time_interval`] admits beside the
/// fixed range.
pub const MAX_TIME_INTERVAL_MS: i64 = 4 * WEEK_MS;
pub const DEFAULT_TIME_INTERVAL_MS: i64 = 60_000;
pub const TIME_INTERVAL_DRAG_SPEED: f64 = 100.0;
pub const DECIMAL_PARAM_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 8);
pub const TIME_PRESETS: [(&str, i64); 7] = [
    ("1m", 60_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("1h", 3_600_000),
    ("1d", DAY_MS),
    ("1w", WEEK_MS),
    ("1mo", CALENDAR_MONTH_MS),
];

/// Whether `ms` is an interval a time bar accepts: a fixed duration inside
/// [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`], or a whole number of
/// calendar months up to a year.
#[must_use]
pub fn is_time_interval(ms: i64) -> bool {
    (MIN_TIME_INTERVAL_MS..=MAX_TIME_INTERVAL_MS).contains(&ms) || calendar_months(ms).is_some()
}

/// The units an interval is written in, longest first — the order
/// [`fmt_time_interval`] tries them. `mo` precedes `ms` and `m` so a suffix is
/// read whole.
const INTERVAL_UNITS: [(&str, i64); 7] = [
    ("mo", CALENDAR_MONTH_MS),
    ("w", WEEK_MS),
    ("d", DAY_MS),
    ("h", 3_600_000),
    ("m", 60_000),
    ("s", 1_000),
    ("ms", 1),
];

/// The scale of a unit suffix (`s`, `m`, `h`, `d`, `w`, `mo`), in either case.
#[must_use]
pub fn interval_unit_ms(suffix: &str) -> Option<i64> {
    INTERVAL_UNITS
        .iter()
        .find(|(unit, _)| unit.eq_ignore_ascii_case(suffix))
        .map(|(_, scale)| *scale)
}

impl ParameterDescriptor {
    pub fn parse(&self, id: &str, text: &str) -> Result<Decimal, BarConfigurationError> {
        let value = match self.kind {
            NumberKind::Count => text
                .parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .map(Decimal::from)
                .ok_or_else(|| BarConfigurationError::InvalidCount {
                    kind: id.to_owned(),
                    parameter: text.to_owned(),
                })?,
            NumberKind::Decimal => text
                .parse::<Decimal>()
                .ok()
                .filter(|n| *n > Decimal::ZERO)
                .ok_or_else(|| BarConfigurationError::InvalidNumber {
                    kind: id.to_owned(),
                    parameter: text.to_owned(),
                })?,
            NumberKind::Duration => Decimal::from(parse_interval(text)?),
        };
        self.check_minimum(id, value)?;
        Ok(value)
    }

    fn check_minimum(&self, id: &str, value: Decimal) -> Result<(), BarConfigurationError> {
        match self.minimum {
            Some(minimum) if value < minimum => Err(BarConfigurationError::BelowMinimum {
                kind: id.to_owned(),
                minimum: format!("{minimum} {}", self.unit),
            }),
            _ => Ok(()),
        }
    }

    pub fn validate(&self, id: &str, value: Decimal) -> Result<(), BarConfigurationError> {
        let valid = value > Decimal::ZERO
            && match self.kind {
                NumberKind::Count => value.fract().is_zero() && value.to_u64().is_some(),
                NumberKind::Decimal => true,
                NumberKind::Duration => value.fract().is_zero() && value.to_i64().is_some(),
            };
        if !valid {
            return Err(match self.kind {
                NumberKind::Count => BarConfigurationError::InvalidCount {
                    kind: id.to_owned(),
                    parameter: value.to_string(),
                },
                NumberKind::Decimal => BarConfigurationError::InvalidNumber {
                    kind: id.to_owned(),
                    parameter: value.to_string(),
                },
                NumberKind::Duration => BarConfigurationError::InvalidInterval {
                    parameter: value.to_string(),
                },
            });
        }
        if self.kind == NumberKind::Duration {
            let ms = value.to_i64().expect("validated interval");
            if !is_time_interval(ms) {
                return Err(BarConfigurationError::IntervalOutOfRange {
                    ms,
                    parameter: value.to_string(),
                });
            }
        }
        self.check_minimum(id, value)
    }

    pub fn format(&self, value: Decimal) -> String {
        if self.kind == NumberKind::Duration {
            fmt_time_interval(value.to_i64().expect("interval representation"))
        } else {
            value.to_string()
        }
    }
}

/// An interval written as [`fmt_time_interval`] writes it — `5m`, `1d`,
/// `1mo` — or as a bare millisecond count; `None` for anything else.
#[must_use]
pub fn parse_time_interval(text: &str) -> Option<i64> {
    parse_interval(text.trim()).ok()
}

fn parse_interval(text: &str) -> Result<i64, BarConfigurationError> {
    // A bare number is milliseconds; a suffix must be a unit, written in
    // lower case as `fmt_time_interval` writes it, or the text is no interval.
    let digits = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &text[digits.len()..];
    let scale = if suffix.is_empty() {
        Some(1)
    } else {
        INTERVAL_UNITS
            .iter()
            .find(|(unit, _)| *unit == suffix)
            .map(|(_, scale)| *scale)
    };
    let ms = scale
        .and_then(|scale| digits.parse::<i64>().ok()?.checked_mul(scale))
        .filter(|v| *v > 0)
        .ok_or_else(|| BarConfigurationError::InvalidInterval {
            parameter: text.to_owned(),
        })?;
    if !is_time_interval(ms) {
        return Err(BarConfigurationError::IntervalOutOfRange {
            ms,
            parameter: text.to_owned(),
        });
    }
    Ok(ms)
}

/// An interval in the largest unit that writes it back exactly: `1mo`, `1w`,
/// `2d`, `36h`, `5m`, `90s`, `1500ms`. A month is named only where
/// [`calendar_months`] reads one, so `30d` stays thirty days.
pub fn fmt_time_interval(ms: i64) -> String {
    if let Some(months) = calendar_months(ms) {
        return format!("{months}mo");
    }
    INTERVAL_UNITS[1..]
        .iter()
        .find(|(_, scale)| ms >= *scale && ms % scale == 0)
        .map_or_else(
            || format!("{ms}ms"),
            |(unit, scale)| format!("{}{unit}", ms / scale),
        )
}
