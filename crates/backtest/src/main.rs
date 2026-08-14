//! `quantick-backtest` — run a strategy over a folder of recorded sessions.
//!
//! ```text
//! cargo run -p quantick-backtest --release -- \
//!     --dir C:/Users/me/quantick-data --symbol WINQ26 --bars tick:100
//! ```
//!
//! The human report goes to **stdout**; one JSON object per line goes to
//! **stderr**, each with an `event_code`, so `… > report.txt` keeps the
//! report readable while `… 2> run.jsonl | jq` follows the machinery. That
//! split is the NDJSON importer's convention, and it is the only interface
//! contract this binary makes.
//!
//! Arguments are parsed by hand. The workspace has no CLI crate in its
//! dependency tree and cultivates that on purpose; thirty lines of `match`
//! is a smaller price than a permanent dependency.
//!
//! # The one wall clock in the crate
//!
//! `Instant` appears here and nowhere else. Its readings go into the stderr
//! diagnostics — how long a session took, how many prints per second — and
//! never into a report, because a report must be byte-identical across runs.
//! `tests/determinism_guard.rs` enforces that boundary by scanning the
//! sources.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use quantick_backtest::bars::BarSpec;
use quantick_backtest::diagnostics::Event;
use quantick_backtest::report::render_run;
use quantick_backtest::run::{RunOutcome, SessionRun, run_session};
use quantick_backtest::strategies::{EmaCross, PlotSignal, Protection};
use quantick_backtest::strategy::Strategy;
use quantick_replay::format::ParseOptions;
use quantick_replay::library::{self, ProblemKind, SessionEntry};
use quantick_replay::session::{Session, SessionDate};
use quantick_sim::history;
use rust_decimal::Decimal;

/// Where sessions come from when `--dir` is absent. It is the desktop app's
/// variable; the harness honours it as a convenience and lets `--dir` win.
const REPLAY_DIR_ENV: &str = "QUANTICK_REPLAY_DIR";

/// Bad arguments — nothing ran.
const EXIT_BAD_ARGUMENTS: u8 = 2;
/// Ran, but produced no output at all (no session to run).
const EXIT_NOTHING_PRODUCED: u8 = 3;

struct Args {
    dir: Option<PathBuf>,
    symbol: Option<String>,
    from: Option<Day>,
    to: Option<Day>,
    bars: BarSpec,
    fast: usize,
    slow: usize,
    confirm_flow: bool,
    script: Option<PathBuf>,
    plot: usize,
    quantity: Decimal,
    protection: Protection,
    write_trades: Option<PathBuf>,
    limit: Option<usize>,
}

/// A calendar day, for the `--from`/`--to` window. Compared against the day
/// the session library read off the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Day(i32, u32, u32);

impl Day {
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Self(year, month, day))
    }

    fn of(date: SessionDate) -> Self {
        Self(date.year, date.month, date.day)
    }
}

fn usage() -> String {
    format!(
        "usage: quantick-backtest --dir <replay folder> [options]

  --dir <folder>           sessions to run (default: ${REPLAY_DIR_ENV})
  --symbol <NAME>          only sessions of this instrument
  --from <YYYY-MM-DD>      first session day, inclusive
  --to <YYYY-MM-DD>        last session day, inclusive
  --limit <n>              stop after n sessions
  --bars <kind:parameter>  tick:100 (default), volume:5, dollar:500000,
                           time:1m, imbalance:2500
  --fast <n> --slow <n>    EMA lengths for the built-in cross (9 / 21)
  --confirm-flow           require CVD to agree with the cross
  --script <file.pine>     take the signal from a script's plot instead
  --plot <n>               which plot column of that script (default 0)
  --quantity <n>           contracts per entry (default 1)
  --stop <points>          protective stop distance from the entry mark
  --target <points>        protective target distance from the entry mark
  --write-trades <folder>  write one quantick-trades CSV per session
  --help

exit codes: 0 ran, 2 bad arguments, 3 ran but found nothing to run, 1 failed"
    )
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        dir: None,
        symbol: None,
        from: None,
        to: None,
        bars: BarSpec::Tick(100),
        fast: 9,
        slow: 21,
        confirm_flow: false,
        script: None,
        plot: 0,
        quantity: Decimal::ONE,
        protection: Protection::default(),
        write_trades: None,
        limit: None,
    };

    let mut raw = std::env::args().skip(1);
    while let Some(arg) = raw.next() {
        let mut value = || raw.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--dir" => args.dir = Some(PathBuf::from(value()?)),
            "--symbol" => args.symbol = Some(value()?),
            "--from" => {
                let text = value()?;
                args.from =
                    Some(Day::parse(&text).ok_or_else(|| format!("--from `{text}` is not a day"))?);
            }
            "--to" => {
                let text = value()?;
                args.to =
                    Some(Day::parse(&text).ok_or_else(|| format!("--to `{text}` is not a day"))?);
            }
            "--limit" => args.limit = Some(positive_count("--limit", &value()?)?),
            "--bars" => args.bars = BarSpec::parse(&value()?)?,
            "--fast" => args.fast = positive_count("--fast", &value()?)?,
            "--slow" => args.slow = positive_count("--slow", &value()?)?,
            "--confirm-flow" => args.confirm_flow = true,
            "--script" => args.script = Some(PathBuf::from(value()?)),
            "--plot" => {
                let text = value()?;
                args.plot = text
                    .parse()
                    .map_err(|_| format!("--plot `{text}` is not a column index"))?;
            }
            "--quantity" => args.quantity = positive_decimal("--quantity", &value()?)?,
            "--stop" => args.protection.stop_points = Some(positive_decimal("--stop", &value()?)?),
            "--target" => {
                args.protection.target_points = Some(positive_decimal("--target", &value()?)?);
            }
            "--write-trades" => args.write_trades = Some(PathBuf::from(value()?)),
            "--help" | "-h" => return Err("help".to_string()),
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }

    if args.dir.is_none() {
        args.dir = std::env::var_os(REPLAY_DIR_ENV).map(PathBuf::from);
    }
    if args.script.is_none() && args.fast >= args.slow {
        return Err(format!(
            "--fast {} must be shorter than --slow {}: a cross of the longer over the \
             shorter is the same signal read backwards",
            args.fast, args.slow
        ));
    }
    if let (Some(from), Some(to)) = (args.from, args.to)
        && from > to
    {
        return Err("--from is after --to: the window is empty".to_string());
    }
    Ok(args)
}

