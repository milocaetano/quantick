//! Binance aggTrade wire format and its mapping to the engine's [`Trade`].
//!
//! This module is pure: no network, no async, no wall-clock. It exists so the
//! translation from Binance's JSON to the engine's input is deterministic and
//! unit-testable against recorded messages, and so no non-determinism can leak
//! into the engine through the feed.
//!
//! # The two wire shapes
//!
//! Binance delivers aggregate trades in two shapes that share the same fields:
//!
//! - **REST** `GET /api/v3/aggTrades` returns a JSON **array** of aggTrade
//!   objects (used for startup backfill).
//! - **WebSocket** `<symbol>@aggTrade` pushes one aggTrade **object** per message
//!   (used for the live stream). Single-stream messages also carry event
//!   envelope fields (`e`, `E`, `s`) which we don't need; serde ignores them.
//!
//! Both decode into [`AggTrade`]; [`parse_rest`] handles the array and
//! [`parse_ws_message`] the single object.
//!
//! # Field mapping
//!
//! | Binance | [`Trade`]        | note |
//! |---------|------------------|------|
//! | `a`     | `agg_id`         | monotonic per symbol |
//! | `p`     | `price`          | decimal string → [`Decimal`] |
//! | `q`     | `quantity`       | decimal string → [`Decimal`] |
//! | `T`     | `timestamp_ms`   | trade time (not event time `E`) |
//! | `m`     | `side`           | `is_buyer_maker` → aggressor (inverts) |

use std::str::FromStr as _;

use rust_decimal::Decimal;
use serde::Deserialize;

use quantick_engine::{Side, Trade};

/// One Binance aggregate trade, decoded from either wire shape.
///
/// Price and quantity are kept as their raw decimal **strings** here and parsed
/// to [`Decimal`] in [`AggTrade::to_trade`], so a malformed number surfaces as a
/// first-class [`MapError`] rather than an opaque deserialisation failure.
/// Envelope fields present only on the WebSocket shape (`e`, `E`, `s`) and the
/// REST-only `f`/`l`/`M` fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AggTrade {
    /// Aggregate trade id (`a`), monotonic per symbol.
    #[serde(rename = "a")]
    pub agg_id: u64,
    /// Price (`p`), as the raw decimal string Binance sends.
    #[serde(rename = "p")]
    pub price: String,
    /// Quantity (`q`), as the raw decimal string Binance sends.
    #[serde(rename = "q")]
    pub quantity: String,
    /// Trade time (`T`), epoch milliseconds. Note: not the event time `E`.
    #[serde(rename = "T")]
    pub trade_time_ms: i64,
    /// Whether the buyer was the maker (`m`). Inverts to the aggressor side.
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

impl AggTrade {
    /// Map this wire record to an engine [`Trade`].
    ///
    /// # Errors
    ///
    /// Returns [`MapError`] if `price` or `quantity` is not a valid decimal.
    pub fn to_trade(&self) -> Result<Trade, MapError> {
        let price = Decimal::from_str(&self.price).map_err(|_| MapError::Price {
            agg_id: self.agg_id,
            value: self.price.clone(),
        })?;
        let quantity = Decimal::from_str(&self.quantity).map_err(|_| MapError::Quantity {
            agg_id: self.agg_id,
            value: self.quantity.clone(),
        })?;
        Ok(Trade {
            agg_id: self.agg_id,
            timestamp_ms: self.trade_time_ms,
            price,
            quantity,
            side: Side::from_buyer_is_maker(self.is_buyer_maker),
        })
    }
}

/// An aggTrade field that could not be mapped to the engine's `Trade`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// `price` was not a valid decimal.
    Price {
        /// The offending trade's aggregate id.
        agg_id: u64,
        /// The raw string that failed to parse.
        value: String,
    },
    /// `quantity` was not a valid decimal.
    Quantity {
        /// The offending trade's aggregate id.
        agg_id: u64,
        /// The raw string that failed to parse.
        value: String,
    },
}

impl std::fmt::Display for MapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MapError::Price { agg_id, value } => {
                write!(f, "aggTrade {agg_id}: invalid price {value:?}")
            }
            MapError::Quantity { agg_id, value } => {
                write!(f, "aggTrade {agg_id}: invalid quantity {value:?}")
            }
        }
    }
}

impl std::error::Error for MapError {}

/// Parse a REST `GET /api/v3/aggTrades` response (a JSON array).
///
/// # Errors
///
/// Returns the underlying [`serde_json::Error`] if the body is not a valid
/// array of aggTrade objects.
pub fn parse_rest(json: &str) -> Result<Vec<AggTrade>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Parse a single WebSocket `<symbol>@aggTrade` message (a JSON object).
///
/// # Errors
///
/// Returns the underlying [`serde_json::Error`] if the message is not a valid
/// aggTrade object.
pub fn parse_ws_message(json: &str) -> Result<AggTrade, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn rest_array_parses_and_maps() {
        let json = r#"[
            {"a":26129,"p":"36000.10","q":"0.005","f":27781,"l":27781,"T":1700000000000,"m":true,"M":true},
            {"a":26130,"p":"36000.20","q":"0.010","f":27782,"l":27783,"T":1700000000200,"m":false,"M":true}
        ]"#;
        let raw = parse_rest(json).unwrap();
        assert_eq!(raw.len(), 2);

        let t0 = raw[0].to_trade().unwrap();
        assert_eq!(t0.agg_id, 26129);
        assert_eq!(t0.price, dec("36000.10"));
        assert_eq!(t0.quantity, dec("0.005"));
        assert_eq!(t0.timestamp_ms, 1_700_000_000_000);
        // buyer is maker => aggressor is the seller.
        assert_eq!(t0.side, Side::Sell);

        let t1 = raw[1].to_trade().unwrap();
        assert_eq!(t1.side, Side::Buy);
    }

    #[test]
    fn ws_message_ignores_envelope_fields() {
        let json = r#"{"e":"aggTrade","E":1700000000450,"s":"BTCUSDT","a":26131,
            "p":"35999.80","q":"1.200","f":27784,"l":27790,"T":1700000000450,"m":false,"M":true}"#;
        let raw = parse_ws_message(json).unwrap();
        assert_eq!(raw.agg_id, 26131);
        let t = raw.to_trade().unwrap();
        assert_eq!(t.price, dec("35999.80"));
        assert_eq!(t.quantity, dec("1.200"));
        assert_eq!(t.side, Side::Buy);
    }

    #[test]
    fn invalid_price_is_a_map_error_not_a_panic() {
        let raw = AggTrade {
            agg_id: 7,
            price: "not-a-number".to_string(),
            quantity: "1.0".to_string(),
            trade_time_ms: 0,
            is_buyer_maker: false,
        };
        let err = raw.to_trade().unwrap_err();
        assert_eq!(
            err,
            MapError::Price {
                agg_id: 7,
                value: "not-a-number".to_string()
            }
        );
    }
}
