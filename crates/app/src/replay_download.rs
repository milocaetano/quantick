//! Downloading a replay session, and where the data comes from.
//!
//! Market Replay could always *play* a recording; making one was a command
//! line. This is the port that closes that gap: a [`SessionSource`] knows how
//! to ask a provider which days it holds and how to write the two files
//! `quantick-replay` reads. MetaTrader is the first implementation and is not
//! privileged — a second provider registers here and the interface does not
//! change.
//!
//! # Why a separate process, not the live bridge
//!
//! A tape export moves tens of millions of ticks. Sharing the streaming
//! bridge's socket with that would let a download stall — or take down — a
//! chart someone is trading on, and would need the EA attached to a chart. The
//! MetaTrader source therefore launches `tools/mt5/export_session.py` once per
//! request and reads its progress off stderr, reusing the interpreter
//! resolution the live bridge already owns.
//!
//! # What is pure here, and what is not
//!
//! Everything that decides *what* to run and *what a line means* is pure and
//! tested: [`Mt5SessionSource::command`] builds the argv, [`parse_event`] turns
//! one stderr line into a [`DownloadEvent`]. Only [`run`] touches a process,
//! which is what lets a second, fake source drive the whole interface in tests
//! without MetaTrader, Python, or a network.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};

use crate::config::ProviderKind;
use crate::feed::mt5_bridge;

/// Where `export_session.py` lives, relative to the repository root.
const EXPORT_SCRIPT: &str = "tools/mt5/export_session.py";

/// Trading days of context offered by default.
///
/// A trader asked for "the day before" and every trader asked for more than
/// one: a week of sessions is what makes a gap, a level or a range legible
/// before the tape starts. Cheap, too — a week of minute candles is under a
/// megabyte against tens for the tape.
pub const DEFAULT_CONTEXT_SESSIONS: u32 = 5;

/// What one download is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    /// Instrument as the provider names it, e.g. `WINQ26`.
    pub symbol: String,
    /// Session day to fetch the tape for, `YYYY-MM-DD`. `None` asks only which
    /// days are available.
    pub day: Option<String>,
    /// Trading days of minute candles to fetch before `day`.
    pub context_sessions: u32,
    /// Replay folder to write into.
    pub out_dir: PathBuf,
    /// The broker's clock, as seconds ahead of UTC, when the trader had to
    /// state it.
    ///
    /// `None` asks the provider to measure it. Measuring needs a fresh tick,
    /// which needs an open market — and replays are downloaded after the
    /// close, which is exactly when there is none. So the interface can hand
    /// the answer over instead of failing, and says that it was declared
    /// rather than measured.
    pub utc_offset_s: Option<i32>,
}

impl DownloadRequest {
    /// Whether this only asks what is available, writing nothing.
    #[must_use]
    pub fn is_probe(&self) -> bool {
        self.day.is_none()
    }
}

/// Something that happened during a download, in the interface's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEvent {
    /// The provider holds data for these days, `YYYY-MM-DD`, ascending.
    DaysAvailable {
        /// The instrument they belong to.
        symbol: String,
        /// Days a tape can be fetched for.
        days: Vec<String>,
    },
    /// Ticks written so far.
    Progress {
        /// Count, not a fraction: the total is not known until the end.
        ticks: u64,
    },
    /// The tape landed.
    TapeWritten {
        /// Where it landed.
        path: PathBuf,
        /// How many prints it holds.
        ticks: u64,
        /// How big it is on disk.
        bytes: u64,
    },
    /// The context candles landed.
    ContextWritten {
        /// Trading days of candles served.
        sessions: u32,
        /// Whether the provider served every session that was asked for.
        complete: bool,
    },
    /// The whole request finished; the tape is at `tape`.
    Done {
        /// The session file to open.
        tape: PathBuf,
    },
    /// The request failed, with the one thing to do about it.
    Failed {
        /// What happened, in the user's terms.
        headline: String,
        /// The single next step, when there is one.
        fix: Option<String>,
    },
}

