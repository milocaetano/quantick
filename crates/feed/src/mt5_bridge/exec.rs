//! Starting the MetaTrader *execution* bridge: the second process beside the
//! tick bridge, which carries orders to the logged-in terminal.
//!
//! It is found and launched exactly as the tick bridge is — the same script
//! search ([`resolve_script`]), the same interpreter fallbacks
//! ([`interpreter_candidates`]) — but supervised by nobody: the broker venue
//! sees the link drop and disarms, and the next time a broker route is asked
//! for, a fresh process is launched. The process ends with its owner.

use std::process::{Child, Command, Stdio};

use tracing::{info, warn};

use super::{default_search_roots, interpreter_candidates, resolve_script};

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
    use super::ExecBridge;

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
