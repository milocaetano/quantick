//! Convert recorded MT5 order-flow NDJSON into quantick replay sessions.
//!
//! The recorders used around the desk write one JSON object per line, mixing
//! `tick` and `book_update` events. This turns the trade prints into the folder
//! layout the chart's Market Replay reads:
//!
//! ```text
//! cargo run -p quantick-replay --example import_mt5_ndjson -- \
//!     --in F:/src/milostrategies/runtime/mt5_orderflow_20260316.ndjson \
//!     --out F:/replay
//! ```
//!
//! Options:
//!
//! ```text
//! --in <file>              recording to read; repeat for several
//! --out <folder>           replay folder to write into (created if absent)
//! --symbol <name>          keep only this instrument
//! --side-source <policy>   bid_ask (default) | flags | tick_rule
//! --spread-within-second   even out prints inside each recorded second
//! --timezone <offset>      override the offset the rows are written in
//! ```
//!
//! # What it does with an inferred aggressor
//!
//! MT5 tick flags are broker-dependent, so the default policy reads the side
//! from the print's position against the recorded bid/ask and falls back to the
//! tick rule when there is no quote. Whichever policy runs is written into the
//! session's `# side_source=` line, and the run prints how often each rule
//! decided — including how often it disagreed with the broker's own flags.
//!
//! # About the one-second timestamps
//!
//! These recordings stamp trades to the second. Replay therefore delivers a
//! whole second of prints at once, which is honest but visibly stepped.
//! `--spread-within-second` distributes the prints of each second evenly across
//! it for smoother playback; because those sub-second times are made up, the
//! session is marked `# time_resolution=1s_interpolated` and the chart says so.
//!
//! Every diagnostic line is a single JSON object with an `event_code`, so a
//! person with `jq` — or an AI reading the log — can follow the run without
//! scraping prose.

use std::collections::BTreeMap;
use std::io::{BufRead as _, BufWriter, Write as _};
use std::path::{Path, PathBuf};
use std::str::FromStr as _;

use quantick_engine::{Side, Trade};
use quantick_replay::format::{self, Quote, UtcOffset, WriteHeader};
use rust_decimal::Decimal;
use serde_json::Value;

/// MQL5 tick flag: the print carries a `last` price (i.e. it is a trade).
const TICK_FLAG_LAST: u64 = 8;
/// MQL5 tick flag: a buyer-initiated trade, per the broker.
const TICK_FLAG_BUY: u64 = 32;
/// MQL5 tick flag: a seller-initiated trade, per the broker.
const TICK_FLAG_SELL: u64 = 64;

/// How the aggressor side is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidePolicy {
    /// Compare the print against the recorded bid/ask; tick rule as fallback.
    BidAsk,
    /// Trust the broker's BUY/SELL tick flags.
    Flags,
    /// Uptick is a buy, downtick a sell, unchanged repeats the last decision.
    TickRule,
}

impl SidePolicy {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "bid_ask" => Some(SidePolicy::BidAsk),
            "flags" => Some(SidePolicy::Flags),
            "tick_rule" => Some(SidePolicy::TickRule),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SidePolicy::BidAsk => "bid_ask",
            SidePolicy::Flags => "flags",
            SidePolicy::TickRule => "tick_rule",
        }
    }
}

/// Emit one structured diagnostic line to stderr.
macro_rules! emit {
    ($code:expr $(, $key:ident = $value:expr)* $(,)?) => {{
        let mut fields = serde_json::Map::new();
        fields.insert("event_code".to_string(), Value::from($code));
        $( fields.insert(stringify!($key).to_string(), Value::from($value)); )*
        eprintln!("{}", Value::Object(fields));
    }};
}

/// One converted print, before it is written out.
struct Row {
    timestamp_ms: i64,
    price: Decimal,
    volume: Decimal,
    side: Side,
    quote: Option<Quote>,
}

