//! Risk per trade: how many contracts the entry gets, before it is placed.
//!
//! The trader says what one trade may lose. The stop says where the loss
//! ends — placed with the wheel, by a saved strategy, or typed as an offset,
//! all three arriving as the same [`Bracket`]. This module turns the pair
//! into a quantity, and says plainly when it cannot.
//!
//! # Where the line with the kernel falls
//!
//! [`quantick_sim::size_for_risk`] owns the arithmetic and owns nothing
//! else. It reports what the numbers say — including "the smallest tradable
//! size already loses more than the budget" — and never decides whether an
//! entry may be placed. That decision is *policy*, it is the trader's lock,
//! and it lives here.
//!
//! The split is not tidiness. The same kernel has to serve a future
//! account-level ceiling ("max positioned risk" beside "risk per trade"),
//! and a kernel that had baked in one surface's refusal would have to be
//! forked to serve the second.
//!
//! # Risk per trade, not risk
//!
//! The name is deliberate and it is the whole vocabulary of this module. The
//! trader's stated direction is a *pair* of ceilings — a maximum for the
//! position and a maximum for each trade — so that entering small and
//! scaling in stays possible without the total ever passing the maximum.
//! This is the second of that pair. A bare "risk" would have to be renamed
//! the day the first lands, and renaming a persisted key is a migration
//! nobody should have to pay for a word.
//!
//! # One sentence, two readers
//!
//! [`RiskState::sentence`] is the only place the trader-facing wording
//! exists. The ticket renders it and the control plane publishes it, so an
//! operator without a mouse reads exactly what is on screen. Two functions
//! here would be two surfaces that drift.

use std::collections::BTreeMap;

use quantick_engine::Side;
use quantick_sim::{
    Bracket, Currency, InstrumentMoney, Money, SizeOutcome, SizeRefusal, size_for_risk,
};
use rust_decimal::Decimal;

/// How the risk per trade is expressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum RiskBasis {
    /// Off: the trader types the quantity, as they always have.
    #[default]
    Off,
    /// A fixed amount of money per trade.
    Amount,
    /// A percentage of the declared practice capital in the instrument's
    /// own currency.
    PercentOfCapital,
}

impl RiskBasis {
    /// The token this basis persists and travels as.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Amount => "amount",
            Self::PercentOfCapital => "percent",
        }
    }

    /// The basis `token` names, or `None` when it names none. An unknown
    /// token is not an error to shout about: it reads as "never set".
    pub(crate) fn from_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "amount" => Some(Self::Amount),
            "percent" => Some(Self::PercentOfCapital),
            _ => None,
        }
    }
}

/// What the trader set. Persisted in the paper-state sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RiskSettings {
    pub basis: RiskBasis,
    /// The fixed amount, when the basis is [`RiskBasis::Amount`].
    pub amount: Decimal,
    /// The percentage, when the basis is [`RiskBasis::PercentOfCapital`].
    pub percent: Decimal,
    /// Refuse an entry whose risk exceeds the budget.
    ///
    /// On by default, at the trader's own instruction: with a risk per trade
    /// set, there is to be no entry that exceeds it, and going past it is a
    /// deliberate act of turning this off rather than a wheel that quietly
    /// kept turning.
    pub lock: bool,
}

impl Default for RiskSettings {
    fn default() -> Self {
        Self {
            basis: RiskBasis::Off,
            amount: Decimal::ZERO,
            percent: Decimal::ZERO,
            lock: true,
        }
    }
}

/// What the trader set, and for which instrument.
///
/// Four values that always travel together: the risk per trade means
/// nothing without the money it is measured in, and the money means nothing
/// without the symbol it belongs to. Passing them separately let a caller
/// size one instrument against another's declaration - and it pushed the
/// sizing entry point past the argument count anyone can read.
pub(crate) struct RiskContext<'a> {
    pub settings: &'a RiskSettings,
    pub capital: &'a Capital,
    pub book: &'a InstrumentBook,
    pub symbol: &'a str,
}

/// Why no budget could be resolved at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BudgetRefusal {
    /// The mode is off; there is nothing to size against.
    NotSet,
    /// The fixed amount is zero or negative.
    AmountNotPositive,
    /// The percentage is zero or negative.
    PercentNotPositive,
    /// A percentage was asked for in a currency with no declared capital.
    /// Never converted from another currency: there is no exchange rate in
    /// this workspace and a converted capital would be a guess.
    NoCapital(Currency),
}

/// The practice capital, one amount per currency.
///
/// A map and not a number, from the first commit. A trader charting B3 in
/// reais and BTCUSDT in dollars has two capitals, and nothing here converts
/// between them — so nothing here may add them either. Keyed by the
/// currency code so the instrument's own currency selects which capital
/// applies.
pub(crate) type Capital = BTreeMap<String, Decimal>;

/// The declared money for each symbol, keyed by the bare symbol.
///
/// Bare, like `ruler_steps` beside it: what a point of an instrument is
/// worth describes the instrument, not who streams it, so a recorded
/// session must not make a trader type it again.
pub(crate) type InstrumentBook = BTreeMap<String, InstrumentMoney>;

/// One instrument's money as the sidecar stores it.
///
/// Decimals travel as strings, the sidecar's own convention beside
/// `ruler_steps`: a TOML float would round a point value the trader typed
/// exactly, and this is the one number in the file that must not drift.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InstrumentMoneyRecord {
    /// What one point of price is worth, per unit held.
    pub point_value: String,
    /// The smallest tradable increment of quantity.
    pub size_step: String,
    /// The currency the point value is in.
    pub currency: String,
    /// The smallest tradable quantity; absent reads as one step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_size: Option<String>,
    /// A ceiling on quantity, when the trader wants one. Absent is no
    /// ceiling, which is the honest default: this workspace is not told an
    /// instrument's real maximum.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_size: Option<String>,
}

