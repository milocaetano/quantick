//! The derivatives data port: what a venue must hand the Liquidations and
//! OI + Funding models. No feed produces these yet; a feed that learns to
//! advertises it through its capabilities and fills these types, and the
//! models take them as plain slices with a clock the caller passes in.

use rust_decimal::Decimal;

/// Which side of the book a forced close came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LiquidatedSide {
    /// A long was closed: the venue sold into the book.
    Long,
    /// A short was closed: the venue bought from the book.
    Short,
}

/// One forced close the venue reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidationPrint {
    /// Venue time, epoch milliseconds.
    pub timestamp_ms: i64,
    /// The position that was closed.
    pub side: LiquidatedSide,
    /// Average fill price of the close.
    pub price: Decimal,
    /// Filled quantity, in base units.
    pub quantity: Decimal,
}

impl LiquidationPrint {
    /// Price times quantity, in quote units.
    #[must_use]
    pub fn notional(&self) -> Decimal {
        self.price * self.quantity
    }
}

/// Open interest at one moment, with the price the feed stamped beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenInterestSample {
    /// Venue time of the reading, epoch milliseconds.
    pub timestamp_ms: i64,
    /// Open contracts, in base units.
    pub open_interest: Decimal,
    /// The instrument's last price when the reading was taken.
    pub price: Decimal,
}

/// How the open interest reached us, so the pane can say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenInterestOrigin {
    /// Polled from the venue's REST endpoint at this interval.
    VenueRest { poll_interval_ms: i64 },
    /// Pushed by the venue's stream.
    VenueStream,
}

/// The perpetual's funding state at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingSnapshot {
    /// Venue time of the snapshot, epoch milliseconds.
    pub timestamp_ms: i64,
    /// The rate the next settlement charges, as a fraction (`0.0001` is
    /// 0.01 %).
    pub rate: Decimal,
    /// The venue's estimate of the settlement after it, when it publishes
    /// one.
    pub predicted_rate: Option<Decimal>,
    /// When the next settlement happens, epoch milliseconds.
    pub next_funding_ms: i64,
    /// Time between settlements.
    pub interval_ms: i64,
}
