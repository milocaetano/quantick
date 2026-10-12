//! The trade-history file format: closed trades as an append-friendly CSV.
//!
//! Strings in, strings out — this module never touches the filesystem; the
//! app decides where files live and when to write them (the same division
//! `replay::format` uses). Each data row is self-contained, so a journal
//! can be appended one trade at a time and a torn final line (a crash
//! mid-append) costs one row, which the parser *reports* instead of
//! silently dropping.
//!
//! ```text
//! # quantick-trades 2
//! # symbol=BTCUSDT
//! # source=live
//! opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason,entry_agg_id,exit_agg_id,mae_points,mfe_points
//! 1700000000000,1700000060000,long,2,100.5,103.25,5.5,take_profit,41,57,1.5,6.0
//! ```
//!
//! `# source=` records where the whole session's trades came from (a
//! session is wholly live or wholly a replay). Files from before the line
//! existed parse with [`TradeHistory::source`] `None` — unrecorded, not
//! guessed.
//!
//! `side` is the position's direction (`long`/`short`); `exit_reason` uses
//! [`ExitReason::as_str`] tokens. Timestamps are venue epoch milliseconds;
//! prices and quantities are exact decimals; `pnl_points` is points, not
//! currency. `entry_agg_id`/`exit_agg_id` are the aggregate ids of the
//! prints that opened and closed the trade — the audit trail back to the
//! tape — and `mae_points`/`mfe_points` are the worst-adverse and
//! best-favorable excursions (see [`ClosedTrade`]).
//!
//! Version 1 files (the first 8 columns only) still load: the missing
//! fields come back as `None`, and writing such a row emits empty fields —
//! unknown is not zero.
//!
//! The trader's own words about a session ride the same file as append-only
//! comment lines, so the CSV columns never change and a reader that skips
//! comments still reads every trade. The last line of each kind wins:
//!
//! ```text
//! # name=Morning fade practice
//! # tags=1700000000000:1700000060000 #fade #msc
//! # tags=1700000000000:1700000060000:1 #scale-out
//! ```
//!
//! A tags line names its trade by opening and closing times; a third field
//! counts the earlier trades of the file with the same two (see
//! [`TradeRef`]) and is left out when there are none. An empty `# name=`
//! clears the name; a `# tags=` line naming a trade and no tags clears that
//! trade's tags.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

use quantick_trading::{ClosedTrade, ExitReason};

/// Magic token on the first line, naming the format.
pub const FORMAT_NAME: &str = "quantick-trades";
/// Version written. Version 1 is still read; anything newer than
/// [`FORMAT_VERSION`] is an error, never a guess.
pub const FORMAT_VERSION: u32 = 2;
/// Extension the app uses for history files.
pub const FILE_EXTENSION: &str = "csv";
/// The version-2 column header, fixed order. The first eight columns are
/// exactly the version-1 header, so a version bump only ever appends.
pub const HEADER: &str = "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason,entry_agg_id,exit_agg_id,mae_points,mfe_points";
/// The version-1 column header, still accepted on read.
const HEADER_V1: &str =
    "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason";
/// Longest session name kept, in characters; longer text is cut, never
/// refused, because a name is a note and not an identifier.
pub const MAX_SESSION_NAME_CHARS: usize = 80;
/// Longest tag, in characters, without its `#`.
pub const MAX_TAG_CHARS: usize = 32;
/// Most tags one trade carries.
pub const MAX_TAGS_PER_TRADE: usize = 16;

/// Which trade of one session file a `# tags=` line speaks for: its opening
/// and closing venue times — the pair the ledger shows for every row — and
/// how many earlier trades of the same file share both. The partial closes
/// of one position, or two fills in one millisecond, close trades with equal
/// times; `nth` tells them apart and is 0 for every trade whose times are its
/// own. Trade rows are only ever appended, so a trade's `nth` never changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TradeRef {
    pub opened_ms: i64,
    pub closed_ms: i64,
    pub nth: u32,
}

/// Hands out each trade's [`TradeRef`] in the order its file holds them.
/// The reader and the account appending to a file count with this one rule.
#[derive(Debug, Default)]
pub struct TradeRefs(BTreeMap<(i64, i64), u32>);