/// The prints of one instrument-day, with the recordings they came from. The
/// provenance is per session, not per run: a session must name the file it was
/// actually built from, even when one command converts several recordings.
#[derive(Default)]
struct SessionRows {
    rows: Vec<Row>,
    sources: Vec<String>,
}

impl SessionRows {
    fn note_source(&mut self, source: &str) {
        if !self.sources.iter().any(|s| s == source) {
            self.sources.push(source.to_string());
        }
    }
}

/// Counters worth reporting honestly at the end of a run.
#[derive(Default)]
struct Stats {
    lines: u64,
    ticks: u64,
    trades: u64,
    skipped_not_a_trade: u64,
    skipped_other_symbol: u64,
    skipped_unparseable: u64,
    side_from_quote: u64,
    side_from_flags: u64,
    side_from_tick_rule: u64,
    side_unknown_dropped: u64,
    flags_disagreed: u64,
}

struct Args {
    inputs: Vec<PathBuf>,
    out: PathBuf,
    symbol: Option<String>,
    policy: SidePolicy,
    spread_within_second: bool,
    timezone: Option<UtcOffset>,
}

fn parse_args() -> Result<Args, String> {
    let mut inputs = Vec::new();
    let mut out = None;
    let mut symbol = None;
    let mut policy = SidePolicy::BidAsk;
    let mut spread_within_second = false;
    let mut timezone = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--in" => inputs.push(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--symbol" => symbol = Some(value()?),
            "--side-source" => {
                let text = value()?;
                policy = SidePolicy::parse(&text).ok_or_else(|| {
                    format!("--side-source `{text}` (expected bid_ask, flags or tick_rule)")
                })?;
            }
            "--spread-within-second" => spread_within_second = true,
            "--timezone" => {
                let text = value()?;
                timezone = Some(
                    UtcOffset::parse(&text)
                        .ok_or_else(|| format!("--timezone `{text}` is not a UTC offset"))?,
                );
            }
            "--help" | "-h" => return Err("help".to_string()),
            other => return Err(format!("unexpected argument `{other}`")),
        }
    }

    if inputs.is_empty() {
        return Err("--in <recording.ndjson> is required".to_string());
    }
    Ok(Args {
        inputs,
        out: out.ok_or("--out <replay folder> is required")?,
        symbol,
        policy,
        spread_within_second,
        timezone,
    })
}

fn main() -> std::process::ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            if message == "help" {
                eprintln!("{}", usage());
            } else {
                emit!("IMPORT_BAD_ARGUMENTS", detail = message, usage = usage());
            }
            return std::process::ExitCode::from(2);
        }
    };

    match run(&args) {
        Ok(0) => {
            emit!(
                "IMPORT_NOTHING_WRITTEN",
                fix = "check --symbol, or whether the recording holds trade ticks at all",
            );
            std::process::ExitCode::from(3)
        }
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            emit!("IMPORT_FAILED", detail = message);
            std::process::ExitCode::FAILURE
        }
    }
}

fn usage() -> String {
    "usage: import_mt5_ndjson --in <recording.ndjson> --out <replay folder> \
     [--symbol NAME] [--side-source bid_ask|flags|tick_rule] [--spread-within-second] \
     [--timezone -03:00]"
        .to_string()
}

