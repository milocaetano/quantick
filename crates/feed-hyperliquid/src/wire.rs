//! Pure Hyperliquid trade decoding and mapping.
//!
//! The API reports the aggressor side directly: `B` is a taker buy and `A` is
//! a taker sell. Prices and sizes remain strings until this layer parses them
//! into exact decimals. No wall clock or network is consulted here.

use std::collections::{BTreeSet, VecDeque};
use std::str::FromStr as _;

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;
use serde::Deserialize;

/// Number of venue identities retained to suppress REST/WebSocket and
/// reconnect-snapshot overlap. At busy BTC rates this covers far more than the
/// short recovery window sent by the venue while keeping memory bounded.
pub const DEFAULT_SEEN_TRADE_CAPACITY: usize = 65_536;

/// One public Hyperliquid trade as delivered by REST or WebSocket.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawTrade {
    /// Perpetual coin name, for example `BTC`.
    pub coin: String,
    /// Aggressor side: `B` (buy) or `A` (sell).
    pub side: String,
    /// Execution price.
    #[serde(rename = "px")]
    pub price: String,
    /// Executed base-asset quantity.
    #[serde(rename = "sz")]
    pub size: String,
    /// Block time in epoch milliseconds.
    #[serde(rename = "time")]
    pub timestamp_ms: i64,
    /// 50-bit hash of buyer and seller order ids.
    pub tid: u64,
}

impl RawTrade {
    fn key(&self) -> TradeKey {
        TradeKey {
            timestamp_ms: self.timestamp_ms,
            tid: self.tid,
        }
    }
}

/// Result of mapping one batch.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MappedBatch {
    /// Valid, new trades in chronological order.
    pub trades: Vec<Trade>,
    /// Already-seen venue identities suppressed.
    pub duplicates: usize,
    /// Previously unseen trades older than the published timeline suppressed.
    pub stale: usize,
    /// Malformed records rejected individually.
    pub errors: Vec<TradeMapError>,
}

/// A trade record that cannot honestly become an engine trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeMapError {
    /// The payload belongs to a different subscription.
    Symbol {
        /// Configured coin.
        expected: String,
        /// Coin carried by the payload.
        got: String,
        /// Venue trade id.
        tid: u64,
    },
    /// Side was neither `B` nor `A`.
    Side {
        /// Venue trade id.
        tid: u64,
        /// Invalid token.
        value: String,
    },
    /// Price was not a positive decimal.
    Price {
        /// Venue trade id.
        tid: u64,
        /// Invalid token.
        value: String,
    },
    /// Size was not a positive decimal.
    Size {
        /// Venue trade id.
        tid: u64,
        /// Invalid token.
        value: String,
    },
    /// Epoch time was negative.
    Timestamp {
        /// Venue trade id.
        tid: u64,
        /// Invalid time.
        value: i64,
    },
}

impl std::fmt::Display for TradeMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Symbol { expected, got, tid } => {
                write!(f, "trade {tid}: expected coin {expected}, got {got}")
            }
            Self::Side { tid, value } => write!(f, "trade {tid}: invalid side {value:?}"),
            Self::Price { tid, value } => write!(f, "trade {tid}: invalid price {value:?}"),
            Self::Size { tid, value } => write!(f, "trade {tid}: invalid size {value:?}"),
            Self::Timestamp { tid, value } => {
                write!(f, "trade {tid}: invalid timestamp {value}")
            }
        }
    }
}

impl std::error::Error for TradeMapError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TradeKey {
    timestamp_ms: i64,
    tid: u64,
}

/// Stateful venue-id deduplication and monotonic engine-id assignment.
#[derive(Debug, Clone)]
pub struct TradeMapper {
    symbol: String,
    next_id: u64,
    last_timestamp_ms: Option<i64>,
    seen_capacity: usize,
    seen: BTreeSet<TradeKey>,
    seen_order: VecDeque<TradeKey>,
}