impl TradeRefs {
    /// The counter after `trades`, a file's parsed rows in file order: where
    /// the next trade appended to that file is counted from.
    #[must_use]
    pub fn after<'a>(trades: impl IntoIterator<Item = &'a ClosedTrade>) -> Self {
        let mut refs = Self::default();
        for trade in trades {
            refs.next(trade);
        }
        refs
    }

    /// The next trade's reference: its times, and how many trades handed
    /// out before it shared them.
    pub fn next(&mut self, trade: &ClosedTrade) -> TradeRef {
        let seen = self
            .0
            .entry((trade.opened_ms, trade.closed_ms))
            .or_default();
        let nth = *seen;
        *seen = seen.saturating_add(1);
        TradeRef {
            opened_ms: trade.opened_ms,
            closed_ms: trade.closed_ms,
            nth,
        }
    }
}

/// Where a session's trades came from — recorded once per file, so the
/// report can keep practice runs out of the real track record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSource {
    /// A live venue feed was driving the tape.
    Live,
    /// A recorded session was driving the tape (market replay).
    Replay,
}

impl SessionSource {
    /// The token the `# source=` header line carries.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Replay => "replay",
        }
    }

    /// The inverse of [`Self::as_str`]; unknown tokens are refused.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "live" => Some(Self::Live),
            "replay" => Some(Self::Replay),
            _ => None,
        }
    }
}

/// A parsed history file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeHistory {
    /// Symbol from the `# symbol=` comment, when present.
    pub symbol: Option<String>,
    /// Session source from the `# source=` comment; `None` for files from
    /// before the line existed (unrecorded, never guessed).
    pub source: Option<SessionSource>,
    /// The session's name from the last `# name=` line; `None` when it was
    /// never named or the last line cleared it.
    pub name: Option<String>,
    /// Each tagged trade's tags from the last `# tags=` line naming it.
    /// Trades never tagged are absent.
    pub tags: BTreeMap<TradeRef, Vec<String>>,
    /// Rows that parsed, in file order (which is closing order).
    pub trades: Vec<ClosedTrade>,
    /// Rows that did not parse, reported instead of silently dropped.
    pub problems: Vec<HistoryProblem>,
}

impl TradeHistory {
    /// Each trade's [`TradeRef`], index-aligned with [`Self::trades`].
    #[must_use]
    pub fn refs(&self) -> Vec<TradeRef> {
        let mut refs = TradeRefs::default();
        self.trades.iter().map(|trade| refs.next(trade)).collect()
    }
}

/// One unreadable row: where and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryProblem {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

/// The file is not a readable quantick-trades file at all (wrong magic,
/// unknown version, wrong header) — as opposed to a file with bad rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryError {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// The lines that open a new history file: magic, symbol, session source,
/// column header.
#[must_use]
pub fn write_header(symbol: &str, source: SessionSource) -> String {
    format!(
        "# {FORMAT_NAME} {FORMAT_VERSION}\n# symbol={symbol}\n# source={}\n{HEADER}\n",
        source.as_str()
    )
}

/// One closed trade as one CSV line (newline included), the exact inverse
/// of what [`parse`] reads. Fields the trade does not know (a row loaded
/// from a version-1 file) are written empty, never invented.
#[must_use]
pub fn write_trade(trade: &ClosedTrade) -> String {
    let side = match trade.side {
        Side::Buy => "long",
        Side::Sell => "short",
    };
    format!(
        "{},{},{},{},{},{},{},{},{},{},{},{}\n",
        trade.opened_ms,
        trade.closed_ms,
        side,
        trade.quantity,
        trade.entry_price,
        trade.exit_price,
        trade.pnl_points,
        trade.exit_reason.as_str(),
        opt_u64(trade.entry_agg_id),
        opt_u64(trade.exit_agg_id),
        opt_decimal(trade.mae_points),
        opt_decimal(trade.mfe_points),
    )
}

/// The line that names a session (newline included). The name is cleaned
/// with [`clean_session_name`]; an empty one writes the clearing line.
#[must_use]
pub fn write_name(name: &str) -> String {
    format!("# name={}\n", clean_session_name(name))
}

/// The line that sets one trade's tags (newline included). Tags are taken
/// as [`normalize_tag`] leaves them; none writes the clearing line.
#[must_use]
pub fn write_tags(trade: TradeRef, tags: &[String]) -> String {
    let mut line = format!("# tags={}:{}", trade.opened_ms, trade.closed_ms);
    if trade.nth > 0 {
        line.push_str(&format!(":{}", trade.nth));
    }
    for tag in tags {
        line.push_str(" #");
        line.push_str(tag);
    }
    line.push('\n');
    line
}