fn run(args: &Args) -> Result<usize, String> {
    // Grouped by instrument, then by session day, so one recording spanning a
    // rollover still produces one file per day.
    let mut sessions: BTreeMap<(String, i64), SessionRows> = BTreeMap::new();
    let mut stats = Stats::default();
    let mut offset = args.timezone;

    for input in &args.inputs {
        let file = std::fs::File::open(input)
            .map_err(|e| format!("cannot read {}: {e}", input.display()))?;
        let source: String = input.file_name().map_or_else(
            || input.display().to_string(),
            |n| n.to_string_lossy().into(),
        );
        let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
        let mut last_price: BTreeMap<String, Decimal> = BTreeMap::new();
        let mut last_side: BTreeMap<String, Side> = BTreeMap::new();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => return Err(format!("cannot read {}: {e}", input.display())),
            }
            stats.lines += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
                stats.skipped_unparseable += 1;
                continue;
            };
            if event.get("event_type").and_then(Value::as_str) != Some("tick") {
                continue;
            }
            stats.ticks += 1;

            let symbol = event
                .get("symbol")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            if let Some(wanted) = &args.symbol
                && &symbol != wanted
            {
                stats.skipped_other_symbol += 1;
                continue;
            }

            let flags = event.get("flags").and_then(Value::as_u64).unwrap_or(0);
            let volume = number(event.get("volume"))
                .or_else(|| number(event.get("volume_real")))
                .unwrap_or_default();
            let Some(price) = number(event.get("last")) else {
                stats.skipped_not_a_trade += 1;
                continue;
            };
            // A trade print carries a last price and a size. Quote-only ticks
            // (bid/ask moves) are not prints and must not become candles.
            if flags & TICK_FLAG_LAST == 0 || volume <= Decimal::ZERO || price <= Decimal::ZERO {
                stats.skipped_not_a_trade += 1;
                continue;
            }

            let Some((timestamp_ms, row_offset)) = event
                .get("time")
                .and_then(Value::as_str)
                .and_then(parse_iso)
            else {
                stats.skipped_unparseable += 1;
                continue;
            };
            let offset = *offset.get_or_insert(row_offset);

            let bid = number(event.get("bid")).filter(|v| *v > Decimal::ZERO);
            let ask = number(event.get("ask")).filter(|v| *v > Decimal::ZERO);
            let quote = match (bid, ask) {
                (Some(bid), Some(ask)) if ask >= bid => Some(Quote { bid, ask }),
                _ => None,
            };

            let previous = last_price.get(&symbol).copied();
            let carried = last_side.get(&symbol).copied();
            let Some((side, decided_by)) =
                decide_side(args.policy, price, quote, flags, previous, carried)
            else {
                stats.side_unknown_dropped += 1;
                continue;
            };
            match decided_by {
                SidePolicy::BidAsk => stats.side_from_quote += 1,
                SidePolicy::Flags => stats.side_from_flags += 1,
                SidePolicy::TickRule => stats.side_from_tick_rule += 1,
            }
            if let Some(flagged) = side_from_flags(flags)
                && flagged != side
            {
                stats.flags_disagreed += 1;
            }
            last_price.insert(symbol.clone(), price);
            last_side.insert(symbol.clone(), side);

            let day = (timestamp_ms.saturating_add(offset.millis())).div_euclid(86_400_000);
            stats.trades += 1;
            let session = sessions.entry((symbol, day)).or_default();
            session.note_source(&source);
            session.rows.push(Row {
                timestamp_ms,
                price,
                volume,
                side,
                quote,
            });
        }

        emit!(
            "IMPORT_FILE_READ",
            file = input.display().to_string(),
            lines = stats.lines,
            ticks = stats.ticks,
            trades = stats.trades,
        );
    }

    let offset = offset.unwrap_or(UtcOffset::UTC);
    let mut written = 0;

    for ((symbol, day), mut session) in sessions {
        // Stable sort: prints stamped to the same second keep the order the
        // recorder saw them in.
        session.rows.sort_by_key(|row| row.timestamp_ms);
        if args.spread_within_second {
            spread_within_second(&mut session.rows);
        }
        let path = session_path(&args.out, &symbol, day, offset);
        let count = session.rows.len();
        write_session(
            &path,
            &symbol,
            offset,
            args.policy,
            args.spread_within_second,
            &session.sources.join(", "),
            &session.rows,
        )
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        written += 1;
        emit!(
            "IMPORT_SESSION_WRITTEN",
            file = path.display().to_string(),
            symbol = symbol,
            trades = count as u64,
            first_ms = session.rows.first().map_or(0, |r| r.timestamp_ms),
            last_ms = session.rows.last().map_or(0, |r| r.timestamp_ms),
        );
    }

    emit!(
        "IMPORT_DONE",
        sessions_written = written as u64,
        trades = stats.trades,
        ticks = stats.ticks,
        skipped_not_a_trade = stats.skipped_not_a_trade,
        skipped_other_symbol = stats.skipped_other_symbol,
        skipped_unparseable = stats.skipped_unparseable,
        side_source = args.policy.label(),
        side_from_quote = stats.side_from_quote,
        side_from_flags = stats.side_from_flags,
        side_from_tick_rule = stats.side_from_tick_rule,
        side_unknown_dropped = stats.side_unknown_dropped,
        flags_disagreed = stats.flags_disagreed,
        time_resolution = if args.spread_within_second {
            "1s_interpolated"
        } else {
            "1s"
        },
    );
    if !args.spread_within_second && written > 0 {
        emit!(
            "IMPORT_TIME_RESOLUTION_NOTE",
            detail = "the recording stamps trades to the second, so replay delivers a second of prints at a time",
            fix = "re-run with --spread-within-second for smoother playback (sub-second times are then interpolated and labelled as such)",
        );
    }
    Ok(written)
}

