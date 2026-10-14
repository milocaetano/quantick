//! The ticket's money settings as a named call edits them.
//!
//! `trade.set_risk` and `trade.set_instrument_money` change the same risk per
//! trade, declared capital and instrument money the ticket's fields do. The
//! validation and the edit live here, beside the account they change, so a
//! host only reads its input and hands it over: the refusals are the
//! account's sentences, and a refused edit changes nothing.

use quantick_trading::{Currency, InstrumentMoney, MoneySource};
use rust_decimal::Decimal;

use super::PaperAccount;
use crate::risk_sizing::RiskBasis;

/// A risk-per-trade change, validated before any account is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskEdit {
    /// The basis the risk is measured on.
    pub basis: RiskBasis,
    /// The fixed amount one trade may lose; `None` keeps the current one.
    pub amount: Option<Decimal>,
    /// The share of capital one trade may lose; `None` keeps the current one.
    pub percent: Option<Decimal>,
    /// The lock; `None` keeps the current one.
    pub lock: Option<bool>,
    /// The capital to declare and its currency. A zero or negative amount
    /// clears that currency's declaration.
    pub capital: Option<(Decimal, Currency)>,
    /// The currency the call named, which stamps a new amount.
    pub currency: Option<Currency>,
}

/// The fields of a risk-per-trade change, as text a caller typed.
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskEditText<'a> {
    /// `off`, `amount` or `percent`.
    pub basis: &'a str,
    /// The fixed amount, as a decimal string.
    pub amount: Option<&'a str>,
    /// The percentage, as a decimal string.
    pub percent: Option<&'a str>,
    /// The capital to declare, as a decimal string.
    pub capital: Option<&'a str>,
    /// The capital's currency code.
    pub currency: Option<&'a str>,
    /// The lock.
    pub lock: Option<bool>,
}

fn decimal(field: &str, text: Option<&str>) -> Result<Option<Decimal>, String> {
    text.map(|text| {
        Decimal::from_str_exact(text.trim()).map_err(|_| format!("{field} must be a decimal"))
    })
    .transpose()
}

fn currency(code: &str) -> Result<Currency, String> {
    Currency::new(code).ok_or_else(|| "currency must not be blank".to_owned())
}

impl RiskEdit {
    /// Read a change from text, refusing what the ticket's fields refuse.
    ///
    /// # Errors
    ///
    /// The sentence a caller reads: an unknown basis, a field that is not a
    /// decimal, a risk at or below zero, or a capital without its currency
    /// (or a currency without its capital).
    pub fn parse(text: RiskEditText<'_>) -> Result<Self, String> {
        let basis = RiskBasis::from_token(text.basis)
            .ok_or_else(|| "basis must be `off`, `amount` or `percent`".to_owned())?;
        let amount = decimal("amount", text.amount)?;
        let percent = decimal("percent", text.percent)?;
        let capital = decimal("capital", text.capital)?;
        // The UI and the launch hook both refuse a non-positive risk; a third
        // entry point that accepts one would persist it and fan it to every
        // tab, leaving the ticket saying "set a risk per trade above zero"
        // about a number the trader never typed - and surviving a restart.
        for (field, value) in [("amount", amount), ("percent", percent)] {
            if value.is_some_and(|value| value <= Decimal::ZERO) {
                return Err(format!("{field} must be above zero"));
            }
        }
        // A capital with no currency has nothing to be keyed by, and a
        // currency with no capital declares nothing. Both or neither.
        let (capital, currency) = match (capital, text.currency) {
            (Some(amount), Some(code)) => {
                let currency = currency(code)?;
                (Some((amount, currency.clone())), Some(currency))
            }
            (None, None) => (None, None),
            _ => {
                return Err("capital and currency go together - nothing here converts \
                            between currencies"
                    .to_owned());
            }
        };
        Ok(Self {
            basis,
            amount,
            percent,
            lock: text.lock,
            capital,
            currency,
        })
    }
}

impl PaperAccount {
    /// Apply a validated risk-per-trade change.
    pub fn apply_risk_edit(&mut self, edit: RiskEdit) {
        // The currency an amount set through this call is denominated in: the
        // one the call named, or the chart's own instrument. Read before the
        // mutation so it describes the instrument the caller was looking at.
        let instrument_currency = self.symbol_money().map(|money| money.currency.clone());
        let mut risk = self.risk_settings().clone();
        risk.basis = edit.basis;
        if let Some(amount) = edit.amount {
            risk.amount = amount;
            // Stamped with the currency it was entered in, never left to
            // adopt whichever instrument a tab happens to be on later.
            risk.amount_currency = edit.currency.or(instrument_currency);
        }
        if let Some(percent) = edit.percent {
            risk.percent = percent;
        }
        if let Some(lock) = edit.lock {
            risk.lock = lock;
        }
        self.set_risk_settings(risk);
        if let Some((amount, currency)) = edit.capital {
            let mut declared = self.capital().clone();
            if amount > Decimal::ZERO {
                declared.insert(currency.code().to_owned(), amount);
            } else {
                declared.remove(currency.code());
            }
            self.set_capital(declared);
        }
    }

    /// Declare what a point of `symbol` (the chart's own when `None` or
    /// blank) is worth, or clear the declaration when any of the three
    /// facts is missing: half a declaration is no declaration.
    ///
    /// # Errors
    ///
    /// No symbol to declare for, a fact that is not a decimal or is not
    /// above zero, or a blank currency. A refused declaration changes
    /// nothing.
    pub fn declare_instrument_money(
        &mut self,
        symbol: Option<&str>,
        point_value: Option<&str>,
        size_step: Option<&str>,
        currency_code: Option<&str>,
    ) -> Result<(), String> {
        let symbol = match symbol.map(str::trim) {
            Some(symbol) if !symbol.is_empty() => symbol.to_owned(),
            _ => self.symbol().to_owned(),
        };
        if symbol.is_empty() {
            return Err("no chart symbol to declare money for - name one".to_owned());
        }
        let mut book = self.instrument_money().clone();
        let (Some(point_value), Some(size_step), Some(code)) =
            (point_value, size_step, currency_code)
        else {
            book.remove(&symbol);
            self.set_instrument_money(book);
            return Ok(());
        };
        let parse = |field: &str, text: &str| {
            Decimal::from_str_exact(text.trim()).map_err(|_| format!("{field} must be a decimal"))
        };
        let point_value = parse("point_value", point_value)?;
        let size_step = parse("size_step", size_step)?;
        if point_value <= Decimal::ZERO || size_step <= Decimal::ZERO {
            return Err("point_value and size_step must both be above zero".to_owned());
        }
        let currency = currency(code)?;
        let existing = book.get(&symbol);
        let money = InstrumentMoney {
            point_value,
            size_step,
            // Whatever minimum is already declared is the trader's, the same
            // as the maximum beside it.
            min_size: existing.map_or(size_step, |money| money.min_size),
            max_size: existing.and_then(|money| money.max_size),
            currency,
            source: MoneySource::Declared,
        };
        book.insert(symbol, money);
        self.set_instrument_money(book);
        Ok(())
    }
}

#[cfg(test)]
#[path = "ticket_edit_tests.rs"]
mod ticket_edit_tests;
