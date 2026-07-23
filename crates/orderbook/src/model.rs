use rust_decimal::Decimal;

use crate::BookError;

/// One side of a limit order book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BookSide {
    /// Resting buy orders.
    Bid,
    /// Resting sell orders.
    Ask,
}

impl std::fmt::Display for BookSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bid => f.write_str("bid"),
            Self::Ask => f.write_str("ask"),
        }
    }
}

/// A price level carrying the total resting quantity at that price.
///
/// A zero quantity is valid in a [`BookDelta`] and means "remove this level".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookLevel {
    pub(crate) price: Decimal,
    pub(crate) quantity: Decimal,
}

impl BookLevel {
    /// Construct a validated level.
    ///
    /// # Errors
    ///
    /// Returns [`BookError::InvalidPrice`] when `price <= 0`, or
    /// [`BookError::InvalidQuantity`] when `quantity < 0`.
    pub fn new(price: Decimal, quantity: Decimal) -> Result<Self, BookError> {
        validate_values(None, price, quantity)?;
        Ok(Self { price, quantity })
    }

    /// The level price.
    #[must_use]
    pub fn price(self) -> Decimal {
        self.price
    }

    /// The total resting quantity at this price.
    #[must_use]
    pub fn quantity(self) -> Decimal {
        self.quantity
    }
}

/// How much of the exchange book a snapshot is known to cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookCoverage {
    /// The source declares that the snapshot covers the complete book.
    Full,
    /// The source exposes at most this many levels on each side.
    Limited {
        /// Maximum number of levels returned per side.
        levels_per_side: usize,
    },
}

/// A complete replacement for the currently known local book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSnapshot {
    last_update_id: u64,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    coverage: BookCoverage,
}

impl BookSnapshot {
    /// Construct a snapshot value.
    ///
    /// Level and crossed-book validation happens atomically when the snapshot
    /// is installed into an [`OrderBook`](crate::OrderBook).
    #[must_use]
    pub fn new(
        last_update_id: u64,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
        coverage: BookCoverage,
    ) -> Self {
        Self {
            last_update_id,
            bids,
            asks,
            coverage,
        }
    }

    /// Exchange update id represented by this snapshot.
    #[must_use]
    pub fn last_update_id(&self) -> u64 {
        self.last_update_id
    }

    /// Bid levels supplied by the snapshot.
    #[must_use]
    pub fn bids(&self) -> &[BookLevel] {
        &self.bids
    }

    /// Ask levels supplied by the snapshot.
    #[must_use]
    pub fn asks(&self) -> &[BookLevel] {
        &self.asks
    }

    /// Declared snapshot coverage.
    #[must_use]
    pub fn coverage(&self) -> BookCoverage {
        self.coverage
    }

    pub(crate) fn into_parts(self) -> (u64, Vec<BookLevel>, Vec<BookLevel>, BookCoverage) {
        (self.last_update_id, self.bids, self.asks, self.coverage)
    }
}

/// One exchange depth event containing absolute updates for touched levels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookDelta {
    first_update_id: u64,
    final_update_id: u64,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
}

impl BookDelta {
    /// Construct a delta value.
    ///
    /// The update-id range and book invariants are validated when the delta is
    /// applied to an [`OrderBook`](crate::OrderBook).
    #[must_use]
    pub fn new(
        first_update_id: u64,
        final_update_id: u64,
        bids: Vec<BookLevel>,
        asks: Vec<BookLevel>,
    ) -> Self {
        Self {
            first_update_id,
            final_update_id,
            bids,
            asks,
        }
    }

    /// First exchange update id represented by this event.
    #[must_use]
    pub fn first_update_id(&self) -> u64 {
        self.first_update_id
    }

    /// Final exchange update id represented by this event.
    #[must_use]
    pub fn final_update_id(&self) -> u64 {
        self.final_update_id
    }

    /// Absolute bid updates.
    #[must_use]
    pub fn bids(&self) -> &[BookLevel] {
        &self.bids
    }

    /// Absolute ask updates.
    #[must_use]
    pub fn asks(&self) -> &[BookLevel] {
        &self.asks
    }
}

pub(crate) fn validate_level(side: BookSide, level: BookLevel) -> Result<(), BookError> {
    validate_values(Some(side), level.price, level.quantity)
}

fn validate_values(
    side: Option<BookSide>,
    price: Decimal,
    quantity: Decimal,
) -> Result<(), BookError> {
    if price <= Decimal::ZERO {
        return Err(BookError::InvalidPrice { side, price });
    }
    if quantity < Decimal::ZERO {
        return Err(BookError::InvalidQuantity {
            side,
            price,
            quantity,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_rejects_non_positive_price() {
        assert_eq!(
            BookLevel::new(Decimal::ZERO, Decimal::ONE),
            Err(BookError::InvalidPrice {
                side: None,
                price: Decimal::ZERO,
            })
        );
        assert!(matches!(
            BookLevel::new(-Decimal::ONE, Decimal::ONE),
            Err(BookError::InvalidPrice { .. })
        ));
    }

    #[test]
    fn level_rejects_negative_quantity_but_accepts_zero() {
        assert!(matches!(
            BookLevel::new(Decimal::ONE, -Decimal::ONE),
            Err(BookError::InvalidQuantity { .. })
        ));
        assert_eq!(
            BookLevel::new(Decimal::ONE, Decimal::ZERO)
                .unwrap()
                .quantity(),
            Decimal::ZERO
        );
    }
}