impl InstrumentMoneyRecord {
    /// The money this record declares, or `None` when it declares nothing
    /// usable.
    ///
    /// A record that does not parse is dropped rather than repaired: a
    /// half-read point value would size a position, and guessing which half
    /// was meant is exactly the invention this feature exists to avoid. The
    /// symbol simply has no money, and the ticket says so.
    pub(crate) fn to_money(&self, symbol: &str) -> Option<InstrumentMoney> {
        let parse = |field: &str, text: &str| match Decimal::from_str_exact(text.trim()) {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INSTRUMENT_MONEY_UNREADABLE",
                    symbol,
                    field,
                    value = text,
                    action = "symbol_has_no_money",
                    "an instrument money field is not a number"
                );
                None
            }
        };
        let point_value = parse("point_value", &self.point_value)?;
        let size_step = parse("size_step", &self.size_step)?;
        let min_size = match &self.min_size {
            Some(text) => parse("min_size", text)?,
            None => size_step,
        };
        let max_size = match &self.max_size {
            Some(text) => Some(parse("max_size", text)?),
            None => None,
        };
        let currency = Currency::new(&self.currency)?;
        let money = InstrumentMoney {
            point_value,
            size_step,
            min_size,
            max_size,
            currency,
            source: quantick_sim::MoneySource::Declared,
        };
        // The same grid the kernel would refuse: caught here so a bad record
        // reads as "no money for this symbol" rather than as a refusal on
        // every aim.
        let usable = money.point_value > Decimal::ZERO
            && money.size_step > Decimal::ZERO
            && money.min_size > Decimal::ZERO
            && money.max_size.is_none_or(|max| max >= money.min_size);
        usable.then_some(money)
    }
}

/// The settings the sidecar holds, resolved with the defaults a trader who
/// never touched them should get.
///
/// A number that does not parse reads as never set, not as zero: a zero risk
/// per trade would size nothing and look like a bug, where "never set" is
/// the honest reading of a field the trader has not filled in.
pub(crate) fn settings_from_sidecar(
    basis: Option<&str>,
    amount: Option<&str>,
    percent: Option<&str>,
    lock: Option<bool>,
) -> RiskSettings {
    let decimal = |text: Option<&str>| {
        text.and_then(|text| Decimal::from_str_exact(text.trim()).ok())
            .unwrap_or(Decimal::ZERO)
    };
    RiskSettings {
        basis: basis.and_then(RiskBasis::from_token).unwrap_or_default(),
        amount: decimal(amount),
        percent: decimal(percent),
        // The lock stands until the trader takes it down. A risk per trade
        // that could be exceeded by default would not be one.
        lock: lock.unwrap_or(true),
    }
}

/// The book the ticket sizes against, from what the sidecar holds.
pub(crate) fn book_from_records(
    records: &BTreeMap<String, InstrumentMoneyRecord>,
) -> InstrumentBook {
    records
        .iter()
        .filter_map(|(symbol, record)| record.to_money(symbol).map(|money| (symbol.clone(), money)))
        .collect()
}

/// What the sidecar should hold for this book. The inverse of
/// [`book_from_records`], so a value survives a save-and-load unchanged.
pub(crate) fn records_from_book(book: &InstrumentBook) -> BTreeMap<String, InstrumentMoneyRecord> {
    book.iter()
        .map(|(symbol, money)| {
            (
                symbol.clone(),
                InstrumentMoneyRecord {
                    point_value: number(money.point_value),
                    size_step: number(money.size_step),
                    currency: money.currency.code().to_owned(),
                    min_size: Some(number(money.min_size)),
                    max_size: money.max_size.map(number),
                },
            )
        })
        .collect()
}

/// What the sidecar should hold for this capital.
pub(crate) fn records_from_capital(capital: &Capital) -> BTreeMap<String, String> {
    capital
        .iter()
        .map(|(code, amount)| (code.clone(), number(*amount)))
        .collect()
}

/// The capital map, from what the sidecar holds. An amount that does not
/// parse, or is not positive, is no capital at all.
pub(crate) fn capital_from_records(records: &BTreeMap<String, String>) -> Capital {
    records
        .iter()
        .filter_map(|(code, amount)| {
            let currency = Currency::new(code)?;
            let amount = Decimal::from_str_exact(amount.trim()).ok()?;
            (amount > Decimal::ZERO).then(|| (currency.code().to_owned(), amount))
        })
        .collect()
}

/// The budget one trade may lose, in the instrument's own currency.
///
/// # Errors
///
/// [`BudgetRefusal`] when the mode is off, the numbers are not positive, or
/// a percentage was asked for against a capital that was never declared.
pub(crate) fn budget_for(
    settings: &RiskSettings,
    capital: &Capital,
    currency: &Currency,
) -> Result<Money, BudgetRefusal> {
    match settings.basis {
        RiskBasis::Off => Err(BudgetRefusal::NotSet),
        RiskBasis::Amount => {
            if settings.amount <= Decimal::ZERO {
                return Err(BudgetRefusal::AmountNotPositive);
            }
            Ok(Money::new(settings.amount, currency.clone()))
        }
        RiskBasis::PercentOfCapital => {
            if settings.percent <= Decimal::ZERO {
                return Err(BudgetRefusal::PercentNotPositive);
            }
            let declared = capital
                .get(currency.code())
                .copied()
                .filter(|amount| *amount > Decimal::ZERO)
                .ok_or_else(|| BudgetRefusal::NoCapital(currency.clone()))?;
            let amount = declared
                .saturating_mul(settings.percent)
                .checked_div(Decimal::ONE_HUNDRED)
                .unwrap_or(Decimal::ZERO);
            if amount <= Decimal::ZERO {
                return Err(BudgetRefusal::PercentNotPositive);
            }
            Ok(Money::new(amount, currency.clone()))
        }
    }
}

