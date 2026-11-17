//! A bridge with no terminal behind it, for tests and for callers that need
//! a broker venue without a socket.
//!
//! Shipped rather than hidden behind `#[cfg(test)]` for the reason
//! `quantick_trading::fake` is: the app's wiring proves itself against it
//! from another crate. It decides nothing — a test says what the bridge
//! answers, line by line, and reads back what the venue sent.

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use crate::link::{BridgeLink, LinkEvent};

#[derive(Debug, Default)]
struct Wire {
    inbound: Vec<LinkEvent>,
    sent: Vec<String>,
    connected: bool,
}

/// The link end the venue owns.
#[derive(Debug, Clone, Default)]
pub struct FakeBridge {
    wire: Arc<Mutex<Wire>>,
}

/// The test's end: what the bridge says, and what it was sent.
#[derive(Debug, Clone)]
pub struct FakeBridgeHandle {
    wire: Arc<Mutex<Wire>>,
}

impl FakeBridge {
    /// A link and the handle that scripts it.
    #[must_use]
    pub fn new() -> (Self, FakeBridgeHandle) {
        let bridge = Self::default();
        let handle = FakeBridgeHandle {
            wire: Arc::clone(&bridge.wire),
        };
        (bridge, handle)
    }
}

fn lock(wire: &Mutex<Wire>) -> std::sync::MutexGuard<'_, Wire> {
    wire.lock().unwrap_or_else(PoisonError::into_inner)
}

impl BridgeLink for FakeBridge {
    fn send(&mut self, line: &str) {
        let mut wire = lock(&self.wire);
        // Like the socket: nothing connected, nothing delivered.
        if wire.connected {
            wire.sent.push(line.to_owned());
        }
    }

    fn poll(&mut self, out: &mut Vec<LinkEvent>) {
        out.append(&mut lock(&self.wire).inbound);
    }
}

impl FakeBridgeHandle {
    /// A bridge process `instance` dials in and says hello.
    pub fn connect(&self, instance: &str) {
        let mut wire = lock(&self.wire);
        wire.connected = true;
        wire.inbound.push(LinkEvent::Line(
            serde_json::json!({
                "type": "exec_hello",
                "schema": crate::wire::EXEC_SCHEMA_VERSION,
                "bridge": "fake",
                "bridge_version": "0",
                "instance": instance,
            })
            .to_string(),
        ));
    }

    /// The bridge's connection drops.
    pub fn disconnect(&self) {
        let mut wire = lock(&self.wire);
        wire.connected = false;
        wire.inbound.push(LinkEvent::Disconnected);
    }

    /// The bridge sends one line.
    pub fn push(&self, line: &Value) {
        lock(&self.wire)
            .inbound
            .push(LinkEvent::Line(line.to_string()));
    }

    /// The bridge sends raw text, readable or not.
    pub fn push_raw(&self, line: &str) {
        lock(&self.wire)
            .inbound
            .push(LinkEvent::Line(line.to_owned()));
    }

    /// Every line delivered to the bridge so far, decoded.
    #[must_use]
    pub fn sent(&self) -> Vec<Value> {
        lock(&self.wire)
            .sent
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// The delivered lines, decoded, and forget them.
    pub fn take_sent(&self) -> Vec<Value> {
        let lines = std::mem::take(&mut lock(&self.wire).sent);
        lines
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }
}

/// A session staged for a capture or a demo: an account, an own position
/// with its stop and target at the broker, and one own working order — every
/// state the order surfaces paint, with no terminal behind it.
#[derive(Debug, Clone)]
pub struct StagedSession<'a> {
    pub symbol: &'a str,
    pub magic: u64,
    pub login: u64,
    pub company: &'a str,
    /// `demo` or `real`, as `account_info().trade_mode` names it.
    pub trade_mode: &'a str,
    pub balance: &'a str,
    pub currency: &'a str,
    /// The position's entry; the stop, target and working order sit at
    /// fixed fractions of it.
    pub entry: rust_decimal::Decimal,
}

/// The tickets a staged session's position and working order carry.
pub const STAGED_POSITION_TICKET: u64 = 88_412_094;
pub const STAGED_ORDER_TICKET: u64 = 88_412_096;

impl FakeBridgeHandle {
    /// Say hello and push `session` as the bridge's state.
    pub fn stage(&self, session: &StagedSession<'_>) {
        use rust_decimal::Decimal;
        let at = |per_mille: i64| {
            let offset = (session.entry * Decimal::new(per_mille, 4)).round_dp(0);
            (session.entry + offset).round_dp(0).to_string()
        };
        self.connect("staged");
        self.push(&serde_json::json!({
            "type": "state",
            "symbol": session.symbol,
            "account": {
                "login": session.login,
                "server": format!("{}-MT5", session.company.split_whitespace().next().unwrap_or("Broker")),
                "company": session.company,
                "trade_mode": session.trade_mode,
                "margin_mode": "exchange",
                "balance": session.balance,
                "currency": session.currency,
            },
            "positions": [{
                "ticket": STAGED_POSITION_TICKET, "side": "buy", "volume": "1",
                "price_open": session.entry.to_string(), "sl": at(-15), "tp": at(15),
                "magic": session.magic, "time_msc": 0,
            }],
            "orders": [{
                "ticket": STAGED_ORDER_TICKET, "side": "buy", "kind": "limit", "volume": "1",
                "price": at(-30), "sl": null, "tp": null,
                "magic": session.magic, "time_setup_msc": 0,
            }],
            "deals": [],
            "closed_orders": [],
        }));
    }
}

/// A connector whose venue speaks to a staged session: what a capture asks
/// for instead of a terminal. `demo` picks the staged account's kind;
/// `entry` is where the staged position opened.
#[derive(Debug)]
pub struct StagedConnector {
    pub demo: bool,
    pub magic: u64,
    pub entry: rust_decimal::Decimal,
    handle: Option<FakeBridgeHandle>,
}

impl StagedConnector {
    #[must_use]
    pub fn new(demo: bool, magic: u64, entry: rust_decimal::Decimal) -> Self {
        Self {
            demo,
            magic,
            entry,
            handle: None,
        }
    }
}

impl quantick_trading::BrokerConnector for StagedConnector {
    fn connect(&mut self, symbol: &str) -> Option<Box<dyn quantick_trading::BrokerVenue>> {
        let (bridge, handle) = FakeBridge::new();
        let config = crate::BrokerConfig {
            symbol: symbol.to_owned(),
            magic: self.magic,
            deviation_points: 10,
            session_tag: "staged".to_owned(),
        };
        let venue = crate::Mt5Venue::new(config, Box::new(bridge));
        let demo = self.demo;
        handle.stage(&StagedSession {
            symbol,
            magic: self.magic,
            login: if demo { 59_001_122 } else { 51_234_567 },
            company: "XP Investimentos",
            trade_mode: if demo { "demo" } else { "real" },
            balance: if demo { "100000.00" } else { "12340.00" },
            currency: "BRL",
            entry: self.entry,
        });
        self.handle = Some(handle);
        Some(Box::new(venue))
    }

    fn sever(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.disconnect();
        }
    }
}
