//! What a closed trade made, in the three figures a trader reads it by.
//!
//! The journal stores one number per trade, [`ClosedTrade::pnl_points`]:
//! the price move times the quantity. Two contracts that each made 200
//! points are stored as 400, which is honest arithmetic and a misleading
//! readout — the trader thinks in "200 points on 2 contracts", and in what
//! that paid. This module derives both from the stored number without ever
//! changing it:
//!
//! - **per contract** — the move one unit made: `pnl_points / quantity`,
//!   read in the instrument's price unit (points, pips, or a quote-asset
//!   move for linear crypto, which never says "points");
//! - **grouped** — the stored figure itself, the legacy readout;
//! - **money** — `pnl_points × point_value`, in the instrument's own
//!   currency, from the trader's declared record or the instrument catalog.
//!
//! Money is never guessed. A symbol with no point value reads
//! [`UNSET_MONEY`], and a total that would include one reads the same
//! rather than a partial sum. Currencies are never added together: there is
//! no exchange rate anywhere in this workspace.
//!
//! Money is derived when shown, from the catalog in force — a point value
//! edited today re-reads yesterday's trades. Stamping it into history is a
//! separate decision this module does not take.
//!
//! [`ClosedTrade::pnl_points`]: quantick_sim::ClosedTrade::pnl_points

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

use quantick_sim::{ClosedTrade, Currency, InstrumentMoney, Money};
use rust_decimal::Decimal;

use crate::format::{fmt_decimal, fmt_exact};
pub use crate::instruments::PnlPrimary;
use crate::instruments::{InstrumentCatalog, PriceUnit};
use crate::risk_sizing::InstrumentBook;

/// What a money figure reads when no point value is known: a dash and what
/// to do about it, never a bare dash and never a zero.
pub const UNSET_MONEY: &str = "— set pt value";

/// Places a money amount is shown with: cents, whatever the instrument.
const MONEY_PLACES: u32 = 2;

/// The mode a readout uses: the trader's choice when they made one, else
/// money where a point value is known and points per contract where not.
#[must_use]
pub fn effective_primary(chosen: Option<PnlPrimary>, has_money: bool) -> PnlPrimary {
    chosen.unwrap_or(if has_money {
        PnlPrimary::Money
    } else {
        PnlPrimary::PointsPerContract
    })
}

/// The catalog and the trader's display choice, held by the account and
/// fanned out app-wide like the risk settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PnlSettings {
    /// Shared by every tab; read-only after startup.
    pub catalog: Arc<InstrumentCatalog>,
    /// The mode the trader picked; `None` defers to the catalog's
    /// `[display] primary`, then to [`effective_primary`]'s default.
    pub chosen: Option<PnlPrimary>,
}

impl PnlSettings {
    /// The mode a readout of a symbol with or without money uses.
    #[must_use]
    pub fn primary(&self, has_money: bool) -> PnlPrimary {
        effective_primary(
            self.chosen.or_else(|| self.catalog.default_primary()),
            has_money,
        )
    }

    /// The shipped catalog with the trader's file at `trader_file` laid
    /// over it, and the display choice the paper sidecar stored.
    ///
    /// A missing trader file is the common case and reports nothing; one
    /// that does not parse leaves the shipped catalog standing and is
    /// logged by name. An unknown stored token reads as no choice.
    #[must_use]
    pub fn load(shipped: &str, trader_file: Option<&Path>, chosen: Option<&str>) -> Self {
        let trader = trader_file.and_then(|path| std::fs::read_to_string(path).ok());
        let (catalog, warning) = InstrumentCatalog::layered(shipped, trader.as_deref());
        if let Some(warning) = warning {
            tracing::warn!(
                target: "quantick::paper",
                event_code = "INSTRUMENTS_CATALOG",
                %warning,
                "instrument catalog not applied"
            );
        }
        Self {
            catalog: Arc::new(catalog),
            chosen: chosen.and_then(PnlPrimary::from_token),
        }
    }
}

/// Every readout's one entry point: the units a symbol resolves to and the
/// mode in force, so a ticket, a report row and a total cannot disagree.
#[derive(Clone, Copy)]
pub struct PnlReader<'a> {
    pub book: PnlBook<'a>,
    pub settings: &'a PnlSettings,
}

