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
