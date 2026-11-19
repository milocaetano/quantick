//! Everything a venue reports back: fills, closures, refusals.

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::order::{Order, OrderId};
use crate::position::ClosedTrade;

/// Why an exit fill happened. Persisted in the trade history, so each
/// variant has a stable token (see [`ExitReason::as_str`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// The protective stop was traded at or through.
    StopLoss,
    /// The protective limit was traded at or through.
    TakeProfit,
    /// The user closed or flattened at market.
    Manual,
    /// An opposite entry order netted against the position.
    Reversal,
    /// The session was reset (a replay seek rebuilds the past); the
    /// simulator flattens at the last mark rather than pretend continuity.
    Reset,
}

impl ExitReason {
    /// Stable snake_case token used in the trade-history format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StopLoss => "stop_loss",
            Self::TakeProfit => "take_profit",
            Self::Manual => "manual",
            Self::Reversal => "reversal",
            Self::Reset => "reset",
        }
    }

    /// Inverse of [`as_str`](Self::as_str); `None` for an unknown token.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "stop_loss" => Some(Self::StopLoss),
            "take_profit" => Some(Self::TakeProfit),
            "manual" => Some(Self::Manual),
            "reversal" => Some(Self::Reversal),
            "reset" => Some(Self::Reset),
            _ => None,
        }
    }
}

/// What a simulated execution was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRole {
    /// An entry order (market, limit or stop) met the market.
    Entry(OrderId),
    /// The position's protective stop fired.
    StopLoss,
    /// The position's protective limit fired.
    TakeProfit,
    /// A user-commanded market close.
    Close,
    /// A session reset flattened the position at the last mark. The only
    /// fill not proven by a print — labeled so the history never lies.
    Reset,
}

/// One simulated execution. `agg_id`/`timestamp_ms` are those of the print
/// that caused the fill — the audit trail back to the tape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    pub timestamp_ms: i64,
    pub agg_id: u64,
    /// Direction of this execution (not of the position).
    pub side: Side,
    pub price: Decimal,
    pub quantity: Decimal,
    pub role: FillRole,
}

/// Why an order was removed without filling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The user cancelled it.
    User,
    /// A flatten command swept it.
    Flatten,
    /// A session reset discarded it.
    Reset,
    /// The tape traded at or through the order's cancel-at price before the
    /// order filled — the move the order was waiting to fade completed
    /// without it, so the order removed itself as instructed.
    PriceTouched,
    /// A flat-only order's price was reached while a position was open:
    /// filling would have traded against (or piled onto) a position the
    /// order's owner never accounted for, so it stood down instead.
    AccountOccupied,
    /// The other leg of this order's OCO pair filled. Its ladder part is
    /// closed, so this leg would have exited a quantity nobody holds.
    OcoFilled,
    /// The position this leg protected is gone - taken by another part, by
    /// a hand close, or reversed by an opposite entry. Protection never
    /// outlives what it protects.
    PositionClosed,
    /// The trader replaced the protection wholesale: a new bracket, or a
    /// bracketed entry averaging into the same position.
    BracketReplaced,
    /// The venue removed it on its own account: it expired, or it was
    /// cancelled at the venue's own terminal. Only a real venue reports
    /// this; a simulator removes nothing it was not told to.
    Venue,
}

/// The most bytes of a venue's own words a [`VenueText`] holds. Sized past
/// every return-code comment MetaTrader writes (the longest are under fifty)
/// and past the bridge's own refusals.
pub const VENUE_TEXT_BYTES: usize = 96;

/// A venue's own words, held inline so a refusal stays `Copy`: the strategy
/// kernel keeps the reason it was refused in plain-`Copy` state, and a heap
/// string here would ripple through every one of those types for one
/// adapter's sake. Longer text is cut at a character boundary and says so
/// ([`VenueText::is_truncated`], and a trailing `...` when displayed) rather
/// than ending silently mid-sentence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VenueText {
    bytes: [u8; VENUE_TEXT_BYTES],
    len: u8,
    truncated: bool,
}

impl VenueText {
    /// Hold `text`, or as much of it as fits.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut end = text.len().min(VENUE_TEXT_BYTES);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let mut bytes = [0; VENUE_TEXT_BYTES];
        bytes[..end].copy_from_slice(&text.as_bytes()[..end]);
        Self {
            bytes,
            len: u8::try_from(end).unwrap_or(u8::MAX),
            truncated: end < text.len(),
        }
    }

    /// The words held, exactly as the venue wrote them up to the cut.
    #[must_use]
    pub fn as_str(&self) -> &str {
        let held = &self.bytes[..usize::from(self.len)];
        // `new` only ever cuts at a character boundary, so this never fails;
        // an empty answer is still better than a panic on a refusal path.
        std::str::from_utf8(held).unwrap_or_default()
    }

    /// Whether the venue said more than this holds.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        self.truncated
    }
}