/// What the ticket knows about the size of the entry it is about to place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RiskState {
    /// Fixed risk is off; the quantity is the trader's to type.
    Off,
    /// This symbol has no declared money, so nothing can be sized on it.
    NoInstrumentMoney { symbol: String },
    /// The budget itself could not be resolved.
    NoBudget(BudgetRefusal),
    /// The entry could not be sized against its protection.
    Refused(SizeRefusal),
    /// Sized, and inside the budget.
    Sized {
        quantity: Decimal,
        risk: Money,
        outcome: SizeOutcome,
        budget: Money,
    },
    /// The smallest tradable size already loses more than the budget. With
    /// the lock on, this entry does not go out.
    OverBudget {
        quantity: Decimal,
        risk: Money,
        budget: Money,
        /// The widest stop that would fit the budget at this size, in
        /// points, when one can be named.
        fits_within_points: Option<Decimal>,
    },
}

impl RiskState {
    /// The stable token this state travels as, for an operator reading the
    /// session rather than the screen.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::NoInstrumentMoney { .. } => "instrument_unknown",
            Self::NoBudget(_) => "no_budget",
            Self::Refused(_) => "refused",
            Self::Sized { outcome, .. } => match outcome {
                SizeOutcome::Exact => "sized",
                SizeOutcome::RoundedDown => "short_of_budget",
                SizeOutcome::CappedAtMax => "capped_at_max",
                // Unreachable: an over-budget floor is `OverBudget`, which
                // is the whole reason the two are separate states here.
                SizeOutcome::FlooredAboveBudget => "clamped_at_minimum",
            },
            Self::OverBudget { .. } => "clamped_at_minimum",
        }
    }

    /// The quantity the ticket should carry, when this state names one.
    pub(crate) fn derived_quantity(&self) -> Option<Decimal> {
        match self {
            Self::Sized { quantity, .. } | Self::OverBudget { quantity, .. } => Some(*quantity),
            _ => None,
        }
    }

    /// Whether an entry in this state is refused, given the trader's lock.
    ///
    /// Only the over-budget state is ever blocked, and only with the lock
    /// on. Everything else either sized cleanly or never took the quantity
    /// away from the trader in the first place.
    pub(crate) fn blocks_entry(&self, lock: bool) -> bool {
        lock && matches!(self, Self::OverBudget { .. })
    }

    /// The line the trader reads, and the line an operator reads. One
    /// function, so the two can never say different things.
    ///
    /// Written to sit small and quiet under the quantity field: it explains
    /// the number without taking the screen away from the chart.
    pub(crate) fn sentence(&self) -> String {
        match self {
            Self::Off => String::new(),
            Self::NoInstrumentMoney { symbol } => format!(
                "risk sizing is off for {symbol}: nothing here knows what one point is worth. \
                 Fill in the point value and the size step to turn it on."
            ),
            Self::NoBudget(refusal) => refusal.sentence(),
            Self::Refused(refusal) => refusal.advice().to_owned(),
            Self::Sized {
                quantity,
                risk,
                outcome,
                budget,
            } => match outcome {
                SizeOutcome::Exact => format!(
                    "{} at risk, the whole of your {} risk per trade",
                    money(risk),
                    money(budget)
                ),
                SizeOutcome::CappedAtMax => format!(
                    "{} is this instrument's largest size - {} at risk, inside your {}",
                    number(*quantity),
                    money(risk),
                    money(budget)
                ),
                SizeOutcome::RoundedDown | SizeOutcome::FlooredAboveBudget => format!(
                    "{} is the largest size inside your {} risk per trade - {} at risk",
                    number(*quantity),
                    money(budget),
                    money(risk)
                ),
            },
            Self::OverBudget {
                quantity,
                risk,
                budget,
                fits_within_points,
            } => {
                let tighten = match fits_within_points {
                    Some(points) => format!("Tighten the stop to {} pts, or ", number(*points)),
                    None => "Tighten the stop, or ".to_owned(),
                };
                format!(
                    "the smallest size is {} and this stop risks {} - over your {} risk per \
                     trade. {tighten}raise the risk.",
                    number(*quantity),
                    money(risk),
                    money(budget)
                )
            }
        }
    }
}

impl BudgetRefusal {
    /// A sentence for the trader, saying what to do instead.
    pub(crate) fn sentence(&self) -> String {
        match self {
            Self::NotSet => "set a risk per trade to size the entry from your stop".to_owned(),
            Self::AmountNotPositive => "set a risk per trade above zero".to_owned(),
            Self::PercentNotPositive => "set a risk percentage above zero".to_owned(),
            Self::NoCapital(currency) => format!(
                "no {} capital declared, so a percentage of it cannot be sized. Declare a {} \
                 capital, or set a fixed amount - nothing here converts between currencies.",
                currency.code(),
                currency.code()
            ),
        }
    }
}

