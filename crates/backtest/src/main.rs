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
use quantick_backtest::strategies::{EmaCross, ForceRegion, PlotSignal, Protection};
use quantick_backtest::strategy::Strategy;
use quantick_engine::Side;
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

/// Bar rule a run uses when none is named. Tick bars because they are the
/// one rule every instrument supports — a volume default would be wrong on
/// any feed that does not report traded size.
const DEFAULT_BARS: BarSpec = BarSpec::Tick(100);
/// Fast EMA length of the built-in cross.
const DEFAULT_FAST_EMA: usize = 9;
/// Slow EMA length of the built-in cross.
const DEFAULT_SLOW_EMA: usize = 21;
/// Contracts per entry. One, so a run reports points per contract and the
/// reader multiplies by whatever size they actually trade.
const DEFAULT_QUANTITY: Decimal = Decimal::ONE;

/// The strategies this binary can run, by the name `--strategy` takes.
///
/// This is the registration point: a rule implementing
/// [`quantick_backtest::strategy::Strategy`] docks by adding one row here
/// and one arm to [`build_strategy`], with no other file touched. WP-03's
/// operational rules arrive that way.
const STRATEGIES: &[(&str, &str)] = &[
    (
        "ema-cross",
        "fast/slow EMA cross, optionally confirmed by cumulative delta",
    ),
    (
        "script",
        "signal from a compiled .pine plot column (needs --script)",
    ),
    (
        "force-region",
        "the chart's armed-rectangle kernel over a fixed price region (needs --region)",
    ),
];

/// Which strategy a run uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrategyKind {
    EmaCross,
    Script,
    ForceRegion,
}

impl StrategyKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "ema-cross" => Some(Self::EmaCross),
            "script" => Some(Self::Script),
            "force-region" => Some(Self::ForceRegion),
            _ => None,
        }
    }
}

/// `--region low:high:side` — the fixed stand-in for the rectangle a hand
/// draws on the chart. The harness can mechanise the trigger; where the
/// region sits is the human half of the setup, so it must be *given*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RegionSpec {
    low: Decimal,
    high: Decimal,
    side: Side,
}

impl RegionSpec {
    fn parse(text: &str) -> Result<Self, String> {
        let refuse = || {
            format!(
                "--region `{text}` is not low:high:side (side is buy or sell), e.g. \
                 108000:109000:sell"
            )
        };
        let mut parts = text.split(':');
        let (Some(low), Some(high), Some(side), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(refuse());
        };
        let low = positive_decimal("--region", low).map_err(|_| refuse())?;
        let high = positive_decimal("--region", high).map_err(|_| refuse())?;
        let side = match side {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return Err(refuse()),
        };
        Ok(Self { low, high, side })
    }
}

