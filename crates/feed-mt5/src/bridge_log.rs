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
}

impl BridgeReport {
    fn progress(code: &str, headline: impl Into<String>) -> Self {
        Self {
            event_code: code.to_owned(),
            severity: BridgeSeverity::Progress,
            headline: headline.into(),
            next_step: None,
        }
    }

    fn attention(code: &str, headline: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self {
            event_code: code.to_owned(),
            severity: BridgeSeverity::Attention,
            headline: headline.into(),
            next_step: Some(next_step.into()),
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

    let report = match code {
        // -- fatal, and every one of them has a fix the user can perform ----
        "BRIDGE_NO_MT5_PACKAGE" => BridgeReport::attention(
            code,
            "The MetaTrader 5 Python package is missing",
            "Install it once, then reconnect: pip install MetaTrader5",
        ),
        "BRIDGE_TERMINAL_ATTACH_FAILED" => BridgeReport::attention(
            code,
            "MetaTrader 5 is not running, or is not logged in",
            "Open the MetaTrader 5 terminal and log in — quantick keeps retrying.",
        ),
        "BRIDGE_SYMBOL_NOT_FOUND" => BridgeReport::attention(
            code,
            format!("MetaTrader does not list {symbol}"),
            "Add the contract to Market Watch, or pick the exact name your \
             broker uses (front-month contracts look like WINQ26).",
        ),
        "BRIDGE_SYMBOL_SELECT_FAILED" => BridgeReport::attention(
            code,
            format!("MetaTrader would not open {symbol}"),
            "Right-click it in Market Watch and choose Show, then reconnect.",
        ),
        "BRIDGE_UTC_OFFSET_UNKNOWN" => BridgeReport::attention(
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
        "BRIDGE_BOOK_SUBSCRIBE_FAILED" => BridgeReport::attention(
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