/// The size this entry gets, and why.
///
/// Everything the ticket needs to draw and everything the control plane
/// needs to publish, decided once per aim rather than once per reader.
pub(crate) fn evaluate(
    context: &RiskContext<'_>,
    side: Side,
    entry: Decimal,
    bracket: &Bracket,
) -> RiskState {
    if context.settings.basis == RiskBasis::Off {
        return RiskState::Off;
    }
    let Some(money) = context.book.get(context.symbol) else {
        return RiskState::NoInstrumentMoney {
            symbol: context.symbol.to_owned(),
        };
    };
    let budget = match budget_for(context.settings, context.capital, &money.currency) {
        Ok(budget) => budget,
        Err(refusal) => return RiskState::NoBudget(refusal),
    };
    let sized = match size_for_risk(side, entry, bracket, &budget, money) {
        Ok(sized) => sized,
        Err(refusal) => return RiskState::Refused(refusal),
    };
    if sized.outcome == SizeOutcome::FlooredAboveBudget {
        return RiskState::OverBudget {
            quantity: sized.quantity,
            risk: sized.risk,
            fits_within_points: widest_stop_that_fits(&budget, money),
            budget,
        };
    }
    RiskState::Sized {
        quantity: sized.quantity,
        risk: sized.risk,
        outcome: sized.outcome,
        budget,
    }
}

/// The size an aimed entry gets, and the protection it was sized against.
///
/// Two passes, because the ladder and the size each need the other. A saved
/// strategy resolves its rungs *at* a quantity, and the quantity is what
/// sizing is computing. `resolve` is handed a quantity and answers with the
/// bracket the ticket would carry at it — the caller's own funnel, so the
/// wheel, the saved ladder and the typed offsets all arrive here by one
/// road. The first pass reads the share-weighted stop distance, which the
/// weights make independent of the probe; the second re-resolves at the size
/// that distance bought and restates the risk, so the number on screen is
/// the risk of the ladder that will really rest.
pub(crate) fn sized_for_aim(
    context: &RiskContext<'_>,
    side: Side,
    entry: Decimal,
    probe_quantity: Decimal,
    resolve: &dyn Fn(Decimal) -> Bracket,
) -> (RiskState, Bracket) {
    let probe = resolve(probe_quantity);
    let state = evaluate(context, side, entry, &probe);
    let Some(quantity) = state.derived_quantity() else {
        return (state, probe);
    };
    let resting = resolve(quantity);
    if resting == probe {
        return (state, resting);
    }
    let Some(risk) = risk_of(
        context.book,
        context.symbol,
        side,
        entry,
        &resting,
        quantity,
    ) else {
        return (state, resting);
    };
    (with_restated_risk(state, risk), resting)
}

/// What `quantity` risks at this protection, when the symbol's money is
/// declared.
pub(crate) fn risk_of(
    book: &InstrumentBook,
    symbol: &str,
    side: Side,
    entry: Decimal,
    bracket: &Bracket,
    quantity: Decimal,
) -> Option<Money> {
    let money = book.get(symbol)?;
    quantick_sim::risk_at(side, entry, bracket, quantity, money).ok()
}

/// Replace a state's risk with the risk of the ladder that will actually
/// rest, keeping the quantity that was sized.
///
/// A saved strategy resolves its rungs *at* a quantity, and the last rung
/// takes the rounding remainder, so the ladder resting at the sized quantity
/// is not always the one whose share-weighted distance produced that size.
/// The number on screen has to be the one the trader will really carry.
///
/// The quantity is deliberately *not* recomputed. Sizing again from the new
/// ladder could pick a new quantity, which would resolve a new ladder, and a
/// risk control that chases its own tail is one whose answer depends on how
/// long it looked. One restatement, and if the honest risk now exceeds the
/// budget the state says so — which is what the lock then acts on.
pub(crate) fn with_restated_risk(state: RiskState, restated: Money) -> RiskState {
    match state {
        RiskState::Sized {
            quantity,
            outcome,
            budget,
            ..
        } => {
            if restated.amount > budget.amount {
                RiskState::OverBudget {
                    quantity,
                    risk: restated,
                    fits_within_points: None,
                    budget,
                }
            } else {
                RiskState::Sized {
                    quantity,
                    risk: restated,
                    outcome,
                    budget,
                }
            }
        }
        RiskState::OverBudget {
            quantity,
            budget,
            fits_within_points,
            ..
        } => RiskState::OverBudget {
            quantity,
            risk: restated,
            budget,
            fits_within_points,
        },
        other => other,
    }
}

/// The widest stop, in points, whose loss at the smallest tradable size
/// still fits inside `budget`.
///
/// The number the over-budget sentence offers instead of leaving the trader
/// to divide it themselves mid-tape.
fn widest_stop_that_fits(budget: &Money, money: &InstrumentMoney) -> Option<Decimal> {
    let per_point = money.min_size.saturating_mul(money.point_value);
    if per_point <= Decimal::ZERO {
        return None;
    }
    let points = budget.amount.checked_div(per_point)?;
    if points <= Decimal::ZERO {
        return None;
    }
    Some(points.round_dp(2).normalize())
}

/// A decimal as a trader writes it: no trailing zeros, no exponent.
fn number(value: Decimal) -> String {
    value.normalize().to_string()
}

/// An amount with its currency, in the order a trader reads it.
fn money(value: &Money) -> String {
    format!("{} {}", number(value.amount), value.currency.code())
}

/// What a launch hook asked for: a risk per trade, and optionally the money
/// to measure it in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RiskHook {
    pub settings: RiskSettings,
    pub capital: Capital,
    /// The money to give whichever symbol the tab opens on. `None` leaves
    /// the instrument undeclared, which is itself a state worth capturing:
    /// it is what a trader sees before they fill the two fields in.
    pub money: Option<InstrumentMoney>,
}