impl PnlReader<'_> {
    /// The mode a readout of `symbol` leads with.
    #[must_use]
    pub fn primary(&self, symbol: &str) -> PnlPrimary {
        self.settings
            .primary(self.book.units(symbol).money.is_some())
    }

    /// One stored figure of `symbol` (move × quantity) as a row reads it:
    /// the primary figure, then the one that keeps it honest.
    #[must_use]
    pub fn trade(&self, symbol: &str, pnl_points: Decimal, quantity: Decimal) -> String {
        let units = self.book.units(symbol);
        let pnl = TradePnl::of(pnl_points, quantity, &units);
        fmt_trade(&pnl, &units, self.settings.primary(units.money.is_some()))
    }

    /// The trades' running total, never across currencies.
    #[must_use]
    pub fn sum<'t>(&self, rows: impl IntoIterator<Item = (&'t str, &'t ClosedTrade)>) -> PnlTotal {
        let mut total = PnlTotal::default();
        for (symbol, trade) in rows {
            let units = self.book.units(symbol);
            total.add(
                &TradePnl::of(trade.pnl_points, trade.quantity, &units),
                &units,
            );
        }
        total
    }

    /// [`Self::sum`] as a total row reads it, the secondary figure behind.
    #[must_use]
    pub fn total<'t>(&self, rows: impl IntoIterator<Item = (&'t str, &'t ClosedTrade)>) -> String {
        self.word(&self.sum(rows))
    }

    /// The mode a total leads with: the trader's choice, else money where
    /// every trade in it has a point value.
    #[must_use]
    pub fn total_primary(&self, total: &PnlTotal) -> PnlPrimary {
        self.settings.primary(total.money().is_some())
    }

    /// A total as a row reads it: the primary figure, the secondary behind.
    #[must_use]
    pub fn word(&self, total: &PnlTotal) -> String {
        total.fmt_pair(self.total_primary(total))
    }

    /// A total as a headline reads it: the primary figure and, apart, the
    /// one that keeps it honest.
    #[must_use]
    pub fn word_split(&self, total: &PnlTotal) -> (String, String) {
        let primary = self.total_primary(total);
        (total.fmt(primary), total.fmt(secondary_of(primary)))
    }

    /// What a readout cached under this reader was worded by — compared
    /// with [`Self::wrote`] to know when to word it again.
    #[must_use]
    pub fn key(&self) -> PnlKey {
        PnlKey {
            chosen: self.settings.chosen,
            declared: self.book.declared.clone(),
        }
    }

    /// Whether `key` still describes this reader: the same choice and the
    /// same declared money. The catalog is fixed after startup.
    #[must_use]
    pub fn wrote(&self, key: &PnlKey) -> bool {
        key.chosen == self.settings.chosen && key.declared == *self.book.declared
    }

    /// The primary figure of one trade, for a curve: `None` where the
    /// trade has no money to plot.
    #[must_use]
    pub fn value(&self, symbol: &str, trade: &ClosedTrade, primary: PnlPrimary) -> Option<Decimal> {
        let units = self.book.units(symbol);
        primary_value(
            &TradePnl::of(trade.pnl_points, trade.quantity, &units),
            primary,
        )
    }
}

impl<'a> PnlReader<'a> {
    /// This reader bound to one symbol, for a row that words its own trade.
    #[must_use]
    pub fn of(self, symbol: &'a str) -> SymbolPnl<'a> {
        SymbolPnl {
            reader: self,
            symbol,
        }
    }
}

/// Nothing declared, no catalog and no choice: what a surface reads before
/// any account is wired to it. Points per contract, money unset.
static PLAIN: LazyLock<(InstrumentBook, PnlSettings)> = LazyLock::new(Default::default);

impl PnlReader<'static> {
    /// A reader with no money anywhere: per-contract points, honestly.
    #[must_use]
    pub fn plain() -> Self {
        let (declared, settings) = &*PLAIN;
        Self {
            book: PnlBook {
                declared,
                catalog: &settings.catalog,
            },
            settings,
        }
    }
}

/// The choice and the declared money a cached readout was worded under.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PnlKey {
    chosen: Option<PnlPrimary>,
    declared: InstrumentBook,
}

