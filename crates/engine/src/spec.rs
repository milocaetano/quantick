//! The bar vocabulary: which rule cuts bars and its parameter, as a value and as the
//! `kind:parameter` string (`tick:50`, `time:1m`, `imbalance:volume:500`).
//! Legacy adapter: definitions and parsing live in [`crate::bar_registry`].

use rust_decimal::Decimal;

use crate::bar_registry::{BUILTIN_BARS, BarConfiguration, BarConfigurationError};
pub use crate::bar_registry::{
    DECIMAL_PARAM_FLOOR, DEFAULT_TIME_INTERVAL_MS, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS,
    fmt_time_interval, parse_time_interval,
};
use crate::{BarBuilder, ImbalanceUnit};
use rust_decimal::prelude::ToPrimitive;

/// Which bar rule, without its parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// Close every N trades.
    Tick,
    /// Close every N units of traded quantity.
    Volume,
    /// Close every N notional (price × quantity).
    Dollar,
    /// Close every N milliseconds of trade time.
    Time,
    /// Close when aggressor imbalance beats an adaptive threshold
    /// (López de Prado tick imbalance bars).
    Imbalance,
    /// Close every N exchange deals, as the venue counts them — ProfitChart's
    /// *Trades* periodicity. Distinct from [`Tick`](Self::Tick) wherever a
    /// print folds several deals, which on MetaTrader is every print.
    Trades,
}

impl BarKind {
    /// All legacy enum variants. Selectors enumerate registered definitions.
    pub const ALL: [BarKind; 6] = [
        BarKind::Tick,
        BarKind::Volume,
        BarKind::Dollar,
        BarKind::Time,
        BarKind::Imbalance,
        BarKind::Trades,
    ];

    /// The spec this kind opens on when nothing chose a parameter; comes from the registered definition.
    #[must_use]
    pub fn default_spec(self) -> BarSpec {
        BarSpec::try_from(
            BUILTIN_BARS
                .find(self.label())
                .expect("built-in registration")
                .default_config(),
        )
        .expect("legacy kind")
    }

    /// A short display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BarKind::Tick => "tick",
            BarKind::Volume => "volume",
            BarKind::Dollar => "dollar",
            BarKind::Time => "time",
            BarKind::Imbalance => "imbalance",
            BarKind::Trades => "trades",
        }
    }

    /// Whether this rule counts the venue's deals and so needs a feed that reports that count.
    /// A consumer without one does not offer the kind: it has nothing to cut on.
    #[must_use]
    pub fn needs_deal_counter(self) -> bool {
        BUILTIN_BARS
            .find(self.label())
            .expect("built-in registration")
            .requirements
            .deal_counter
    }

    /// Whether this rule measures traded size and so needs a venue that prints one.
    /// Tick, time and imbalance (trades unit) count events and work on quote-driven feeds;
    /// volume and dollar do not. Imbalance volume/dollar units are gated per feed.
    #[must_use]
    pub fn needs_traded_volume(self) -> bool {
        BUILTIN_BARS
            .find(self.label())
            .expect("built-in registration")
            .requirements
            .traded_volume
    }

    /// The unit the closing rule counts in, for the forming bar's countdown.
    #[must_use]
    pub fn progress_unit(self) -> &'static str {
        BUILTIN_BARS
            .find(self.label())
            .expect("built-in registration")
            .progress_unit
    }
}

/// A bar rule together with its threshold parameter. A small value — a count,
/// a `Decimal` or an interval — so it is `Copy` and passes by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarSpec {
    /// N trades per bar.
    Tick(u64),
    /// N units of quantity per bar.
    Volume(Decimal),
    /// N notional per bar.
    Dollar(Decimal),
    /// N milliseconds per bar, within [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`] when
    /// parsed, or a whole number of [`crate::time_bucket::CALENDAR_MONTH_MS`] (calendar months).
    Time(i64),
    /// The adaptive imbalance rule: the measure θ accumulates (trades, volume
    /// or dollar — López de Prado's TIB/VIB/DIB) and the target trades per
    /// bar, which counts trades in every unit.
    Imbalance(ImbalanceUnit, u64),
    /// N exchange deals per bar, joined to prints through the venue's session
    /// counter, fed through [`BarBuilder::deal_counter_input`].
    Trades(u64),
}

impl BarSpec {
    /// The kind, discarding the parameter.
    #[must_use]
    pub fn kind(&self) -> BarKind {
        match self {
            BarSpec::Tick(_) => BarKind::Tick,
            BarSpec::Volume(_) => BarKind::Volume,
            BarSpec::Dollar(_) => BarKind::Dollar,
            BarSpec::Time(_) => BarKind::Time,
            BarSpec::Imbalance(..) => BarKind::Imbalance,
            BarSpec::Trades(_) => BarKind::Trades,
        }
    }

