//! One curve in one currency: the report's walks merged on request.
//!
//! Per asset, a report walks each unit that adds up on its own curve
//! ([`super::EquityWalks`]). Merged, every trade's money is converted into one
//! currency at the rate of the civil day it closed on and walked as one
//! curve with one net. Only money merges: points of different instruments
//! are never added, so a points report asked to merge keeps a curve per
//! instrument and says why.
//!
//! Nothing is guessed. A trade with no point value has no money, a day with
//! no rate has no conversion: either trade stays out of the curve and the
//! net, and [`MergeReadout`] lists it beside the rates it did use.

use std::collections::{BTreeMap, BTreeSet};

use quantick_civil::CivilDate;
use quantick_trading::Currency;
use rust_decimal::Decimal;

use super::ReportView;
use super::readout::{EquityWalk, EquityWalks};
use crate::pnl::{PnlPrimary, PnlReader, fmt_amount};
use crate::rates::{RateSource, USD};

/// The most rates or unconverted entries a readout names one by one; the
/// counts say how many more there are.
pub const MAX_LISTED: usize = 16;

/// Why a points report asked to merge keeps one curve per instrument.
pub const POINTS_APART: &str =
    "points of different instruments never add up: one curve each, Merge all reads money";

/// One rate a merge converted with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateUsed {
    pub day: CivilDate,
    pub from: Currency,
    pub to: Currency,
    pub rate: Decimal,
}

/// Why a trade stayed out of a merged curve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Unconvertible {
    /// Nobody declared what a point of it is worth: it has no money.
    PointValueUnset,
    /// No rate from its currency to the target on the day it closed.
    NoRate(Currency),
}

/// Trades of one symbol, closed on one day, that a merge could not convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unconverted {
    pub symbol: String,
    pub day: CivilDate,
    pub why: Unconvertible,
    pub trades: usize,
}

/// What a merge converted, with what, and what it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReadout {
    pub currency: Currency,
    /// Each merged symbol and the currency its money is in.
    pub assets: Vec<(String, Currency)>,
    /// Every rate used, by day then currency; an equivalent code (USDT
    /// read as USD) needs none and is not listed.
    pub rates: Vec<RateUsed>,
    pub unconverted: Vec<Unconverted>,
    /// The merged net, in [`Self::currency`].
    pub net: Decimal,
}

/// The merge a view was cut under, and what its controls offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeView {
    /// The currency asked for; `None` reads per asset.
    pub request: Option<Currency>,
    /// What the merge converted; `None` per asset or in points.
    pub readout: Option<MergeReadout>,
    /// Why a merge asked of a points report did not happen.
    pub note: Option<&'static str>,
    /// How many curves the selection walks per asset.
    pub per_asset_walks: usize,
    /// The one money curve's currency per asset, when there is exactly one.
    pub own: Option<Currency>,
    /// The currencies a merge can read in: the selection's, and USD.
    pub currencies: Vec<Currency>,
    /// Whether the report reads money, the one figure a merge converts.
    pub reads_money: bool,
}

impl MergeView {
    /// The line a curve shows for its merge: what was converted, or why a
    /// points report kept its curves apart; `None` per asset.
    #[must_use]
    pub fn caption(&self) -> Option<String> {
        self.readout
            .as_ref()
            .map(MergeReadout::caption)
            .or_else(|| self.note.map(str::to_owned))
    }
}