/// Why a download could not be started at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    /// The exporter script was not found next to the app or the repository.
    ScriptMissing {
        /// What was looked for.
        looked_for: String,
    },
    /// No Python interpreter could be launched.
    NoInterpreter {
        /// The names that were tried.
        tried: String,
    },
}

impl DownloadError {
    /// What to change so a download can start.
    #[must_use]
    pub fn advice(&self) -> String {
        match self {
            DownloadError::ScriptMissing { looked_for } => format!(
                "quantick could not find {looked_for}. Run the app from the \
                 repository, or copy the tools folder next to the executable."
            ),
            DownloadError::NoInterpreter { tried } => format!(
                "No Python interpreter was found (tried {tried}). Install \
                 Python 3 and make sure it is on PATH."
            ),
        }
    }
}

impl std::fmt::Display for DownloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadError::ScriptMissing { looked_for } => {
                write!(f, "the exporter {looked_for} was not found")
            }
            DownloadError::NoInterpreter { .. } => write!(f, "no Python interpreter was found"),
        }
    }
}

/// Turn one line of an exporter's stderr into an event.
///
/// Unknown codes and non-JSON lines are `None`: every line still reaches the
/// log verbatim, and a provider that grows a new diagnostic must never break a
/// download that is already running.
#[must_use]
pub fn parse_event(line: &str) -> Option<DownloadEvent> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let code = value.get("event_code")?.as_str()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let number = |key: &str| value.get(key).and_then(serde_json::Value::as_u64);

    match code {
        "EXPORT_DAYS_AVAILABLE" => Some(DownloadEvent::DaysAvailable {
            symbol: text("symbol").unwrap_or_default(),
            days: value
                .get("days")?
                .as_array()?
                .iter()
                .filter_map(|d| d.as_str().map(str::to_owned))
                .collect(),
        }),
        "EXPORT_TAPE_PROGRESS" => Some(DownloadEvent::Progress {
            ticks: number("ticks")?,
        }),
        "EXPORT_TAPE_WRITTEN" => Some(DownloadEvent::TapeWritten {
            path: PathBuf::from(text("path")?),
            ticks: number("ticks").unwrap_or_default(),
            bytes: number("bytes").unwrap_or_default(),
        }),
        "EXPORT_CONTEXT_WRITTEN" => Some(DownloadEvent::ContextWritten {
            sessions: u32::try_from(number("sessions").unwrap_or_default()).unwrap_or(u32::MAX),
            complete: value
                .get("complete")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
        }),
        "EXPORT_DONE" => Some(DownloadEvent::Done {
            tape: PathBuf::from(text("tape")?),
        }),
        // Every failure the exporter can report carries a `fix`, and every one
        // of them is something the person at the keyboard can act on: the
        // terminal is closed, the symbol is misspelled, the day is a holiday.
        // Surfacing the raw MT5 error code instead would be the difference
        // between a user who fixes it and one who files a bug.
        "EXPORT_PKG_MISSING" => Some(DownloadEvent::Failed {
            headline: "the MetaTrader 5 Python package is not installed".to_string(),
            fix: text("fix"),
        }),
        "EXPORT_ATTACH_FAILED" => Some(DownloadEvent::Failed {
            headline: "quantick could not reach the MetaTrader 5 terminal".to_string(),
            fix: text("fix"),
        }),
        "EXPORT_SYMBOL_UNKNOWN" | "EXPORT_SYMBOL_UNAVAILABLE" => Some(DownloadEvent::Failed {
            headline: format!(
                "the terminal does not serve {}",
                text("symbol").unwrap_or_else(|| "this symbol".to_string())
            ),
            fix: text("fix"),
        }),
        "EXPORT_TAPE_EMPTY" => Some(DownloadEvent::Failed {
            headline: format!(
                "{} has no trades on {}",
                text("symbol").unwrap_or_else(|| "this symbol".to_string()),
                text("day").unwrap_or_else(|| "that day".to_string())
            ),
            fix: text("fix"),
        }),
        "EXPORT_TAPE_FAILED" => Some(DownloadEvent::Failed {
            headline: "the terminal served no ticks for that day".to_string(),
            fix: text("fix"),
        }),
        "EXPORT_UTC_OFFSET_UNMEASURED" => Some(DownloadEvent::Failed {
            headline: "quantick does not know this broker's clock offset yet".to_string(),
            fix: text("fix"),
        }),
        _ => None,
    }
}