/// A reader bound to one symbol: what a ledger row carries so the row and
/// the totals above it word the same trade the same way.
#[derive(Clone, Copy)]
pub struct SymbolPnl<'a> {
    pub reader: PnlReader<'a>,
    pub symbol: &'a str,
}

impl SymbolPnl<'_> {
    /// One closed trade of this symbol, as a row reads it.
    #[must_use]
    pub fn closed(&self, trade: &ClosedTrade) -> String {
        self.reader
            .trade(self.symbol, trade.pnl_points, trade.quantity)
    }

    /// Fold one closed trade of this symbol into `total`.
    pub fn add_to(&self, total: &mut PnlTotal, trade: &ClosedTrade) {
        let units = self.reader.book.units(self.symbol);
        total.add(
            &TradePnl::of(trade.pnl_points, trade.quantity, &units),
            &units,
        );
    }
}

/// Where a symbol's P&L units are looked up: the trader's declared records
/// first, then the catalog.
#[derive(Clone, Copy)]
pub struct PnlBook<'a> {
    pub declared: &'a InstrumentBook,
    pub catalog: &'a InstrumentCatalog,
}

impl<'a> PnlBook<'a> {
    /// The units `symbol` reads in. A declared record supplies the money;
    /// the catalog supplies the price unit either way.
    #[must_use]
    pub fn units(&self, symbol: &str) -> PnlUnits<'a> {
        let spec = self.catalog.lookup(symbol);
        PnlUnits {
            money: self
                .declared
                .get(symbol)
                .or_else(|| spec.map(|spec| &spec.money)),
            price_unit: spec.map_or(PriceUnit::Points, |spec| spec.price_unit),
            pip_size: spec.and_then(|spec| spec.pip_size),
        }
    }
}

/// One symbol's P&L units, resolved.
#[derive(Debug, Clone, Copy)]
pub struct PnlUnits<'a> {
    /// `None` when nobody declared what a point is worth.
    pub money: Option<&'a InstrumentMoney>,
    pub price_unit: PriceUnit,
    pub pip_size: Option<Decimal>,
}

impl PnlUnits<'_> {
    /// The suffix a price-unit figure carries: `pts`, `pips`, or the quote
    /// asset's code and `move` for linear crypto.
    #[must_use]
    pub fn unit_suffix(&self) -> String {
        match self.price_unit {
            PriceUnit::Points => "pts".to_owned(),
            PriceUnit::Pips => "pips".to_owned(),
            PriceUnit::Quote => match self.money {
                Some(money) => format!("{} move", money.currency.code()),
                None => "move".to_owned(),
            },
        }
    }

    /// A price move in this instrument's unit: pips divide by the pip size.
    fn in_unit(&self, move_points: Decimal) -> Decimal {
        match (self.price_unit, self.pip_size) {
            (PriceUnit::Pips, Some(pip)) => move_points.checked_div(pip).unwrap_or(move_points),
            _ => move_points,
        }
    }
}

/// One closed trade's three figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradePnl {
    /// The move one unit made, in the instrument's unit.
    pub per_unit: Decimal,
    pub quantity: Decimal,
    /// The move times the quantity, in the instrument's unit.
    pub grouped: Decimal,
    /// `None` when no point value is known.
    pub money: Option<Money>,
}

impl TradePnl {
    /// The figures `pnl_points` (move × quantity, as stored) reads as.
    #[must_use]
    pub fn of(pnl_points: Decimal, quantity: Decimal, units: &PnlUnits<'_>) -> Self {
        // A zero quantity cannot close a trade; if a row ever carries one,
        // the stored figure is the only honest per-unit reading.
        let per_move = pnl_points.checked_div(quantity).unwrap_or(pnl_points);
        Self {
            per_unit: units.in_unit(per_move),
            quantity,
            grouped: units.in_unit(pnl_points),
            money: units.money.map(|money| {
                Money::new(
                    pnl_points.saturating_mul(money.point_value),
                    money.currency.clone(),
                )
            }),
        }
    }
}

fn signed(value: Decimal, text: String) -> String {
    if value > Decimal::ZERO {
        format!("+{text}")
    } else {
        text
    }
}

/// `+80.00 BRL` — money always shows cents and its currency.
#[must_use]
pub fn fmt_money(money: &Money) -> String {
    fmt_cents(money.amount, money.currency.code())
}

