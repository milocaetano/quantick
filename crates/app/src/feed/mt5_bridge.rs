//! Starting the MetaTrader bridge for the user, and saying so out loud.
//!
//! Selecting a MetaTrader feed should be one action. The pieces that make it
//! one live here, apart from the feed itself, because they are a different
//! job: the feed translates a *connected* bridge into trades, while this
//! module is about the minutes before that — finding the script, finding an
//! interpreter, launching it, and reporting what happened in words that mean
//! something to someone who has never opened a terminal.
//!
//! Three failures were only ever visible in a log file, and all three are
//! ordinary:
//!
//! - **quantick was not started from the repository.** `bridge_command`
//!   carries a relative path, and a double-clicked binary resolves it against
//!   whatever directory the shortcut chose. [`resolve_script`] therefore looks
//!   beside the executable and up its ancestors as well as in the working
//!   directory, so the same configuration works from either.
//! - **`python` is not what the interpreter is called.** Windows installs a
//!   `py` launcher and, on a machine with no Python at all, a stub `python.exe`
//!   that opens the Microsoft Store. [`interpreter_candidates`] tries the
//!   plausible names in order — but only for the default command, since a
//!   configured one is an instruction, not a guess.
//! - **The bridge started and then stopped.** Its exit is not the interesting
//!   part; the JSON line it printed first is. Those lines are read, logged, and
//!   turned into [`FeedNotice`]s through [`quantick_feed_mt5::bridge_log`].

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quantick_feed_mt5::bridge_log::{BridgeSeverity, report_for_line};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::MetaTraderSettings;

use super::FeedNotice;

/// How long the autostart waits for a bridge that is already running before
/// launching its own. Long enough for an attached Expert Advisor's reconnect
/// cycle to notice the port, short enough that a cold start feels immediate.
const AUTOSTART_GRACE: Duration = Duration::from_secs(3);

/// Gap between autostart attempts. The common failure is a terminal still
/// starting up, which resolves in seconds.
const AUTOSTART_RETRY: Duration = Duration::from_secs(5);

/// How many times the autostart relaunches an exiting bridge before leaving it
/// alone. Every remaining failure (terminal closed, unknown symbol, unknown
/// server offset) needs a human, and retrying it forever only buries the log
/// line that says so.
const AUTOSTART_ATTEMPTS: u32 = 5;

/// The program name that means "the bridge quantick ships with", and so the
/// only one whose interpreter quantick is allowed to second-guess.
const DEFAULT_INTERPRETER: &str = "python";

/// Interpreter names tried, in order, for the default command.
///
/// `py` is the launcher a standard Windows install puts on PATH, and it
/// resolves a real interpreter even when `python` is the Store stub.
const INTERPRETER_FALLBACKS: [&str; 2] = ["python", "py"];

/// How deep to climb from the executable looking for the repository layout.
/// `target/release/quantick-app.exe` needs three; one more absorbs a wrapper
/// directory without turning the search into a filesystem crawl.
const ANCESTOR_SEARCH_DEPTH: usize = 4;

/// The interpreters to try for `program`, most likely first.
///
/// Only the shipped default gets alternatives: someone who configured
/// `bridge_command` told quantick exactly what to run, and silently running
/// something else would be a worse failure than the honest one.
#[must_use]
pub fn interpreter_candidates(program: &str) -> Vec<String> {
    if program == DEFAULT_INTERPRETER {
        INTERPRETER_FALLBACKS
            .iter()
            .map(|p| (*p).to_string())
            .collect()
    } else {
        vec![program.to_string()]
    }
}

