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
//! # quantick-trades 1
//! # symbol=BTCUSDT
//! opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason
//! 1700000000000,1700000060000,long,2,100.5,103.25,5.5,take_profit
//! ```
//!
//! `side` is the position's direction (`long`/`short`); `exit_reason` uses
//! [`ExitReason::as_str`] tokens. Timestamps are venue epoch milliseconds;
//! prices and quantities are exact decimals; `pnl_points` is points, not
//! currency.

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::events::ExitReason;
use crate::simulator::ClosedTrade;

/// Magic token on the first line, naming the format.
pub const FORMAT_NAME: &str = "quantick-trades";
/// Version written and the only one accepted — an unknown version is an
/// error, never a guess.
pub const FORMAT_VERSION: u32 = 1;
/// Extension the app uses for history files.
pub const FILE_EXTENSION: &str = "csv";
/// The column header, fixed order.
pub const HEADER: &str =
    "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,exit_reason";

/// A parsed history file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeHistory {
    /// Symbol from the `# symbol=` comment, when present.
    pub symbol: Option<String>,
    /// Rows that parsed, in file order (which is closing order).
    pub trades: Vec<ClosedTrade>,
    /// Rows that did not parse, reported instead of silently dropped.
    pub problems: Vec<HistoryProblem>,
}

/// One unreadable row: where and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryProblem {
    /// 1-based line number.
    pub line: usize,
    pub message: String,
}

/// The file is not a readable quantick-trades file at all (wrong magic,
/// wrong version, wrong header) — as opposed to a file with bad rows.
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

/// The lines that open a new history file: magic, symbol, column header.
#[must_use]
pub fn write_header(symbol: &str) -> String {
    format!("# {FORMAT_NAME} {FORMAT_VERSION}\n# symbol={symbol}\n{HEADER}\n")
}

/// One closed trade as one CSV line (newline included), the exact inverse
/// of what [`parse`] reads.
#[must_use]
pub fn write_trade(trade: &ClosedTrade) -> String {
    let side = match trade.side {
        Side::Buy => "long",
        Side::Sell => "short",
    };
    format!(
        "{},{},{},{},{},{},{},{}\n",
        trade.opened_ms,
        trade.closed_ms,
        side,
        trade.quantity,
        trade.entry_price,
        trade.exit_price,
        trade.pnl_points,
        trade.exit_reason.as_str(),
    )
}

/// Read a history file back. Fatal errors are reserved for "this is not a
/// quantick-trades file"; a bad *row* becomes a [`HistoryProblem`] and the
/// rest of the file still loads.
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
    let expected_magic = format!("# {FORMAT_NAME} {FORMAT_VERSION}");
    if magic.trim() != expected_magic {
        return Err(HistoryError {
            line: magic_line + 1,
            message: format!(
                "not a {FORMAT_NAME} version {FORMAT_VERSION} file - first line is `{}`",
                magic.trim()
            ),
        });
    }

    let mut symbol = None;
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
            if let Some(value) = comment.trim().strip_prefix("symbol=") {
                symbol = Some(value.trim().to_owned());
            }
            continue;
        }
        if !header_seen {
            if trimmed != HEADER {
                return Err(HistoryError {
                    line: line_number,
                    message: format!("expected the column header `{HEADER}`, found `{trimmed}`"),
                });
            }
            header_seen = true;
            continue;
        }
        match parse_row(trimmed) {
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
            message: format!("no column header - expected `{HEADER}` after the comments"),
        });
    }
    Ok(TradeHistory {
        symbol,
        trades,
        problems,
    })
}

fn parse_row(row: &str) -> Result<ClosedTrade, String> {
    let fields: Vec<&str> = row.split(',').collect();
    if fields.len() != 8 {
        return Err(format!("expected 8 fields, found {}", fields.len()));
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
    Ok(ClosedTrade {
        side,
        quantity,
        entry_price,
        exit_price,
        opened_ms,
        closed_ms,
        pnl_points,
        exit_reason,
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
        }
    }

    #[test]
    fn write_then_parse_round_trips_exactly() {
        let trades = [
            sample(Side::Buy, 5, ExitReason::TakeProfit),
            sample(Side::Sell, -3, ExitReason::StopLoss),
            sample(Side::Buy, 0, ExitReason::Reset),
        ];
        let mut text = write_header("BTCUSDT");
        for trade in &trades {
            text.push_str(&write_trade(trade));
        }
        let history = parse(&text).expect("round trip parses");
        assert_eq!(history.symbol.as_deref(), Some("BTCUSDT"));
        assert_eq!(history.trades, trades.to_vec());
        assert!(history.problems.is_empty());
    }

    #[test]
    fn a_torn_final_line_costs_one_reported_row_not_the_file() {
        let mut text = write_header("WINQ26");
        text.push_str(&write_trade(&sample(Side::Buy, 5, ExitReason::Manual)));
        // A crash mid-append leaves a partial row.
        text.push_str("1700000000000,17000");
        let history = parse(&text).expect("the file still loads");
        assert_eq!(history.trades.len(), 1);
        assert_eq!(history.problems.len(), 1);
        assert_eq!(
            history.problems[0].line, 5,
            "magic + symbol + header + row, then the tear"
        );
        assert!(history.problems[0].message.contains("8 fields"));
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
            parse("# quantick-trades 2\nopened_ms\n").expect_err("future versions are refused");
        assert!(
            error
                .message
                .contains("not a quantick-trades version 1 file")
        );
    }

    #[test]
    fn missing_header_is_fatal() {
        let error = parse("# quantick-trades 1\n# symbol=X\n").expect_err("header required");
        assert!(error.message.contains("no column header"));
    }

    #[test]
    fn unknown_exit_reason_is_a_problem_row() {
        let mut text = write_header("X");
        text.push_str("1,2,long,1,100,101,1,liquidated\n");
        let history = parse(&text).expect("file loads");
        assert!(history.trades.is_empty());
        assert!(history.problems[0].message.contains("liquidated"));
    }

    #[test]
    fn comments_and_blank_lines_are_tolerated_between_rows() {
        let mut text = write_header("X");
        text.push('\n');
        text.push_str("# a note a human left\n");
        text.push_str(&write_trade(&sample(Side::Sell, 7, ExitReason::TakeProfit)));
        let history = parse(&text).expect("file loads");
        assert_eq!(history.trades.len(), 1);
        assert!(history.problems.is_empty());
    }
}