/// Decide the aggressor side, returning which rule actually decided it.
fn decide_side(
    policy: SidePolicy,
    price: Decimal,
    quote: Option<Quote>,
    flags: u64,
    previous_price: Option<Decimal>,
    carried_side: Option<Side>,
) -> Option<(Side, SidePolicy)> {
    match policy {
        SidePolicy::Flags => side_from_flags(flags)
            .map(|side| (side, SidePolicy::Flags))
            .or_else(|| {
                side_from_tick_rule(price, previous_price, carried_side)
                    .map(|side| (side, SidePolicy::TickRule))
            }),
        SidePolicy::BidAsk => quote
            .and_then(|quote| {
                if price >= quote.ask {
                    Some(Side::Buy)
                } else if price <= quote.bid {
                    Some(Side::Sell)
                } else {
                    None
                }
            })
            .map(|side| (side, SidePolicy::BidAsk))
            .or_else(|| {
                side_from_tick_rule(price, previous_price, carried_side)
                    .map(|side| (side, SidePolicy::TickRule))
            }),
        SidePolicy::TickRule => side_from_tick_rule(price, previous_price, carried_side)
            .map(|side| (side, SidePolicy::TickRule)),
    }
}

fn side_from_flags(flags: u64) -> Option<Side> {
    match (flags & TICK_FLAG_BUY != 0, flags & TICK_FLAG_SELL != 0) {
        (true, false) => Some(Side::Buy),
        (false, true) => Some(Side::Sell),
        // Both bits or neither says nothing; guessing would invent order flow.
        _ => None,
    }
}

fn side_from_tick_rule(
    price: Decimal,
    previous: Option<Decimal>,
    carried: Option<Side>,
) -> Option<Side> {
    match previous {
        Some(previous) if price > previous => Some(Side::Buy),
        Some(previous) if price < previous => Some(Side::Sell),
        Some(_) => carried,
        // The very first print of a session has nothing to compare against.
        None => None,
    }
}

/// Spread the prints of each recorded second evenly across it, so a chart fed
/// from second-resolution data still moves smoothly.
fn spread_within_second(rows: &mut [Row]) {
    let mut start = 0;
    while start < rows.len() {
        let second = rows[start].timestamp_ms.div_euclid(1000);
        let mut end = start + 1;
        while end < rows.len() && rows[end].timestamp_ms.div_euclid(1000) == second {
            end += 1;
        }
        let count = end - start;
        if count > 1 {
            let base = second * 1000;
            for (offset, row) in rows[start..end].iter_mut().enumerate() {
                row.timestamp_ms = base + (offset as i64 * 1000) / count as i64;
            }
        }
        start = end;
    }
}

/// `<out>/<SYMBOL>/<YYYYMMDD>.csv` for a day index in the session's timezone.
fn session_path(out: &Path, symbol: &str, day: i64, offset: UtcOffset) -> PathBuf {
    let civil = format::civil_parts(day * 86_400_000 - offset.millis(), offset);
    out.join(sanitize(symbol)).join(format!(
        "{:04}{:02}{:02}.csv",
        civil.year, civil.month, civil.day
    ))
}