/// A provider that can list and download replay sessions.
///
/// One method, deliberately: everything a source has to decide is *what
/// command answers this request*. Running it, reading its progress and
/// cancelling it are the same for every provider and live in [`run`].
pub trait SessionSource: Send + Sync {
    /// Name for the interface, e.g. `MetaTrader 5`.
    fn label(&self) -> String;

    /// The provider whose instruments this source can actually deliver.
    ///
    /// The browser offers contracts to click, and a contract this source
    /// cannot serve is a click that ends in a refusal — a MetaTrader exporter
    /// asked for `BTCUSDT` because that is what the chart behind the window
    /// happens to show. Answering here keeps that judgement with the source
    /// that knows, rather than in the caller that would have to be edited
    /// again for the next one.
    fn provider(&self) -> ProviderKind;

    /// The commands that could answer `request`, best first.
    ///
    /// A list rather than one command because launching is where Windows
    /// disagrees with itself: `python` is often a Store stub that exits
    /// immediately, and the real interpreter answers to `py`. The runner tries
    /// each in turn and keeps the first that starts — the same order the live
    /// bridge already uses, so a machine that streams can also download.
    ///
    /// # Errors
    ///
    /// [`DownloadError`] when the provider cannot be invoked at all — a
    /// missing script, no interpreter to try. A provider that *can* run
    /// reports its own problems as [`DownloadEvent::Failed`] instead, so the
    /// user reads them in the same place.
    fn commands(&self, request: &DownloadRequest) -> Result<Vec<Command>, DownloadError>;
}

/// Downloads served by the MetaTrader 5 terminal this machine is logged in to.
pub struct Mt5SessionSource {
    /// How to launch Python, from the same setting the live bridge uses.
    interpreter: String,
    /// Where to look for the exporter script.
    roots: Vec<PathBuf>,
    /// Where the live bridge wrote this broker's measured clock, when the
    /// platform has a durable home for it.
    ///
    /// Resolved once here rather than read inside `arguments`, which must stay
    /// pure: an argv that depends on the machine cannot be asserted in a test,
    /// and this flag would then be the one part of the command line no test
    /// covers.
    clock_cache: Option<PathBuf>,
}

impl Mt5SessionSource {
    /// A source that resolves Python and the script the way the bridge does.
    #[must_use]
    pub fn new(interpreter: &str) -> Self {
        Self {
            interpreter: interpreter.to_string(),
            roots: mt5_bridge::default_search_roots(),
            clock_cache: mt5_bridge::clock_cache_path(),
        }
    }

    /// The same, looking for the script under `roots` — for tests.
    #[cfg(test)]
    #[must_use]
    pub fn with_roots(interpreter: &str, roots: Vec<PathBuf>) -> Self {
        Self {
            interpreter: interpreter.to_string(),
            roots,
            clock_cache: mt5_bridge::clock_cache_path(),
        }
    }

    /// The arguments `export_session.py` is called with, in order.
    ///
    /// Split out from [`command`](Self::command) so the mapping from a
    /// request to a command line is testable without a Python on the machine.
    #[must_use]
    pub fn arguments(
        script: &Path,
        request: &DownloadRequest,
        clock_cache: Option<&Path>,
    ) -> Vec<String> {
        let mut args = vec![
            script.display().to_string(),
            "--symbol".to_string(),
            request.symbol.clone(),
            "--out".to_string(),
            request.out_dir.display().to_string(),
        ];
        if let Some(offset) = request.utc_offset_s {
            args.push("--utc-offset-s".to_string());
            args.push(offset.to_string());
        }
        // Where the live bridge wrote down this broker's clock. Without it the
        // exporter only knows the copy inside its own checkout, and after the
        // close — when replays are actually downloaded — no offset means no
        // download at all.
        if let Some(cache) = clock_cache {
            args.push("--clock-cache".to_string());
            args.push(cache.display().to_string());
        }
        match &request.day {
            Some(day) => {
                args.push("--day".to_string());
                args.push(day.clone());
                args.push("--context-sessions".to_string());
                args.push(request.context_sessions.to_string());
            }
            None => args.push("--probe".to_string()),
        }
        args
    }
}

