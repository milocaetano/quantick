//! The bar vocabulary: which rule cuts bars, and its parameter, as a value and
//! as the `kind:parameter` string a person types — `tick:50`, `volume:5`,
//! `dollar:500000`, `time:1m`, `imbalance:volume:500`, `trades:2000`.
//!
//! Legacy value vocabulary. Definitions, parsing and construction belong to
//! [`crate::bar_registry`]; the chart and runner retain its resolved configurations.
//! This closed enum remains an adapter for callers of the original API.

use rust_decimal::Decimal;

use crate::bar_registry::{BUILTIN_BARS, BarConfiguration, BarConfigurationError};
pub use crate::bar_registry::{
    DECIMAL_PARAM_FLOOR, DEFAULT_TIME_INTERVAL_MS, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS,
    fmt_time_interval,
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

    /// The spec this kind opens on when nothing has chosen a parameter for it
    /// yet — the value a fresh chart, and every kind the trader has not
    /// visited, starts from.
    ///
    /// The default comes from the registered definition, like every consumer's.
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

    /// Whether this rule counts the venue's deals, and so needs a feed whose
    /// prints come with that count.
    ///
    /// A consumer without a counter does not offer the kind rather than grey
    /// it out: without one the rule has nothing to cut on, and an entry that
    /// can never work would only invite the reading "tick is the same thing",
    /// which is the misunderstanding this kind exists to correct.
    #[must_use]
    pub fn needs_deal_counter(self) -> bool {
        BUILTIN_BARS
            .find(self.label())
            .expect("built-in registration")
            .requirements
            .deal_counter
    }

    /// Whether this rule measures traded size, and so needs a venue that
    /// prints one.
    ///
    /// Tick, time and imbalance bars all count *events*: imbalance in its
    /// default trades unit sums a signed ±1 per trade (López de Prado's tick
    /// imbalance bars), never a quantity. They stay meaningful on a
    /// quote-driven feed. Volume and dollar bars do not — fed one synthetic
    /// unit per tick, a "volume 500" bar is a 500-tick bar wearing a
    /// misleading label. The imbalance *kind* answers for that default: its
    /// volume/dollar units measure size too, and a consumer gates them per
    /// feed exactly as this method gates the kinds.
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
    /// N milliseconds per bar, inside
    /// [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`] when parsed.
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

    /// This spec with its parameter held to what the builders will accept.
    ///
    /// A spec can hold a zero — a workspace written by an older build, a
    /// config file, or a control call that asked for one — and a zero-length
    /// bar rule is not a rule: a volume bar that closes on no volume closes on
    /// every trade. The floors are `1` for the counted kinds and
    /// [`DECIMAL_PARAM_FLOOR`] for the two measured in `Decimal`.
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

    /// The interval this spec cuts bars at, when it cuts by time at all.
    ///
    /// Only a time spec has one: a tick or volume bar covers whatever span its
    /// count happened to take, which is not an interval anything can be folded
    /// to.
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

    /// This spec in the `kind:parameter` vocabulary [`Self::parse`] reads —
    /// the form a feeds configuration's `default_bars`, a saved workspace and
    /// the backtest's `--bars` all use.
    ///
    /// The round trip is the point: whatever a chart is showing, a config or a
    /// workspace file can ask for by name.
    #[must_use]
    pub fn to_config_string(&self) -> String {
        BarConfiguration::from(*self).to_config_string()
    }

    /// Parse a `kind:parameter` spec string: `tick:50`, `trades:2000`,
    /// `volume:5`, `dollar:500000`, `imbalance:100` (also
    /// `imbalance:volume:500` / `imbalance:dollar:500` to pick what θ
    /// accumulates), `time:1m` (also `time:30s`, `time:1h`, `time:1500ms` or a
    /// bare millisecond count).
    ///
    /// Every rule a chart control enforces holds here too — a positive
    /// parameter, and a time interval inside
    /// [`MIN_TIME_INTERVAL_MS`]..=[`MAX_TIME_INTERVAL_MS`] — so a config or a
    /// command line cannot ask for a chart no control could have produced.
    ///
    /// # Errors
    ///
    /// A [`BarSpecError`] naming the reason as a variant; its `Display` is the
    /// human-readable sentence, with the accepted forms, for the caller to
    /// surface verbatim.
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

/// Why a `kind:parameter` string is not a [`BarSpec`].
///
/// The variant is the reason, for a caller that repairs its own request — a
/// control call, a script — without reading prose. `Display` is the sentence a
/// person reads, the same one the config loader and the backtest have always
/// printed.
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
                "time interval '{param}' is outside {}..={} — the domain both time-bar \
                 controls accept",
                fmt_time_interval(MIN_TIME_INTERVAL_MS),
                fmt_time_interval(MAX_TIME_INTERVAL_MS),
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