impl TradeMapper {
    /// Mapper for one perpetual coin with the bounded default dedupe window.
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self::with_seen_capacity(symbol, DEFAULT_SEEN_TRADE_CAPACITY)
    }

    /// Mapper with an explicit dedupe capacity, primarily for focused tests.
    #[must_use]
    pub fn with_seen_capacity(symbol: impl Into<String>, seen_capacity: usize) -> Self {
        Self {
            symbol: symbol.into().to_uppercase(),
            next_id: 1,
            last_timestamp_ms: None,
            seen_capacity: seen_capacity.max(1),
            seen: BTreeSet::new(),
            seen_order: VecDeque::new(),
        }
    }

    /// Map a REST page or WebSocket batch in deterministic chronological order.
    ///
    /// Bad rows are returned in [`MappedBatch::errors`] while valid neighboring
    /// rows remain usable. The caller must log the ledger; silently treating a
    /// partially malformed feed as complete would violate data honesty.
    pub fn map_batch(&mut self, mut raw: Vec<RawTrade>) -> MappedBatch {
        raw.sort_by_key(|trade| trade.key());
        let mut mapped = MappedBatch {
            trades: Vec::with_capacity(raw.len()),
            ..MappedBatch::default()
        };

        for raw_trade in raw {
            let (price, quantity, side) = match self.validate(&raw_trade) {
                Ok(fields) => fields,
                Err(error) => {
                    mapped.errors.push(error);
                    continue;
                }
            };
            let key = raw_trade.key();
            if self.seen.contains(&key) {
                mapped.duplicates += 1;
                continue;
            }
            self.remember(key);

            if self
                .last_timestamp_ms
                .is_some_and(|last| raw_trade.timestamp_ms < last)
            {
                mapped.stale += 1;
                continue;
            }

            let agg_id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            self.last_timestamp_ms = Some(raw_trade.timestamp_ms);
            mapped.trades.push(Trade {
                agg_id,
                timestamp_ms: raw_trade.timestamp_ms,
                price,
                quantity,
                side,
            });
        }
        mapped
    }

    /// Last timestamp published into the engine timeline.
    #[must_use]
    pub fn last_timestamp_ms(&self) -> Option<i64> {
        self.last_timestamp_ms
    }

    /// Number of engine trades assigned since this mapper was created.
    #[must_use]
    pub fn published_count(&self) -> u64 {
        self.next_id.saturating_sub(1)
    }

    fn validate(&self, raw: &RawTrade) -> Result<(Decimal, Decimal, Side), TradeMapError> {
        if !raw.coin.eq_ignore_ascii_case(&self.symbol) {
            return Err(TradeMapError::Symbol {
                expected: self.symbol.clone(),
                got: raw.coin.clone(),
                tid: raw.tid,
            });
        }
        if raw.timestamp_ms < 0 {
            return Err(TradeMapError::Timestamp {
                tid: raw.tid,
                value: raw.timestamp_ms,
            });
        }
        let side = match raw.side.as_str() {
            "B" => Side::Buy,
            "A" => Side::Sell,
            value => {
                return Err(TradeMapError::Side {
                    tid: raw.tid,
                    value: value.to_owned(),
                });
            }
        };
        let price = Decimal::from_str(&raw.price).map_err(|_| TradeMapError::Price {
            tid: raw.tid,
            value: raw.price.clone(),
        })?;
        if price <= Decimal::ZERO {
            return Err(TradeMapError::Price {
                tid: raw.tid,
                value: raw.price.clone(),
            });
        }
        let quantity = Decimal::from_str(&raw.size).map_err(|_| TradeMapError::Size {
            tid: raw.tid,
            value: raw.size.clone(),
        })?;
        if quantity <= Decimal::ZERO {
            return Err(TradeMapError::Size {
                tid: raw.tid,
                value: raw.size.clone(),
            });
        }
        Ok((price, quantity, side))
    }

    fn remember(&mut self, key: TradeKey) {
        self.seen.insert(key);
        self.seen_order.push_back(key);
        while self.seen_order.len() > self.seen_capacity {
            if let Some(expired) = self.seen_order.pop_front() {
                self.seen.remove(&expired);
            }
        }
    }
}

/// A decoded frame on the trade subscription socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeMessage {
    /// Subscription acknowledgement.
    Subscribed,
    /// Application-level heartbeat response.
    Pong,
    /// One chronological unit of venue trade data (the mapper sorts it).
    Trades(Vec<RawTrade>),
    /// A valid envelope for a channel this socket did not request.
    Other { channel: String },
}