impl std::fmt::Display for VenueText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())?;
        if self.truncated {
            f.write_str("...")?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for VenueText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.to_string())
    }
}

/// Which kind of request a venue refused, when its refusal answers one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueRequest {
    /// A new order.
    Submit,
    /// A working order's removal.
    Cancel,
    /// A working order's new price or protections.
    Modify,
    /// The open position's stop and target.
    Stops,
    /// A deal taking the position off.
    Close,
}

impl VenueRequest {
    /// The kind in one word, for a log line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Submit => "submit",
            Self::Cancel => "cancel",
            Self::Modify => "modify",
            Self::Stops => "stops",
            Self::Close => "close",
        }
    }
}

/// Why a command was refused. Messages are written for a beginner: they say
/// what was wrong *and* what to do instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// No print has been seen yet — there is no market to trade against.
    NoMarketPrice,
    /// Quantity must be strictly positive.
    QuantityNotPositive,
    /// A price must be strictly positive.
    PriceNotPositive,
    /// A limit at or through the market would fill immediately.
    LimitOnWrongSide(Side),
    /// A stop at or through the market would trigger immediately.
    StopOnWrongSide(Side),
    /// A cancel-at price on the fill side of the market would cancel the
    /// order immediately (or race its own fill).
    CancelAtOnWrongSide(Side),
    /// A stop loss on the profit side of the reference price.
    StopLossOnWrongSide(Side),
    /// A take profit on the losing side of the reference price.
    TakeProfitOnWrongSide(Side),
    /// Close or bracket command with no open position.
    NoPosition,
    /// The order id does not name a pending order (it may have filled).
    UnknownOrder(OrderId),
    /// The venue itself refused: `code` is its numeric answer (a broker
    /// return code), `message` its own comment, reproduced verbatim. No
    /// code means the venue's own gateway refused before asking the venue,
    /// and the message is that gateway's. `request` names what was
    /// refused, when the venue knows.
    Venue {
        code: Option<i64>,
        message: VenueText,
        request: Option<VenueRequest>,
    },
    /// The venue adapter will not carry the command to the venue at all:
    /// the link is down, the account cannot be traded here, or the command
    /// asks for something the venue has no native way to do. The sentence
    /// says which, and what to do instead.
    Blocked(&'static str),
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoMarketPrice => {
                write!(
                    f,
                    "no trade has printed yet - wait for the first print before placing orders"
                )
            }
            Self::QuantityNotPositive => write!(f, "quantity must be greater than zero"),
            Self::PriceNotPositive => write!(f, "price must be greater than zero"),
            Self::LimitOnWrongSide(Side::Buy) => {
                write!(
                    f,
                    "a buy limit at or above the market would fill immediately - use a market order, or a stop to buy a breakout"
                )
            }
            Self::LimitOnWrongSide(Side::Sell) => {
                write!(
                    f,
                    "a sell limit at or below the market would fill immediately - use a market order, or a stop to sell a breakdown"
                )
            }
            Self::StopOnWrongSide(Side::Buy) => {
                write!(
                    f,
                    "a buy stop must sit above the market (it chases strength) - to buy below the market use a limit"
                )
            }
            Self::StopOnWrongSide(Side::Sell) => {
                write!(
                    f,
                    "a sell stop must sit below the market (it chases weakness) - to sell above the market use a limit"
                )
            }
            Self::CancelAtOnWrongSide(Side::Buy) => {
                write!(
                    f,
                    "a buy limit's cancel-at price must sit above the market - it names the move that makes waiting pointless; below the market it would cancel the order instantly"
                )
            }
            Self::CancelAtOnWrongSide(Side::Sell) => {
                write!(
                    f,
                    "a sell limit's cancel-at price must sit below the market - it names the move that makes waiting pointless; above the market it would cancel the order instantly"
                )
            }
            Self::StopLossOnWrongSide(Side::Buy) => {
                write!(
                    f,
                    "a long's stop loss must sit below the price it protects - above it, it would exit instantly"
                )
            }
            Self::StopLossOnWrongSide(Side::Sell) => {
                write!(
                    f,
                    "a short's stop loss must sit above the price it protects - below it, it would exit instantly"
                )
            }
            Self::TakeProfitOnWrongSide(Side::Buy) => {
                write!(
                    f,
                    "a long's take profit must sit above the price it targets - below it, it would exit instantly"
                )
            }
            Self::TakeProfitOnWrongSide(Side::Sell) => {
                write!(
                    f,
                    "a short's take profit must sit below the price it targets - above it, it would exit instantly"
                )
            }
            Self::NoPosition => write!(f, "there is no open position"),
            Self::UnknownOrder(id) => {
                write!(
                    f,
                    "order {id} is not pending - it may have already filled or been cancelled"
                )
            }
            Self::Venue {
                code: Some(code),
                message,
                ..
            } => write!(f, "the broker refused: {message} (code {code})"),
            Self::Venue {
                code: None,
                message,
                ..
            } => write!(f, "the broker bridge refused: {message}"),
            Self::Blocked(sentence) => f.write_str(sentence),
        }
    }
}

