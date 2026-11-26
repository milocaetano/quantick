//! The order vocabulary as a named call states it: which way, and how an
//! entry meets the market. The published names (`ActionSide`,
//! `ActionEntryKind`) are the wire's, so these stay apart from [`Side`] and
//! [`EntryKind`] and convert into them.

use quantick_engine::Side;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::EntryKind;

/// How an entry meets the market, stated rather than inferred.
///
/// The chart's aim infers the kind from where the pointer sits relative to
/// the last price. An action has no pointer, and an action whose meaning
/// depends on the market at the instant it lands is one nobody can replay —
/// so here the caller says which order they mean, and a kind that cannot
/// rest at the given price is refused with the venue's own words.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionEntryKind {
    /// Fill at the next print, whatever it is. Takes no price.
    Market,
    /// Rest at `price` until the market trades at or through it.
    Limit,
    /// Arm at `price` until the market trades at or through it.
    Stop,
}

impl ActionEntryKind {
    /// The simulator's entry kind.
    #[must_use]
    pub fn into_engine(self) -> EntryKind {
        match self {
            Self::Market => EntryKind::Market,
            Self::Limit => EntryKind::Limit,
            Self::Stop => EntryKind::Stop,
        }
    }
}

/// Which way the order goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionSide {
    Buy,
    Sell,
}

impl ActionSide {
    /// The engine's side.
    #[must_use]
    pub fn into_engine(self) -> Side {
        match self {
            Self::Buy => Side::Buy,
            Self::Sell => Side::Sell,
        }
    }
}