/// Parse `QUANTICK_PAPER_RISK`.
///
/// Grammar, colon-separated:
///
/// - `100` — lose at most 100 per trade, instrument left undeclared.
/// - `100:0.20:1:BRL` — the same, with this symbol's point value, size step
///   and currency, so a derived size is on screen with no clicks.
/// - `2%@10000:0.20:1:BRL` — a percentage of a declared capital instead.
/// - a trailing `:unlocked` takes the lock down, which is the only way a
///   capture reaches "over budget and still placeable".
///
/// The money triple is all-or-nothing and its currency is required: there is
/// no default currency to fall back on, and inventing one would put a
/// figure on screen in a currency nobody chose.
pub(crate) fn parse_hook(spec: &str) -> Option<RiskHook> {
    let mut fields: Vec<&str> = spec.split(':').map(str::trim).collect();
    let mut lock = true;
    if fields
        .last()
        .is_some_and(|last| last.eq_ignore_ascii_case("unlocked"))
    {
        lock = false;
        fields.pop();
    }
    let (risk, money_fields) = fields.split_first()?;
    let mut capital = Capital::new();
    let settings = if let Some((percent, rest)) = risk.split_once('%') {
        let percent = Decimal::from_str_exact(percent.trim()).ok()?;
        let declared = rest.strip_prefix('@')?;
        let declared = Decimal::from_str_exact(declared.trim()).ok()?;
        if percent <= Decimal::ZERO || declared <= Decimal::ZERO {
            return None;
        }
        RiskSettings {
            basis: RiskBasis::PercentOfCapital,
            amount: Decimal::ZERO,
            percent,
            lock,
        }
    } else {
        let amount = Decimal::from_str_exact(risk).ok()?;
        if amount <= Decimal::ZERO {
            return None;
        }
        RiskSettings {
            basis: RiskBasis::Amount,
            amount,
            percent: Decimal::ZERO,
            lock,
        }
    };
    let money = match money_fields {
        [] => None,
        [point_value, size_step, currency] => {
            let point_value = Decimal::from_str_exact(point_value).ok()?;
            let size_step = Decimal::from_str_exact(size_step).ok()?;
            let currency = Currency::new(currency)?;
            if point_value <= Decimal::ZERO || size_step <= Decimal::ZERO {
                return None;
            }
            Some(InstrumentMoney {
                point_value,
                size_step,
                min_size: size_step,
                max_size: None,
                currency,
                source: quantick_sim::MoneySource::Declared,
            })
        }
        _ => return None,
    };
    // A percentage needs its capital keyed by the currency it is in, which
    // only the money triple names. Asking for a percentage without one is
    // an incomplete request, not a defaulted one.
    if settings.basis == RiskBasis::PercentOfCapital {
        let currency = money.as_ref()?.currency.clone();
        let declared = risk
            .split_once('%')
            .and_then(|(_, rest)| rest.strip_prefix('@'))
            .and_then(|text| Decimal::from_str_exact(text.trim()).ok())?;
        capital.insert(currency.code().to_owned(), declared);
    }
    Some(RiskHook {
        settings,
        capital,
        money,
    })
}

/// Everything the risk block reads and writes, borrowed from the ticket.
///
/// A struct rather than nine arguments, and the whole surface lives here
/// rather than in the ticket, so the file that already carries the order
/// form does not absorb another feature. The ticket calls this once.
pub(crate) struct RiskBlock<'a> {
    pub symbol: &'a str,
    pub settings: &'a mut RiskSettings,
    pub capital: &'a mut Capital,
    pub book: &'a mut InstrumentBook,
    pub amount_text: &'a mut String,
    pub percent_text: &'a mut String,
    pub capital_text: &'a mut String,
    pub point_value_text: &'a mut String,
    pub size_step_text: &'a mut String,
    pub currency_text: &'a mut String,
}

