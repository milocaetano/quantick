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
use std::sync::Arc;

use quantick_sim::{Currency, InstrumentMoney, Money};
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
    let rounded = money.amount.round_dp(MONEY_PLACES);
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
    signed(rounded, format!("{text} {}", money.currency.code()))
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
        let suffix = units.unit_suffix();
        match &self.unit {
            None if !self.mixed_units => self.unit = Some(suffix),
            Some(unit) if *unit != suffix => {
                self.unit = None;
                self.mixed_units = true;
            }
            _ => {}
        }
    }

    /// The total's money, one amount per currency, when every trade in it
    /// had a point value.
    #[must_use]
    pub fn money(&self) -> Option<&BTreeMap<Currency, Decimal>> {
        (!self.money_unknown).then_some(&self.money)
    }

    /// The total in the primary figure. Money lists each currency on its
    /// own; a points total over instruments in different units says so.
    #[must_use]
    pub fn fmt(&self, primary: PnlPrimary) -> String {
        let unit = match (&self.unit, self.mixed_units) {
            (_, true) => "mixed units".to_owned(),
            (Some(unit), false) => unit.clone(),
            (None, false) => "pts".to_owned(),
        };
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
