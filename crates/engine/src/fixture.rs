//! Plain-text CSV fixtures for recorded trades.
//!
//! Fixtures are how the engine's determinism is guarded: a committed file of
//! trades is replayed against committed expected bars (see the golden harness).
//! The format is deliberately trivial — one trade per line, comma-separated, no
//! quoting — so fixtures are easy to read, hand-write and diff, and the parser
//! needs no dependencies.
//!
//! ```text
//! # comment lines and blank lines are ignored
//! agg_id,timestamp_ms,price,quantity,side
//! 1,1700000000000,36000.10,0.005,buy
//! 2,1700000000200,36000.20,0.010,sell
//! ```
//!
//! The first non-comment line may be the header shown above; it is detected and
//! skipped. `side` is `buy` or `sell` (the aggressor — see [`Side`]).
//!
//! [`write_trades`] is the inverse of [`parse_trades`]: parsing a written file
//! yields the original trades.

use std::fmt::Write as _;
use std::str::FromStr as _;

use rust_decimal::Decimal;

use crate::{Side, Trade};

/// The canonical header line, written by [`write_trades`] and skipped by
/// [`parse_trades`].
pub const HEADER: &str = "agg_id,timestamp_ms,price,quantity,side";

/// An error encountered while parsing a trade fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number in the source text.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "trade fixture parse error on line {}: {}",
            self.line, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// Parse trades from CSV fixture text.
///
/// Blank lines and `#` comments are skipped; a leading [`HEADER`] line is
/// optional and ignored. Trades are returned in file order — the engine relies
/// on the caller feeding trades in the order they occurred.
///
/// # Errors
///
/// Returns a [`ParseError`] (carrying the 1-based line number) on the first
/// malformed row: wrong field count, unparseable number, or an unknown side.
pub fn parse_trades(text: &str) -> Result<Vec<Trade>, ParseError> {
    let mut trades = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line == HEADER {
            continue;
        }
        trades.push(parse_trade_line(line, idx + 1)?);
    }
    Ok(trades)
}

fn parse_trade_line(line: &str, line_no: usize) -> Result<Trade, ParseError> {
    let mkerr = |message: String| ParseError {
        line: line_no,
        message,
    };
    let fields: Vec<&str> = line.split(',').map(str::trim).collect();
    if fields.len() != 5 {
        return Err(mkerr(format!(
            "expected 5 comma-separated fields, found {}",
            fields.len()
        )));
    }
    let agg_id = fields[0]
        .parse::<u64>()
        .map_err(|e| mkerr(format!("agg_id `{}`: {e}", fields[0])))?;
    let timestamp_ms = fields[1]
        .parse::<i64>()
        .map_err(|e| mkerr(format!("timestamp_ms `{}`: {e}", fields[1])))?;
    let price =
        Decimal::from_str(fields[2]).map_err(|e| mkerr(format!("price `{}`: {e}", fields[2])))?;
    let quantity = Decimal::from_str(fields[3])
        .map_err(|e| mkerr(format!("quantity `{}`: {e}", fields[3])))?;
    let side = match fields[4] {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        other => return Err(mkerr(format!("side `{other}` (expected `buy` or `sell`)"))),
    };
    Ok(Trade {
        agg_id,
        timestamp_ms,
        price,
        quantity,
        side,
    })
}

/// Serialise trades to CSV fixture text, including the [`HEADER`] line.
///
/// The inverse of [`parse_trades`]: `parse_trades(&write_trades(ts)) == ts` for
/// any slice of trades, so fixtures round-trip losslessly.
#[must_use]
pub fn write_trades(trades: &[Trade]) -> String {
    let mut out = String::with_capacity(HEADER.len() + 1 + trades.len() * 40);
    out.push_str(HEADER);
    out.push('\n');
    for t in trades {
        writeln!(
            out,
            "{},{},{},{},{}",
            t.agg_id,
            t.timestamp_ms,
            t.price,
            t.quantity,
            t.side.as_str()
        )
        .expect("writing to a String is infallible");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn parses_a_small_fixture() {
        let text = "\
# a couple of trades
agg_id,timestamp_ms,price,quantity,side
1,1700000000000,36000.10,0.005,buy

2,1700000000200,36000.20,0.010,sell
";
        let trades = parse_trades(text).unwrap();
        assert_eq!(trades.len(), 2);
        assert_eq!(
            trades[0],
            Trade {
                agg_id: 1,
                timestamp_ms: 1_700_000_000_000,
                price: dec("36000.10"),
                quantity: dec("0.005"),
                side: Side::Buy,
            }
        );
        assert_eq!(trades[1].side, Side::Sell);
    }

    #[test]
    fn round_trips_through_write_and_parse() {
        let trades = vec![
            Trade {
                agg_id: 10,
                timestamp_ms: 1_700_000_000_000,
                price: dec("36000.10"),
                quantity: dec("0.005"),
                side: Side::Buy,
            },
            Trade {
                agg_id: 11,
                timestamp_ms: 1_700_000_000_050,
                price: dec("35999.90"),
                quantity: dec("2.5"),
                side: Side::Sell,
            },
        ];
        let text = write_trades(&trades);
        assert!(text.starts_with(HEADER));
        assert_eq!(parse_trades(&text).unwrap(), trades);
    }

    #[test]
    fn write_is_deterministic() {
        let trades = vec![Trade {
            agg_id: 1,
            timestamp_ms: 1,
            price: dec("1.23"),
            quantity: dec("4.5"),
            side: Side::Buy,
        }];
        assert_eq!(write_trades(&trades), write_trades(&trades));
    }

    #[test]
    fn rejects_wrong_field_count() {
        let err = parse_trades("1,2,3\n").unwrap_err();
        assert_eq!(err.line, 1);
        assert!(err.message.contains("found 3"), "{}", err.message);
    }

    #[test]
    fn rejects_unknown_side() {
        let err = parse_trades("1,2,3.0,4.0,hold\n").unwrap_err();
        assert!(err.message.contains("hold"), "{}", err.message);
    }

    #[test]
    fn reports_the_offending_line_number() {
        let text = "1,1,1.0,1.0,buy\nnot,a,valid,trade,row,extra\n";
        let err = parse_trades(text).unwrap_err();
        assert_eq!(err.line, 2);
    }
}
