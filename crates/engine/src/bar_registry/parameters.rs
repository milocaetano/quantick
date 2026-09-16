//! Neutral parameter contracts, shared by text, controls and configuration.
use rust_decimal::{Decimal, prelude::ToPrimitive};

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
            Self::UnknownChoice { choice } => write!(f, "unknown bar parameter choice '{choice}'"),
            Self::InvalidInterval { parameter } => write!(
                f,
                "'{parameter}' is not a time interval, like '1m' or '30s'"
            ),
            Self::IntervalOutOfRange { parameter, .. } => {
                write!(f, "time interval '{parameter}' is outside 100ms..=24h")
            }
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
pub const MAX_TIME_INTERVAL_MS: i64 = 86_400_000;
pub const DEFAULT_TIME_INTERVAL_MS: i64 = 60_000;
pub const TIME_INTERVAL_DRAG_SPEED: f64 = 100.0;
pub const DECIMAL_PARAM_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 8);
pub const TIME_PRESETS: [(&str, i64); 4] = [
    ("1m", 60_000),
    ("5m", 300_000),
    ("15m", 900_000),
    ("1h", 3_600_000),
];

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
        Ok(value)
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
            if !(MIN_TIME_INTERVAL_MS..=MAX_TIME_INTERVAL_MS).contains(&ms) {
                return Err(BarConfigurationError::IntervalOutOfRange {
                    ms,
                    parameter: value.to_string(),
                });
            }
        }
        Ok(())
    }

    pub fn format(&self, value: Decimal) -> String {
        if self.kind == NumberKind::Duration {
            fmt_time_interval(value.to_i64().expect("interval representation"))
        } else {
            value.to_string()
        }
    }
}

fn parse_interval(text: &str) -> Result<i64, BarConfigurationError> {
    let (digits, scale) = if let Some(v) = text.strip_suffix("ms") {
        (v, 1)
    } else if let Some(v) = text.strip_suffix('h') {
        (v, 3_600_000)
    } else if let Some(v) = text.strip_suffix('m') {
        (v, 60_000)
    } else if let Some(v) = text.strip_suffix('s') {
        (v, 1_000)
    } else {
        (text, 1)
    };
    let ms = digits
        .parse::<i64>()
        .ok()
        .and_then(|v| v.checked_mul(scale))
        .filter(|v| *v > 0)
        .ok_or_else(|| BarConfigurationError::InvalidInterval {
            parameter: text.to_owned(),
        })?;
    if !(MIN_TIME_INTERVAL_MS..=MAX_TIME_INTERVAL_MS).contains(&ms) {
        return Err(BarConfigurationError::IntervalOutOfRange {
            ms,
            parameter: text.to_owned(),
        });
    }
    Ok(ms)
}

pub fn fmt_time_interval(ms: i64) -> String {
    if ms >= 3_600_000 && ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms >= 60_000 && ms % 60_000 == 0 {
        format!("{}m", ms / 60_000)
    } else if ms >= 1_000 && ms % 1_000 == 0 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{ms}ms")
    }
}