#[derive(Deserialize)]
struct Envelope {
    channel: String,
    #[serde(default)]
    data: serde_json::Value,
}

/// Decode a WebSocket envelope for the trades connection.
///
/// # Errors
///
/// Returns a serde error when the envelope or `trades` payload is malformed.
pub fn parse_trade_message(json: &str) -> Result<TradeMessage, serde_json::Error> {
    let envelope: Envelope = serde_json::from_str(json)?;
    match envelope.channel.as_str() {
        "subscriptionResponse" => Ok(TradeMessage::Subscribed),
        "pong" => Ok(TradeMessage::Pong),
        "trades" => serde_json::from_value(envelope.data).map(TradeMessage::Trades),
        _ => Ok(TradeMessage::Other {
            channel: envelope.channel,
        }),
    }
}

/// Decode the `recentTrades` info response.
///
/// # Errors
///
/// Returns the underlying serde error for a malformed response.
pub fn parse_rest_trades(json: &str) -> Result<Vec<RawTrade>, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(tid: u64, timestamp_ms: i64, side: &str) -> RawTrade {
        RawTrade {
            coin: "BTC".to_owned(),
            side: side.to_owned(),
            price: "63000.5".to_owned(),
            size: "0.25".to_owned(),
            timestamp_ms,
            tid,
        }
    }

    #[test]
    fn batch_is_sorted_and_sides_are_the_reported_aggressor() {
        let mut mapper = TradeMapper::new("btc");
        let batch = mapper.map_batch(vec![raw(3, 1_002, "A"), raw(1, 1_000, "B")]);
        assert!(batch.errors.is_empty());
        assert_eq!(batch.trades.len(), 2);
        assert_eq!(batch.trades[0].agg_id, 1);
        assert_eq!(batch.trades[0].timestamp_ms, 1_000);
        assert_eq!(batch.trades[0].side, Side::Buy);
        assert_eq!(batch.trades[1].side, Side::Sell);
    }

    #[test]
    fn overlap_is_deduplicated_by_time_and_tid() {
        let mut mapper = TradeMapper::new("BTC");
        assert_eq!(mapper.map_batch(vec![raw(7, 1_000, "B")]).trades.len(), 1);
        let overlap = mapper.map_batch(vec![raw(7, 1_000, "B"), raw(8, 1_001, "A")]);
        assert_eq!(overlap.duplicates, 1);
        assert_eq!(overlap.trades.len(), 1);
        assert_eq!(overlap.trades[0].agg_id, 2);
    }

    #[test]
    fn unseen_backwards_trade_is_labelled_stale() {
        let mut mapper = TradeMapper::new("BTC");
        mapper.map_batch(vec![raw(1, 2_000, "B")]);
        let late = mapper.map_batch(vec![raw(2, 1_999, "A")]);
        assert!(late.trades.is_empty());
        assert_eq!(late.stale, 1);
        assert_eq!(mapper.last_timestamp_ms(), Some(2_000));
    }

    #[test]
    fn dedupe_storage_is_bounded_without_reordering_the_timeline() {
        let mut mapper = TradeMapper::with_seen_capacity("BTC", 2);
        mapper.map_batch(vec![
            raw(1, 1_000, "B"),
            raw(2, 1_001, "B"),
            raw(3, 1_002, "B"),
        ]);
        let expired_but_old = mapper.map_batch(vec![raw(1, 1_000, "B")]);
        assert_eq!(expired_but_old.stale, 1);
        assert!(expired_but_old.trades.is_empty());
    }

    #[test]
    fn malformed_rows_do_not_hide_valid_neighbors() {
        let mut mapper = TradeMapper::new("BTC");
        let mut bad = raw(2, 1_001, "?");
        bad.price = "not-a-price".to_owned();
        let batch = mapper.map_batch(vec![raw(1, 1_000, "B"), bad, raw(3, 1_002, "A")]);
        assert_eq!(batch.trades.len(), 2);
        assert_eq!(batch.errors.len(), 1);
        assert!(matches!(batch.errors[0], TradeMapError::Side { .. }));
    }
}
