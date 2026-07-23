use rust_decimal::Decimal;

use crate::BookSide;

/// A domain or sequence violation that prevented a book mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookError {
    /// A delta was applied before any snapshot initialized the book.
    NotInitialized,
    /// The event's first update id was greater than its final update id.
    InvalidSequence {
        /// Event's first update id.
        first_update_id: u64,
        /// Event's final update id.
        final_update_id: u64,
    },
    /// The event begins after the next update id required by the local book.
    SequenceGap {
        /// Next update id the local book needed.
        expected_update_id: u64,
        /// First update id carried by the received event.
        first_update_id: u64,
        /// Final update id carried by the received event.
        final_update_id: u64,
    },
    /// A price was zero or negative.
    InvalidPrice {
        /// Side being installed/applied, or `None` when caught by the
        /// side-independent [`BookLevel`](crate::BookLevel) constructor.
        side: Option<BookSide>,
        /// Invalid price.
        price: Decimal,
    },
    /// A quantity was negative.
    InvalidQuantity {
        /// Side being installed/applied, or `None` when caught by the
        /// side-independent [`BookLevel`](crate::BookLevel) constructor.
        side: Option<BookSide>,
        /// Price whose quantity was invalid.
        price: Decimal,
        /// Invalid quantity.
        quantity: Decimal,
    },
    /// One snapshot or delta mentioned the same side/price more than once.
    DuplicatePrice {
        /// Side containing the duplicate.
        side: BookSide,
        /// Repeated price.
        price: Decimal,
    },
    /// The resulting best bid was greater than or equal to the best ask.
    CrossedBook {
        /// Highest resulting bid.
        best_bid: Decimal,
        /// Lowest resulting ask.
        best_ask: Decimal,
    },
}

impl std::fmt::Display for BookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInitialized => f.write_str("order book has no installed snapshot"),
            Self::InvalidSequence {
                first_update_id,
                final_update_id,
            } => write!(
                f,
                "invalid update-id range: first {first_update_id} exceeds final {final_update_id}"
            ),
            Self::SequenceGap {
                expected_update_id,
                first_update_id,
                final_update_id,
            } => write!(
                f,
                "order-book sequence gap: expected update {expected_update_id}, got range \
                 {first_update_id}..={final_update_id}"
            ),
            Self::InvalidPrice { side, price } => {
                write_side(f, *side)?;
                write!(f, "price must be positive, got {price}")
            }
            Self::InvalidQuantity {
                side,
                price,
                quantity,
            } => {
                write_side(f, *side)?;
                write!(
                    f,
                    "quantity at price {price} must be non-negative, got {quantity}"
                )
            }
            Self::DuplicatePrice { side, price } => {
                write!(f, "duplicate {side} price {price} in one book event")
            }
            Self::CrossedBook { best_bid, best_ask } => write!(
                f,
                "crossed order book: best bid {best_bid} is not below best ask {best_ask}"
            ),
        }
    }
}

impl std::error::Error for BookError {}

fn write_side(f: &mut std::fmt::Formatter<'_>, side: Option<BookSide>) -> std::fmt::Result {
    if let Some(side) = side {
        write!(f, "{side} ")
    } else {
        Ok(())
    }
}
