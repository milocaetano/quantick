//! Pending order types: what the user asked for, before the tape answers.

use quantick_engine::Side;
use rust_decimal::Decimal;

/// Simulator-assigned order identifier, monotonic within one session.
///
/// Ids are never reused, so a stale id (an order that already filled or was
/// cancelled) is always detectable instead of silently addressing a
/// different order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OrderId(pub u64);

impl std::fmt::Display for OrderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// How an entry order meets the market (fill rules in the crate doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Fill at the next print, whatever its price.
    Market,
    /// Rest until a print trades at or through the price; fill at the price.
    Limit,
    /// Arm until a print trades at or through the trigger; fill at that
    /// print's price.
    Stop,
}

impl EntryKind {
    /// Lowercase label for logs and order lists.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Limit => "limit",
            Self::Stop => "stop",
        }
    }
}

/// Protective exit prices attached to an entry order, applied to the
/// position when the entry fills. `None` means "no protection on that side"
/// — the simulator never invents a level the user did not place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bracket {
    /// Exit price on the losing side (a protective stop).
    pub stop_loss: Option<Decimal>,
    /// Exit price on the winning side (a resting limit).
    pub take_profit: Option<Decimal>,
}

impl Bracket {
    /// A bracket with no protective prices.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// True when neither protective price is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stop_loss.is_none() && self.take_profit.is_none()
    }
}

/// A pending (not yet filled) entry order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: OrderId,
    /// Direction of the entry: `Buy` opens or adds to a long, `Sell` to a
    /// short. An order against the current position closes it first and
    /// opens the remainder (netting).
    pub side: Side,
    pub kind: EntryKind,
    /// Limit price for `Limit`, trigger price for `Stop`, `None` for
    /// `Market` — a market order has no price of its own by definition.
    pub price: Option<Decimal>,
    pub quantity: Decimal,
    pub bracket: Bracket,
    /// Price-cancel level for a resting limit: a print trading at or
    /// through it before the order fills removes the order
    /// ([`crate::CancelReason::PriceTouched`]) — "cancel the retest entry
    /// once the move completes without it". Only limit entries carry one;
    /// validation keeps it on the far side of the market from the limit
    /// price, so no single print can ever satisfy both fill and cancel.
    pub cancel_at: Option<Decimal>,
    /// Venue time of the last print seen when the order was placed. The
    /// simulator has no clock of its own.
    pub placed_ms: i64,
}