/// Find the bridge script named by `arg`, looking in `roots` in order.
///
/// An absolute path is taken as given (present or not — reporting "not found"
/// for the path someone explicitly wrote is clearer than searching elsewhere
/// and running a different file). A relative one is resolved against each root
/// and, for every root, against its ancestors: a binary in `target/release`
/// is three levels below the `bridge/` folder it ships with.
#[must_use]
pub fn resolve_script(arg: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let relative = Path::new(arg);
    if relative.is_absolute() {
        return relative.exists().then(|| relative.to_path_buf());
    }
    for root in roots {
        for base in root.ancestors().take(ANCESTOR_SEARCH_DEPTH) {
            let candidate = base.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The directories a bridge script is looked for under, most specific first:
/// the working directory (how the repo runs it), then the directory holding
/// the executable (how a shortcut or a copied build runs it).
#[must_use]
pub fn default_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .filter(|exe_dir| !roots.contains(exe_dir))
    {
        roots.push(exe_dir);
    }
    roots
}

/// Everything the supervisor needs that is not configuration.
pub struct Supervision {
    /// The instrument the bridge is asked to stream.
    pub symbol: String,
    /// Flipped by the feed when any bridge says hello, so an already-running
    /// bridge (or an attached Expert Advisor) is never fought.
    pub connected: Arc<AtomicBool>,
    /// Where user-facing reports go.
    pub notices: mpsc::Sender<FeedNotice>,
}

/// Launch a bridge when nothing else is feeding us, and keep it alive.
///
/// Deliberately passive at the start: a bridge that is already running, or an
/// Expert Advisor attached to a chart, gets the grace period to dial in. Only
/// silence triggers a launch.
pub async fn supervise(settings: MetaTraderSettings, sup: Supervision) {
    let symbol = sup.symbol.as_str();
    let Some((host, port)) = settings.bridge_endpoint() else {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_AUTOSTART_SKIPPED",
            symbol = %symbol,
            listen_addr = %settings.listen_addr,
            action = "wait_for_manual_bridge",
            "cannot derive a dial address from listen_addr; not starting a bridge"
        );
        return;
    };
    let Some((program, extra)) = settings.bridge_command.split_first() else {
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_AUTOSTART_SKIPPED",
            symbol = %symbol,
            action = "wait_for_manual_bridge",
            "bridge_command is empty; not starting a bridge"
        );
        return;
    };

    // The script argument is resolved once, up front: a path that does not
    // exist is a setup problem to report, not something five launch attempts
    // will fix.
    let roots = default_search_roots();
    let mut args: Vec<String> = extra.to_vec();
    if let Some(first) = args.first_mut() {
        match resolve_script(first, &roots) {
            Some(found) => *first = found.to_string_lossy().into_owned(),
            None => {
                let looked_in = roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_SCRIPT_NOT_FOUND",
                    symbol = %symbol,
                    script = %first,
                    looked_in = %looked_in,
                    action = "wait_for_manual_bridge",
                    "cannot find the bridge script; not starting a bridge"
                );
                let _ = sup
                    .notices
                    .send(FeedNotice::attention(
                        "quantick cannot find the MetaTrader bridge script",
                        format!(
                            "Looked for {first} under: {looked_in}. Run quantick from its \
                             folder, or set bridge_command in your configuration."
                        ),
                    ))
                    .await;
                return;
            }
        }
    }

    tokio::time::sleep(AUTOSTART_GRACE).await;
    let candidates = interpreter_candidates(program);
    // Which interpreter worked, so a retry does not re-probe the ones that are
    // not installed on this machine.
    let mut chosen: Option<String> = None;

    for attempt in 1..=AUTOSTART_ATTEMPTS {
        if sup.connected.load(Ordering::Relaxed) {
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_AUTOSTART_NOT_NEEDED",
                symbol = %symbol,
                action = "leave_running_bridge_alone",
                "a bridge is already connected; quantick started none of its own"
            );
            return;
        }

        let _ = sup
            .notices
            .send(FeedNotice::working("starting the MetaTrader bridge"))
            .await;

        let try_now: Vec<String> = match &chosen {
            Some(program) => vec![program.clone()],
            None => candidates.clone(),
        };
        let mut child = None;
        for program in try_now {
            let mut command = Command::new(&program);
            command
                .args(&args)
                .arg("--symbol")
                .arg(symbol)
                .arg("--host")
                .arg(host)
                .arg("--port")
                .arg(port)
                .stdin(Stdio::null())
                // Read rather than inherit: the same lines still reach the log
                // (one story, one place), and they also become something the
                // chart can show.
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            match command.spawn() {
                Ok(spawned) => {
                    info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "MT5_BRIDGE_SPAWNED",
                        symbol = %symbol,
                        program = %program,
                        pid = spawned.id(),
                        attempt,
                        host,
                        port,
                        "started a bridge for this feed"
                    );
                    chosen = Some(program);
                    child = Some(spawned);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "MT5_BRIDGE_INTERPRETER_ABSENT",
                        symbol = %symbol,
                        program = %program,
                        action = "try_next_candidate",
                        "no such interpreter on PATH"
                    );
                }
                Err(error) => {
                    warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "MT5_BRIDGE_SPAWN_FAILED",
                        symbol = %symbol,
                        program = %program,
                        attempt,
                        %error,
                        action = "try_next_candidate",
                        "could not start the bridge"
                    );
                }
            }
        }

        let Some(mut child) = child else {
            let tried = candidates.join(" or ");
            let _ = sup
                .notices
                .send(FeedNotice::attention(
                    "quantick could not start the MetaTrader bridge",
                    format!(
                        "No Python interpreter was found (tried {tried}). Install Python \
                         and the MetaTrader5 package, then reconnect."
                    ),
                ))
                .await;
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_NO_INTERPRETER",
                symbol = %symbol,
                tried = %tried,
                action = "retry_after_backoff",
                "no interpreter could be started"
            );
            tokio::time::sleep(AUTOSTART_RETRY).await;
            continue;
        };

        // Whether the bridge explained itself before exiting. When it did, the
        // exit needs no second message: the reason is already on screen.
        let mut explained = false;
        if let Some(stderr) = child.stderr.take() {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if forward_bridge_line(&line, symbol, &sup.notices).await {
                    explained = true;
                }
            }
        }

        let status = child.wait().await;
        warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_EXITED",
            symbol = %symbol,
            attempt,
            max_attempts = AUTOSTART_ATTEMPTS,
            status = ?status.as_ref().map(|s| s.code()),
            explained,
            action = "retry_after_backoff",
            "the bridge quantick started has exited"
        );
        if !explained && !sup.connected.load(Ordering::Relaxed) {
            let code = status.ok().and_then(|s| s.code());
            let _ = sup
                .notices
                .send(FeedNotice::attention(
                    "the MetaTrader bridge stopped",
                    match code {
                        Some(code) => format!(
                            "It exited with code {code} without saying why. Check that \
                             MetaTrader 5 is running and logged in."
                        ),
                        None => "It stopped without saying why. Check that MetaTrader 5 \
                                 is running and logged in."
                            .to_owned(),
                    },
                ))
                .await;
        }
        tokio::time::sleep(AUTOSTART_RETRY).await;
    }

    warn!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MT5_BRIDGE_AUTOSTART_GAVE_UP",
        symbol = %symbol,
        attempts = AUTOSTART_ATTEMPTS,
        action = "wait_for_manual_bridge",
        "the bridge would not stay up; read its own log lines above for the reason"
    );
    let _ = sup
        .notices
        .send(FeedNotice::attention(
            "the MetaTrader bridge would not stay running",
            "quantick stopped retrying after several attempts. Fix what the last \
             message reported, then switch feeds to try again.",
        ))
        .await;
}

