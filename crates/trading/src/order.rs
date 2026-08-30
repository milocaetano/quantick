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

/// Identifies one one-cancels-the-other pair: the take profit and the stop
/// loss of a single ladder part. Monotonic within a session like [`OrderId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OcoId(pub u64);

impl std::fmt::Display for OcoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "oco{}", self.0)
    }
}

/// What an order is for. An entry opens or adds; the other two reduce a
/// position that already exists, and are created by the simulator when an
/// entry carrying a [`Bracket`] fills — never placed directly by a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrderRole {
    /// A user entry: market, limit or stop.
    #[default]
    Entry,
    /// The winning-side leg of a ladder part: a resting limit.
    TakeProfit,
    /// The losing-side leg of a ladder part: a protective stop.
    StopLoss,
}

impl OrderRole {
    /// Stable snake_case label for logs, order lists and the control plane.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::TakeProfit => "take_profit",
            Self::StopLoss => "stop_loss",
        }
    }

    /// True for the two roles that only ever reduce an open position.
    #[must_use]
    pub fn is_protective(self) -> bool {
        !matches!(self, Self::Entry)
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

/// The most parts one entry's exit ladder may carry.
///
/// A bound, not a preference: the ladder is walked on every print, so its
/// cost has to be something a reader can see rather than something the
/// configuration decides. Four covers the ladders traders actually build
/// (halves, thirds, quarters) and keeps [`Bracket`] a `Copy` array instead
/// of a heap allocation on a per-trade path.
pub const MAX_EXIT_PARTS: usize = 4;

/// One rung of an exit ladder: a slice of the entry with its own protection.
///
/// Both levels are optional and independent — a part may take profit with no
/// stop, or stop out with no target — but a part with neither protects
/// nothing and is refused at placement rather than resting uselessly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ExitPart {
    /// How much of the entry this part protects. `None` means the whole
    /// fill, which is what a plain bracket wants: it never has to know the
    /// quantity in advance, and it still works when the fill is partial.
    pub quantity: Option<Decimal>,
    /// Exit price on the losing side (a protective stop).
    pub stop_loss: Option<Decimal>,
    /// Exit price on the winning side (a resting limit).
    pub take_profit: Option<Decimal>,
}

impl ExitPart {
    /// True when the part carries no protection at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stop_loss.is_none() && self.take_profit.is_none()
    }
}

/// Why an exit ladder was refused before it ever rested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderError {
    /// More parts than [`MAX_EXIT_PARTS`].
    TooManyParts,
    /// A part carries neither a stop loss nor a take profit.
    PartProtectsNothing,
    /// A part's quantity is zero or negative.
    PartQuantityNotPositive,
}

impl LadderError {
    /// A sentence for the trader, saying what to do instead.
    #[must_use]
    pub fn advice(self) -> &'static str {
        match self {
            Self::TooManyParts => "an exit ladder takes at most four parts - merge two of them",
            Self::PartProtectsNothing => {
                "every part needs a target or a stop - give this one either, or remove it"
            }
            Self::PartQuantityNotPositive => "every part must protect a positive quantity",
        }
    }
}

/// The protection attached to an entry, applied when the entry fills.
///
/// One shape covers both cases the trader knows: a plain bracket is a ladder
/// of one part covering the whole fill, and a strategy's partial exits are a
/// ladder of several. There is deliberately no second mechanism — a position
/// protected two different ways is a position whose two screens disagree.
///
/// Empty means "no protection on this entry"; the simulator never invents a
/// level the user did not place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bracket {
    parts: [Option<ExitPart>; MAX_EXIT_PARTS],
}

impl Bracket {
    /// A bracket with no protective prices.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The plain bracket: one part, both levels, covering the whole fill.
    #[must_use]
    pub fn whole(stop_loss: Option<Decimal>, take_profit: Option<Decimal>) -> Self {
        if stop_loss.is_none() && take_profit.is_none() {
            return Self::none();
        }
        let mut parts = [None; MAX_EXIT_PARTS];
        parts[0] = Some(ExitPart {
            quantity: None,
            stop_loss,
            take_profit,
        });
        Self { parts }
    }

    /// A ladder of parts, in the order the trader listed them.
    ///
    /// # Errors
    ///
    /// [`LadderError`] when there are too many parts, when a part protects
    /// nothing, or when a part's quantity is not positive.
    pub fn ladder(parts: &[ExitPart]) -> Result<Self, LadderError> {
        if parts.len() > MAX_EXIT_PARTS {
            return Err(LadderError::TooManyParts);
        }
        let mut slots = [None; MAX_EXIT_PARTS];
        for (slot, part) in slots.iter_mut().zip(parts) {
            if part.is_empty() {
                return Err(LadderError::PartProtectsNothing);
            }
            if part
                .quantity
                .is_some_and(|quantity| quantity <= Decimal::ZERO)
            {
                return Err(LadderError::PartQuantityNotPositive);
            }
            *slot = Some(*part);
        }
        Ok(Self { parts: slots })
    }

    /// The parts, in the trader's own order.
    pub fn parts(&self) -> impl Iterator<Item = &ExitPart> {
        self.parts.iter().flatten()
    }

    /// True when nothing is protected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts().next().is_none()
    }

    /// The single protective stop, when this bracket is one whole-fill part.
    /// `None` for a real ladder — several parts have several stops, and
    /// answering with one of them would be a lie.
    #[must_use]
    pub fn stop_loss(&self) -> Option<Decimal> {
        self.sole_whole_part()?.stop_loss
    }

    /// The single target; see [`Bracket::stop_loss`].
    #[must_use]
    pub fn take_profit(&self) -> Option<Decimal> {
        self.sole_whole_part()?.take_profit
    }

    /// True when this bracket splits the entry rather than covering it whole.
    #[must_use]
    pub fn is_laddered(&self) -> bool {
        !self.is_empty() && self.sole_whole_part().is_none()
    }

    fn sole_whole_part(&self) -> Option<&ExitPart> {
        let mut parts = self.parts();
        let first = parts.next()?;
        if parts.next().is_some() || first.quantity.is_some() {
            return None;
        }
        Some(first)
    }
}

/// A pending (not yet filled) order: a user entry, or one of the protective
/// legs the simulator created when a bracketed entry filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: OrderId,
    /// Direction of the entry: `Buy` opens or adds to a long, `Sell` to a
    /// short. An order against the current position closes it first and
    /// opens the remainder (netting). On a protective leg this is the
    /// reducing side — a sell protects a long.
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
    /// Fill only into an account with no open position: if the print that
    /// would fill this order arrives while a position is open, the order
    /// cancels instead ([`crate::CancelReason::AccountOccupied`]). The
    /// strategy kernel sets this on its resting entries — an order whose
    /// reason to exist assumed a flat account must never execute against a
    /// position a human opened while it rested.
    pub flat_only: bool,
    /// Venue time of the last print seen when the order was placed. The
    /// simulator has no clock of its own.
    pub placed_ms: i64,
    /// Entry, or one of the two protective legs of a ladder part.
    pub role: OrderRole,
    /// The OCO pair this leg belongs to; `None` on an entry. When one leg
    /// of a pair fills, its sibling cancels
    /// ([`crate::CancelReason::OcoFilled`]).
    pub oco: Option<OcoId>,
    /// True on a protective leg: it may only reduce an open position, and
    /// is clamped to what remains rather than reversing it.
    pub reduce_only: bool,
}

impl Order {
    /// True when this order protects a position rather than opening one.
    #[must_use]
    pub fn is_protective(&self) -> bool {
        self.role.is_protective()
    }
}