/// `amount` with exactly two places, signed, then `unit`.
fn fmt_cents(amount: Decimal, unit: &str) -> String {
    let rounded = amount.round_dp(MONEY_PLACES);
    let mut text = rounded.to_string();
    let places = text
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    if places == 0 {
        text.push('.');
    }
    for _ in places..MONEY_PLACES as usize {
        text.push('0');
    }
    signed(rounded, format!("{text} {unit}"))
}

/// One plotted value in the figure it was plotted in: money with cents and
/// its currency, a points figure as points with its unit.
#[must_use]
pub fn fmt_amount(value: Decimal, primary: PnlPrimary, unit: &str) -> String {
    match primary {
        PnlPrimary::Money => fmt_cents(value, unit),
        PnlPrimary::PointsPerContract | PnlPrimary::PointsTimesQty => {
            format!("{} {unit}", signed(value, fmt_decimal(value)))
        }
    }
}

/// The figure that keeps `primary` honest: money is backed by the
/// per-contract move, a points figure by the money.
#[must_use]
pub fn secondary_of(primary: PnlPrimary) -> PnlPrimary {
    match primary {
        PnlPrimary::Money => PnlPrimary::PointsPerContract,
        PnlPrimary::PointsPerContract | PnlPrimary::PointsTimesQty => PnlPrimary::Money,
    }
}

/// `+200 pts × 2` — the move per contract and the quantity it was held at.
#[must_use]
pub fn fmt_per_unit(pnl: &TradePnl, units: &PnlUnits<'_>) -> String {
    format!(
        "{} {} × {}",
        signed(pnl.per_unit, fmt_decimal(pnl.per_unit)),
        units.unit_suffix(),
        fmt_decimal(pnl.quantity)
    )
}

/// `+400 pts` — the move times the quantity.
#[must_use]
pub fn fmt_grouped(pnl: &TradePnl, units: &PnlUnits<'_>) -> String {
    format!(
        "{} {}",
        signed(pnl.grouped, fmt_decimal(pnl.grouped)),
        units.unit_suffix()
    )
}

fn fmt_money_or_unset(money: Option<&Money>) -> String {
    money.map_or_else(|| UNSET_MONEY.to_owned(), fmt_money)
}

/// The readout a trade row shows: the primary figure, then the one that
/// keeps it honest. Money leads with the per-contract figure behind it; a
/// points figure leads with the money behind it.
#[must_use]
pub fn fmt_trade(pnl: &TradePnl, units: &PnlUnits<'_>, primary: PnlPrimary) -> String {
    let money = fmt_money_or_unset(pnl.money.as_ref());
    match primary {
        PnlPrimary::Money => format!("{money} · {}", fmt_per_unit(pnl, units)),
        PnlPrimary::PointsPerContract => format!("{} · {money}", fmt_per_unit(pnl, units)),
        PnlPrimary::PointsTimesQty => format!("{} · {money}", fmt_grouped(pnl, units)),
    }
}

/// The primary figure alone, as a number a curve can plot: `None` for money
/// a trade cannot express.
#[must_use]
pub fn primary_value(pnl: &TradePnl, primary: PnlPrimary) -> Option<Decimal> {
    match primary {
        PnlPrimary::Money => pnl.money.as_ref().map(|money| money.amount),
        PnlPrimary::PointsPerContract => Some(pnl.per_unit),
        PnlPrimary::PointsTimesQty => Some(pnl.grouped),
    }
}

/// A running total over trades, in all three figures at once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PnlTotal {
    per_unit: Decimal,
    grouped: Decimal,
    /// Money per currency, never across currencies.
    money: BTreeMap<Currency, Decimal>,
    /// Whether any trade in the total had no point value.
    money_unknown: bool,
    /// The unit suffix shared by every trade, or `None` once they differ.
    unit: Option<String>,
    mixed_units: bool,
}

impl PnlTotal {
    /// Fold one trade in.
    pub fn add(&mut self, pnl: &TradePnl, units: &PnlUnits<'_>) {
        self.per_unit = self.per_unit.saturating_add(pnl.per_unit);
        self.grouped = self.grouped.saturating_add(pnl.grouped);
        match &pnl.money {
            Some(money) => {
                let sum = self.money.entry(money.currency.clone()).or_default();
                *sum = sum.saturating_add(money.amount);
            }
            None => self.money_unknown = true,
        }
        self.note_unit(units.unit_suffix());
    }