/// Log one bridge stderr line and, when it means something to a person, send
/// it on as a notice. Returns whether it produced a user-facing report.
async fn forward_bridge_line(line: &str, symbol: &str, notices: &mpsc::Sender<FeedNotice>) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Every line reaches the log verbatim, recognized or not: a Python
    // traceback is exactly the case where hiding the unknown would hurt.
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MT5_BRIDGE_SAID",
        symbol = %symbol,
        line = %trimmed,
        "bridge output"
    );
    let Some(report) = report_for_line(trimmed) else {
        return false;
    };
    let notice = match report.severity {
        BridgeSeverity::Progress => FeedNotice::working(report.headline),
        BridgeSeverity::Attention => FeedNotice::attention(
            report.headline,
            report
                .next_step
                .unwrap_or_else(|| "See bridge/mt5/README.md for this bridge's setup.".to_owned()),
        ),
    };
    let needs_user = matches!(notice, FeedNotice::Attention { .. });
    let _ = notices.send(notice).await;
    needs_user
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway directory tree, cleaned up when the test ends.
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            // Unique per test without a temp-file crate: the process id keeps
            // parallel runs apart, the tag keeps tests within one run apart.
            let path =
                std::env::temp_dir().join(format!("quantick-bridge-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create temp tree");
            Self(path)
        }

        fn file(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("create dirs");
            std::fs::write(&path, b"# bridge").expect("write file");
            path
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_script_is_found_in_the_working_directory() {
        let tree = TempTree::new("cwd");
        let script = tree.file("bridge/mt5/quantick_bridge.py");
        assert_eq!(
            resolve_script(
                "bridge/mt5/quantick_bridge.py",
                std::slice::from_ref(&tree.0)
            ),
            Some(script)
        );
    }

    #[test]
    fn the_script_is_found_from_a_binary_deep_in_target() {
        // The case a double-clicked build hits: the executable sits three
        // levels below the folder that ships the bridge.
        let tree = TempTree::new("target");
        let script = tree.file("bridge/mt5/quantick_bridge.py");
        std::fs::create_dir_all(tree.0.join("target/release")).expect("create target");
        let exe_dir = tree.0.join("target/release");
        assert_eq!(
            resolve_script("bridge/mt5/quantick_bridge.py", &[exe_dir]),
            Some(script)
        );
    }

    #[test]
    fn a_script_that_is_nowhere_is_reported_as_missing() {
        let tree = TempTree::new("missing");
        assert_eq!(
            resolve_script(
                "bridge/mt5/quantick_bridge.py",
                std::slice::from_ref(&tree.0)
            ),
            None
        );
    }

    #[test]
    fn an_absolute_path_is_taken_as_written() {
        let tree = TempTree::new("absolute");
        let script = tree.file("elsewhere/bridge.py");
        // Found without consulting any root...
        assert_eq!(
            resolve_script(&script.to_string_lossy(), &[]),
            Some(script.clone())
        );
        // ...and a wrong absolute path is a miss, not a search elsewhere.
        let wrong = tree.0.join("elsewhere/absent.py");
        assert_eq!(
            resolve_script(&wrong.to_string_lossy(), std::slice::from_ref(&tree.0)),
            None
        );
    }

    #[test]
    fn a_directory_never_passes_for_a_script() {
        let tree = TempTree::new("dir");
        std::fs::create_dir_all(tree.0.join("bridge/mt5")).expect("create dirs");
        assert_eq!(
            resolve_script("bridge/mt5", std::slice::from_ref(&tree.0)),
            None
        );
    }

    #[test]
    fn only_the_default_interpreter_gets_alternatives() {
        assert_eq!(interpreter_candidates("python"), ["python", "py"]);
        // A configured command is an instruction, not a starting point.
        assert_eq!(interpreter_candidates("python3.12"), ["python3.12"]);
        assert_eq!(interpreter_candidates("py"), ["py"]);
    }

    #[test]
    fn the_search_roots_are_real_directories_without_duplicates() {
        let roots = default_search_roots();
        assert!(!roots.is_empty(), "at least the working directory");
        let mut unique = roots.clone();
        unique.dedup();
        assert_eq!(unique.len(), roots.len(), "no root is searched twice");
    }

    #[tokio::test]
    async fn a_fatal_bridge_line_becomes_an_actionable_notice() {
        let (tx, mut rx) = mpsc::channel(4);
        let produced = forward_bridge_line(
            r#"{"event_code":"BRIDGE_TERMINAL_ATTACH_FAILED","mt5_error":"(-10005)"}"#,
            "WINQ26",
            &tx,
        )
        .await;
        assert!(produced, "a fatal line needs the user");
        let Some(FeedNotice::Attention {
            headline,
            next_step,
        }) = rx.recv().await
        else {
            panic!("expected an attention notice");
        };
        assert!(headline.contains("MetaTrader 5"), "headline: {headline}");
        assert!(!next_step.is_empty(), "an attention always says what to do");
    }

    #[tokio::test]
    async fn progress_lines_do_not_count_as_an_explanation() {
        let (tx, mut rx) = mpsc::channel(4);
        let produced =
            forward_bridge_line(r#"{"event_code":"BRIDGE_STARTING"}"#, "WINQ26", &tx).await;
        assert!(!produced, "progress is not the reason a bridge died");
        assert!(matches!(rx.recv().await, Some(FeedNotice::Working { .. })));
    }

    #[tokio::test]
    async fn a_missing_script_is_reported_before_anything_is_launched() {
        // The setup failure that needs no Python, no terminal and no port: the
        // supervisor must name what it looked for and stop, rather than spend
        // five attempts spawning an interpreter against a path that is not
        // there.
        let (tx, mut rx) = mpsc::channel(4);
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:9100".to_string(),
            bridge_autostart: true,
            bridge_command: vec![
                "python".to_string(),
                "definitely/not/here/quantick_bridge.py".to_string(),
            ],
            ..MetaTraderSettings::default()
        };
        supervise(
            settings,
            Supervision {
                symbol: "WINQ26".to_string(),
                connected: Arc::new(AtomicBool::new(false)),
                notices: tx,
            },
        )
        .await;

        let Some(FeedNotice::Attention {
            headline,
            next_step,
        }) = rx.recv().await
        else {
            panic!("expected an attention notice");
        };
        assert!(headline.contains("cannot find"), "headline: {headline}");
        assert!(
            next_step.contains("definitely/not/here/quantick_bridge.py"),
            "the step must name what was looked for: {next_step}"
        );
    }

    #[tokio::test]
    async fn unrecognized_output_is_logged_but_says_nothing_to_the_user() {
        let (tx, mut rx) = mpsc::channel(4);
        for line in ["", "   ", "Traceback (most recent call last):", "{}"] {
            assert!(
                !forward_bridge_line(line, "WINQ26", &tx).await,
                "line: {line}"
            );
        }
        assert!(
            rx.try_recv().is_err(),
            "noise must not reach the chart as a notice"
        );
    }
}
