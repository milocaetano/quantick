//! Pure Binance Spot depth wire decoding.
//!
//! Binance sends prices and quantities as decimal strings. They are decoded to
//! [`Decimal`] here, before they reach synchronization or the consumer, so the
//! full exchange precision is retained and malformed data is reported rather
//! than rounded or silently skipped.

use std::str::FromStr as _;

use quantick_orderbook::{BookCoverage, BookDelta, BookError, BookLevel, BookSnapshot};
use rust_decimal::Decimal;
use serde::Deserialize;

/// One absolute quantity at one price level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthLevel {
    /// Positive price.
    pub price: Decimal,
    /// Absolute quantity. Zero means "remove this level".
    pub quantity: Decimal,
}

/// A REST `/api/v3/depth` snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthSnapshot {
    /// Binance update id represented by this snapshot.
    pub last_update_id: u64,
    /// Bid levels.
    pub bids: Vec<DepthLevel>,
    /// Ask levels.
    pub asks: Vec<DepthLevel>,
}

impl DepthSnapshot {
    /// Convert this Binance wire snapshot to the exchange-neutral order-book
    /// model, preserving the REST coverage limit.
    ///
    /// # Errors
    ///
    /// Returns [`BookError`] if a level violates the generic domain rules.
    pub fn to_book_snapshot(&self, requested_levels: usize) -> Result<BookSnapshot, BookError> {
        Ok(BookSnapshot::new(
            self.last_update_id,
            to_book_levels(&self.bids)?,
            to_book_levels(&self.asks)?,
            BookCoverage::Limited {
                levels_per_side: requested_levels,
            },
        ))
    }
}

/// One `<symbol>@depth@100ms` diff-depth event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepthUpdate {
    /// Exchange event time, epoch milliseconds (`E`).
    pub event_time_ms: i64,
    /// Upper-case Binance symbol (`s`).
    pub symbol: String,
    /// First update id covered by this event (`U`).
    pub first_update_id: u64,
    /// Final update id covered by this event (`u`).
    pub final_update_id: u64,
    /// Absolute bid quantities changed by this event.
    pub bids: Vec<DepthLevel>,
    /// Absolute ask quantities changed by this event.
    pub asks: Vec<DepthLevel>,
}

impl DepthUpdate {
    /// Convert this Binance wire event to an exchange-neutral absolute delta.
    ///
    /// # Errors
    ///
    /// Returns [`BookError`] if a level violates the generic domain rules.
    pub fn to_book_delta(&self) -> Result<BookDelta, BookError> {
        Ok(BookDelta::new(
            self.first_update_id,
            self.final_update_id,
            to_book_levels(&self.bids)?,
            to_book_levels(&self.asks)?,
        ))
    }
}

/// A malformed Binance depth payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthWireError {
    /// The JSON shape did not match the Binance contract.
    Json(String),
    /// A price was not a decimal.
    InvalidPrice {
        /// `"bid"` or `"ask"`.
        side: &'static str,
        /// Position in the side's level array.
        index: usize,
        /// Raw value received.
        value: String,
    },
    /// A quantity was not a decimal.
    InvalidQuantity {
        /// `"bid"` or `"ask"`.
        side: &'static str,
        /// Position in the side's level array.
        index: usize,
        /// Raw value received.
        value: String,
    },
    /// Prices must be strictly positive.
    NonPositivePrice {
        /// `"bid"` or `"ask"`.
        side: &'static str,
        /// Position in the side's level array.
        index: usize,
        /// Parsed invalid value.
        value: Decimal,
    },
    /// Quantities are absolute and may be zero, but never negative.
    NegativeQuantity {
        /// `"bid"` or `"ask"`.
        side: &'static str,
        /// Position in the side's level array.
        index: usize,
        /// Parsed invalid value.
        value: Decimal,
    },
    /// `U` must not be greater than `u`.
    InvalidUpdateRange {
        /// First update id (`U`).
        first_update_id: u64,
        /// Final update id (`u`).
        final_update_id: u64,
    },
}

impl std::fmt::Display for DepthWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(error) => write!(f, "invalid depth JSON: {error}"),
            Self::InvalidPrice { side, index, value } => {
                write!(f, "invalid {side} price at index {index}: {value:?}")
            }
            Self::InvalidQuantity { side, index, value } => {
                write!(f, "invalid {side} quantity at index {index}: {value:?}")
            }
            Self::NonPositivePrice { side, index, value } => {
                write!(f, "non-positive {side} price at index {index}: {value}")
            }
            Self::NegativeQuantity { side, index, value } => {
                write!(f, "negative {side} quantity at index {index}: {value}")
            }
            Self::InvalidUpdateRange {
                first_update_id,
                final_update_id,
            } => write!(
                f,
                "invalid depth update range: first {first_update_id} is after final {final_update_id}"
            ),
        }
    }
}