/// Draw the risk-per-trade block. Returns whether anything changed, so the
/// caller persists exactly when a trader actually moved something.
///
/// Every field follows the same rule: while it does not hold the keyboard,
/// its text is rewritten from the model. That is what makes switching
/// symbols show the new instrument's numbers without a marker to keep in
/// step — and it is why a half-typed value is never clobbered mid-edit.
pub(crate) fn draw_risk_block(ui: &mut eframe::egui::Ui, block: RiskBlock<'_>) -> bool {
    use eframe::egui;

    use crate::paper_trading::{caption, pill_toggle};
    use crate::theme;

    let mut changed = false;
    ui.add_space(4.0);
    ui.label(caption("RISK PER TRADE"));

    ui.horizontal(|ui| {
        for (basis, label, hover) in [
            (RiskBasis::Off, "off", "type the size yourself, as always"),
            (
                RiskBasis::Amount,
                "amount",
                "a fixed amount of money one trade may lose",
            ),
            (
                RiskBasis::PercentOfCapital,
                "% of capital",
                "a share of the capital you declared for this instrument's currency",
            ),
        ] {
            let on = block.settings.basis == basis;
            if pill_toggle(ui, label, on, hover).clicked() && !on {
                block.settings.basis = basis;
                changed = true;
            }
        }
    });

    let declared = block.book.get(block.symbol).cloned();
    let currency_code = declared.as_ref().map_or_else(
        || block.currency_text.trim().to_uppercase(),
        |money| money.currency.code().to_owned(),
    );

    match block.settings.basis {
        RiskBasis::Off => {}
        RiskBasis::Amount => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Lose at most")
                        .color(theme::TEXT_MUTED)
                        .small(),
                );
                if edit_decimal(ui, block.amount_text, &mut block.settings.amount, 64.0) {
                    changed = true;
                }
                ui.label(
                    egui::RichText::new(if currency_code.is_empty() {
                        "per trade"
                    } else {
                        currency_code.as_str()
                    })
                    .color(theme::TEXT_FAINT)
                    .small(),
                );
            });
        }
        RiskBasis::PercentOfCapital => {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Lose at most")
                        .color(theme::TEXT_MUTED)
                        .small(),
                );
                if edit_decimal(ui, block.percent_text, &mut block.settings.percent, 44.0) {
                    changed = true;
                }
                ui.label(egui::RichText::new("% of").color(theme::TEXT_FAINT).small());
                let mut capital_value = currency_code
                    .is_empty()
                    .then_some(Decimal::ZERO)
                    .or_else(|| block.capital.get(&currency_code).copied())
                    .unwrap_or(Decimal::ZERO);
                if edit_decimal(ui, block.capital_text, &mut capital_value, 72.0)
                    && !currency_code.is_empty()
                {
                    if capital_value > Decimal::ZERO {
                        block.capital.insert(currency_code.clone(), capital_value);
                    } else {
                        block.capital.remove(&currency_code);
                    }
                    changed = true;
                }
                ui.label(
                    egui::RichText::new(if currency_code.is_empty() {
                        "capital"
                    } else {
                        currency_code.as_str()
                    })
                    .color(theme::TEXT_FAINT)
                    .small(),
                );
            });
            ui.label(
                egui::RichText::new(
                    "a share of the capital you declared, not of your session's result",
                )
                .color(theme::TEXT_FAINT)
                .small(),
            );
        }
    }

    if block.settings.basis != RiskBasis::Off {
        let mut lock = block.settings.lock;
        if ui
            .checkbox(
                &mut lock,
                egui::RichText::new("Refuse an entry over it").small(),
            )
            .on_hover_text(
                "on: an entry whose stop risks more than this does not go out. Turn it off to \
                 take one anyway.",
            )
            .changed()
        {
            block.settings.lock = lock;
            changed = true;
        }

        // The two facts no feed reports. Declared per symbol, never derived:
        // a wrong point value is a wrong position size, and unlike a wrong
        // row on a chart it is invisible until it fills.
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if block.symbol.is_empty() {
                    "instrument"
                } else {
                    block.symbol
                })
                .color(theme::TEXT_MUTED)
                .small(),
            );
            ui.label(
                egui::RichText::new("point value")
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
            let mut point_value = declared
                .as_ref()
                .map_or(Decimal::ZERO, |money| money.point_value);
            let point_changed = edit_decimal(ui, block.point_value_text, &mut point_value, 56.0);
            ui.label(egui::RichText::new("step").color(theme::TEXT_FAINT).small());
            let mut size_step = declared
                .as_ref()
                .map_or(Decimal::ZERO, |money| money.size_step);
            let step_changed = edit_decimal(ui, block.size_step_text, &mut size_step, 56.0);
            if !declared.is_some() || block.currency_text.trim().is_empty() {
                *block.currency_text = currency_code.clone();
            }
            let currency_changed = ui
                .add(
                    egui::TextEdit::singleline(block.currency_text)
                        .desired_width(48.0)
                        .hint_text("BRL"),
                )
                .on_hover_text("the currency this point value is in - never converted")
                .changed();
            if point_changed || step_changed || currency_changed {
                let currency = Currency::new(block.currency_text);
                match (
                    currency,
                    point_value > Decimal::ZERO,
                    size_step > Decimal::ZERO,
                ) {
                    (Some(currency), true, true) => {
                        block.book.insert(
                            block.symbol.to_owned(),
                            InstrumentMoney {
                                point_value,
                                size_step,
                                min_size: size_step,
                                max_size: declared.as_ref().and_then(|money| money.max_size),
                                currency,
                                source: quantick_sim::MoneySource::Declared,
                            },
                        );
                    }
                    // Half a declaration is no declaration. Dropping it is
                    // what keeps the ticket saying "nothing here knows what
                    // one point is worth" instead of sizing off a stray
                    // number the trader was still typing.
                    _ => {
                        block.book.remove(block.symbol);
                    }
                }
                changed = true;
            }
        });
        ui.label(
            egui::RichText::new(
                "what one point of price is worth per unit held, and the smallest size you can \
                 trade. No feed reports these; saved per symbol.",
            )
            .color(theme::TEXT_FAINT)
            .small(),
        );
    }

    changed
}

