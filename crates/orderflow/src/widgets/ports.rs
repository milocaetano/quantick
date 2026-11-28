//! The derivatives data port: what a venue must hand the Liquidations
//! model. No feed produces these yet. A feed that learns to provide them
//! advertises it through its capabilities and fills these types; the model
//! takes them as plain slices with a clock the caller passes in.

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
