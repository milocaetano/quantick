//! What a bridge says about itself on stderr, read as something a person can
//! act on.
//!
//! Both bridges (`bridge/mt5/quantick_bridge.py` and the `QuantickBridge.mq5`
//! Expert Advisor) print one JSON object per line, each carrying an
//! `event_code` from a fixed vocabulary — the same vocabulary this crate uses
//! for its own `MT5_*` logs. That was already enough to *diagnose* a broken
//! setup by reading a log file. It was not enough to *tell* someone, in the
//! chart they are staring at, that MetaTrader is closed.
//!
//! This module is the translation: bridge line in, [`BridgeReport`] out, with
//! the one next step spelled out for the codes a normal user can actually hit.
//! It stays here rather than in the app because the vocabulary is the bridge's,
//! and this crate is where the bridge protocol already lives — a second copy of
//! these strings next to the UI would drift the first time a code is renamed.
//!
//! Unrecognized and non-JSON lines report nothing. A bridge is allowed to log
//! whatever it likes (a Python traceback is not JSON); silence here means "keep
//! this in the log", not "swallow it", and the caller still logs the raw line.

use serde_json::Value;

/// How much a bridge line matters to the person watching the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeSeverity {
    /// Something is under way and needs nobody: starting, retrying, waiting.
    Progress,
    /// Nothing more will happen until a human does something.
    Attention,
}

/// One bridge line, translated for a reader who is not reading logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeReport {
    /// The `event_code` this came from, so logs and UI can be correlated.
    pub event_code: String,
    /// Whether this needs the user.
    pub severity: BridgeSeverity,
    /// What happened, in the user's terms — no event codes, no jargon.
    pub headline: String,
    /// The single next step. Always present for [`BridgeSeverity::Attention`];
    /// progress reports have nothing to ask for.
    pub next_step: Option<String>,
    /// Whether the bridge stops after printing this.
    ///
    /// Separate from [`severity`](Self::severity) because the two really do
    /// come apart: a symbol with no Depth of Market needs the user *and* keeps
    /// streaming trades. A caller watching for "did it explain why it died?"
    /// must ask this, not whether something needed attention at some point —
    /// otherwise one survivable warning silences every later crash.
    pub ends_session: bool,
}

impl BridgeReport {
    fn progress(code: &str, headline: impl Into<String>) -> Self {
        Self {
            event_code: code.to_owned(),
            severity: BridgeSeverity::Progress,
            headline: headline.into(),
            next_step: None,
            ends_session: false,
        }
    }

    /// Needs the user, and the bridge exits after saying it.
    fn fatal(code: &str, headline: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self {
            event_code: code.to_owned(),
            severity: BridgeSeverity::Attention,
            headline: headline.into(),
            next_step: Some(next_step.into()),
            ends_session: true,
        }
    }

    /// Needs the user, but the bridge carries on with less than the full
    /// picture.
    fn survivable(code: &str, headline: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self {
            event_code: code.to_owned(),
            severity: BridgeSeverity::Attention,
            headline: headline.into(),
            next_step: Some(next_step.into()),
            ends_session: false,
        }
    }
}

/// The `event_code` of one bridge stderr line, if it has one.
///
/// Useful on its own for callers that log every line but only react to some.
#[must_use]
pub fn event_code_of(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    Some(value.get("event_code")?.as_str()?.to_owned())
}

/// Translate one bridge stderr line, or `None` when it says nothing a user
/// needs (unknown code, routine statistics, or not JSON at all).
/// A count with thin spaces every three digits, the way the rest of the UI
/// writes them.
///
/// `1525621` in a sentence the trader is supposed to act on is a number they
/// have to count the digits of. This is the only place the bridge's structured
/// fields become prose, so it is the place that owes them a readable shape.
fn grouped(value: i64) -> String {
    let digits = value.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    out
}

