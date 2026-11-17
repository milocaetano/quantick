//! Starting the MetaTrader *execution* bridge: the second process beside the
//! tick bridge, which carries orders to the logged-in terminal.
//!
//! It is found and launched exactly as the tick bridge is — the same script
//! search ([`resolve_script`]), the same interpreter fallbacks
//! ([`interpreter_candidates`]) — but supervised by nobody: the broker venue
//! sees the link drop and disarms, and the next time a broker route is asked
//! for ([`BrokerConnector::revive`]), a fresh process is launched if the old
//! one has exited; it dials the same listener and the same venue. The process
//! ends with its owner.
//!
//! [`Mt5Connector`] is that owner: the route's [`BrokerConnector`], which
//! listens on the configured loopback port, launches the bridge and builds
//! the MetaTrader venue over the socket.

use std::process::{Child, Command, Stdio};

use quantick_broker_mt5::{BrokerConfig, Mt5Venue, TcpBridgeLink};
use quantick_sources::config::MetaTraderBrokerSettings;
use quantick_trading::{BrokerConnector, BrokerVenue};
use tracing::{info, warn};

use super::{default_search_roots, interpreter_candidates, resolve_script};

/// The MetaTrader route's connector: `[broker.metatrader]`, and the bridge
/// it launched, ended with it.
#[derive(Debug)]
pub struct Mt5Connector {
    settings: MetaTraderBrokerSettings,
    bridge: Option<ExecBridge>,
    /// A venue was built and its listener waits for a bridge to dial.
    listening: bool,
}

impl Mt5Connector {
    #[must_use]
    pub fn new(settings: MetaTraderBrokerSettings) -> Self {
        Self {
            settings,
            bridge: None,
            listening: false,
        }
    }
}

impl BrokerConnector for Mt5Connector {
    fn connect(&mut self, symbol: &str) -> Option<Box<dyn BrokerVenue>> {
        let settings = &self.settings;
        let link = match TcpBridgeLink::listen(settings.exec_port) {
            Ok(link) => link,
            Err(error) => {
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BROKER_EXEC_LISTEN_FAILED",
                    port = settings.exec_port,
                    %error,
                    action = "retry_when_the_list_opens",
                    "could not listen for the execution bridge"
                );
                return None;
            }
        };
        self.listening = true;
        self.bridge = ExecBridge::launch(&settings.exec_bridge_command, settings.exec_port, symbol);
        // Unique per run: the process and the moment it built the venue.
        let started_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let config = BrokerConfig {
            symbol: symbol.to_owned(),
            magic: settings.magic,
            deviation_points: settings.deviation_points,
            session_tag: format!("q{}-{started_ms}", std::process::id()),
        };
        Some(Box::new(Mt5Venue::new(config, Box::new(link))))
    }

    fn revive(&mut self, symbol: &str) {
        if !self.listening || self.bridge.as_mut().is_some_and(ExecBridge::running) {
            return;
        }
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_EXEC_BRIDGE_RELAUNCH",
            port = self.settings.exec_port,
            "the execution bridge is not running; starting it again"
        );
        // The listener is still the venue's: the new process dials it, and
        // its new instance tells the venue nothing it sent is remembered.
        self.bridge = None;
        self.bridge = ExecBridge::launch(
            &self.settings.exec_bridge_command,
            self.settings.exec_port,
            symbol,
        );
    }
}

/// A running execution bridge, ended when dropped.
#[derive(Debug)]
pub struct ExecBridge {
    child: Child,
}

impl ExecBridge {
    /// Launch `command` (program plus arguments, the configured
    /// `exec_bridge_command`) dialing `127.0.0.1:port`, watching `symbol`.
    /// `None` when the command is empty, the script cannot be found or no
    /// interpreter starts — each logged with its reason.
    #[must_use]
    pub fn launch(command: &[String], port: u16, symbol: &str) -> Option<Self> {
        let Some((program, extra)) = command.split_first() else {
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_EXEC_BRIDGE_NOT_CONFIGURED",
                action = "wait_for_manual_bridge",
                "exec_bridge_command is empty; not starting an execution bridge"
            );
            return None;
        };
        let mut args = extra.to_vec();
        if let Some(first) = args.first_mut() {
            let roots = default_search_roots();
            let Some(found) = resolve_script(first, &roots) else {
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_EXEC_BRIDGE_SCRIPT_MISSING",
                    script = %first,
                    action = "wait_for_manual_bridge",
                    "the execution bridge script was not found"
                );
                return None;
            };
            *first = found.to_string_lossy().into_owned();
        }
        for candidate in interpreter_candidates(program) {
            let spawned = Command::new(&candidate)
                .args(&args)
                .arg("--host")
                .arg("127.0.0.1")
                .arg("--port")
                .arg(port.to_string())
                .arg("--symbol")
                .arg(symbol)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            match spawned {
                Ok(child) => {
                    info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "MT5_EXEC_BRIDGE_SPAWNED",
                        program = %candidate,
                        pid = child.id(),
                        port,
                        "started the execution bridge"
                    );
                    return Some(Self { child });
                }
                Err(error) => warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_EXEC_BRIDGE_SPAWN_FAILED",
                    program = %candidate,
                    %error,
                    action = "try_next_candidate",
                    "could not start the execution bridge"
                ),
            }
        }
        None
    }

    /// Whether the process is still running.
    pub fn running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The process id, for a log line or a test that ends it.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for ExecBridge {
    fn drop(&mut self) {
        // The bridge only relays; ending it closes the link, which the venue
        // reads as lost and disarms on. Orders already at the broker stay.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecBridge, Mt5Connector};
    use quantick_sources::config::MetaTraderBrokerSettings;
    use quantick_trading::BrokerConnector;

    /// A bridge whose process ran and ended: the test binary listing itself.
    fn ended_bridge() -> ExecBridge {
        let child = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .arg("--list")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the test binary starts");
        let mut bridge = ExecBridge { child };
        bridge.child.wait().expect("it ends");
        bridge
    }

    #[test]
    fn a_bridge_that_exited_is_not_running() {
        assert!(!ended_bridge().running());
    }

    #[test]
    fn revive_relaunches_only_an_ended_bridge_behind_a_listener() {
        let settings = MetaTraderBrokerSettings {
            exec_bridge_command: Vec::new(),
            ..MetaTraderBrokerSettings::default()
        };
        let mut never_listened = Mt5Connector::new(settings.clone());
        never_listened.bridge = Some(ended_bridge());
        never_listened.revive("WINZ26");
        assert!(
            never_listened.bridge.is_some(),
            "no venue was built, so nothing is relaunched"
        );

        let mut connector = Mt5Connector::new(settings);
        connector.listening = true;
        connector.bridge = Some(ended_bridge());
        connector.revive("WINZ26");
        // The ended one is let go and a launch attempted; an empty command
        // launches nothing, which is all a test without Python can see.
        assert!(connector.bridge.is_none());
    }

    #[test]
    fn an_empty_command_launches_nothing() {
        assert!(ExecBridge::launch(&[], 1, "WINZ26").is_none());
    }

    #[test]
    fn a_missing_script_launches_nothing() {
        let command = [
            "python".to_owned(),
            "no/such/quantick_bridge_exec.py".to_owned(),
        ];
        assert!(ExecBridge::launch(&command, 1, "WINZ26").is_none());
    }
}
