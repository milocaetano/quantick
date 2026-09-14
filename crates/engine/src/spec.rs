//! The bar vocabulary: which rule cuts bars, and its parameter, as a value and
//! as the `kind:parameter` string a person types — `tick:50`, `volume:5`,
//! `dollar:500000`, `time:1m`, `imbalance:volume:500`, `trades:2000`.
//!
//! One definition for every consumer. The chart, the backtest and the bot
//! read this [`BarSpec`] and build through its [`BarSpec::build`], so the spec
//! a trader reads off a tab is the spec a backtest runs, and "same trades in,
//! same bars out" holds across consumers, not only within one. A consumer that
//! cannot honour a kind — the backtest has no deal counter to cut
//! [`BarSpec::Trades`] on — refuses it at its own boundary; it never keeps a
//! vocabulary of its own.

use rust_decimal::Decimal;

use crate::{
    BarBuilder, DealBarBuilder, DollarBarBuilder, ImbalanceBarBuilder, ImbalanceUnit,
    TickBarBuilder, TimeBarBuilder, VolumeBarBuilder,
};

/// Smallest interval a time-bar spec may ask for, in milliseconds.
///
/// A tenth of a second is already finer than any venue's own bar; below it
/// the series is a tick chart wearing a clock.
pub const MIN_TIME_INTERVAL_MS: i64 = 100;

/// Largest interval a time-bar spec may ask for, in milliseconds — one day,
/// the coarsest that still fits inside a session.
pub const MAX_TIME_INTERVAL_MS: i64 = 86_400_000;

/// The interval a time-bar spec opens on when nothing has chosen one: a real
/// timeframe, not a one-second chart.
pub const DEFAULT_TIME_INTERVAL_MS: i64 = 60_000;