/// A decimal field that follows the model while the trader is not typing in
/// it.
///
/// Returns whether the value changed. An unparsable or blank field leaves
/// the model alone and is not a change: a half-typed "0." must not reset a
/// point value to nothing between two keystrokes.
fn edit_decimal(
    ui: &mut eframe::egui::Ui,
    text: &mut String,
    value: &mut Decimal,
    width: f32,
) -> bool {
    use eframe::egui;

    let response = ui.add(egui::TextEdit::singleline(text).desired_width(width));
    if !response.has_focus() && !response.changed() {
        let shown = if *value > Decimal::ZERO {
            number(*value)
        } else {
            String::new()
        };
        if *text != shown {
            *text = shown;
        }
        return false;
    }
    if !response.changed() {
        return false;
    }
    match Decimal::from_str_exact(text.trim()) {
        Ok(parsed) if parsed >= Decimal::ZERO && parsed != *value => {
            *value = parsed;
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_sim::MoneySource;

    fn d(text: &str) -> Decimal {
        Decimal::from_str_exact(text).expect("test decimal literal")
    }

    fn brl() -> Currency {
        Currency::new("BRL").expect("BRL")
    }

    fn win() -> InstrumentMoney {
        InstrumentMoney {
            point_value: d("0.20"),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: brl(),
            source: MoneySource::Declared,
        }
    }

    fn book() -> InstrumentBook {
        [("WIN$N".to_owned(), win())].into_iter().collect()
    }

    fn fixed(amount: &str) -> RiskSettings {
        RiskSettings {
            basis: RiskBasis::Amount,
            amount: d(amount),
            ..RiskSettings::default()
        }
    }

    fn long_stop(stop: &str) -> Bracket {
        Bracket::whole(Some(d(stop)), None)
    }

    /// The four values that always travel together, for a WIN$N ticket.
    fn ctx<'a>(
        settings: &'a RiskSettings,
        capital: &'a Capital,
        book: &'a InstrumentBook,
        symbol: &'a str,
    ) -> RiskContext<'a> {
        RiskContext {
            settings,
            capital,
            book,
            symbol,
        }
    }

    #[test]
    fn the_lock_is_on_until_the_trader_turns_it_off() {
        assert!(
            RiskSettings::default().lock,
            "a risk per trade that can be exceeded by default is not a risk per trade"
        );
    }

    #[test]
    fn fixed_risk_off_leaves_the_quantity_to_the_trader() {
        let state = evaluate(
            &ctx(&RiskSettings::default(), &Capital::new(), &book(), "WIN$N"),
            Side::Buy,
            d("140000"),
            &long_stop("139500"),
        );
        assert_eq!(state, RiskState::Off);
        assert_eq!(state.derived_quantity(), None);
        assert!(!state.blocks_entry(true));
        assert!(state.sentence().is_empty());
    }

    #[test]
    fn a_symbol_with_no_declared_money_says_so_instead_of_sizing() {
        let state = evaluate(
            &ctx(&fixed("100"), &Capital::new(), &book(), "WDO$N"),
            Side::Buy,
            d("5432.5"),
            &long_stop("5425"),
        );
        assert_eq!(state.code(), "instrument_unknown");
        assert_eq!(state.derived_quantity(), None, "it never guesses a size");
        assert!(state.sentence().contains("WDO$N"));
        assert!(state.sentence().contains("point value"));
    }

    #[test]
    fn a_sized_entry_carries_the_quantity_and_lets_it_through() {
        let state = evaluate(
            &ctx(&fixed("100"), &Capital::new(), &book(), "WIN$N"),
            Side::Buy,
            d("140000"),
            &long_stop("139800"),
        );
        assert_eq!(state.derived_quantity(), Some(d("2")));
        assert_eq!(state.code(), "short_of_budget");
        assert!(
            !state.blocks_entry(true),
            "inside the budget, nothing blocks"
        );
        assert!(state.sentence().contains("80 BRL"));
    }

    /// The trader's own scenario, at the policy layer this time: the kernel
    /// reported the over-budget floor, and the lock is what refuses it.
    #[test]
    fn a_stop_too_wide_for_the_budget_is_blocked_while_the_lock_is_on() {
        let state = evaluate(
            &ctx(&fixed("100"), &Capital::new(), &book(), "WIN$N"),
            Side::Buy,
            d("140000"),
            &long_stop("136000"),
        );
        assert_eq!(state.code(), "clamped_at_minimum");
        assert_eq!(state.derived_quantity(), Some(Decimal::ONE));
        assert!(state.blocks_entry(true), "the lock refuses it");
        assert!(
            !state.blocks_entry(false),
            "turning the lock off is how a trader takes it anyway"
        );
    }

    /// The discreet line has to carry the three things a trader needs to act
    /// on: what it costs, what they set, and the stop that would fit.
    #[test]
    fn the_over_budget_line_names_the_risk_the_budget_and_the_stop_that_would_fit() {
        let state = evaluate(
            &ctx(&fixed("100"), &Capital::new(), &book(), "WIN$N"),
            Side::Buy,
            d("140000"),
            &long_stop("136000"),
        );
        let sentence = state.sentence();
        assert!(sentence.contains("800 BRL"), "the real risk: {sentence}");
        assert!(sentence.contains("100 BRL"), "the budget: {sentence}");
        // 100 / (1 contract x 0.20) = 500 points.
        assert!(
            sentence.contains("500 pts"),
            "the stop that fits: {sentence}"
        );
        assert!(
            sentence.contains("raise the risk"),
            "what to do: {sentence}"
        );
    }

    #[test]
    fn a_percentage_sizes_against_the_capital_declared_in_that_currency() {
        let settings = RiskSettings {
            basis: RiskBasis::PercentOfCapital,
            percent: d("2"),
            ..RiskSettings::default()
        };
        let capital: Capital = [("BRL".to_owned(), d("10000"))].into_iter().collect();
        let budget = budget_for(&settings, &capital, &brl()).expect("budget");
        assert_eq!(budget.amount, d("200"));
        assert_eq!(budget.currency, brl());
    }

    #[test]
    fn a_percentage_in_a_currency_with_no_capital_is_refused_not_converted() {
        let settings = RiskSettings {
            basis: RiskBasis::PercentOfCapital,
            percent: d("2"),
            ..RiskSettings::default()
        };
        // Capital exists, but in another currency. Nothing here converts.
        let capital: Capital = [("BRL".to_owned(), d("10000"))].into_iter().collect();
        let usdt = Currency::new("USDT").expect("USDT");
        let refusal = budget_for(&settings, &capital, &usdt).expect_err("refuses");
        assert_eq!(refusal, BudgetRefusal::NoCapital(usdt));
        assert!(refusal.sentence().contains("converts"));
    }

    #[test]
    fn an_entry_with_no_stop_is_reported_by_the_kernels_own_name() {
        let state = evaluate(
            &ctx(&fixed("100"), &Capital::new(), &book(), "WIN$N"),
            Side::Buy,
            d("140000"),
            &Bracket::none(),
        );
        assert_eq!(state, RiskState::Refused(SizeRefusal::NoStop));
        assert_eq!(state.code(), "refused");
        assert!(state.sentence().contains("needs a stop"));
        assert!(
            !state.blocks_entry(true),
            "an unsizable entry is not an over-budget one - the trader still types a size"
        );
    }

    /// A saved strategy's rungs carry absolute quantities, resolved at some
    /// size; the last rung takes the rounding remainder. So the ladder that
    /// rests at the sized quantity is not always the one whose weighted
    /// distance produced that size, and the risk on screen has to be the one
    /// the trader will really carry.
    #[test]
    fn a_ladder_whose_rungs_round_reports_the_risk_it_actually_carries() {
        let money = win();
        let budget = Money::new(d("100"), brl());
        // Sized on a weighted 300-point distance: 60.00 per contract.
        let weighted = RiskState::Sized {
            quantity: Decimal::ONE,
            risk: Money::new(d("60"), brl()),
            outcome: SizeOutcome::RoundedDown,
            budget: budget.clone(),
        };
        // The ladder that actually rests puts the whole contract on the far
        // rung - one contract cannot be split in halves - so the real risk
        // is the 400-point stop, not the weighted mean.
        let resting = Bracket::whole(Some(d("139600")), None);
        let restated = risk_of(
            &[("WIN$N".to_owned(), money)].into_iter().collect(),
            "WIN$N",
            Side::Buy,
            d("140000"),
            &resting,
            Decimal::ONE,
        )
        .expect("risk");
        assert_eq!(restated.amount, d("80"), "400 points x 0.20 x 1");
        let state = with_restated_risk(weighted, restated);
        match state {
            RiskState::Sized { risk, quantity, .. } => {
                assert_eq!(risk.amount, d("80"), "not the 60 the weighting predicted");
                assert_eq!(quantity, Decimal::ONE, "the size is not chased");
            }
            other => panic!("still inside the budget, so still sized: {other:?}"),
        }
    }

    /// The same restatement, when the rounding pushes the real risk *past*
    /// the budget: the state has to change, because the lock acts on it.
    #[test]
    fn a_restated_risk_over_the_budget_becomes_an_over_budget_state() {
        let budget = Money::new(d("100"), brl());
        let sized = RiskState::Sized {
            quantity: Decimal::ONE,
            risk: Money::new(d("60"), brl()),
            outcome: SizeOutcome::RoundedDown,
            budget,
        };
        let state = with_restated_risk(sized, Money::new(d("120"), brl()));
        assert_eq!(state.code(), "clamped_at_minimum");
        assert!(
            state.blocks_entry(true),
            "a ladder that rounded past the budget is refused like any other"
        );
    }

    #[test]
    fn the_hook_stands_a_fixed_risk_up_with_no_instrument_declared() {
        let hook = parse_hook("100").expect("parses");
        assert_eq!(hook.settings.basis, RiskBasis::Amount);
        assert_eq!(hook.settings.amount, d("100"));
        assert!(hook.settings.lock, "the lock stands unless asked otherwise");
        assert_eq!(
            hook.money, None,
            "an undeclared instrument is a state worth photographing"
        );
    }

    #[test]
    fn the_hook_can_declare_the_instruments_money_too() {
        let hook = parse_hook("100:0.20:1:BRL").expect("parses");
        let money = hook.money.expect("money");
        assert_eq!(money.point_value, d("0.20"));
        assert_eq!(money.size_step, Decimal::ONE);
        assert_eq!(money.currency, brl());
    }

    #[test]
    fn the_hook_takes_the_lock_down_on_request() {
        let hook = parse_hook("100:0.20:1:BRL:unlocked").expect("parses");
        assert!(
            !hook.settings.lock,
            "the only way a capture reaches an over-budget entry that still places"
        );
    }

    #[test]
    fn the_hook_declares_a_capital_for_the_percentage_it_asks_for() {
        let hook = parse_hook("2%@10000:0.20:1:BRL").expect("parses");
        assert_eq!(hook.settings.basis, RiskBasis::PercentOfCapital);
        assert_eq!(hook.settings.percent, d("2"));
        assert_eq!(hook.capital.get("BRL"), Some(&d("10000")));
        let budget = budget_for(&hook.settings, &hook.capital, &brl()).expect("budget");
        assert_eq!(budget.amount, d("200"));
    }

    #[test]
    fn a_hook_that_does_not_parse_is_refused_rather_than_defaulted() {
        for spec in [
            "",
            "nonsense",
            "0",
            "-5",
            // The money triple is all-or-nothing: a point value with no
            // currency would put a figure on screen in a currency nobody
            // chose.
            "100:0.20",
            "100:0.20:1",
            "100:0.20:1:BRL:extra",
            // A percentage with no currency has no capital to key.
            "2%@10000",
            "2%@0:0.20:1:BRL",
        ] {
            assert_eq!(parse_hook(spec), None, "spec {spec:?}");
        }
    }

    #[test]
    fn the_basis_token_round_trips() {
        for basis in [
            RiskBasis::Off,
            RiskBasis::Amount,
            RiskBasis::PercentOfCapital,
        ] {
            assert_eq!(RiskBasis::from_token(basis.token()), Some(basis));
        }
        assert_eq!(RiskBasis::from_token("nonsense"), None);
    }
}