impl MergeReadout {
    /// The line under a merged curve: the target, the assets, the rates and
    /// what was left out.
    #[must_use]
    pub fn caption(&self) -> String {
        let assets = self
            .assets
            .iter()
            .map(|(symbol, currency)| format!("{symbol} {}", currency.code()))
            .collect::<Vec<_>>()
            .join(" + ");
        let mut caption = format!("Merged in {} · {assets}", self.currency.code());
        let mut pairs: BTreeMap<(&str, &str), Vec<&RateUsed>> = BTreeMap::new();
        for rate in &self.rates {
            pairs
                .entry((rate.from.code(), rate.to.code()))
                .or_default()
                .push(rate);
        }
        for ((from, to), used) in pairs {
            let low = used.iter().map(|rate| rate.rate).min().unwrap_or_default();
            let high = used.iter().map(|rate| rate.rate).max().unwrap_or_default();
            let span = match used.as_slice() {
                [one] => format!("{} on {}", low.round_dp(4).normalize(), one.day.iso()),
                _ if low == high => {
                    format!("{} on {} days", low.round_dp(4).normalize(), used.len())
                }
                _ => format!(
                    "{} to {} over {} days",
                    low.round_dp(4).normalize(),
                    high.round_dp(4).normalize(),
                    used.len()
                ),
            };
            caption.push_str(&format!(" · {from}/{to} {span}"));
        }
        if !self.unconverted.is_empty() {
            let trades: usize = self.unconverted.iter().map(|entry| entry.trades).sum();
            let listed = self
                .unconverted
                .iter()
                .take(3)
                .map(|entry| {
                    format!(
                        "{} {} {}",
                        entry.symbol,
                        entry.day.iso(),
                        entry.why_text(&self.currency)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let more = self.unconverted.len().saturating_sub(3);
            let more = if more > 0 {
                format!(" +{more} more")
            } else {
                String::new()
            };
            caption.push_str(&format!(
                " · not converted: {trades} trade(s) - {listed}{more}"
            ));
        }
        caption
    }
}

impl Unconverted {
    /// Why, in words: `pt value unset`, `no USDT/BRL rate`.
    #[must_use]
    pub fn why_text(&self, target: &Currency) -> String {
        match &self.why {
            Unconvertible::PointValueUnset => "pt value unset".to_owned(),
            Unconvertible::NoRate(from) => format!("no {}/{} rate", from.code(), target.code()),
        }
    }
}

impl ReportView {
    /// This worded view merged into `target`, through `rates`; `None`
    /// leaves it per asset. Call after [`Self::with_pnl`]: the per-asset
    /// walks it replaces are what the controls are offered from.
    #[must_use]
    pub fn merged(
        mut self,
        target: Option<Currency>,
        pnl: PnlReader<'_>,
        rates: &dyn RateSource,
    ) -> Self {
        let rules = pnl.settings.catalog.currencies();
        let walks = self.equity.walks();
        let money = walks
            .iter()
            .any(|walk| walk.primary == PnlPrimary::Money || walk.fallback.is_some());
        let mut currencies: BTreeSet<Currency> = self
            .rows
            .iter()
            .filter_map(|row| pnl.book.money(&row.symbol))
            .map(|money| rules.canonical(&money.currency).clone())
            .collect();
        currencies.extend(Currency::new(USD));
        self.merge = MergeView {
            request: target.clone(),
            readout: None,
            note: None,
            per_asset_walks: walks.len(),
            own: match walks {
                [walk] if walk.primary == PnlPrimary::Money => {
                    Currency::new(&walk.unit).map(|currency| rules.canonical(&currency).clone())
                }
                _ => None,
            },
            currencies: currencies.into_iter().collect(),
            reads_money: money,
        };
        let Some(target) = target else {
            return self;
        };
        if !money {
            self.merge.note = (walks.len() > 1).then_some(POINTS_APART);
            return self;
        }
        let (readout, values) = self.convert(&target, pnl, rates);
        let walk = EquityWalk::walk(values, PnlPrimary::Money, target.code().to_owned());
        self.equity = EquityWalks::single(walk);
        let total = pnl.sum(
            self.rows
                .iter()
                .map(|row| (row.symbol.as_str(), &row.trade)),
        );
        self.net_text = (
            fmt_amount(readout.net, PnlPrimary::Money, target.code()),
            total.fmt(PnlPrimary::Money),
        );
        self.net_tint = readout.net;
        self.merge.readout = Some(readout);
        self
    }

    /// One row's money in `target`, or why it has none.
    fn converted(
        &self,
        row: &super::HistoryRow,
        target: &Currency,
        pnl: PnlReader<'_>,
        rates: &dyn RateSource,
    ) -> Result<(Decimal, Option<RateUsed>), Unconvertible> {
        let money = pnl.book.money(&row.symbol);
        let amount = pnl.value(&row.symbol, &row.trade, PnlPrimary::Money);
        let (Some(money), Some(amount)) = (money, amount) else {
            return Err(Unconvertible::PointValueUnset);
        };
        let day = CivilDate::from_ms(row.trade.closed_ms, self.tz);
        let rate = rates
            .rate(&money.currency, target, day)
            .ok_or_else(|| Unconvertible::NoRate(money.currency.clone()))?;
        let rules = pnl.settings.catalog.currencies();
        let used =
            (rules.canonical(&money.currency) != rules.canonical(target)).then(|| RateUsed {
                day,
                from: money.currency.clone(),
                to: target.clone(),
                rate,
            });
        Ok((amount.saturating_mul(rate), used))
    }

    /// Every row converted: the net, the rates used and what was left out,
    /// and each row's amount in `target` (zero where it could not be).
    fn convert(
        &self,
        target: &Currency,
        pnl: PnlReader<'_>,
        rates: &dyn RateSource,
    ) -> (MergeReadout, Vec<Decimal>) {
        let mut net = Decimal::ZERO;
        let mut values = Vec::with_capacity(self.rows.len());
        let mut assets: BTreeMap<String, Currency> = BTreeMap::new();
        let mut used: BTreeMap<(CivilDate, Currency), RateUsed> = BTreeMap::new();
        let mut left: BTreeMap<(String, CivilDate, Unconvertible), usize> = BTreeMap::new();
        for row in &self.rows {
            match self.converted(row, target, pnl, rates) {
                Ok((amount, rate)) => {
                    net = net.saturating_add(amount);
                    values.push(amount);
                    if let Some(money) = pnl.book.money(&row.symbol) {
                        assets.insert(row.symbol.clone(), money.currency.clone());
                    }
                    if let Some(rate) = rate {
                        used.insert((rate.day, rate.from.clone()), rate);
                    }
                }
                Err(why) => {
                    values.push(Decimal::ZERO);
                    let day = CivilDate::from_ms(row.trade.closed_ms, self.tz);
                    *left.entry((row.symbol.clone(), day, why)).or_default() += 1;
                }
            }
        }
        let readout = MergeReadout {
            currency: target.clone(),
            assets: assets.into_iter().collect(),
            rates: used.into_values().collect(),
            unconverted: left
                .into_iter()
                .map(|((symbol, day, why), trades)| Unconverted {
                    symbol,
                    day,
                    why,
                    trades,
                })
                .collect(),
            net,
        };
        (readout, values)
    }
}

#[cfg(test)]
#[path = "merge_tests.rs"]
mod merge_tests;