fn positive_count(flag: &str, text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{flag} `{text}` is not a positive whole number")),
    }
}

fn positive_decimal(flag: &str, text: &str) -> Result<Decimal, String> {
    match text.parse::<Decimal>() {
        Ok(d) if d > Decimal::ZERO => Ok(d),
        _ => Err(format!("{flag} `{text}` is not a positive number")),
    }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if message == "help" {
                eprintln!("{}", usage());
                return ExitCode::SUCCESS;
            }
            Event::new("BACKTEST_BAD_ARGUMENTS")
                .text("detail", &message)
                .text("usage", &usage())
                .emit();
            return ExitCode::from(EXIT_BAD_ARGUMENTS);
        }
    };

    match run(&args) {
        Ok(0) => {
            Event::new("BACKTEST_NOTHING_RUN")
                .text(
                    "fix",
                    "check --dir, --symbol and the --from/--to window; a folder whose \
                     files were rejected reports the reason above",
                )
                .emit();
            ExitCode::from(EXIT_NOTHING_PRODUCED)
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(message) => {
            Event::new("BACKTEST_FAILED")
                .text("detail", &message)
                .emit();
            ExitCode::FAILURE
        }
    }
}

/// Returns how many sessions actually ran.
fn run(args: &Args) -> Result<usize, String> {
    let Some(dir) = args.dir.as_deref() else {
        return Err(format!(
            "no session folder: pass --dir <folder> or set {REPLAY_DIR_ENV}"
        ));
    };

    let mut strategy = build_strategy(args)?;
    let library = library::scan(dir);
    for problem in &library.problems {
        Event::new("BACKTEST_LIBRARY_PROBLEM")
            .text("subject", &problem.subject())
            .text("code", problem.kind.code())
            .text("detail", &problem.detail)
            .text("advice", problem.advice())
            .emit();
    }
    let selected: Vec<&SessionEntry> = library
        .sessions
        .iter()
        .filter(|entry| selects(args, entry))
        .take(args.limit.unwrap_or(usize::MAX))
        .collect();
    Event::new("BACKTEST_LIBRARY_SCANNED")
        .text("root", &dir.display().to_string())
        .count("sessions_found", library.sessions.len() as u64)
        .count("sessions_selected", selected.len() as u64)
        .count("problems", library.problems.len() as u64)
        .text("strategy", strategy.name())
        .text("bars", &args.bars.summary())
        .emit();

    // A folder that cannot be used at all is a failure, not an empty run.
    // `ProblemKind::NoSessions` is fatal to the library and deliberately not
    // included: a folder that exists and holds nothing playable *is* the
    // "ran and produced nothing" case, which has its own exit code.
    if let Some(problem) = library.problems.iter().find(|problem| {
        matches!(
            problem.kind,
            ProblemKind::FolderMissing | ProblemKind::NotAFolder | ProblemKind::FolderUnreadable
        )
    }) {
        return Err(format!(
            "{}: {} — {}",
            problem.kind.code(),
            problem.detail,
            problem.kind.advice()
        ));
    }

    let started = Instant::now();
    let mut runs: Vec<SessionRun> = Vec::with_capacity(selected.len());
    let mut prints_total = 0u64;
    let mut trade_files = BTreeSet::new();
    for entry in selected {
        // One session in memory at a time: a real WIN day is tens of
        // millions of prints, and holding the library would be gigabytes.
        let session = match Session::load(&entry.path, ParseOptions::default()) {
            Ok(session) => session,
            Err(error) => {
                Event::new("BACKTEST_SESSION_FAILED")
                    .text("file", &entry.path.display().to_string())
                    .text("detail", &error.to_string())
                    .text("advice", error.advice())
                    .emit();
                continue;
            }
        };
        let session_started = Instant::now();
        let outcome = run_session(&session, args.bars, strategy.as_mut());
        let elapsed = session_started.elapsed();
        prints_total += outcome.prints as u64;
        drop(session);

        if let Some(folder) = args.write_trades.as_deref() {
            write_trades(folder, &outcome, &mut trade_files)?;
        }
        Event::new("BACKTEST_SESSION_DONE")
            .text("session", &outcome.label)
            .text("file", &outcome.path.display().to_string())
            .count("prints", outcome.prints as u64)
            .count("bars", outcome.bars as u64)
            .count("trades", outcome.report.trades)
            .decimal("net_points", outcome.report.net_points)
            .maybe_decimal("expectancy_points", outcome.report.expectancy_points)
            .maybe_decimal("profit_factor", outcome.report.profit_factor)
            .decimal("max_drawdown_points", outcome.report.max_drawdown_points)
            .count("rejections", outcome.anomalies.rejections())
            .count("brackets_dropped", outcome.anomalies.dropped_brackets())
            .flag("open_at_end", outcome.open_at_end)
            .integer("elapsed_ms", elapsed.as_millis() as i64)
            .integer("prints_per_second", rate(outcome.prints as u64, elapsed))
            .emit();
        runs.push(outcome);
    }

    if runs.is_empty() {
        return Ok(0);
    }
    let elapsed = started.elapsed();
    let outcome = RunOutcome::from_sessions(strategy.name(), args.bars, runs);
    print!("{}", render_run(&outcome));

    Event::new("BACKTEST_DONE")
        .count("sessions", outcome.sessions.len() as u64)
        .count("prints", prints_total)
        .count("trades", outcome.aggregate.trades)
        .decimal("net_points", outcome.aggregate.net_points)
        .maybe_decimal("expectancy_points", outcome.aggregate.expectancy_points)
        .maybe_decimal("profit_factor", outcome.aggregate.profit_factor)
        .count("rejections", outcome.anomalies.rejections())
        .count("brackets_dropped", outcome.anomalies.dropped_brackets())
        .integer("elapsed_ms", elapsed.as_millis() as i64)
        .integer("prints_per_second", rate(prints_total, elapsed))
        .emit();
    Ok(outcome.sessions.len())
}