impl std::error::Error for DepthWireError {}

#[derive(Debug, Deserialize)]
struct RawSnapshot {
    #[serde(rename = "lastUpdateId")]
    last_update_id: u64,
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
struct RawUpdate {
    #[serde(rename = "E")]
    event_time_ms: i64,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "U")]
    first_update_id: u64,
    #[serde(rename = "u")]
    final_update_id: u64,
    #[serde(rename = "b")]
    bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    asks: Vec<[String; 2]>,
}

fn parse_levels(
    raw: Vec<[String; 2]>,
    side: &'static str,
) -> Result<Vec<DepthLevel>, DepthWireError> {
    raw.into_iter()
        .enumerate()
        .map(|(index, [price_raw, quantity_raw])| {
            let price =
                Decimal::from_str(&price_raw).map_err(|_| DepthWireError::InvalidPrice {
                    side,
                    index,
                    value: price_raw,
                })?;
            let quantity =
                Decimal::from_str(&quantity_raw).map_err(|_| DepthWireError::InvalidQuantity {
                    side,
                    index,
                    value: quantity_raw,
                })?;
            if price <= Decimal::ZERO {
                return Err(DepthWireError::NonPositivePrice {
                    side,
                    index,
                    value: price,
                });
            }
            if quantity < Decimal::ZERO {
                return Err(DepthWireError::NegativeQuantity {
                    side,
                    index,
                    value: quantity,
                });
            }
            Ok(DepthLevel { price, quantity })
        })
        .collect()
}

fn to_book_levels(levels: &[DepthLevel]) -> Result<Vec<BookLevel>, BookError> {
    levels
        .iter()
        .map(|level| BookLevel::new(level.price, level.quantity))
        .collect()
}

/// Parse a REST `/api/v3/depth` response.
///
/// # Errors
///
/// Returns [`DepthWireError`] for an invalid JSON shape, decimal, price or
/// quantity.
pub fn parse_snapshot(json: &str) -> Result<DepthSnapshot, DepthWireError> {
    let raw: RawSnapshot =
        serde_json::from_str(json).map_err(|error| DepthWireError::Json(error.to_string()))?;
    Ok(DepthSnapshot {
        last_update_id: raw.last_update_id,
        bids: parse_levels(raw.bids, "bid")?,
        asks: parse_levels(raw.asks, "ask")?,
    })
}

/// Parse one WebSocket diff-depth message.
///
/// # Errors
///
/// Returns [`DepthWireError`] for an invalid JSON shape, update-id range,
/// decimal, price or quantity.
pub fn parse_update(json: &str) -> Result<DepthUpdate, DepthWireError> {
    let raw: RawUpdate =
        serde_json::from_str(json).map_err(|error| DepthWireError::Json(error.to_string()))?;
    if raw.first_update_id > raw.final_update_id {
        return Err(DepthWireError::InvalidUpdateRange {
            first_update_id: raw.first_update_id,
            final_update_id: raw.final_update_id,
        });
    }
    Ok(DepthUpdate {
        event_time_ms: raw.event_time_ms,
        symbol: raw.symbol,
        first_update_id: raw.first_update_id,
        final_update_id: raw.final_update_id,
        bids: parse_levels(raw.bids, "bid")?,
        asks: parse_levels(raw.asks, "ask")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_zero_is_valid_and_exact() {
        let update =
            parse_update(r#"{"E":1,"s":"BTCUSDT","U":10,"u":10,"b":[["1.2300","0.0000"]],"a":[]}"#)
                .unwrap();
        assert_eq!(update.bids[0].price, Decimal::new(12300, 4));
        assert_eq!(update.bids[0].quantity, Decimal::ZERO);
    }

    #[test]
    fn rejects_negative_quantity() {
        let error =
            parse_update(r#"{"E":1,"s":"BTCUSDT","U":10,"u":10,"b":[["1","-0.1"]],"a":[]}"#)
                .unwrap_err();
        assert!(matches!(error, DepthWireError::NegativeQuantity { .. }));
    }

    #[test]
    fn rejects_reversed_update_range() {
        let error =
            parse_update(r#"{"E":1,"s":"BTCUSDT","U":11,"u":10,"b":[],"a":[]}"#).unwrap_err();
        assert_eq!(
            error,
            DepthWireError::InvalidUpdateRange {
                first_update_id: 11,
                final_update_id: 10
            }
        );
    }
}