impl SessionSource for Mt5SessionSource {
    fn label(&self) -> String {
        "MetaTrader 5".to_string()
    }

    fn provider(&self) -> ProviderKind {
        ProviderKind::MetaTrader
    }

    fn commands(&self, request: &DownloadRequest) -> Result<Vec<Command>, DownloadError> {
        let script = mt5_bridge::resolve_script(EXPORT_SCRIPT, &self.roots).ok_or_else(|| {
            DownloadError::ScriptMissing {
                looked_for: EXPORT_SCRIPT.to_string(),
            }
        })?;
        let candidates = mt5_bridge::interpreter_candidates(&self.interpreter);
        if candidates.is_empty() {
            return Err(DownloadError::NoInterpreter {
                tried: self.interpreter.clone(),
            });
        }
        let args = Self::arguments(&script, request, self.clock_cache.as_deref());
        Ok(candidates
            .into_iter()
            .map(|program| {
                let mut command = Command::new(program);
                command.args(&args);
                command
            })
            .collect())
    }
}

/// A download in flight.
pub struct DownloadJob {
    /// Events as they arrive; the sender is dropped when the job ends.
    pub events: Receiver<DownloadEvent>,
    cancelled: Arc<AtomicBool>,
}

impl DownloadJob {
    /// Ask the download to stop.
    ///
    /// The exporter writes each file beside its destination and moves it into
    /// place only once whole, so a cancelled download leaves nothing the
    /// session browser would list as a playable day.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

/// Start `request` on `source`, reporting progress on a channel.
///
/// Runs on its own thread: an export of a busy session day moves tens of
/// millions of ticks, and none of that may happen on the thread drawing the
/// chart.
///
/// # Errors
///
/// [`DownloadError`] when the source cannot be invoked at all.
pub fn run(
    source: &dyn SessionSource,
    request: &DownloadRequest,
) -> Result<DownloadJob, DownloadError> {
    let commands = source.commands(request)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let label = source.label();

    std::thread::Builder::new()
        .name("quantick-replay-download".into())
        .spawn(move || pump(commands, &label, &tx, &flag))
        .expect("spawn replay download thread");

    Ok(DownloadJob {
        events: rx,
        cancelled,
    })
}

/// How many of the exporter's last lines an unexplained exit carries.
///
/// Enough for a Python traceback's business end — the raise and the line that
/// raised it — and short enough to stay inside the panel without turning the
/// interface into a log viewer.
const FAILURE_TAIL_LINES: usize = 4;

/// What the exporter said just before it stopped, as advice a person can read.
///
/// `None` when it said nothing: an empty box under a headline would be one
/// more thing to interpret, and silence is already reported by the headline.
fn last_words<'a>(tail: impl Iterator<Item = &'a str>) -> Option<String> {
    let body: Vec<&str> = tail.collect();
    if body.is_empty() {
        return None;
    }
    Some(format!("It last said:\n{}", body.join("\n")))
}

