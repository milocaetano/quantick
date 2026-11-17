//! What the terminal says about the account it is logged into.

use rust_decimal::Decimal;

use crate::wire::{AccountLine, decimal};

/// Whether real money is at stake, as the terminal reports it
/// (`account_info().trade_mode`). Read, never configured: a route the
/// trader picks is checked against this, not the other way round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeMode {
    Demo,
    Contest,
    Real,
    /// A value this crate does not know. Treated as real by every caller
    /// that has to choose: guessing "demo" is the mistake that costs money.
    Unknown,
}

impl TradeMode {
    /// Whether a caller must treat the account as real money.
    #[must_use]
    pub fn is_real_money(self) -> bool {
        matches!(self, Self::Real | Self::Unknown)
    }
}

/// How the account holds positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginMode {
    /// One net position per symbol (retail netting).
    Netting,
    /// One net position per symbol, on an exchange (B3).
    Exchange,
    /// Several positions per symbol. Refused: every surface here reads one
    /// net position, and a hedged account would show the trader a sum that
    /// is not what the broker closes.
    Hedging,
    Unknown,
}

impl MarginMode {
    /// Whether the account nets, the only kind this venue trades.
    #[must_use]
    pub fn nets(self) -> bool {
        matches!(self, Self::Netting | Self::Exchange)
    }
}

/// The account facts a surface shows beside a broker route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountFacts {
    pub login: u64,
    pub server: String,
    pub company: String,
    pub trade_mode: TradeMode,
    pub margin_mode: MarginMode,
    /// `None` when the terminal sent something that is not a number.
    pub balance: Option<Decimal>,
    pub currency: String,
}

impl AccountFacts {
    pub(crate) fn from_line(line: &AccountLine) -> Self {
        Self {
            login: line.login,
            server: line.server.clone(),
            company: line.company.clone(),
            trade_mode: match line.trade_mode.as_str() {
                "demo" => TradeMode::Demo,
                "contest" => TradeMode::Contest,
                "real" => TradeMode::Real,
                _ => TradeMode::Unknown,
            },
            margin_mode: match line.margin_mode.as_str() {
                "netting" => MarginMode::Netting,
                "exchange" => MarginMode::Exchange,
                "hedging" => MarginMode::Hedging,
                _ => MarginMode::Unknown,
            },
            balance: decimal(&line.balance),
            currency: line.currency.clone(),
        }
    }

    /// Whether `other` is the same account: same login on the same server.
    #[must_use]
    pub fn same_account(&self, other: &Self) -> bool {
        self.login == other.login && self.server == other.server
    }
}
