//! Fixtures: bridge lines as the protocol writes them, and a venue wired
//! to a fake bridge.

#![allow(dead_code)]

use quantick_broker_mt5::fake::{FakeBridge, FakeBridgeHandle};
use quantick_broker_mt5::{BrokerConfig, Mt5Venue};
use quantick_trading::VenueEvent;
use rust_decimal::Decimal;
use serde_json::{Value, json};

pub const SYMBOL: &str = "WINZ26";
pub const MAGIC: u64 = 7_710_001;
pub const TAG: &str = "s1";
pub const LOGIN: u64 = 1_234_567;

pub fn dec(value: i64) -> Decimal {
    Decimal::from(value)
}

pub fn config() -> BrokerConfig {
    BrokerConfig {
        symbol: SYMBOL.into(),
        magic: MAGIC,
        deviation_points: 10,
        session_tag: TAG.into(),
    }
}

pub fn venue() -> (Mt5Venue, FakeBridgeHandle) {
    let (bridge, handle) = FakeBridge::new();
    (Mt5Venue::new(config(), Box::new(bridge)), handle)
}

/// A venue whose bridge said hello and pushed an empty demo state, handed
/// the time 0 just after.
pub fn connected() -> (Mt5Venue, FakeBridgeHandle) {
    let (mut venue, handle) = venue();
    handle.connect("bridge-1");
    venue.poll();
    handle.push(&Snapshot::new().line());
    venue.poll();
    venue.tick(0);
    handle.take_sent();
    if let Some(why) = venue.core().write_block() {
        panic!("not ready to trade: {why}");
    }
    (venue, handle)
}

pub fn account(login: u64, trade_mode: &str, margin_mode: &str) -> Value {
    json!({
        "login": login,
        "server": "Broker-DEMO",
        "company": "Broker Ltd",
        "trade_mode": trade_mode,
        "margin_mode": margin_mode,
        "balance": "100000.00",
        "currency": "BRL",
    })
}

pub fn position(ticket: u64, side: &str, volume: i64, price: i64, magic: u64) -> Value {
    json!({
        "ticket": ticket, "side": side, "volume": volume.to_string(),
        "price_open": price.to_string(), "sl": null, "tp": null,
        "magic": magic, "time_msc": 0,
    })
}

pub fn with_stops(mut line: Value, sl: Option<i64>, tp: Option<i64>) -> Value {
    line["sl"] = sl.map_or(Value::Null, |p| json!(p.to_string()));
    line["tp"] = tp.map_or(Value::Null, |p| json!(p.to_string()));
    line
}