/// Run the command and translate its stderr until it ends or is cancelled.
fn pump(
    mut commands: Vec<Command>,
    label: &str,
    tx: &Sender<DownloadEvent>,
    cancelled: &AtomicBool,
) {
    use std::io::{BufRead, BufReader};

    // Try each candidate interpreter in turn: on Windows `python` is often a
    // Store stub, and the working one answers to `py`.
    let mut last_error = None;
    let mut spawned = None;
    for command in &mut commands {
        command.stdout(Stdio::null()).stderr(Stdio::piped());
        match command.spawn() {
            Ok(child) => {
                spawned = Some(child);
                break;
            }
            Err(e) => last_error = Some(e),
        }
    }
    let Some(mut child) = spawned else {
        let detail =
            last_error.map_or_else(|| "no interpreter to try".to_string(), |e| e.to_string());
        let _ = tx.send(DownloadEvent::Failed {
            headline: format!("{label} could not be started: {detail}"),
            fix: Some("Install Python 3 and make sure it is on PATH.".to_string()),
        });
        return;
    };
    let Some(stderr) = child.stderr.take() else {
        return;
    };

    // The last thing the exporter said before it stopped, kept for the exit
    // that explains nothing. A `NameError` on the line before the file is
    // written exits 1 with a perfectly clear Python traceback on stderr, and
    // the interface used to show none of it — "MetaTrader 5 stopped with code
    // 1" is what a whole afternoon of that bug looked like from the outside.
    let mut tail: VecDeque<String> = VecDeque::with_capacity(FAILURE_TAIL_LINES);
    // Whether a refusal already reached the interface in the exporter's own
    // words. Those are plain sentences with a fix attached; the exit code that
    // follows them explains nothing and must not overwrite them.
    let mut explained = false;
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        // Verbatim to the log, recognised or not: a diagnostic this build does
        // not know about is still the thing a person needs when it goes wrong.
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_DOWNLOAD_LINE",
            source = label,
            line = line.as_str(),
            "exporter said"
        );
        if cancelled.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        match parse_event(&line) {
            Some(event) => {
                explained |= matches!(event, DownloadEvent::Failed { .. });
                if tx.send(event).is_err() {
                    // The interface is gone; so is the reason to keep going.
                    let _ = child.kill();
                    break;
                }
            }
            // Only what the app could *not* read is worth quoting back: a
            // recognised line already reached the panel as plain words, and
            // showing its JSON underneath would be the event code the
            // interface deliberately never puts in front of a person.
            None if !line.trim().is_empty() => {
                if tail.len() == FAILURE_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
            None => {}
        }
    }

    match child.wait() {
        Ok(status) if status.success() => {}
        // A refusal the exporter already explained, in its own words, with its
        // own fix. The exit code adds nothing a person can act on.
        Ok(_) if explained => {}
        Ok(status) if cancelled.load(Ordering::Relaxed) => {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_DOWNLOAD_CANCELLED",
                code = status.code().unwrap_or(-1),
                "download cancelled by the user"
            );
        }
        Ok(status) => {
            // A non-zero exit with no `Failed` line already sent would leave
            // the interface waiting forever, so the exit itself becomes one —
            // carrying what the exporter last said, because an exit code alone
            // is a fact about the process, not about the download.
            let _ = tx.send(DownloadEvent::Failed {
                headline: format!("{label} stopped with code {}", status.code().unwrap_or(-1)),
                fix: last_words(tail.iter().map(String::as_str)),
            });
        }
        Err(e) => {
            let _ = tx.send(DownloadEvent::Failed {
                headline: format!("{label} could not be waited on: {e}"),
                fix: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> DownloadRequest {
        DownloadRequest {
            symbol: "WINQ26".to_string(),
            day: Some("2026-08-12".to_string()),
            context_sessions: 5,
            out_dir: PathBuf::from("/replay"),
            utc_offset_s: None,
        }
    }

    #[test]
    fn a_declared_broker_clock_reaches_the_exporter() {
        // Measuring needs an open market; replays are downloaded after the
        // close. Handing the answer over is what keeps the flow usable at the
        // hour a trader actually prepares.
        let declared = DownloadRequest {
            utc_offset_s: Some(-10_800),
            ..request()
        };
        let args = Mt5SessionSource::arguments(Path::new("tools/export.py"), &declared, None);
        assert!(args.windows(2).any(|w| w == ["--utc-offset-s", "-10800"]));

        let measured = Mt5SessionSource::arguments(Path::new("tools/export.py"), &request(), None);
        assert!(
            !measured.iter().any(|a| a == "--utc-offset-s"),
            "left out entirely when the provider should measure it"
        );
    }

    #[test]
    fn a_days_reply_is_read_into_the_calendar() {
        let line = r#"{"event_code":"EXPORT_DAYS_AVAILABLE","symbol":"WINQ26",
            "days":["2026-08-11","2026-08-12"]}"#;
        assert_eq!(
            parse_event(line),
            Some(DownloadEvent::DaysAvailable {
                symbol: "WINQ26".to_string(),
                days: vec!["2026-08-11".to_string(), "2026-08-12".to_string()],
            })
        );
    }

    #[test]
    fn progress_and_completion_carry_the_numbers_the_bar_shows() {
        assert_eq!(
            parse_event(r#"{"event_code":"EXPORT_TAPE_PROGRESS","ticks":250000}"#),
            Some(DownloadEvent::Progress { ticks: 250_000 })
        );
        assert_eq!(
            parse_event(
                r#"{"event_code":"EXPORT_TAPE_WRITTEN","path":"a/b.csv","ticks":9,"bytes":123}"#
            ),
            Some(DownloadEvent::TapeWritten {
                path: PathBuf::from("a/b.csv"),
                ticks: 9,
                bytes: 123,
            })
        );
    }

    #[test]
    fn a_short_context_is_reported_as_short() {
        assert_eq!(
            parse_event(r#"{"event_code":"EXPORT_CONTEXT_WRITTEN","sessions":2,"complete":false}"#),
            Some(DownloadEvent::ContextWritten {
                sessions: 2,
                complete: false,
            })
        );
    }

    #[test]
    fn every_failure_a_user_can_hit_arrives_with_its_next_step() {
        for line in [
            r#"{"event_code":"EXPORT_ATTACH_FAILED","fix":"open the terminal"}"#,
            r#"{"event_code":"EXPORT_SYMBOL_UNKNOWN","symbol":"WINX99","fix":"check the spelling"}"#,
            r#"{"event_code":"EXPORT_TAPE_EMPTY","symbol":"WINQ26","day":"2026-08-15","fix":"pick another day"}"#,
            r#"{"event_code":"EXPORT_PKG_MISSING","fix":"pip install MetaTrader5"}"#,
            r#"{"event_code":"EXPORT_UTC_OFFSET_UNMEASURED","fix":"connect live once"}"#,
        ] {
            let Some(DownloadEvent::Failed { headline, fix }) = parse_event(line) else {
                panic!("{line} must reach the user as a failure");
            };
            assert!(!headline.is_empty(), "{line}");
            assert!(fix.is_some(), "a failure with no next step is a dead end");
            assert!(
                !headline.contains("EXPORT_"),
                "the user reads plain words, not event codes: {headline}"
            );
        }
    }

    #[test]
    fn unknown_and_malformed_lines_are_passed_over() {
        assert_eq!(parse_event("not json at all"), None);
        assert_eq!(
            parse_event(r#"{"event_code":"EXPORT_SOMETHING_NEW"}"#),
            None
        );
        assert_eq!(parse_event(""), None);
    }

    #[test]
    fn a_download_asks_for_the_day_and_its_context() {
        let args = Mt5SessionSource::arguments(Path::new("tools/export.py"), &request(), None);
        assert!(args.windows(2).any(|w| w == ["--symbol", "WINQ26"]));
        assert!(args.windows(2).any(|w| w == ["--day", "2026-08-12"]));
        assert!(args.windows(2).any(|w| w == ["--context-sessions", "5"]));
        assert!(!args.iter().any(|a| a == "--probe"));
    }

    #[test]
    fn a_probe_asks_for_nothing_but_the_days() {
        let probe = DownloadRequest {
            day: None,
            ..request()
        };
        assert!(probe.is_probe());
        let args = Mt5SessionSource::arguments(Path::new("tools/export.py"), &probe, None);
        assert!(args.iter().any(|a| a == "--probe"));
        assert!(
            !args.iter().any(|a| a == "--day"),
            "a probe writes nothing, so it names no day"
        );
    }

    #[test]
    fn a_missing_script_says_where_to_put_it() {
        let source = Mt5SessionSource::with_roots("python", vec![PathBuf::from("/nowhere")]);
        let err = source
            .commands(&request())
            .expect_err("there is no exporter under /nowhere");
        assert_eq!(
            err,
            DownloadError::ScriptMissing {
                looked_for: EXPORT_SCRIPT.to_string()
            }
        );
        assert!(err.advice().contains("repository"), "{}", err.advice());
    }

    /// A second implementation of the port, proving nothing about downloading
    /// is MetaTrader-shaped. It answers from a canned script instead of a
    /// terminal, which is what lets the interface be driven in tests with no
    /// Python, no MetaTrader and no network.
    struct ScriptedSource {
        lines: Vec<String>,
    }

    impl SessionSource for ScriptedSource {
        fn label(&self) -> String {
            "Scripted".to_string()
        }

        fn provider(&self) -> ProviderKind {
            // A second implementer, so the port is exercised by more than the
            // one provider that shipped with it.
            ProviderKind::Hyperliquid
        }

        fn commands(&self, _request: &DownloadRequest) -> Result<Vec<Command>, DownloadError> {
            // Never launched: the test reads `lines` directly. A real second
            // provider would build its own command here.
            Err(DownloadError::NoInterpreter {
                tried: "none".to_string(),
            })
        }
    }

    #[test]
    fn a_second_source_drives_the_same_events() {
        let source = ScriptedSource {
            lines: vec![
                r#"{"event_code":"EXPORT_TAPE_PROGRESS","ticks":1000}"#.to_string(),
                r#"{"event_code":"EXPORT_DONE","tape":"x/y.csv"}"#.to_string(),
            ],
        };
        assert_eq!(source.label(), "Scripted");
        let events: Vec<_> = source.lines.iter().filter_map(|l| parse_event(l)).collect();
        assert_eq!(
            events,
            vec![
                DownloadEvent::Progress { ticks: 1000 },
                DownloadEvent::Done {
                    tape: PathBuf::from("x/y.csv")
                },
            ],
            "the event vocabulary belongs to the port, not to MetaTrader"
        );
    }

    /// An exit code is a fact about the process. What the exporter said is the
    /// fact about the download — and the one the trader can act on.
    #[test]
    fn an_unexplained_exit_carries_the_exporter_s_last_words() {
        let advice = last_words(
            [
                "Traceback (most recent call last):",
                "  File \"export_session.py\", line 490, in export_tape",
                "NameError: name 'flagged' is not defined",
            ]
            .into_iter(),
        )
        .expect("something was said");
        assert!(
            advice.contains("NameError: name 'flagged' is not defined"),
            "the reason has to survive the trip to the panel: {advice}"
        );
    }

    #[test]
    fn an_exit_that_said_nothing_offers_no_empty_box() {
        assert_eq!(last_words(std::iter::empty()), None);
    }

    /// The clock cache is stated by the caller, never read from the machine
    /// inside `arguments`: an argv that changes with the documents folder is
    /// an argv no test can assert.
    #[test]
    fn the_clock_cache_is_passed_when_there_is_one() {
        let args = Mt5SessionSource::arguments(
            Path::new("tools/export.py"),
            &request(),
            Some(Path::new("/docs/Quantick/mt5-clock.json")),
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--clock-cache", "/docs/Quantick/mt5-clock.json"]),
            "the exporter must be told where the measured clock lives: {args:?}"
        );
    }

    /// A platform with no documents folder gets the scripts' own default
    /// rather than an invented path, so the flag is simply absent.
    #[test]
    fn no_durable_home_passes_no_clock_cache() {
        let args = Mt5SessionSource::arguments(Path::new("tools/export.py"), &request(), None);
        assert!(
            !args.iter().any(|arg| arg == "--clock-cache"),
            "nothing to state, so nothing is stated: {args:?}"
        );
    }
}