/// The smallest a `Decimal` bar parameter — volume units, dollar notional — is
/// allowed to be.
///
/// Not zero: a bar rule that closes on no quantity closes on every trade, and
/// the chart that produces is not what anyone asked for. Small enough that no
/// parameter a trader would choose is touched by it.
pub const DECIMAL_PARAM_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 8);

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
    /// All bar kinds, for building a selector.
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
    /// One arm per variant, and the only per-kind table of defaults: a
    /// consumer that retains a parameter per kind builds its set by mapping
    /// this over [`Self::ALL`], so a new kind's default is written here and
    /// nowhere else.
    #[must_use]
    pub fn default_spec(self) -> BarSpec {
        match self {
            BarKind::Tick => BarSpec::Tick(50),
            BarKind::Trades => BarSpec::Trades(2_000),
            BarKind::Volume => BarSpec::Volume(Decimal::from(5u64)),
            BarKind::Dollar => BarSpec::Dollar(Decimal::from(500_000u64)),
            BarKind::Time => BarSpec::Time(DEFAULT_TIME_INTERVAL_MS),
            BarKind::Imbalance => BarSpec::Imbalance(ImbalanceUnit::Trades, 100),
        }
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
        matches!(self, BarKind::Trades)
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
        match self {
            BarKind::Volume | BarKind::Dollar => true,
            BarKind::Tick | BarKind::Time | BarKind::Imbalance | BarKind::Trades => false,
        }
    }

    /// The unit the closing rule counts in, for the forming bar's countdown.
    #[must_use]
    pub fn progress_unit(self) -> &'static str {
        match self {
            BarKind::Tick | BarKind::Imbalance => "ticks",
            BarKind::Volume => "vol",
            BarKind::Dollar => "notional",
            BarKind::Time => "ms",
            BarKind::Trades => "deals",
        }
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
        match self {
            BarSpec::Tick(n) => BarSpec::Tick((*n).max(1)),
            BarSpec::Trades(n) => BarSpec::Trades((*n).max(1)),
            BarSpec::Time(ms) => BarSpec::Time((*ms).max(1)),
            BarSpec::Imbalance(unit, target) => BarSpec::Imbalance(*unit, (*target).max(1)),
            BarSpec::Volume(units) => BarSpec::Volume((*units).max(DECIMAL_PARAM_FLOOR)),
            BarSpec::Dollar(notional) => BarSpec::Dollar((*notional).max(DECIMAL_PARAM_FLOOR)),
        }
    }

    /// Construct the matching builder. This is the whole "bar rule → builder"
    /// dispatch: one place, every consumer of the engine.
    #[must_use]
    pub fn build(&self) -> Box<dyn BarBuilder> {
        match self {
            BarSpec::Tick(n) => Box::new(TickBarBuilder::new(*n)),
            BarSpec::Volume(units) => Box::new(VolumeBarBuilder::new(*units)),
            BarSpec::Dollar(notional) => Box::new(DollarBarBuilder::new(*notional)),
            BarSpec::Time(ms) => Box::new(TimeBarBuilder::new(*ms)),
            BarSpec::Imbalance(unit, target) => {
                Box::new(ImbalanceBarBuilder::with_unit(*target, *unit))
            }
            BarSpec::Trades(n) => Box::new(DealBarBuilder::new(*n)),
        }
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
        match self {
            BarSpec::Tick(n) => format!("tick({n})"),
            BarSpec::Volume(u) => format!("volume({u})"),
            BarSpec::Dollar(d) => format!("dollar({d})"),
            BarSpec::Time(ms) => format!("time({})", fmt_time_interval(*ms)),
            BarSpec::Imbalance(ImbalanceUnit::Trades, target) => format!("imbalance({target})"),
            BarSpec::Imbalance(unit, target) => format!("imbalance({} {target})", unit.as_str()),
            BarSpec::Trades(n) => format!("trades({n})"),
        }
    }

    /// This spec in the `kind:parameter` vocabulary [`Self::parse`] reads —
    /// the form a feeds configuration's `default_bars`, a saved workspace and
    /// the backtest's `--bars` all use.
    ///
    /// The round trip is the point: whatever a chart is showing, a config or a
    /// workspace file can ask for by name.
    #[must_use]
    pub fn to_config_string(&self) -> String {
        match self {
            BarSpec::Tick(n) => format!("tick:{n}"),
            BarSpec::Volume(units) => format!("volume:{units}"),
            BarSpec::Dollar(notional) => format!("dollar:{notional}"),
            BarSpec::Time(ms) => format!("time:{}", fmt_time_interval(*ms)),
            // The trades unit keeps its historical short form, so every spec
            // a workspace saved before units existed still reads back as the
            // same chart.
            BarSpec::Imbalance(ImbalanceUnit::Trades, target) => format!("imbalance:{target}"),
            BarSpec::Imbalance(unit, target) => format!("imbalance:{}:{target}", unit.as_str()),
            BarSpec::Trades(n) => format!("trades:{n}"),
        }
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
        let (kind, param) = text
            .split_once(':')
            .ok_or_else(|| BarSpecError::NotKindParameter {
                text: text.to_owned(),
            })?;
        let (kind, param) = (kind.trim(), param.trim());
        let positive_count = |kind: BarKind| -> Result<u64, BarSpecError> {
            match param.parse::<u64>() {
                Ok(n) if n > 0 => Ok(n),
                _ => Err(BarSpecError::NotPositiveCount {
                    kind,
                    param: param.to_owned(),
                }),
            }
        };
        let positive_decimal = |kind: BarKind| -> Result<Decimal, BarSpecError> {
            match param.parse::<Decimal>() {
                Ok(d) if d > Decimal::ZERO => Ok(d),
                _ => Err(BarSpecError::NotPositiveNumber {
                    kind,
                    param: param.to_owned(),
                }),
            }
        };
        match kind {
            "tick" => Ok(BarSpec::Tick(positive_count(BarKind::Tick)?)),
            "trades" => Ok(BarSpec::Trades(positive_count(BarKind::Trades)?)),
            "imbalance" => {
                // The parameter is `target` or `unit:target`. The unit picks
                // what θ accumulates; the target counts trades in every unit.
                let (unit, target) = match param.split_once(':') {
                    None => (ImbalanceUnit::Trades, param),
                    Some((token, target)) => {
                        let token = token.trim();
                        let unit = ImbalanceUnit::parse_token(token).ok_or_else(|| {
                            BarSpecError::UnknownImbalanceUnit {
                                unit: token.to_owned(),
                            }
                        })?;
                        (unit, target.trim())
                    }
                };
                match target.parse::<u64>() {
                    Ok(n) if n > 0 => Ok(BarSpec::Imbalance(unit, n)),
                    _ => Err(BarSpecError::NotPositiveTarget {
                        target: target.to_owned(),
                    }),
                }
            }
            "volume" => Ok(BarSpec::Volume(positive_decimal(BarKind::Volume)?)),
            "dollar" => Ok(BarSpec::Dollar(positive_decimal(BarKind::Dollar)?)),
            "time" => {
                let ms = parse_time_interval(param)?;
                if !(MIN_TIME_INTERVAL_MS..=MAX_TIME_INTERVAL_MS).contains(&ms) {
                    return Err(BarSpecError::IntervalOutOfRange {
                        ms,
                        param: param.to_owned(),
                    });
                }
                Ok(BarSpec::Time(ms))
            }
            _ => Err(BarSpecError::UnknownKind {
                kind: kind.to_owned(),
            }),
        }
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

/// Parse a time interval in the same vocabulary [`fmt_time_interval`] emits:
/// `1h`, `5m`, `90s`, `1500ms`, or a bare millisecond count. The round trip is
/// deliberate — whatever the status bar can say, a config can ask for.
fn parse_time_interval(text: &str) -> Result<i64, BarSpecError> {
    let parse_scaled = |digits: &str, scale: i64| -> Result<i64, BarSpecError> {
        digits
            .parse::<i64>()
            .ok()
            .and_then(|n| n.checked_mul(scale))
            .filter(|ms| *ms > 0)
            .ok_or_else(|| BarSpecError::NotAnInterval {
                text: text.to_owned(),
            })
    };
    // `ms` before `m` and `s`: the longest suffix owns the string.
    if let Some(digits) = text.strip_suffix("ms") {
        parse_scaled(digits, 1)
    } else if let Some(digits) = text.strip_suffix('h') {
        parse_scaled(digits, 3_600_000)
    } else if let Some(digits) = text.strip_suffix('m') {
        parse_scaled(digits, 60_000)
    } else if let Some(digits) = text.strip_suffix('s') {
        parse_scaled(digits, 1_000)
    } else {
        parse_scaled(text, 1)
    }
}

/// A time-bar interval for humans: `1m`, `5m`, `1h` for round units, `90s`
/// for whole seconds, raw milliseconds otherwise. The vocabulary the chart's
/// timeframe chips speak, so the status bar, the toolbar and the chips can
/// never disagree about what `60000` means.
#[must_use]
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
