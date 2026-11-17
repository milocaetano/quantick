//! `[broker.metatrader]`: executing on the logged-in MetaTrader 5 terminal.
//!
//! Read and checked here; nothing in this crate acts on it. The venue that
//! does lives in `quantick-broker-mt5`, built by the application only when
//! `enabled` is on — a real-broker venue stays off until configuration
//! turns it on.

use serde::Deserialize;

use super::MetaTraderSettings;
use super::split_host_port;

/// Brokers orders may be routed to (`[broker]` in the TOML). Absent, every
/// one of them is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrokerSettings {
    pub metatrader: MetaTraderBrokerSettings,
}

/// The MetaTrader 5 execution channel (`[broker.metatrader]`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetaTraderBrokerSettings {
    /// Whether broker routes may be offered at all. Off by default; on, the
    /// app still starts on the simulator and a route is the trader's pick.
    pub enabled: bool,
    /// The loopback port quantick listens on for the execution bridge.
    /// Bound on `127.0.0.1` only, whatever else is configured.
    pub exec_port: u16,
    /// Stamped on every order quantick sends; anything on the symbol with
    /// another magic is someone else's and is never touched. Never 0, which
    /// is the terminal's own hand.
    pub magic: u64,
    /// Slippage a market order accepts, in points.
    pub deviation_points: u32,
    /// Whether agents (the control plane, MCP) may place broker orders.
    /// Off by default; even on, an agent can never arm a broker route.
    pub allow_agent_orders: bool,
    /// The execution bridge to launch when a broker route is first asked
    /// for, as program plus arguments; `--port` (and `--symbol`) are
    /// appended from the running app. Resolved and launched the way the
    /// tick bridge's `bridge_command` is. Empty starts nothing: the trader
    /// runs the bridge by hand.
    pub exec_bridge_command: Vec<String>,
}

/// The largest slippage the config accepts, in points. Past this a market
/// order is a limit-less sweep in all but name.
pub const MAX_DEVIATION_POINTS: u32 = 10_000;

impl Default for MetaTraderBrokerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exec_port: 9180,
            magic: 7_710_001,
            deviation_points: 10,
            allow_agent_orders: false,
            exec_bridge_command: vec![
                "python".to_string(),
                "bridge/mt5/quantick_bridge_exec.py".to_string(),
            ],
        }
    }
}

impl BrokerSettings {
    /// Check the broker settings, against the feed listener's ports too: the
    /// execution port must not be one a tick bridge dials.
    ///
    /// # Errors
    ///
    /// The message names the key and why it could never work.
    pub fn validate(&self, feed: &MetaTraderSettings) -> Result<(), String> {
        let mt5 = &self.metatrader;
        if mt5.exec_port == 0 {
            return Err(
                "broker.metatrader.exec_port is 0; that binds whatever the OS hands out, and \
                 the execution bridge has no way to dial it"
                    .to_string(),
            );
        }
        let feed_ports = split_host_port(&feed.listen_addr)
            .map(|(_, port)| port)
            .into_iter()
            .chain(feed.ports.values().copied());
        for port in feed_ports {
            if port == mt5.exec_port {
                return Err(format!(
                    "broker.metatrader.exec_port {port} is also a [metatrader] feed port; \
                     the execution channel needs a port of its own"
                ));
            }
        }
        if mt5.magic == 0 {
            return Err(
                "broker.metatrader.magic is 0, the magic of orders placed by hand at the \
                 terminal; pick a number no other program uses"
                    .to_string(),
            );
        }
        if mt5.deviation_points > MAX_DEVIATION_POINTS {
            return Err(format!(
                "broker.metatrader.deviation_points is {} but at most {MAX_DEVIATION_POINTS} is \
                 accepted",
                mt5.deviation_points
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "broker_tests.rs"]
mod tests;