/// Prints per second, floored. Zero when no time has passed yet — an
/// unmeasurably fast run reports `0`, which is honest, rather than a
/// division by zero.
fn rate(prints: u64, elapsed: std::time::Duration) -> i64 {
    let micros = elapsed.as_micros();
    if micros == 0 {
        return 0;
    }
    i64::try_from(u128::from(prints) * 1_000_000 / micros).unwrap_or(i64::MAX)
}

fn build_strategy(args: &Args) -> Result<Box<dyn Strategy>, String> {
    let Some(path) = args.script.as_deref() else {
        return Ok(Box::new(EmaCross::new(
            args.fast,
            args.slow,
            args.quantity,
            args.protection,
            args.confirm_flow,
        )));
    };
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let name = path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into(),
    );
    match PlotSignal::compile(&name, &source, args.plot, args.quantity, args.protection) {
        Ok(strategy) => Ok(Box::new(strategy)),
        Err(errors) => {
            let file = path.display().to_string();
            for error in &errors {
                Event::new("BACKTEST_SCRIPT_INVALID")
                    .text("file", &file)
                    .text("code", error.code.as_str())
                    .text("detail", &error.render(&file, &source))
                    .emit();
            }
            Err(format!(
                "{file} did not compile ({} error(s) reported above)",
                errors.len()
            ))
        }
    }
}

fn selects(args: &Args, entry: &SessionEntry) -> bool {
    if let Some(symbol) = &args.symbol
        && &entry.symbol != symbol
    {
        return false;
    }
    if args.from.is_none() && args.to.is_none() {
        return true;
    }
    // A window was asked for, so a session whose day is unknown cannot be
    // shown to be inside it. Guessing would be inventing the date.
    let Some(day) = entry.date.map(Day::of) else {
        return false;
    };
    args.from.is_none_or(|from| day >= from) && args.to.is_none_or(|to| day <= to)
}