    /// This spec with its parameter held to what the builders accept: a zero-length rule would
    /// close on every trade. Floors are `1` for counted kinds, [`DECIMAL_PARAM_FLOOR`] for Decimal.
    #[must_use]
    pub fn clamped(&self) -> BarSpec {
        Self::try_from(BarConfiguration::from(*self).clamped()).expect("legacy kind")
    }

    /// Construct the matching builder. This is the whole "bar rule → builder"
    /// dispatch: one place, every consumer of the engine.
    #[must_use]
    pub fn build(&self) -> Box<dyn BarBuilder> {
        BarConfiguration::from(*self).build()
    }

    /// The interval this spec cuts bars at; only a time spec has one.
    #[must_use]
    pub fn time_interval_ms(&self) -> Option<i64> {
        match self {
            Self::Time(ms) => Some(*ms),
            _ => None,
        }
    }

    /// A human-readable summary, e.g. `tick(50)` or `time(1m)`.
    #[must_use]
    pub fn summary(&self) -> String {
        BarConfiguration::from(*self).summary()
    }

    /// This spec in the `kind:parameter` vocabulary [`Self::parse`] reads (configs, workspaces,
    /// `--bars`); the round trip is exact.
    #[must_use]
    pub fn to_config_string(&self) -> String {
        BarConfiguration::from(*self).to_config_string()
    }

    /// Parse `kind:parameter` (`tick:50`, `imbalance:volume:500`, `time:1m`) under every chart-control rule.
    /// # Errors
    /// A [`BarSpecError`]: the variant is the reason, `Display` the sentence to show.
    pub fn parse(text: &str) -> Result<Self, BarSpecError> {
        if let Some((kind, _)) = text.split_once(':')
            && !BarKind::ALL
                .iter()
                .any(|legacy| legacy.label() == kind.trim())
        {
            return Err(BarSpecError::UnknownKind {
                kind: kind.trim().to_owned(),
            });
        }
        let config = BUILTIN_BARS.parse(text).map_err(legacy_error)?;
        Self::try_from(config).map_err(legacy_error)
    }
}

/// Why a `kind:parameter` string is not a [`BarSpec`]; the variant is the machine-readable
/// reason and `Display` the sentence a person reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarSpecError {
    /// No `kind:parameter` shape at all.
    NotKindParameter {
        /// The whole text, as given.
        text: String,
    },
    /// A kind this vocabulary does not have.
    UnknownKind {
        /// The kind as given, trimmed.
        kind: String,
    },
    /// A counted kind (tick, trades) whose parameter is not a positive whole
    /// number.
    NotPositiveCount {
        /// Which kind was asked for.
        kind: BarKind,
        /// The parameter as given, trimmed.
        param: String,
    },
    /// A measured kind (volume, dollar) whose parameter is not a positive
    /// number.
    NotPositiveNumber {
        /// Which kind was asked for.
        kind: BarKind,
        /// The parameter as given, trimmed.
        param: String,
    },
    /// An imbalance unit other than trades, volume or dollar.
    UnknownImbalanceUnit {
        /// The unit as given, trimmed.
        unit: String,
    },
    /// An imbalance target that is not a positive whole number of trades.
    NotPositiveTarget {
        /// The target as given, trimmed.
        target: String,
    },
    /// A time parameter that is not a positive duration in the vocabulary
    /// [`fmt_time_interval`] speaks.
    NotAnInterval {
        /// The parameter as given, trimmed.
        text: String,
    },
    /// A duration outside [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`].
    IntervalOutOfRange {
        /// The duration it parsed to.
        ms: i64,
        /// The parameter as given, trimmed.
        param: String,
    },
}

impl std::fmt::Display for BarSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotKindParameter { text } => {
                write!(
                    f,
                    "'{text}' is not a kind:parameter bar spec, like 'time:1m'"
                )
            }
            Self::UnknownKind { kind } => write!(
                f,
                "unknown bar kind '{kind}'; one of tick, volume, dollar, time, imbalance, trades"
            ),
            Self::NotPositiveCount { kind, param } => write!(
                f,
                "{} bars need a positive whole number, got '{param}'",
                kind.label()
            ),
            Self::NotPositiveNumber { kind, param } => write!(
                f,
                "{} bars need a positive number, got '{param}'",
                kind.label()
            ),
            Self::UnknownImbalanceUnit { unit } => write!(
                f,
                "unknown imbalance unit '{unit}'; one of trades, volume, dollar"
            ),
            Self::NotPositiveTarget { target } => write!(
                f,
                "imbalance bars need a positive whole trade target, got '{target}'"
            ),
            Self::NotAnInterval { text } => {
                write!(f, "'{text}' is not a time interval, like '1m' or '30s'")
            }
            Self::IntervalOutOfRange { param, .. } => write!(
                f,
                "time interval '{param}' is outside {}..={} and is not 1mo..={}mo — the \
                 domain both time-bar controls accept",
                fmt_time_interval(MIN_TIME_INTERVAL_MS),
                fmt_time_interval(MAX_TIME_INTERVAL_MS),
                crate::time_bucket::MAX_CALENDAR_MONTHS,
            ),
        }
    }
}

