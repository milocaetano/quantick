//! What the terminal says about the account it is logged into.
//!
//! The vocabulary is the venue-neutral one in `quantick_trading::broker`, so
//! the account that routes orders reads it without naming this crate; this
//! module only reads the bridge's `account` object into it.

pub use quantick_trading::broker::{AccountFacts, MarginMode, TradeMode};

use crate::wire::{AccountLine, decimal};

/// The bridge's `account` object as [`AccountFacts`].
pub(crate) fn facts_from_line(line: &AccountLine) -> AccountFacts {
    AccountFacts {
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