    /// Fold another total in: the ledger adds this session's trades to the
    /// saved ones it summed when the folder was read.
    pub fn merge(&mut self, other: &Self) {
        self.per_unit = self.per_unit.saturating_add(other.per_unit);
        self.grouped = self.grouped.saturating_add(other.grouped);
        for (currency, amount) in &other.money {
            let sum = self.money.entry(currency.clone()).or_default();
            *sum = sum.saturating_add(*amount);
        }
        self.money_unknown |= other.money_unknown;
        if other.mixed_units {
            self.unit = None;
            self.mixed_units = true;
        } else if let Some(unit) = &other.unit {
            self.note_unit(unit.clone());
        }
    }

    fn note_unit(&mut self, suffix: String) {
        match &self.unit {
            None if !self.mixed_units => self.unit = Some(suffix),
            Some(unit) if *unit != suffix => {
                self.unit = None;
                self.mixed_units = true;
            }
            _ => {}
        }
    }

    /// The move times the quantity, summed — the stored figure, whose sign
    /// a total is tinted by.
    #[must_use]
    pub fn grouped(&self) -> Decimal {
        self.grouped
    }

    /// The total's money, one amount per currency, when every trade in it
    /// had a point value.
    #[must_use]
    pub fn money(&self) -> Option<&BTreeMap<Currency, Decimal>> {
        (!self.money_unknown).then_some(&self.money)
    }

    /// The unit shared by every trade in the total, or that they differ.
    fn unit_text(&self) -> String {
        match (&self.unit, self.mixed_units) {
            (_, true) => "mixed units".to_owned(),
            (Some(unit), false) => unit.clone(),
            (None, false) => "pts".to_owned(),
        }
    }

    /// The total in the primary figure. Money lists each currency on its
    /// own; a points total over instruments in different units says so.
    #[must_use]
    pub fn fmt(&self, primary: PnlPrimary) -> String {
        let unit = self.unit_text();
        match primary {
            PnlPrimary::Money => match self.money() {
                None => UNSET_MONEY.to_owned(),
                Some(money) if money.is_empty() => fmt_decimal(Decimal::ZERO),
                Some(money) => money
                    .iter()
                    .map(|(currency, amount)| fmt_money(&Money::new(*amount, currency.clone())))
                    .collect::<Vec<_>>()
                    .join(" / "),
            },
            PnlPrimary::PointsPerContract => format!(
                "{} {unit}/ct",
                signed(self.per_unit, fmt_decimal(self.per_unit))
            ),
            PnlPrimary::PointsTimesQty => {
                format!("{} {unit}", signed(self.grouped, fmt_decimal(self.grouped)))
            }
        }
    }
}

impl PnlTotal {
    /// The total in the primary figure with its secondary behind it: money
    /// leads the per-contract sum, a points figure leads the money.
    #[must_use]
    pub fn fmt_pair(&self, primary: PnlPrimary) -> String {
        format!(
            "{} · {}",
            self.fmt(primary),
            self.fmt(secondary_of(primary))
        )
    }

    /// The unit the primary figure is plotted in, for an axis label.
    #[must_use]
    pub fn unit_label(&self, primary: PnlPrimary) -> String {
        let unit = self.unit_text();
        match primary {
            PnlPrimary::Money => self.money().map_or_else(
                || UNSET_MONEY.to_owned(),
                |money| {
                    money
                        .keys()
                        .map(Currency::code)
                        .collect::<Vec<_>>()
                        .join(" / ")
                },
            ),
            PnlPrimary::PointsPerContract => format!("{unit}/ct"),
            PnlPrimary::PointsTimesQty => unit,
        }
    }
}

/// The journal export's three appended cells for one trade: the per-unit
/// move, the money and its currency — exact, and empty where unknown.
#[must_use]
pub fn csv_cells(pnl: &TradePnl) -> [String; 3] {
    let (money, currency) = pnl.money.as_ref().map_or_else(
        || (String::new(), String::new()),
        |money| (fmt_exact(money.amount), money.currency.code().to_owned()),
    );
    [fmt_exact(pnl.per_unit), money, currency]
}

#[cfg(test)]
#[path = "pnl_tests.rs"]
mod pnl_tests;