/// The output file name for a session: its own file name, prefixed with the
/// instrument when that is not already how the recording is named.
///
/// Taken from the source file rather than from `Session::label`, which falls
/// back to the file name when a recording's day cannot be read — sanitising
/// that produced names like `WINQ26-WINQ26-2026-08-03-csv`.
fn output_stem(symbol: &str, path: &Path) -> String {
    let sanitize = |text: &str| -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.ends_with('-') {
                out.push('-');
            }
        }
        out.trim_matches('-').to_owned()
    };
    let symbol = sanitize(symbol);
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| sanitize(&s.to_string_lossy()));
    if stem.is_empty() {
        return symbol;
    }
    if symbol.is_empty() || stem.starts_with(&symbol) {
        stem
    } else {
        format!("{symbol}-{stem}")
    }
}

/// One `quantick-trades` CSV per session, in the format `sim::history`
/// defines — the same file the chart's paper trading writes, so a backtest's
/// output opens wherever a live session's does.
fn write_trades(
    folder: &Path,
    run: &SessionRun,
    written: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if run.trades.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let stem = output_stem(&run.symbol, &run.path);
    // Two session files can label alike (the same day recorded twice under
    // different names). Silently writing one over the other would lose a
    // whole day's trades and leave a folder that looks complete, so a
    // collision gets a suffix and says so.
    let mut path = folder.join(format!("{stem}.{}", history::FILE_EXTENSION));
    let mut attempt = 1u32;
    while written.contains(&path) {
        attempt += 1;
        path = folder.join(format!("{stem}-{attempt}.{}", history::FILE_EXTENSION));
    }
    if attempt > 1 {
        Event::new("BACKTEST_TRADES_NAME_TAKEN")
            .text("session", &run.label)
            .text("file", &path.display().to_string())
            .text(
                "detail",
                "another session in this run wrote the same file name; this one was \
                 suffixed rather than overwriting it",
            )
            .emit();
    }
    written.insert(path.clone());

    let mut text = history::write_header(&run.symbol);
    for trade in &run.trades {
        text.push_str(&history::write_trade(trade));
    }
    std::fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Event::new("BACKTEST_TRADES_WRITTEN")
        .text("file", &path.display().to_string())
        .count("trades", run.trades.len() as u64)
        .emit();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_name_carries_the_instrument_exactly_once() {
        // Both layouts exist in a real folder: `WINQ26/2026-08-11.csv` from
        // the exporter, and `WINQ26/WINQ26-2026-08-11.csv` from an older
        // one. They must produce the same readable name, which is also why
        // the collision suffix in `write_trades` exists.
        assert_eq!(
            output_stem("WINQ26", Path::new("C:/data/WINQ26/2026-08-11.csv")),
            "WINQ26-2026-08-11"
        );
        assert_eq!(
            output_stem("WINQ26", Path::new("C:/data/WINQ26/WINQ26-2026-08-11.csv")),
            "WINQ26-2026-08-11"
        );
        // `WDO$` is a real B3 symbol; the `$` must not reach the file name.
        assert_eq!(
            output_stem("WDO$", Path::new("C:/data/WDO$/2026-08-11.csv")),
            "WDO-2026-08-11"
        );
        // Degenerate names stay ugly but never come out empty, which is the
        // only property that matters: an empty stem would write over the
        // folder's own `.csv`.
        assert!(!output_stem("WINQ26", Path::new("C:/data/x/.csv")).is_empty());
        assert!(!output_stem("", Path::new("C:/data/x/2026-08-11.csv")).is_empty());
        assert_eq!(output_stem("WINQ26", Path::new("")), "WINQ26");
    }

    #[test]
    fn a_day_window_parses_and_rejects() {
        assert_eq!(Day::parse("2026-08-11"), Some(Day(2026, 8, 11)));
        assert!(Day::parse("2026-13-01").is_none(), "no month 13");
        assert!(Day::parse("2026-08").is_none(), "a day is required");
        assert!(Day::parse("2026-08-11-01").is_none(), "no fourth field");
        assert!(Day::parse("").is_none());
        assert!(Day(2026, 8, 10) < Day(2026, 8, 11), "days order");
        assert!(Day(2025, 12, 31) < Day(2026, 1, 1), "across a year");
    }

    #[test]
    fn a_rate_over_no_elapsed_time_is_zero_not_a_division_by_zero() {
        assert_eq!(rate(1_000, std::time::Duration::ZERO), 0);
        assert_eq!(rate(1_000, std::time::Duration::from_secs(1)), 1_000);
        assert_eq!(rate(500, std::time::Duration::from_millis(250)), 2_000);
    }
}
