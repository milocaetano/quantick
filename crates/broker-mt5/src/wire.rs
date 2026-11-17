//! The execution channel's lines, both ways.
//!
//! The contract is the "Execution channel" section of
//! `bridge/mt5/PROTOCOL.md`; `bridge/mt5/quantick_bridge_exec.py` is the
//! other end. Prices and volumes travel as decimal strings in both
//! directions, so nothing on this side ever holds a float.

use quantick_engine::Side;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// The protocol version this crate speaks. A bridge announcing another is
/// never written to. 2: order requests carry `expires_ms`, which a bridge
/// that predates it would ignore.
pub const EXEC_SCHEMA_VERSION: u32 = 2;

/// One line from the bridge.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Inbound {
    /// First line of every connection.
    ExecHello {
        schema: u32,
        /// Unique per bridge process: the same value after a reconnect means
        /// the bridge still remembers every reply it gave.
        instance: String,
    },
    /// The answer to one request.
    Reply(Reply),
    /// The account and everything resting on the watched symbol.
    State(State),
    /// A clean goodbye; the link drops next.
    Bye {
        #[serde(default)]
        reason: String,
    },
}

/// The bridge's answer to one request, the terminal's words verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Reply {
    /// The request id this answers; `None` for a request that had none.
    pub id: Option<String>,
    /// The terminal's return code; `None` when the bridge refused before
    /// asking the terminal, and `error` says why.
    pub retcode: Option<i64>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// The order ticket the terminal assigned, 0 when none.
    #[serde(default)]
    pub order: u64,
    #[serde(default)]
    pub deal: u64,
    /// The answer to a `replay_only` resend the bridge never answered
    /// before: it did not execute it, and will not.
    #[serde(default)]
    pub unknown: bool,
}

/// One pushed snapshot of the watched symbol.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct State {
    pub symbol: String,
    pub account: Option<AccountLine>,
    #[serde(default)]
    pub positions: Vec<PositionLine>,
    #[serde(default)]
    pub orders: Vec<OrderLine>,
    /// Deals not yet pushed on this connection, oldest first.
    #[serde(default)]
    pub deals: Vec<DealLine>,
    /// Orders that left the working list, with how they ended.
    #[serde(default)]
    pub closed_orders: Vec<ClosedOrderLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AccountLine {
    pub login: u64,
    pub server: String,
    pub company: String,
    pub trade_mode: String,
    pub margin_mode: String,
    pub balance: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PositionLine {
    pub ticket: u64,
    pub side: String,
    pub volume: String,
    pub price_open: String,
    pub sl: Option<String>,
    pub tp: Option<String>,
    pub magic: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OrderLine {
    pub ticket: u64,
    pub side: String,
    pub kind: String,
    pub volume: String,
    pub price: String,
    pub sl: Option<String>,
    pub tp: Option<String>,
    pub magic: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DealLine {
    pub ticket: u64,
    pub order: u64,
    pub position_id: u64,
    pub side: String,
    pub entry: String,
    pub reason: String,
    pub volume: String,
    pub price: String,
    pub magic: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ClosedOrderLine {
    pub ticket: u64,
    pub state: String,
}

/// Decode one line, or `None` for anything this side does not understand.
#[must_use]
pub fn decode(line: &str) -> Option<Inbound> {
    serde_json::from_str(line).ok()
}

/// A decimal string off the wire.
#[must_use]
pub fn decimal(text: &str) -> Option<Decimal> {
    text.trim().parse().ok()
}

/// A wire side.
#[must_use]
pub fn side(text: &str) -> Option<Side> {
    match text {
        "buy" => Some(Side::Buy),
        "sell" => Some(Side::Sell),
        _ => None,
    }
}

fn side_text(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn number(value: Decimal) -> Value {
    Value::String(value.normalize().to_string())
}

fn optional(value: Option<Decimal>) -> Value {
    value.map_or(Value::Null, number)
}

/// What every request carries for the terminal to stamp it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub magic: u64,
    pub deviation_points: u32,
    /// Unix milliseconds from which the bridge must not execute the request:
    /// by then quantick may have declared the link lost and stopped waiting
    /// for its answer. Never on a `watch`, which trades nothing.
    pub expires_ms: Option<i64>,
}

/// One request to the bridge, before its id is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Watch {
        symbol: String,
    },
    Submit {
        symbol: String,
        side: Side,
        kind: &'static str,
        volume: Decimal,
        price: Option<Decimal>,
        sl: Option<Decimal>,
        tp: Option<Decimal>,
    },
    Cancel {
        order: u64,
    },
    Modify {
        order: u64,
        price: Decimal,
        sl: Option<Decimal>,
        tp: Option<Decimal>,
    },
    PositionStops {
        symbol: String,
        position: u64,
        sl: Option<Decimal>,
        tp: Option<Decimal>,
    },
    Close {
        position: u64,
        volume: Decimal,
    },
}

/// `line` asked again after a dropped link, for its remembered answer only:
/// a bridge that never received it answers `unknown` and executes nothing.
#[must_use]
pub fn replay_only(line: &str) -> String {
    match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(mut fields)) => {
            fields.insert("replay_only".into(), Value::Bool(true));
            Value::Object(fields).to_string()
        }
        // Never a line this side wrote; sent as it is, it could execute.
        _ => String::new(),
    }
}

impl Request {
    /// The line for this request, carrying `id`.
    #[must_use]
    pub fn encode(&self, id: &str, stamp: Stamp) -> String {
        let mut line = Map::new();
        line.insert("id".into(), json!(id));
        let mut put = |key: &str, value: Value| {
            line.insert(key.into(), value);
        };
        match self {
            Self::Watch { symbol } => {
                put("type", json!("watch"));
                put("symbol", json!(symbol));
            }
            Self::Submit {
                symbol,
                side,
                kind,
                volume,
                price,
                sl,
                tp,
            } => {
                put("type", json!("submit"));
                put("symbol", json!(symbol));
                put("side", json!(side_text(*side)));
                put("kind", json!(kind));
                put("volume", number(*volume));
                if let Some(price) = price {
                    put("price", number(*price));
                }
                put("sl", optional(*sl));
                put("tp", optional(*tp));
                put("magic", json!(stamp.magic));
                put("deviation", json!(stamp.deviation_points));
                put("comment", json!(id));
            }
            Self::Cancel { order } => {
                put("type", json!("cancel"));
                put("order", json!(order));
            }
            Self::Modify {
                order,
                price,
                sl,
                tp,
            } => {
                put("type", json!("modify"));
                put("order", json!(order));
                put("price", number(*price));
                put("sl", optional(*sl));
                put("tp", optional(*tp));
            }
            Self::PositionStops {
                symbol,
                position,
                sl,
                tp,
            } => {
                put("type", json!("sltp"));
                put("symbol", json!(symbol));
                put("position", json!(position));
                put("sl", optional(*sl));
                put("tp", optional(*tp));
                put("magic", json!(stamp.magic));
            }
            Self::Close { position, volume } => {
                put("type", json!("close"));
                put("position", json!(position));
                put("volume", number(*volume));
                put("magic", json!(stamp.magic));
                put("deviation", json!(stamp.deviation_points));
                put("comment", json!(id));
            }
        }
        if let Some(expires_ms) = stamp.expires_ms
            && !matches!(self, Self::Watch { .. })
        {
            line.insert("expires_ms".into(), json!(expires_ms));
        }
        Value::Object(line).to_string()
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;