/// A session name as the file keeps it: one line, trimmed, at most
/// [`MAX_SESSION_NAME_CHARS`] characters.
#[must_use]
pub fn clean_session_name(raw: &str) -> String {
    let one_line: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    one_line
        .trim()
        .chars()
        .take(MAX_SESSION_NAME_CHARS)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// One tag as the file keeps it: leading `#`s dropped, lowercase, at most
/// [`MAX_TAG_CHARS`] characters. `None` for text that cannot be one tag —
/// empty, too long, or holding whitespace, a `#` or a comma.
#[must_use]
pub fn normalize_tag(raw: &str) -> Option<String> {
    let bare = raw.trim().trim_start_matches('#');
    let valid = !bare.is_empty()
        && bare.chars().count() <= MAX_TAG_CHARS
        && !bare
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '#' || c == ',');
    valid.then(|| bare.to_lowercase())
}

/// The tags of one trade, normalized, in the order given with repeats
/// dropped. `Err` names the first token that is not a tag, or says there
/// are more than [`MAX_TAGS_PER_TRADE`].
///
/// # Errors
///
/// A token [`normalize_tag`] refuses, or too many tags.
pub fn normalize_tags<'a>(raw: impl IntoIterator<Item = &'a str>) -> Result<Vec<String>, String> {
    let mut tags: Vec<String> = Vec::new();
    for token in raw {
        let tag = normalize_tag(token).ok_or_else(|| {
            format!(
                "`{token}` is not a tag - one word of at most {MAX_TAG_CHARS} characters, no spaces, `#` or commas"
            )
        })?;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    if tags.len() > MAX_TAGS_PER_TRADE {
        return Err(format!(
            "{} tags - a trade carries at most {MAX_TAGS_PER_TRADE}",
            tags.len()
        ));
    }
    Ok(tags)
}

/// `<opened_ms>:<closed_ms>[:<nth>] #a #b` — the body of a `# tags=` line.
fn parse_tags_line(body: &str) -> Result<(TradeRef, Vec<String>), String> {
    let mut tokens = body.split_whitespace();
    let times = tokens.next().unwrap_or_default();
    let mut fields = times.split(':');
    let trade = (|| {
        let opened_ms = fields.next()?.parse().ok()?;
        let closed_ms = fields.next()?.parse().ok()?;
        let nth = fields.next().map_or(Some(0), |nth| nth.parse().ok())?;
        fields.next().is_none().then_some(TradeRef {
            opened_ms,
            closed_ms,
            nth,
        })
    })()
    .ok_or_else(|| {
        format!("tags line must start `<opened_ms>:<closed_ms>[:<nth>]`, found `{times}`")
    })?;
    Ok((trade, normalize_tags(tokens)?))
}

fn opt_u64(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn opt_decimal(value: Option<Decimal>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

/// Read a history file back (version 1 or 2). Fatal errors are reserved for
/// "this is not a quantick-trades file"; a bad *row* becomes a
/// [`HistoryProblem`] and the rest of the file still loads.
pub fn parse(text: &str) -> Result<TradeHistory, HistoryError> {
    let mut lines = text.lines().enumerate();

    // The magic line is the first non-blank line, comment or not.
    let (magic_line, magic) = lines
        .by_ref()
        .find(|(_, line)| !line.trim().is_empty())
        .ok_or(HistoryError {
            line: 1,
            message: format!(
                "empty file - expected `# {FORMAT_NAME} {FORMAT_VERSION}` on the first line"
            ),
        })?;
    let version = parse_magic(magic.trim()).ok_or(HistoryError {
        line: magic_line + 1,
        message: format!(
            "not a {FORMAT_NAME} version 1 or {FORMAT_VERSION} file - first line is `{}`",
            magic.trim()
        ),
    })?;
    let expected_header = match version {
        1 => HEADER_V1,
        _ => HEADER,
    };

    let mut symbol = None;
    let mut source = None;
    let mut name = None;
    let mut tags = BTreeMap::new();
    let mut header_seen = false;
    let mut trades = Vec::new();
    let mut problems = Vec::new();
    for (index, line) in lines {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(comment) = trimmed.strip_prefix('#') {
            let comment = comment.trim();
            if let Some(value) = comment.strip_prefix("symbol=") {
                symbol = Some(value.trim().to_owned());
            } else if let Some(value) = comment.strip_prefix("source=") {
                match SessionSource::parse(value.trim()) {
                    Some(parsed) => source = Some(parsed),
                    // An unknown token stays unrecorded and is reported —
                    // guessing "live" would launder a practice run into
                    // the real track record.
                    None => problems.push(HistoryProblem {
                        line: line_number,
                        message: format!(
                            "unknown source `{}` - treated as unrecorded",
                            value.trim()
                        ),
                    }),
                }
            } else if let Some(value) = comment.strip_prefix("name=") {
                let value = clean_session_name(value);
                name = (!value.is_empty()).then_some(value);
            } else if let Some(value) = comment.strip_prefix("tags=") {
                match parse_tags_line(value) {
                    Ok((times, list)) if list.is_empty() => {
                        tags.remove(&times);
                    }
                    Ok((times, list)) => {
                        tags.insert(times, list);
                    }
                    // A torn or hand-edited tags line costs that line, said
                    // out loud; the trades and every other line still load.
                    Err(message) => problems.push(HistoryProblem {
                        line: line_number,
                        message,
                    }),
                }
            }
            continue;
        }
        if !header_seen {
            if trimmed != expected_header {
                return Err(HistoryError {
                    line: line_number,
                    message: format!(
                        "expected the column header `{expected_header}`, found `{trimmed}`"
                    ),
                });
            }
            header_seen = true;
            continue;
        }
        match parse_row(trimmed, version) {
            Ok(trade) => trades.push(trade),
            Err(message) => problems.push(HistoryProblem {
                line: line_number,
                message,
            }),
        }
    }
    if !header_seen {
        return Err(HistoryError {
            line: magic_line + 1,
            message: format!("no column header - expected `{expected_header}` after the comments"),
        });
    }
    Ok(TradeHistory {
        symbol,
        source,
        name,
        tags,
        trades,
        problems,
    })
}

/// `# quantick-trades N` for a version this parser reads, else `None`.
fn parse_magic(line: &str) -> Option<u32> {
    let rest = line.strip_prefix('#')?.trim();
    let version = rest.strip_prefix(FORMAT_NAME)?.trim();
    let version: u32 = version.parse().ok()?;
    (1..=FORMAT_VERSION).contains(&version).then_some(version)
}

fn parse_row(row: &str, version: u32) -> Result<ClosedTrade, String> {
    let fields: Vec<&str> = row.split(',').collect();
    let expected = if version == 1 { 8 } else { 12 };
    if fields.len() != expected {
        return Err(format!(
            "expected {expected} fields, found {}",
            fields.len()
        ));
    }
    let opened_ms = parse_i64(fields[0], "opened_ms")?;
    let closed_ms = parse_i64(fields[1], "closed_ms")?;
    let side = match fields[2] {
        "long" => Side::Buy,
        "short" => Side::Sell,
        other => return Err(format!("side must be `long` or `short`, found `{other}`")),
    };
    let quantity = parse_decimal(fields[3], "quantity")?;
    let entry_price = parse_decimal(fields[4], "entry_price")?;
    let exit_price = parse_decimal(fields[5], "exit_price")?;
    let pnl_points = parse_decimal(fields[6], "pnl_points")?;
    let exit_reason = ExitReason::parse(fields[7])
        .ok_or_else(|| format!("unknown exit_reason `{}`", fields[7]))?;
    let (entry_agg_id, exit_agg_id, mae_points, mfe_points) = if version == 1 {
        (None, None, None, None)
    } else {
        (
            parse_opt_u64(fields[8], "entry_agg_id")?,
            parse_opt_u64(fields[9], "exit_agg_id")?,
            parse_opt_decimal(fields[10], "mae_points")?,
            parse_opt_decimal(fields[11], "mfe_points")?,
        )
    };
    Ok(ClosedTrade {
        side,
        quantity,
        entry_price,
        exit_price,
        opened_ms,
        closed_ms,
        pnl_points,
        exit_reason,
        entry_agg_id,
        exit_agg_id,
        mae_points,
        mfe_points,
    })
}

fn parse_i64(field: &str, name: &str) -> Result<i64, String> {
    field
        .parse()
        .map_err(|_| format!("{name} must be an integer millisecond timestamp, found `{field}`"))
}

fn parse_decimal(field: &str, name: &str) -> Result<Decimal, String> {
    field
        .parse()
        .map_err(|_| format!("{name} must be a decimal number, found `{field}`"))
}

/// An empty field means "not recorded" (a version-1 row passing through),
/// never zero.
fn parse_opt_u64(field: &str, name: &str) -> Result<Option<u64>, String> {
    if field.is_empty() {
        return Ok(None);
    }
    field
        .parse()
        .map(Some)
        .map_err(|_| format!("{name} must be an integer id or empty, found `{field}`"))
}

fn parse_opt_decimal(field: &str, name: &str) -> Result<Option<Decimal>, String> {
    if field.is_empty() {
        return Ok(None);
    }
    field
        .parse()
        .map(Some)
        .map_err(|_| format!("{name} must be a decimal number or empty, found `{field}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(side: Side, pnl: i64, reason: ExitReason) -> ClosedTrade {
        ClosedTrade {
            side,
            quantity: Decimal::from(2),
            entry_price: Decimal::new(10050, 2),
            exit_price: Decimal::new(10325, 2),
            opened_ms: 1_700_000_000_000,
            closed_ms: 1_700_000_060_000,
            pnl_points: Decimal::from(pnl),
            exit_reason: reason,
            entry_agg_id: Some(41),
            exit_agg_id: Some(57),
            mae_points: Some(Decimal::new(15, 1)),
            mfe_points: Some(Decimal::from(6)),
        }
    }

    #[test]
    fn write_then_parse_round_trips_exactly() {
        let trades = [
            sample(Side::Buy, 5, ExitReason::TakeProfit),
            sample(Side::Sell, -3, ExitReason::StopLoss),
            sample(Side::Buy, 0, ExitReason::Reset),
        ];
        let mut text = write_header("BTCUSDT", SessionSource::Live);
        for trade in &trades {
            text.push_str(&write_trade(trade));
        }
        let history = parse(&text).expect("round trip parses");
        assert_eq!(history.symbol.as_deref(), Some("BTCUSDT"));
        assert_eq!(history.source, Some(SessionSource::Live));
        assert_eq!(history.trades, trades.to_vec());
        assert!(history.problems.is_empty());
    }

    #[test]
    fn the_session_source_round_trips_and_its_absence_stays_unrecorded() {
        let mut text = write_header("BTCUSDT", SessionSource::Replay);
        text.push_str(&write_trade(&sample(Side::Buy, 5, ExitReason::Manual)));
        let history = parse(&text).expect("parses");
        assert_eq!(history.source, Some(SessionSource::Replay));

        // A file from before `# source=` existed: unrecorded, not "live".
        let legacy = format!("# quantick-trades 2\n# symbol=X\n{HEADER}\n");
        let history = parse(&legacy).expect("legacy files still parse");
        assert_eq!(history.source, None, "unrecorded is not guessed");
        assert!(history.problems.is_empty());
    }

    #[test]
    fn an_unknown_source_token_is_reported_and_stays_unrecorded() {
        let text = format!("# quantick-trades 2\n# symbol=X\n# source=backtest\n{HEADER}\n");
        let history = parse(&text).expect("the file still loads");
        assert_eq!(history.source, None);
        assert_eq!(history.problems.len(), 1);
        assert!(history.problems[0].message.contains("backtest"));
    }

    #[test]
    fn a_version_1_file_still_loads_with_honest_unknowns() {
        let text = "# quantick-trades 1\n# symbol=WINQ26\n\
                    opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason\n\
                    1700000000000,1700000060000,long,2,100.5,103.25,5.5,take_profit\n";
        let history = parse(text).expect("version 1 still parses");
        assert_eq!(history.symbol.as_deref(), Some("WINQ26"));
        assert_eq!(history.trades.len(), 1);
        let trade = &history.trades[0];
        assert_eq!(trade.pnl_points, Decimal::new(55, 1));
        assert_eq!(trade.entry_agg_id, None, "v1 did not record it");
        assert_eq!(trade.exit_agg_id, None);
        assert_eq!(trade.mae_points, None, "unknown is not zero");
        assert_eq!(trade.mfe_points, None);
    }

    #[test]
    fn a_v1_row_re_exports_with_empty_fields_not_invented_values() {
        let mut trade = sample(Side::Buy, 5, ExitReason::Manual);
        trade.entry_agg_id = None;
        trade.exit_agg_id = None;
        trade.mae_points = None;
        trade.mfe_points = None;
        let line = write_trade(&trade);
        assert!(
            line.trim_end().ends_with("manual,,,,"),
            "unknown fields stay empty: {line}"
        );
        let mut text = write_header("X", SessionSource::Live);
        text.push_str(&line);
        let history = parse(&text).expect("round trips");
        assert_eq!(history.trades[0], trade);
    }

    #[test]
    fn a_torn_final_line_costs_one_reported_row_not_the_file() {
        let mut text = write_header("WINQ26", SessionSource::Live);
        text.push_str(&write_trade(&sample(Side::Buy, 5, ExitReason::Manual)));
        // A crash mid-append leaves a partial row.
        text.push_str("1700000000000,17000");
        let history = parse(&text).expect("the file still loads");
        assert_eq!(history.trades.len(), 1);
        assert_eq!(history.problems.len(), 1);
        assert_eq!(
            history.problems[0].line, 6,
            "magic + symbol + source + header + row, then the tear"
        );
        assert!(history.problems[0].message.contains("12 fields"));
    }

    #[test]
    fn wrong_magic_is_fatal() {
        let error =
            parse("Date,Time,Price\n").expect_err("a replay tick file is not trade history");
        assert_eq!(error.line, 1);
        assert!(error.message.contains(FORMAT_NAME));
    }

    #[test]
    fn unknown_version_is_fatal_not_guessed() {
        let error =
            parse("# quantick-trades 3\nopened_ms\n").expect_err("future versions are refused");
        assert!(
            error
                .message
                .contains("not a quantick-trades version 1 or 2 file")
        );
    }

    #[test]
    fn missing_header_is_fatal() {
        let error = parse("# quantick-trades 2\n# symbol=X\n").expect_err("header required");
        assert!(error.message.contains("no column header"));
    }

    #[test]
    fn a_v2_file_with_the_v1_header_is_fatal() {
        let error = parse(&format!("# quantick-trades 2\n{HEADER_V1}\n"))
            .expect_err("the header must match the declared version");
        assert!(error.message.contains("expected the column header"));
    }

    #[test]
    fn unknown_exit_reason_is_a_problem_row() {
        let mut text = write_header("X", SessionSource::Live);
        text.push_str("1,2,long,1,100,101,1,liquidated,,,,\n");
        let history = parse(&text).expect("file loads");
        assert!(history.trades.is_empty());
        assert!(history.problems[0].message.contains("liquidated"));
    }

    fn at(opened_ms: i64, closed_ms: i64) -> TradeRef {
        TradeRef {
            opened_ms,
            closed_ms,
            nth: 0,
        }
    }

    #[test]
    fn a_name_and_tags_round_trip_through_comment_lines() {
        let trade = sample(Side::Buy, 5, ExitReason::TakeProfit);
        let times = at(trade.opened_ms, trade.closed_ms);
        let mut text = write_header("WINZ26", SessionSource::Replay);
        text.push_str(&write_trade(&trade));
        text.push_str(&write_name("  Morning fade\npractice "));
        let tags = normalize_tags(["#MSC", "fade", "#msc"]).expect("valid tags");
        text.push_str(&write_tags(times, &tags));
        let history = parse(&text).expect("parses");
        assert_eq!(history.name.as_deref(), Some("Morning fade practice"));
        assert_eq!(
            history.tags.get(&times),
            Some(&vec!["msc".to_owned(), "fade".to_owned()])
        );
        assert_eq!(history.trades, vec![trade], "the columns are untouched");
        assert!(history.problems.is_empty());
    }

    #[test]
    fn the_last_name_and_tags_line_wins_and_an_empty_one_clears() {
        let times = at(1, 2);
        let mut text = write_header("X", SessionSource::Live);
        text.push_str(&write_name("first"));
        text.push_str(&write_tags(times, &["a".to_owned()]));
        text.push_str(&write_tags(at(3, 4), &["keep".to_owned()]));
        text.push_str(&write_name("second"));
        text.push_str(&write_tags(times, &["b".to_owned(), "c".to_owned()]));
        let history = parse(&text).expect("parses");
        assert_eq!(history.name.as_deref(), Some("second"));
        assert_eq!(
            history.tags.get(&times),
            Some(&vec!["b".to_owned(), "c".to_owned()])
        );
        assert_eq!(history.tags.get(&at(3, 4)), Some(&vec!["keep".to_owned()]));

        text.push_str(&write_name(""));
        text.push_str(&write_tags(times, &[]));
        let history = parse(&text).expect("parses");
        assert_eq!(history.name, None, "an empty name clears it");
        assert!(
            !history.tags.contains_key(&times),
            "no tags clears the trade"
        );
        assert!(history.tags.contains_key(&at(3, 4)), "and only that trade");
    }

    /// Two partial closes of one position share both times; each keeps its
    /// own tags, and the line for the first is the one older files wrote.
    #[test]
    fn trades_sharing_their_times_are_tagged_one_by_one() {
        let trade = sample(Side::Buy, 5, ExitReason::TakeProfit);
        let mut text = write_header("X", SessionSource::Live);
        text.push_str(&write_trade(&trade));
        text.push_str(&write_trade(&trade));
        let parsed = parse(&text).expect("parses");
        let refs = parsed.refs();
        assert_eq!(refs[0], at(trade.opened_ms, trade.closed_ms));
        assert_eq!(refs[1].nth, 1, "the second close of the same times");
        assert_eq!(
            write_tags(refs[0], &[]),
            format!("# tags={}:{}\n", trade.opened_ms, trade.closed_ms)
        );
        text.push_str(&write_tags(refs[1], &["scale".to_owned()]));
        text.push_str(&write_tags(refs[0], &["entry".to_owned()]));
        let parsed = parse(&text).expect("parses");
        assert_eq!(parsed.tags.get(&refs[0]), Some(&vec!["entry".to_owned()]));
        assert_eq!(parsed.tags.get(&refs[1]), Some(&vec!["scale".to_owned()]));
        assert!(parsed.problems.is_empty());
        assert!(
            parse(&format!("{text}# tags=1:2:3:4 #a\n"))
                .expect("parses")
                .problems
                .len()
                == 1,
            "a fourth field is not a trade"
        );
    }

    #[test]
    fn a_file_without_metadata_has_no_name_and_no_tags() {
        let mut text = write_header("X", SessionSource::Live);
        text.push_str(&write_trade(&sample(Side::Buy, 5, ExitReason::Manual)));
        let history = parse(&text).expect("parses");
        assert_eq!(history.name, None);
        assert!(history.tags.is_empty());
    }

    #[test]
    fn a_bad_tags_line_is_reported_and_the_rest_still_loads() {
        let mut text = write_header("X", SessionSource::Live);
        text.push_str("# tags=17000 #a\n");
        text.push_str("# tags=1:2 #a,b\n");
        text.push_str(&write_trade(&sample(Side::Buy, 5, ExitReason::Manual)));
        let history = parse(&text).expect("parses");
        assert_eq!(history.trades.len(), 1);
        assert_eq!(history.problems.len(), 2);
        assert!(history.tags.is_empty());
    }

    #[test]
    fn tags_are_one_lowercase_word_and_bounded() {
        assert_eq!(normalize_tag("##Fade"), Some("fade".to_owned()));
        assert_eq!(normalize_tag("two words"), None);
        assert_eq!(normalize_tag("a,b"), None);
        assert_eq!(normalize_tag("#"), None);
        assert_eq!(normalize_tag(&"x".repeat(MAX_TAG_CHARS + 1)), None);
        let many: Vec<String> = (0..=MAX_TAGS_PER_TRADE).map(|i| format!("t{i}")).collect();
        assert!(normalize_tags(many.iter().map(String::as_str)).is_err());
        assert_eq!(
            clean_session_name(&"n".repeat(MAX_SESSION_NAME_CHARS + 5))
                .chars()
                .count(),
            MAX_SESSION_NAME_CHARS
        );
    }

    #[test]
    fn comments_and_blank_lines_are_tolerated_between_rows() {
        let mut text = write_header("X", SessionSource::Live);
        text.push('\n');
        text.push_str("# a note a human left\n");
        text.push_str(&write_trade(&sample(Side::Sell, 7, ExitReason::TakeProfit)));
        let history = parse(&text).expect("file loads");
        assert_eq!(history.trades.len(), 1);
        assert!(history.problems.is_empty());
    }
}