pub fn order(ticket: u64, side: &str, kind: &str, volume: i64, price: i64, magic: u64) -> Value {
    json!({
        "ticket": ticket, "side": side, "kind": kind, "volume": volume.to_string(),
        "price": price.to_string(), "sl": null, "tp": null,
        "magic": magic, "time_setup_msc": 0,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn deal(
    ticket: u64,
    order: u64,
    position_id: u64,
    side: &str,
    entry: &str,
    reason: &str,
    volume: i64,
    price: i64,
) -> Value {
    json!({
        "ticket": ticket, "order": order, "position_id": position_id, "side": side,
        "entry": entry, "reason": reason, "volume": volume.to_string(),
        "price": price.to_string(), "magic": MAGIC, "time_msc": 0,
    })
}

pub fn reply(id: &str, retcode: i64, comment: &str, order: u64) -> Value {
    json!({
        "type": "reply", "id": id, "retcode": retcode, "comment": comment,
        "order": order, "deal": 0, "volume": "0", "price": "0", "cached": false,
    })
}

/// One `state` line, built up field by field.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub symbol: String,
    pub account: Value,
    pub positions: Vec<Value>,
    pub orders: Vec<Value>,
    pub deals: Vec<Value>,
    pub closed_orders: Vec<Value>,
}

impl Snapshot {
    pub fn new() -> Self {
        Self {
            symbol: SYMBOL.into(),
            account: account(LOGIN, "demo", "exchange"),
            positions: Vec::new(),
            orders: Vec::new(),
            deals: Vec::new(),
            closed_orders: Vec::new(),
        }
    }

    pub fn position(mut self, line: Value) -> Self {
        self.positions.push(line);
        self
    }

    pub fn order(mut self, line: Value) -> Self {
        self.orders.push(line);
        self
    }

    pub fn deal(mut self, line: Value) -> Self {
        self.deals.push(line);
        self
    }

    pub fn closed(mut self, ticket: u64, state: &str) -> Self {
        self.closed_orders
            .push(json!({"ticket": ticket, "state": state}));
        self
    }

    pub fn account(mut self, line: Value) -> Self {
        self.account = line;
        self
    }

    pub fn line(&self) -> Value {
        json!({
            "type": "state", "symbol": self.symbol, "account": self.account,
            "positions": self.positions, "orders": self.orders,
            "deals": self.deals, "closed_orders": self.closed_orders,
        })
    }
}

/// The ids of the requests in `lines`, in order.
pub fn ids(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .map(|line| line["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// The single request line of type `kind` among `lines`.
pub fn only(lines: &[Value], kind: &str) -> Value {
    let found: Vec<&Value> = lines.iter().filter(|line| line["type"] == kind).collect();
    assert_eq!(found.len(), 1, "one {kind} line in {lines:?}");
    found[0].clone()
}

pub fn notices(events: &[VenueEvent]) -> Vec<quantick_trading::VenueNotice> {
    events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Notice(notice) => Some(notice.clone()),
            _ => None,
        })
        .collect()
}

/// A broker that answers every request at once, as a netting exchange
/// account would: market orders fill at `fill_price`, pending orders rest,
/// cancels and changes succeed. Only what the parity tests need.
#[derive(Debug)]
pub struct AutoBroker {
    pub fill_price: i64,
    next_ticket: u64,
    position: Option<(u64, String, i64, i64)>,
    orders: Vec<Value>,
}

impl AutoBroker {
    pub fn new(fill_price: i64) -> Self {
        Self {
            fill_price,
            next_ticket: 100,
            position: None,
            orders: Vec::new(),
        }
    }

    fn ticket(&mut self) -> u64 {
        self.next_ticket += 1;
        self.next_ticket
    }

    /// Answer everything the venue sent, then push one state.
    pub fn settle(&mut self, handle: &FakeBridgeHandle) {
        let mut snapshot = Snapshot::new();
        for request in handle.take_sent() {
            let id = request["id"].as_str().unwrap_or_default().to_owned();
            let kind = request["type"].as_str().unwrap_or_default();
            let answer = match kind {
                "submit" if request["kind"] == "market" => {
                    let ticket = self.ticket();
                    let side = request["side"].as_str().unwrap_or_default().to_owned();
                    let volume: i64 = request["volume"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                    self.fill(&mut snapshot, ticket, &side, volume, None);
                    reply(&id, 10009, "Request executed", ticket)
                }
                "submit" => {
                    let ticket = self.ticket();
                    let price: i64 = request["price"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                    let volume: i64 = request["volume"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                    let side = request["side"].as_str().unwrap_or_default();
                    let kind = request["kind"].as_str().unwrap_or_default();
                    self.orders
                        .push(order(ticket, side, kind, volume, price, MAGIC));
                    reply(&id, 10008, "Request placed", ticket)
                }
                "modify" => {
                    let ticket = request["order"].as_u64().unwrap_or(0);
                    for line in &mut self.orders {
                        if line["ticket"] == ticket {
                            line["price"] = request["price"].clone();
                            line["sl"] = request["sl"].clone();
                            line["tp"] = request["tp"].clone();
                        }
                    }
                    reply(&id, 10009, "Request executed", ticket)
                }
                "cancel" => {
                    let ticket = request["order"].as_u64().unwrap_or(0);
                    self.orders.retain(|line| line["ticket"] != ticket);
                    snapshot = snapshot.closed(ticket, "canceled");
                    reply(&id, 10009, "Request executed", ticket)
                }
                "close" => {
                    let ticket = self.ticket();
                    let volume: i64 = request["volume"]
                        .as_str()
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(0);
                    let side = match self.position.as_ref().map(|p| p.1.as_str()) {
                        Some("buy") => "sell",
                        _ => "buy",
                    };
                    let reply_line = reply(&id, 10009, "Request executed", ticket);
                    handle.push(&reply_line);
                    self.fill(&mut snapshot, ticket, side, volume, Some(()));
                    continue;
                }
                _ => continue,
            };
            handle.push(&answer);
        }
        for line in &self.orders {
            snapshot = snapshot.order(line.clone());
        }
        if let Some((ticket, side, volume, price)) = &self.position {
            snapshot = snapshot.position(position(*ticket, side, *volume, *price, MAGIC));
        }
        handle.push(&snapshot.line());
    }

    fn fill(
        &mut self,
        snapshot: &mut Snapshot,
        order: u64,
        side: &str,
        volume: i64,
        _close: Option<()>,
    ) {
        let price = self.fill_price;
        let deal_ticket = self.ticket();
        let (entry, next) = match self.position.take() {
            None => ("in", Some((order, side.to_owned(), volume, price))),
            Some((ticket, held, qty, avg)) if held == side => (
                "in",
                Some((
                    ticket,
                    held,
                    qty + volume,
                    (avg * qty + price * volume) / (qty + volume),
                )),
            ),
            Some((ticket, held, qty, avg)) if volume < qty => {
                ("out", Some((ticket, held, qty - volume, avg)))
            }
            Some((_, _, qty, _)) if volume == qty => ("out", None),
            Some((_, _, qty, _)) => ("inout", Some((order, side.to_owned(), volume - qty, price))),
        };
        let position_id = next.as_ref().map_or(order, |p| p.0);
        *snapshot = snapshot.clone().deal(deal(
            deal_ticket,
            order,
            position_id,
            side,
            entry,
            "expert",
            volume,
            price,
        ));
        self.position = next;
    }
}