impl std::error::Error for BarSpecError {}

impl AsRef<str> for BarKind {
    fn as_ref(&self) -> &str {
        self.label()
    }
}
impl From<BarKind> for &'static crate::bar_registry::BarDefinition {
    fn from(kind: BarKind) -> Self {
        BUILTIN_BARS
            .find(kind.label())
            .expect("built-in registration")
    }
}

/// The closed public enum is a compatibility codec, never the registry's
/// internal representation. Generic consumers keep BarConfiguration throughout.
impl From<BarSpec> for BarConfiguration {
    fn from(spec: BarSpec) -> Self {
        let (parameter, choice) = match spec {
            BarSpec::Tick(n) | BarSpec::Trades(n) => (Decimal::from(n), None),
            BarSpec::Volume(n) | BarSpec::Dollar(n) => (n, None),
            BarSpec::Time(ms) => (Decimal::from(ms), None),
            BarSpec::Imbalance(unit, n) => (Decimal::from(n), Some(unit.as_str())),
        };
        Self::legacy(
            BUILTIN_BARS
                .find(spec.kind().label())
                .expect("built-in registration"),
            parameter,
            choice,
        )
    }
}

impl TryFrom<BarConfiguration> for BarSpec {
    type Error = BarConfigurationError;
    fn try_from(config: BarConfiguration) -> Result<Self, Self::Error> {
        let canonical = BUILTIN_BARS.find(config.id()).ok();
        if !canonical.is_some_and(|definition| std::ptr::eq(definition, config.definition())) {
            return Err(BarConfigurationError::LegacyKindUnavailable {
                kind: config.id().to_owned(),
            });
        }
        let value = config.parameter();
        Ok(match config.id() {
            "tick" => Self::Tick(value.to_u64().expect("count representation")),
            "trades" => Self::Trades(value.to_u64().expect("count representation")),
            "volume" => Self::Volume(value),
            "dollar" => Self::Dollar(value),
            "time" => Self::Time(value.to_i64().expect("interval representation")),
            "imbalance" => Self::Imbalance(
                ImbalanceUnit::parse_token(config.choice().expect("imbalance unit"))
                    .expect("imbalance unit"),
                value.to_u64().expect("count representation"),
            ),
            kind => {
                return Err(BarConfigurationError::LegacyKindUnavailable {
                    kind: kind.to_owned(),
                });
            }
        })
    }
}
impl PartialEq<BarSpec> for BarConfiguration {
    fn eq(&self, other: &BarSpec) -> bool {
        *self == Self::from(*other)
    }
}
impl PartialEq<BarConfiguration> for BarSpec {
    fn eq(&self, other: &BarConfiguration) -> bool {
        other == self
    }
}

fn legacy_error(error: BarConfigurationError) -> BarSpecError {
    match error {
        BarConfigurationError::NotKindParameter { text } => BarSpecError::NotKindParameter { text },
        BarConfigurationError::UnknownKind { kind }
        | BarConfigurationError::LegacyKindUnavailable { kind } => {
            BarSpecError::UnknownKind { kind }
        }
        BarConfigurationError::InvalidCount { kind, parameter } if kind == "imbalance" => {
            BarSpecError::NotPositiveTarget { target: parameter }
        }
        BarConfigurationError::InvalidCount { kind, parameter } => BarSpecError::NotPositiveCount {
            kind: BarKind::ALL
                .into_iter()
                .find(|k| k.label() == kind)
                .expect("built-in kind"),
            param: parameter,
        },
        BarConfigurationError::InvalidNumber { kind, parameter } => {
            BarSpecError::NotPositiveNumber {
                kind: BarKind::ALL
                    .into_iter()
                    .find(|k| k.label() == kind)
                    .expect("built-in kind"),
                param: parameter,
            }
        }
        BarConfigurationError::UnknownChoice { choice } => {
            BarSpecError::UnknownImbalanceUnit { unit: choice }
        }
        BarConfigurationError::InvalidInterval { parameter } => {
            BarSpecError::NotAnInterval { text: parameter }
        }
        BarConfigurationError::IntervalOutOfRange { ms, parameter } => {
            BarSpecError::IntervalOutOfRange {
                ms,
                param: parameter,
            }
        }
        other => unreachable!("legacy parsing cannot produce {other}"),
    }
}