/// Keep instrument names usable as folder names (`WDO$` is a real B3 symbol).
fn sanitize(symbol: &str) -> String {
    symbol
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '$' | '#') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_session(
    path: &Path,
    symbol: &str,
    offset: UtcOffset,
    policy: SidePolicy,
    interpolated: bool,
    source: &str,
    rows: &[Row],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut out = BufWriter::with_capacity(1 << 20, file);

    let mut header = format::write_header(&WriteHeader {
        symbol: symbol.to_string(),
        timezone: offset,
        side_source: policy.label().to_string(),
        source: Some(source.to_string()),
    });
    // The resolution line goes above the column header, where the reader keeps
    // metadata; splice it in rather than re-implementing the writer.
    let resolution = if interpolated {
        "1s_interpolated"
    } else {
        "1s"
    };
    header = header.replace(
        format::CANONICAL_HEADER,
        &format!(
            "# time_resolution={resolution}\n{}",
            format::CANONICAL_HEADER
        ),
    );
    out.write_all(header.as_bytes())?;

    let mut buffer = String::with_capacity(64 * 1024);
    for (index, row) in rows.iter().enumerate() {
        format::write_trade(
            &mut buffer,
            &Trade {
                agg_id: index as u64 + 1,
                timestamp_ms: row.timestamp_ms,
                price: row.price,
                quantity: row.volume,
                side: row.side,
            },
            row.quote,
            offset,
        );
        if buffer.len() >= 32 * 1024 {
            out.write_all(buffer.as_bytes())?;
            buffer.clear();
        }
    }
    out.write_all(buffer.as_bytes())?;
    out.flush()
}

/// Read a JSON number as an exact decimal, keeping the digits the recorder
/// wrote instead of round-tripping through a float twice.
fn number(value: Option<&Value>) -> Option<Decimal> {
    let value = value?;
    // Trailing zeros carry no information here and would show up in every
    // written row ("182035.0"), so the digits are normalised once on the way in.
    if let Some(text) = value.as_str() {
        return Decimal::from_str(text).ok().map(|d| d.normalize());
    }
    Decimal::from_str(&value.as_number()?.to_string())
        .ok()
        .map(|d| d.normalize())
}

/// Parse `2026-03-16T10:01:08-03:00` (optionally with fractional seconds) into
/// an epoch-millisecond instant plus the offset it was written in.
fn parse_iso(text: &str) -> Option<(i64, UtcOffset)> {
    let (date, rest) = text.split_once('T')?;
    let year: i64 = date.get(0..4)?.parse().ok()?;
    let month: u32 = date.get(5..7)?.parse().ok()?;
    let day: u32 = date.get(8..10)?.parse().ok()?;

    // The offset is what follows the clock: `Z`, `+HH:MM` or `-HH:MM`.
    let split = rest
        .rfind(['+', '-'])
        .filter(|at| *at >= 8)
        .or_else(|| rest.find(['Z', 'z']));
    let (clock, zone) = match split {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let offset = if zone.is_empty() {
        UtcOffset::UTC
    } else {
        UtcOffset::parse(zone)?
    };

    let hour: u32 = clock.get(0..2)?.parse().ok()?;
    let minute: u32 = clock.get(3..5)?.parse().ok()?;
    let second: u32 = clock.get(6..8)?.parse().ok()?;
    let milli = match clock.get(9..) {
        Some(fraction) if !fraction.is_empty() => {
            let digits = &fraction[..fraction.len().min(3)];
            let scale = 10u32.pow(3 - digits.len() as u32);
            digits.parse::<u32>().ok()? * scale
        }
        _ => 0,
    };
    if month > 12 || day > 31 || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let civil = format::CivilTime {
        year,
        month,
        day,
        hour,
        minute,
        second: second.min(59),
        milli,
    };
    Some((civil.epoch_millis(offset), offset))
}