struct Args {
    dir: Option<PathBuf>,
    symbol: Option<String>,
    from: Option<Day>,
    to: Option<Day>,
    bars: BarSpec,
    /// `None` until resolved: an explicit `--strategy` wins, a bare
    /// `--script` implies `script`, and the default is the EMA cross.
    strategy: Option<StrategyKind>,
    fast: usize,
    slow: usize,
    confirm_flow: bool,
    script: Option<PathBuf>,
    plot: usize,
    region: Option<RegionSpec>,
    /// Force-region only: on a bar cutting through the region, rest a limit
    /// at the cut edge (cancelled at the bar's target) instead of holding
    /// fire — the chart's "retest limit" option, measured headless.
    retest_limit: bool,
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
    let strategies: String = STRATEGIES
        .iter()
        .map(|(name, what)| format!("\n                           {name} — {what}"))
        .collect();
    format!(
        "usage: quantick-backtest --dir <replay folder> [options]

  --dir <folder>           sessions to run (default: ${REPLAY_DIR_ENV})
  --symbol <NAME>          only sessions of this instrument
  --from <YYYY-MM-DD>      first session day, inclusive
  --to <YYYY-MM-DD>        last session day, inclusive
  --limit <n>              stop after n sessions
  --bars <kind:parameter>  tick:100 (default), volume:5, dollar:500000,
                           time:1m, imbalance:2500 (also imbalance:volume:2500,
                           imbalance:dollar:2500)
  --strategy <name>        which rule to run (default {default}):{strategies}
  --fast <n> --slow <n>    EMA lengths for the built-in cross \
({DEFAULT_FAST_EMA} / {DEFAULT_SLOW_EMA})
  --confirm-flow           require CVD to agree with the cross
  --script <file.pine>     the script the `script` strategy reads
  --plot <n>               which plot column of that script (default 0)
  --region <low:high:side> the force-region strategy's price band and the
                           side it hunts (buy|sell); auto re-arms so every
                           qualifying bar in the recording is measured
  --retest-limit           force-region: a bar cutting through the region
                           rests a limit at the cut edge, cancelled if the
                           bar's projected target trades first
  --quantity <n>           contracts per entry (default {DEFAULT_QUANTITY})
  --stop <points>          protective stop distance from the entry mark
  --target <points>        protective target distance from the entry mark
  --write-trades <folder>  write one quantick-trades CSV per session
  --help

exit codes: 0 ran, 2 bad arguments, 3 ran but found nothing to run, 1 failed",
        default = STRATEGIES[0].0,
    )
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        dir: None,
        symbol: None,
        from: None,
        to: None,
        bars: DEFAULT_BARS,
        strategy: None,
        fast: DEFAULT_FAST_EMA,
        slow: DEFAULT_SLOW_EMA,
        confirm_flow: false,
        script: None,
        plot: 0,
        region: None,
        retest_limit: false,
        quantity: DEFAULT_QUANTITY,
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
            "--strategy" => {
                let name = value()?;
                args.strategy = Some(StrategyKind::parse(&name).ok_or_else(|| {
                    let known: Vec<&str> = STRATEGIES.iter().map(|(name, _)| *name).collect();
                    format!("--strategy `{name}`; one of {}", known.join(", "))
                })?);
            }
            "--fast" => args.fast = positive_count("--fast", &value()?)?,
            "--slow" => args.slow = positive_count("--slow", &value()?)?,
            "--confirm-flow" => args.confirm_flow = true,
            "--script" => args.script = Some(PathBuf::from(value()?)),
            "--region" => args.region = Some(RegionSpec::parse(&value()?)?),
            "--retest-limit" => args.retest_limit = true,
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
    args.strategy = Some(resolve_strategy(
        args.strategy,
        args.script.is_some(),
        args.region.is_some(),
    )?);

    if args.strategy == Some(StrategyKind::EmaCross) && args.fast >= args.slow {
        return Err(format!(
            "--fast {} must be shorter than --slow {}: a cross of the longer over the \
             shorter is the same signal read backwards",
            args.fast, args.slow
        ));
    }
    if args.retest_limit && args.strategy != Some(StrategyKind::ForceRegion) {
        return Err(
            "--retest-limit belongs to the force-region strategy — pass --region <low:high:side>"
                .to_owned(),
        );
    }
    if let (Some(from), Some(to)) = (args.from, args.to)
        && from > to
    {
        return Err("--from is after --to: the window is empty".to_string());
    }
    Ok(args)
}

