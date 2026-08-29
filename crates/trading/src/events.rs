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
    Closed(ClosedTrade),
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
}
