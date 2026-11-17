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

use std::io::{BufRead as _, BufReader, Read};
use std::process::{Child, Command, Stdio};

use quantick_broker_mt5::{BrokerConfig, Mt5Venue, TcpBridgeLink};
use quantick_feed_mt5::MAX_LINE_BYTES;
use quantick_sources::config::MetaTraderBrokerSettings;
use quantick_trading::{BrokerConnector, BrokerVenue};
use tracing::{info, warn};

use super::{default_search_roots, interpreter_candidates, log_bridge_line, resolve_script};

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
                // Read rather than discarded: the bridge logs every answer
                // the terminal gave (a refusal's retcode, its comment, the
                // request it answered), and a traceback says why it died.
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            match spawned {
                Ok(mut child) => {
                    relay_output(child.stdout.take(), symbol);
                    relay_output(child.stderr.take(), symbol);
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

/// Read `source`, one of the bridge's output pipes, on a thread of its own
/// until it closes, logging every line. A pipe nobody reads fills and stalls
/// the bridge; one that cannot be read is dropped, and the bridge's next
/// write fails it - a stopped bridge the venue reads as a lost link.
fn relay_output(source: Option<impl Read + Send + 'static>, symbol: &str) {
    let Some(source) = source else {
        return;
    };
    let symbol = symbol.to_owned();
    let spawned = std::thread::Builder::new()
        .name("mt5-exec-bridge-output".to_owned())
        .spawn(move || {
            relay_lines(source, |line| {
                log_bridge_line("MT5_EXEC_BRIDGE_SAID", &symbol, line);
            });
        });
    if let Err(error) = spawned {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_EXEC_BRIDGE_OUTPUT_UNREAD",
            %error,
            action = "bridge_output_dropped",
            "could not start reading the execution bridge's output"
        );
    }
}

/// Hand `said` every non-blank line `source` prints until it closes, trimmed.
/// A line over [`MAX_LINE_BYTES`] is named rather than held, so a runaway
/// bridge cannot grow memory without limit.
pub(super) fn relay_lines(source: impl Read, mut said: impl FnMut(&str)) {
    let mut reader = BufReader::new(source);
    let limit = MAX_LINE_BYTES as u64 + 1;
    let mut buffer = Vec::new();
    // Inside a line already reported as too long, until its newline.
    let mut skipping = false;
    loop {
        buffer.clear();
        match (&mut reader).take(limit).read_until(b'\n', &mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let ended = buffer.last() == Some(&b'\n');
        if skipping {
            skipping = !ended;
            continue;
        }
        if !ended && buffer.len() as u64 >= limit {
            skipping = true;
            said(&format!("a line over {MAX_LINE_BYTES} bytes was skipped"));
            continue;
        }
        let text = String::from_utf8_lossy(&buffer);
        let line = text.trim();
        if !line.is_empty() {
            said(line);
        }
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
#[path = "exec_tests.rs"]
mod tests;