#[must_use]
pub fn report_for_line(line: &str) -> Option<BridgeReport> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let code = value.get("event_code")?.as_str()?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|text| !text.is_empty())
    };
    let symbol = field("symbol").unwrap_or_else(|| "that contract".to_owned());
    // The backfill's own lines count things, and a count is the whole point of
    // what they say. Grouped for reading: "4000000" in a sentence a trader is
    // meant to act on is a number they have to squint at.
    let count = |name: &str| value.get(name).and_then(Value::as_i64).map(grouped);

    let report = match code {
        // -- fatal, and every one of them has a fix the user can perform ----
        "BRIDGE_NO_MT5_PACKAGE" => BridgeReport::fatal(
            code,
            "The MetaTrader 5 Python package is missing",
            "Install it once, then reconnect: pip install MetaTrader5",
        ),
        "BRIDGE_TERMINAL_ATTACH_FAILED" => BridgeReport::fatal(
            code,
            "MetaTrader 5 is not running, or is not logged in",
            "Open the MetaTrader 5 terminal and log in — quantick keeps retrying.",
        ),
        "BRIDGE_SYMBOL_NOT_FOUND" => BridgeReport::fatal(
            code,
            format!("MetaTrader does not list {symbol}"),
            "Add the contract to Market Watch, or pick the exact name your \
             broker uses (front-month contracts look like WINQ26).",
        ),
        "BRIDGE_SYMBOL_SELECT_FAILED" => BridgeReport::fatal(
            code,
            format!("MetaTrader would not open {symbol}"),
            "Right-click it in Market Watch and choose Show, then reconnect.",
        ),
        "BRIDGE_UTC_OFFSET_UNKNOWN" => BridgeReport::fatal(
            code,
            "The market is quiet, so the server clock could not be measured",
            "Connect once while the market trades — quantick then remembers the \
             offset. Timestamps are never guessed.",
        ),
        // -- under way, nothing to do --------------------------------------
        "BRIDGE_STARTING" => BridgeReport::progress(code, "starting the MetaTrader bridge"),
        "BRIDGE_UTC_OFFSET_MARKET_QUIET" => BridgeReport::progress(
            code,
            "market is quiet — reading the server clock from the last session",
        ),
        "BRIDGE_SESSION_STARTED" => {
            BridgeReport::progress(code, format!("loading {symbol} history from MetaTrader"))
        }
        "BRIDGE_BACKFILL_FAILED" => BridgeReport::progress(
            code,
            "MetaTrader returned no history; the chart starts from live trades",
        ),
        "BRIDGE_DISCONNECTED" => {
            BridgeReport::progress(code, "the bridge lost its connection — reconnecting")
        }
        // -- streaming, but with less than the full picture ------------------
        "BRIDGE_BACKFILL_TRUNCATED" => BridgeReport::survivable(
            code,
            match count("sending") {
                Some(sent) => format!(
                    "{symbol} traded more in this session than quantick opens with — \
                     the newest {sent} prints are on the chart"
                ),
                None => format!("{symbol}'s session was larger than quantick opens with"),
            },
            "The rest of the day is still in MetaTrader: press + older to pull \
             it in. Nothing was lost — this is a limit on what is held at once.",
        ),
        "BRIDGE_BACKFILL_WALK_FAILED" => BridgeReport::survivable(
            code,
            format!("MetaTrader stopped answering partway through {symbol}'s session"),
            "The chart holds what did arrive and keeps streaming. Press + older \
             to ask for the rest.",
        ),
        "BRIDGE_BOOK_SUBSCRIBE_FAILED" => BridgeReport::survivable(
            code,
            format!("{symbol} has no Depth of Market in this terminal"),
            "Trades keep streaming; the book heatmap stays empty. Ask your \
             broker to enable DOM for this contract.",
        ),
        _ => return None,
    };
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_package_names_the_command_that_fixes_it() {
        let report = report_for_line(
            r#"{"event_code":"BRIDGE_NO_MT5_PACKAGE","hint":"pip install MetaTrader5"}"#,
        )
        .expect("a user-visible report");
        assert_eq!(report.severity, BridgeSeverity::Attention);
        let step = report.next_step.expect("attention always has a next step");
        assert!(step.contains("pip install MetaTrader5"), "step was: {step}");
    }

    #[test]
    fn an_unknown_contract_is_named_in_the_headline() {
        let report = report_for_line(
            r#"{"event_code":"BRIDGE_SYMBOL_NOT_FOUND","symbol":"WINZ99","hint":"check"}"#,
        )
        .expect("a user-visible report");
        assert!(
            report.headline.contains("WINZ99"),
            "headline was: {}",
            report.headline
        );
    }

    #[test]
    fn a_symbol_less_line_still_reads_as_a_sentence() {
        // The field is optional in the protocol; the report must not print an
        // empty gap where the contract name would go.
        let report = report_for_line(r#"{"event_code":"BRIDGE_SYMBOL_NOT_FOUND"}"#)
            .expect("a user-visible report");
        assert!(report.headline.contains("that contract"));
        // An empty string counts as absent, not as a contract called "".
        let empty = report_for_line(r#"{"event_code":"BRIDGE_SYMBOL_NOT_FOUND","symbol":""}"#)
            .expect("a user-visible report");
        assert_eq!(empty.headline, report.headline);
    }

    #[test]
    fn progress_lines_ask_for_nothing() {
        for line in [
            r#"{"event_code":"BRIDGE_STARTING","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_SESSION_STARTED","symbol":"WINQ26","book":true}"#,
            r#"{"event_code":"BRIDGE_DISCONNECTED","reason":"eof"}"#,
        ] {
            let report = report_for_line(line).expect("a user-visible report");
            assert_eq!(report.severity, BridgeSeverity::Progress, "line: {line}");
            assert!(report.next_step.is_none(), "line: {line}");
        }
    }

    /// The distinction a caller leans on to decide whether a bridge that died
    /// already said why. Mirrors `bridge/mt5/quantick_bridge.py`: everything
    /// that raises `BridgeExit` ends the session; the DOM warning does not.
    #[test]
    fn only_the_codes_that_stop_the_bridge_end_the_session() {
        let ends = |line: &str| {
            report_for_line(line)
                .expect("a user-visible report")
                .ends_session
        };
        for line in [
            r#"{"event_code":"BRIDGE_NO_MT5_PACKAGE"}"#,
            r#"{"event_code":"BRIDGE_TERMINAL_ATTACH_FAILED"}"#,
            r#"{"event_code":"BRIDGE_SYMBOL_NOT_FOUND","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_SYMBOL_SELECT_FAILED","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_UTC_OFFSET_UNKNOWN"}"#,
        ] {
            assert!(ends(line), "this one exits the process: {line}");
        }
        // Needs the user *and* keeps streaming: the heatmap stays empty while
        // trades carry on. Treating it as "explained itself" would silence
        // every later crash of the same bridge.
        assert!(!ends(
            r#"{"event_code":"BRIDGE_BOOK_SUBSCRIBE_FAILED","symbol":"WDO$"}"#
        ));
        // Same shape for the two the opening block can raise: the trader has
        // less than the whole session and a way to get the rest, but the tape
        // is streaming and killing the session would cost them that too.
        assert!(!ends(
            r#"{"event_code":"BRIDGE_BACKFILL_TRUNCATED","symbol":"WINV26","sending":4000000}"#
        ));
        assert!(!ends(
            r#"{"event_code":"BRIDGE_BACKFILL_WALK_FAILED","symbol":"WINV26"}"#
        ));
        for line in [
            r#"{"event_code":"BRIDGE_STARTING"}"#,
            r#"{"event_code":"BRIDGE_SESSION_STARTED","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_DISCONNECTED"}"#,
            r#"{"event_code":"BRIDGE_BACKFILL_FAILED"}"#,
            r#"{"event_code":"BRIDGE_UTC_OFFSET_MARKET_QUIET"}"#,
        ] {
            assert!(!ends(line), "progress never ends the session: {line}");
        }
    }

    #[test]
    fn a_book_less_symbol_says_what_is_lost_and_what_still_works() {
        let report = report_for_line(
            r#"{"event_code":"BRIDGE_BOOK_SUBSCRIBE_FAILED","symbol":"WDO$","mt5_error":"(0)"}"#,
        )
        .expect("a user-visible report");
        assert_eq!(report.severity, BridgeSeverity::Attention);
        let step = report.next_step.expect("attention always has a next step");
        assert!(step.contains("Trades keep streaming"), "step was: {step}");
    }

    #[test]
    fn a_cut_session_says_what_arrived_and_how_to_reach_the_rest() {
        let report = report_for_line(
            r#"{"event_code":"BRIDGE_BACKFILL_TRUNCATED","symbol":"WINV26","found":9000000,"sending":4000000,"action":"keep_newest"}"#,
        )
        .expect("a user-visible report");
        assert_eq!(report.severity, BridgeSeverity::Attention);
        assert!(
            report.headline.contains("4\u{202f}000\u{202f}000"),
            "the trader is told how much they have, grouped: {}",
            report.headline
        );
        let step = report.next_step.expect("attention always has a next step");
        assert!(
            step.contains("+ older"),
            "and the way back to the rest of the day: {step}"
        );
    }

    #[test]
    fn a_cut_with_no_count_still_says_something_true() {
        // The bridge always sends `sending`, but a report that reads a field
        // must never depend on one: a line from an older bridge would
        // otherwise translate into a sentence with a hole in it.
        let report =
            report_for_line(r#"{"event_code":"BRIDGE_BACKFILL_TRUNCATED","symbol":"WINV26"}"#)
                .expect("a user-visible report");
        assert!(
            report.headline.contains("larger than quantick opens with"),
            "headline was: {}",
            report.headline
        );
    }

    #[test]
    fn noise_reports_nothing_and_never_panics() {
        for line in [
            "",
            "   ",
            "Traceback (most recent call last):",
            "  File \"quantick_bridge.py\", line 1, in <module>",
            "{not json",
            "[]",
            "{}",
            r#"{"event_code":42}"#,
            r#"{"event_code":"BRIDGE_BOOK_STATS","images_sent":10}"#,
        ] {
            assert!(report_for_line(line).is_none(), "line: {line}");
        }
    }

    #[test]
    fn the_event_code_is_readable_on_its_own() {
        assert_eq!(
            event_code_of(r#"{"event_code":"BRIDGE_BOOK_STATS","images_sent":1}"#).as_deref(),
            Some("BRIDGE_BOOK_STATS")
        );
        assert_eq!(event_code_of("not json"), None);
    }

    #[test]
    fn every_attention_carries_a_step_and_every_progress_does_not() {
        // The invariant the UI leans on: an amber card always has something to
        // do under it. Checked across the whole table rather than per case.
        const LINES: [&str; 10] = [
            r#"{"event_code":"BRIDGE_NO_MT5_PACKAGE"}"#,
            r#"{"event_code":"BRIDGE_TERMINAL_ATTACH_FAILED"}"#,
            r#"{"event_code":"BRIDGE_SYMBOL_NOT_FOUND","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_SYMBOL_SELECT_FAILED","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_UTC_OFFSET_UNKNOWN"}"#,
            r#"{"event_code":"BRIDGE_BOOK_SUBSCRIBE_FAILED","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_STARTING"}"#,
            r#"{"event_code":"BRIDGE_UTC_OFFSET_MARKET_QUIET"}"#,
            r#"{"event_code":"BRIDGE_SESSION_STARTED","symbol":"WINQ26"}"#,
            r#"{"event_code":"BRIDGE_BACKFILL_FAILED"}"#,
        ];
        for line in LINES {
            let report = report_for_line(line).expect("a user-visible report");
            assert!(!report.headline.is_empty(), "line: {line}");
            match report.severity {
                BridgeSeverity::Attention => {
                    let step = report.next_step.unwrap_or_default();
                    assert!(!step.is_empty(), "no next step for: {line}");
                }
                BridgeSeverity::Progress => {
                    assert!(report.next_step.is_none(), "line: {line}");
                }
            }
        }
    }
}