/// One thing the simulator did (or refused to do), in the order it happened.
/// The consumer journals fills and closures and surfaces rejections as
/// guidance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenueEvent {
    /// An order was accepted and now rests (or awaits the next print).
    Placed(Order),
    /// A pending order's price was modified.
    Updated(Order),
    /// A pending order was removed without filling.
    Cancelled { order: Order, reason: CancelReason },
    /// A command was refused; the reason says why and what to do instead.
    Rejected(RejectReason),
    /// A simulated execution happened.
    Filled(Fill),
    /// An exit completed a round trip against the average entry.
    Closed {
        trade: ClosedTrade,
        /// The terminal mode at execution. A simulator close has no broker account.
        broker_account: Option<crate::TradeMode>,
    },
    /// The position's protective prices were replaced.
    BracketSet {
        stop_loss: Option<Decimal>,
        take_profit: Option<Decimal>,
    },
    /// An attached protective price was dropped at fill time: the market
    /// moved past it between the command and the fill, and a level that
    /// would exit instantly with a lying label is dropped and reported —
    /// never silently kept.
    BracketDropped { reason: RejectReason },
    /// Something the venue reports that is no order's fate: its link, its
    /// account, an acknowledgement, what a reset left alone.
    Notice(VenueNotice),
}

/// What a real venue says about itself rather than about one order's fate.
///
/// A simulator never emits these: it has no link to lose, no account to
/// change under it and no acknowledgement to wait for. They exist so a venue
/// that does can say so on the same event stream every surface already
/// reads, rather than through a side channel one of them forgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VenueNotice {
    /// The venue confirmed an order (or a change to it) and holds it under
    /// its own reference, a broker ticket.
    Acknowledged { id: OrderId, venue_ref: u64 },
    /// A reset (a symbol switch, a replay seek) left the venue's orders and
    /// position exactly where they are: a real account does not rewind with
    /// the chart. Counts what stayed, so the sentence can say so.
    ResetLeftOpen { working: usize, position: bool },
    /// The link to the venue is down. Reads show the last known state,
    /// marked stale; protective prices already at the venue still rest
    /// there.
    LinkLost,
    /// The link is back and the venue's state is current again.
    LinkRestored,
    /// The venue now speaks for a different account than before. Nothing
    /// known about the previous one carries over.
    AccountChanged,
    /// The venue's account cannot be traded from here; the sentence says why.
    AccountRefused(&'static str),
    /// An action was sent but its answer was lost with the link, and
    /// resending it could repeat it. Its outcome is whatever the venue's own
    /// state shows next, never assumed.
    OutcomeUnknown { id: Option<OrderId> },
}

impl std::fmt::Display for VenueNotice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Acknowledged { id, venue_ref } => {
                write!(
                    f,
                    "order {id} acknowledged by the broker as ticket {venue_ref}"
                )
            }
            Self::ResetLeftOpen { working, position } => {
                let held = if *position { "the position and " } else { "" };
                write!(
                    f,
                    "nothing was closed at the broker: {held}{working} working order(s) stay open there"
                )
            }
            Self::LinkLost => write!(
                f,
                "broker link lost - showing the last known state; stops and targets already at the broker still rest there"
            ),
            Self::LinkRestored => write!(f, "broker link restored"),
            Self::AccountChanged => write!(
                f,
                "the terminal is logged into a different account now - nothing from the previous one carries over"
            ),
            Self::AccountRefused(sentence) => f.write_str(sentence),
            Self::OutcomeUnknown { id: Some(id) } => write!(
                f,
                "the answer for order {id} was lost with the link - check the broker's state before acting again"
            ),
            Self::OutcomeUnknown { id: None } => write!(
                f,
                "the answer for an action was lost with the link - check the broker's state before acting again"
            ),
        }
    }
}

#[cfg(test)]
#[path = "events_tests.rs"]
mod tests;
