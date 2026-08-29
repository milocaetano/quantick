//! What was asked of a venue, before the venue answers.
//!
//! An [`OrderIntent`] is deliberately *not* an [`Order`]: an order has an
//! id, a placement time and a resting price, all of which are the venue's
//! answer. An intent is the question — the thing a chart gesture, a script
//! or a strategy produces, and the only shape a caller needs to build.
//!
//! [`Order`]: crate::Order

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::order::{Bracket, EntryKind, OrderId};

/// One order a caller wants placed.
///
/// The three constructors ([`market`](Self::market), [`limit`](Self::limit),
/// [`stop`](Self::stop)) are the only way to build one, because `kind` and
/// `price` are not independent: a market order has no price of its own and a
/// resting order must have one. Pairing them in the constructor makes the
/// invalid combinations unrepresentable rather than merely rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderIntent {
    /// Direction of the entry: `Buy` opens or adds to a long, `Sell` to a
    /// short. An order against an open position closes it first and opens
    /// the remainder, on any venue that nets.
    pub side: Side,
    /// How the entry means to meet the market.
    pub kind: EntryKind,
    /// Limit price for [`EntryKind::Limit`], trigger price for
    /// [`EntryKind::Stop`], `None` for [`EntryKind::Market`].
    pub price: Option<Decimal>,
    pub quantity: Decimal,
    /// Protective prices to attach to the fill. Empty means "no protection"
    /// — a venue never invents a level the caller did not ask for.
    pub bracket: Bracket,
    /// Price-cancel level for a resting limit: reaching it removes the
    /// order unfilled. `None` on every other kind.
    pub cancel_at: Option<Decimal>,
    /// Fill only into an account with no open position. For callers whose
    /// reason to place the order assumed a flat account.
    pub flat_only: bool,
}

impl OrderIntent {
    /// Meet the market at once, at whatever the next trade prints.
    #[must_use]
    pub fn market(side: Side, quantity: Decimal) -> Self {
        Self {
            side,
            kind: EntryKind::Market,
            price: None,
            quantity,
            bracket: Bracket::none(),
            cancel_at: None,
            flat_only: false,
        }
    }

    /// Rest at `price` until the market trades at or through it.
    #[must_use]
    pub fn limit(side: Side, quantity: Decimal, price: Decimal) -> Self {
        Self {
            price: Some(price),
            kind: EntryKind::Limit,
            ..Self::market(side, quantity)
        }
    }

    /// Arm at `trigger` until the market trades at or through it.
    #[must_use]
    pub fn stop(side: Side, quantity: Decimal, trigger: Decimal) -> Self {
        Self {
            price: Some(trigger),
            kind: EntryKind::Stop,
            ..Self::market(side, quantity)
        }
    }

    /// Attach protective prices to the fill this intent produces.
    #[must_use]
    pub fn with_bracket(mut self, bracket: Bracket) -> Self {
        self.bracket = bracket;
        self
    }

    /// Give a resting limit a price at which to give up and remove itself.
    #[must_use]
    pub fn with_cancel_at(mut self, cancel_at: Option<Decimal>) -> Self {
        self.cancel_at = cancel_at;
        self
    }

    /// Refuse to fill into an account that already holds a position.
    #[must_use]
    pub fn only_when_flat(mut self) -> Self {
        self.flat_only = true;
        self
    }

    /// Whether this intent rests rather than meeting the market at once.
    #[must_use]
    pub fn rests(&self) -> bool {
        !matches!(self.kind, EntryKind::Market)
    }
}

/// Whose protective prices an amendment addresses.
///
/// The distinction is the whole reason the enum exists: a position's bracket
/// is live and judged against the market, while a *working* order's bracket
/// is a promise judged against the order's own resting price and armed only
/// if and when that order fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracketTarget {
    /// The open position's protective prices.
    Position,
    /// A working order's protective prices, which arm on its fill.
    Order(OrderId),
}

/// How much of the open position a close addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAmount {
    /// The whole position.
    All,
    /// At most this quantity. Closing more than is open closes what is
    /// open — a partial close never reverses.
    Partial(Decimal),
}
