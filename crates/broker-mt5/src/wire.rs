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
/// never written to.
pub const EXEC_SCHEMA_VERSION: u32 = 1;

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
        Value::Object(line).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_submit_line_carries_decimals_as_strings_and_the_stamp() {
        let line = Request::Submit {
            symbol: "WINZ26".into(),
            side: Side::Buy,
            kind: "limit",
            volume: Decimal::from(2),
            price: Some(Decimal::new(1_249_000, 1)),
            sl: None,
            tp: Some(Decimal::from(125_300)),
        }
        .encode(
            "ab12-7",
            Stamp {
                magic: 7_710_001,
                deviation_points: 10,
            },
        );
        let value: Value = serde_json::from_str(&line).expect("json");
        assert_eq!(value["type"], "submit");
        assert_eq!(value["id"], "ab12-7");
        assert_eq!(value["price"], "124900");
        assert_eq!(value["volume"], "2");
        assert_eq!(value["sl"], Value::Null);
        assert_eq!(value["tp"], "125300");
        assert_eq!(value["magic"], 7_710_001);
        assert_eq!(value["deviation"], 10);
        assert_eq!(value["comment"], "ab12-7");
    }

    /// The example lines in PROTOCOL.md's execution-channel section: every
    /// bridge line decodes, and every request line is exactly what this side
    /// encodes. A doc that drifts from the code fails here.
    #[test]
    fn protocol_md_examples_match_the_codec() {
        let doc = include_str!("../../../bridge/mt5/PROTOCOL.md");
        let section = doc
            .split("## Execution channel")
            .nth(1)
            .expect("the execution channel section");
        let lines: Vec<&str> = section
            .lines()
            .filter(|line| line.starts_with("{\"type\":"))
            .collect();
        let stamp = Stamp {
            magic: 7_710_001,
            deviation_points: 10,
        };
        let symbol = || "WINZ26".to_owned();
        let expected = [
            Request::Watch { symbol: symbol() }.encode("a1b2-1", stamp),
            Request::Submit {
                symbol: symbol(),
                side: Side::Buy,
                kind: "market",
                volume: Decimal::from(2),
                price: None,
                sl: Some(Decimal::from(124_800)),
                tp: None,
            }
            .encode("a1b2-2", stamp),
            Request::Submit {
                symbol: symbol(),
                side: Side::Sell,
                kind: "limit",
                volume: Decimal::ONE,
                price: Some(Decimal::from(125_300)),
                sl: None,
                tp: None,
            }
            .encode("a1b2-3", stamp),
            Request::Cancel { order: 601 }.encode("a1b2-4", stamp),
            Request::Modify {
                order: 601,
                price: Decimal::from(125_250),
                sl: None,
                tp: Some(Decimal::from(124_900)),
            }
            .encode("a1b2-5", stamp),
            Request::PositionStops {
                symbol: symbol(),
                position: 77,
                sl: Some(Decimal::from(124_800)),
                tp: Some(Decimal::from(125_400)),
            }
            .encode("a1b2-6", stamp),
            Request::Close {
                position: 77,
                volume: Decimal::ONE,
            }
            .encode("a1b2-7", stamp),
        ];
        let mut requests = Vec::new();
        for line in lines {
            let value: Value = serde_json::from_str(line).expect("doc line is json");
            match value["type"].as_str() {
                Some("exec_hello" | "reply" | "state") => {
                    assert!(decode(line).is_some(), "doc line decodes: {line}");
                }
                _ => requests.push(value),
            }
        }
        let encoded: Vec<Value> = expected
            .iter()
            .map(|line| serde_json::from_str(line).expect("json"))
            .collect();
        assert_eq!(requests, encoded);
    }

    #[test]
    fn the_protocol_examples_decode() {
        let hello = r#"{"type":"exec_hello","schema":1,"bridge":"quantick-mt5-exec-py","bridge_version":"0.1.0","instance":"9f1c"}"#;
        assert_eq!(
            decode(hello),
            Some(Inbound::ExecHello {
                schema: 1,
                instance: "9f1c".into()
            })
        );
        let reply = r#"{"type":"reply","id":"ab12-7","retcode":10016,"comment":"Invalid stops","order":0,"deal":0,"volume":"0","price":"0","cached":false}"#;
        let Some(Inbound::Reply(reply)) = decode(reply) else {
            panic!("a reply");
        };
        assert_eq!(reply.retcode, Some(10016));
        assert_eq!(reply.comment.as_deref(), Some("Invalid stops"));
        let refused = r#"{"type":"reply","id":"ab12-8","retcode":null,"error":"position 77 is not open: nothing to close","cached":false}"#;
        let Some(Inbound::Reply(refused)) = decode(refused) else {
            panic!("a refusal");
        };
        assert_eq!(refused.retcode, None);
        assert!(refused.error.is_some());
        let state = r#"{"type":"state","symbol":"WINZ26","account":{"login":1234567,"server":"Broker-DEMO","company":"Broker Ltd","trade_mode":"demo","margin_mode":"exchange","balance":"100000.00","currency":"BRL"},"positions":[{"ticket":77,"side":"buy","volume":"2","price_open":"125000","sl":"124800","tp":null,"magic":7710001,"time_msc":1000}],"orders":[],"deals":[],"closed_orders":[]}"#;
        let Some(Inbound::State(state)) = decode(state) else {
            panic!("a state");
        };
        assert_eq!(state.positions[0].sl.as_deref(), Some("124800"));
        assert_eq!(state.account.map(|a| a.login), Some(1_234_567));
        assert_eq!(decode("{\"type\":\"shout\"}"), None);
        assert_eq!(decode("not json"), None);
    }
}