/// Settle `--strategy` against `--script`.
///
/// A bare `--script` means the script strategy, because naming a file and
/// then not running it is never what was meant. Every other combination is
/// either explicit or a contradiction, and a contradiction is refused rather
/// than resolved by precedence — silently ignoring one of two flags a person
/// typed on purpose is how a run measures something nobody asked for.
fn resolve_strategy(
    asked: Option<StrategyKind>,
    has_script: bool,
    has_region: bool,
) -> Result<StrategyKind, String> {
    match (asked, has_script, has_region) {
        (None, false, false) => Ok(StrategyKind::EmaCross),
        (None, true, false) => Ok(StrategyKind::Script),
        (None, false, true) => Ok(StrategyKind::ForceRegion),
        (None, true, true) => Err(
            "--script and --region name different strategies; drop one so the run \
             measures the rule you meant"
                .to_string(),
        ),
        (Some(StrategyKind::Script), false, _) => {
            Err("--strategy script needs --script <file.pine> to read a signal from".to_string())
        }
        (Some(StrategyKind::ForceRegion), _, false) => Err(
            "--strategy force-region needs --region <low:high:side>: the region is the \
             human half of the setup and the harness never invents it"
                .to_string(),
        ),
        (Some(StrategyKind::EmaCross), true, _) => Err(
            "--strategy ema-cross ignores --script; drop one of them so the run measures \
             the rule you meant"
                .to_string(),
        ),
        (Some(StrategyKind::EmaCross), _, true) | (Some(StrategyKind::Script), _, true) => Err(
            "--region belongs to the force-region strategy; drop one of the flags so the \
             run measures the rule you meant"
                .to_string(),
        ),
        (Some(StrategyKind::ForceRegion), true, true) => Err(
            "--strategy force-region ignores --script; drop one of them so the run \
             measures the rule you meant"
                .to_string(),
        ),
        (Some(kind), _, _) => Ok(kind),
    }
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
            .flag("open_at_end", outcome.open_at_end.is_some())
            .text(
                "open_side",
                match outcome.open_at_end {
                    Some((Side::Buy, _)) => "long",
                    Some((Side::Sell, _)) => "short",
                    None => "",
                },
            )
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

/// Build the named strategy. One arm per row of [`STRATEGIES`] — this is the
/// whole registration surface a new rule has to touch.
fn build_strategy(args: &Args) -> Result<Box<dyn Strategy>, String> {
    match args.strategy.unwrap_or(StrategyKind::EmaCross) {
        StrategyKind::EmaCross => Ok(Box::new(EmaCross::new(
            args.fast,
            args.slow,
            args.quantity,
            args.protection,
            args.confirm_flow,
        ))),
        StrategyKind::Script => build_script_strategy(args),
        StrategyKind::ForceRegion => {
            let region = args
                .region
                .ok_or("the force-region strategy needs --region <low:high:side>")?;
            if args.protection != Protection::default() {
                return Err(
                    "force-region projects its bracket from the trigger bar's own range; \
                     --stop/--target (fixed points) do not apply to it"
                        .to_owned(),
                );
            }
            Ok(Box::new(ForceRegion::new(
                quantick_strategy::Region::new(region.low, region.high),
                quantick_strategy::StrategyParams {
                    side: region.side,
                    quantity: args.quantity,
                    tp_mult: Decimal::ONE,
                    sl_mult: Decimal::ONE,
                    // The harness measures: every qualifying bar in the
                    // recording counts, so the instance re-arms itself.
                    rearm: quantick_strategy::Rearm::Auto,
                    on_break: if args.retest_limit {
                        quantick_strategy::BreakPolicy::RetestLimit
                    } else {
                        quantick_strategy::BreakPolicy::Ignore
                    },
                },
                quantick_strategy::ForceParams::default_band(),
            )))
        }
    }
}

fn build_script_strategy(args: &Args) -> Result<Box<dyn Strategy>, String> {
    // `resolve_strategy` refuses `script` without a file, so this is
    // unreachable through the CLI; the message covers a future caller.
    let path = args
        .script
        .as_deref()
        .ok_or("the script strategy needs --script <file.pine>")?;
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

/// Claim an unused output path for `stem`, suffixing on collision.
///
/// Two session files can reduce to one name — a real folder holds the same
/// day as both `2026-08-11.csv` and `WINQ26-2026-08-11.csv`. Writing one
/// over the other would lose a whole day's trades and leave a folder that
/// looks complete, so the second one gets `-2` and the caller says so.
fn reserve_path(folder: &Path, stem: &str, taken: &mut BTreeSet<PathBuf>) -> PathBuf {
    let mut path = folder.join(format!("{stem}.{}", history::FILE_EXTENSION));
    let mut attempt = 1u32;
    while taken.contains(&path) {
        attempt += 1;
        path = folder.join(format!("{stem}-{attempt}.{}", history::FILE_EXTENSION));
    }
    taken.insert(path.clone());
    path
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
    let plain = folder.join(format!("{stem}.{}", history::FILE_EXTENSION));
    let path = reserve_path(folder, &stem, written);
    if path != plain {
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

    // `Replay`, always: a backtest's tape is a recording, and the whole
    // point of the field is that practice runs stay out of the real track
    // record. There is no flag to override it, because there is no honest
    // reason for a backtest to claim it traded live.
    let mut text = history::write_header(&run.symbol, history::SessionSource::Replay);
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

    #[test]
    fn strategy_selection_refuses_contradictions_instead_of_picking_one() {
        assert_eq!(
            resolve_strategy(None, false, false),
            Ok(StrategyKind::EmaCross)
        );
        assert_eq!(
            resolve_strategy(None, true, false),
            Ok(StrategyKind::Script),
            "a bare --script means the script strategy"
        );
        assert_eq!(
            resolve_strategy(None, false, true),
            Ok(StrategyKind::ForceRegion),
            "a bare --region means the force-region strategy"
        );
        assert_eq!(
            resolve_strategy(Some(StrategyKind::Script), true, false),
            Ok(StrategyKind::Script)
        );
        // Every one of these would otherwise run a rule the operator did not
        // ask for, and the report would look perfectly ordinary doing it.
        assert!(resolve_strategy(Some(StrategyKind::Script), false, false).is_err());
        assert!(resolve_strategy(Some(StrategyKind::EmaCross), true, false).is_err());
        assert!(resolve_strategy(Some(StrategyKind::ForceRegion), false, false).is_err());
        assert!(resolve_strategy(None, true, true).is_err());
        assert!(resolve_strategy(Some(StrategyKind::EmaCross), false, true).is_err());
        assert!(resolve_strategy(Some(StrategyKind::Script), true, true).is_err());
        assert!(resolve_strategy(Some(StrategyKind::ForceRegion), true, true).is_err());
    }

    #[test]
    fn a_region_spec_parses_or_refuses_whole() {
        let spec = RegionSpec::parse("108000:109000:sell").expect("well-formed");
        assert_eq!(spec.side, Side::Sell);
        assert_eq!(spec.low, Decimal::from(108_000));
        assert_eq!(spec.high, Decimal::from(109_000));
        for broken in [
            "108000:109000",
            "108000:109000:short",
            "a:b:buy",
            "108000:109000:buy:extra",
            "-1:2:buy",
        ] {
            assert!(RegionSpec::parse(broken).is_err(), "{broken}");
        }
    }

    #[test]
    fn every_registered_strategy_name_parses() {
        // The usage text and the parser read the same table; a row added to
        // one and not the other would advertise a name that does not work.
        for (name, _) in STRATEGIES {
            assert!(
                StrategyKind::parse(name).is_some(),
                "`{name}` is advertised in --help but not accepted"
            );
        }
        assert!(StrategyKind::parse("nope").is_none());
        assert!(usage().contains(STRATEGIES[0].0));
        assert!(usage().contains(STRATEGIES[1].0));
    }

    #[test]
    fn two_sessions_that_name_alike_do_not_overwrite_each_other() {
        // A real folder holds the same day twice, once as `2026-08-11.csv`
        // and once as `WINQ26-2026-08-11.csv`. Both reduce to one output
        // name, and losing a whole day's trades to a silent overwrite would
        // leave a folder that looks complete.
        let folder = Path::new("out");
        let mut taken = BTreeSet::new();
        let first = reserve_path(folder, "WINQ26-2026-08-11", &mut taken);
        let second = reserve_path(folder, "WINQ26-2026-08-11", &mut taken);
        let third = reserve_path(folder, "WINQ26-2026-08-11", &mut taken);
        assert_eq!(first, folder.join("WINQ26-2026-08-11.csv"));
        assert_eq!(second, folder.join("WINQ26-2026-08-11-2.csv"));
        assert_eq!(third, folder.join("WINQ26-2026-08-11-3.csv"));
        assert_eq!(taken.len(), 3, "every name was kept");

        let other = reserve_path(folder, "WINQ26-2026-08-12", &mut taken);
        assert_eq!(
            other,
            folder.join("WINQ26-2026-08-12.csv"),
            "an unrelated session keeps its plain name"
        );
    }
}
