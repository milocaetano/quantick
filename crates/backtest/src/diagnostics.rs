//! One JSON object per line on stderr, each with an `event_code`.
//!
//! The convention comes from `quantick-replay`'s NDJSON importer: *"a person
//! with `jq` — or an AI reading the log — can follow the run without scraping
//! prose"*. The human report goes to stdout; this is the machine trail beside
//! it, so `> report.txt` keeps the report and leaves the diagnostics on the
//! terminal.
//!
//! Written by hand rather than with `serde_json`: the objects are flat, the
//! values are strings and numbers, and the workspace cultivates supply-chain
//! minimalism on purpose (`quantick-pine` hand-rolls a whole language
//! frontend for the same reason). Escaping is the only subtle part and it is
//! tested below.

use std::fmt::Write as _;

use rust_decimal::Decimal;

/// A diagnostic line under construction.
///
/// ```
/// use quantick_backtest::diagnostics::Event;
/// let line = Event::new("BACKTEST_SESSION_DONE")
///     .text("session", "WINQ26 2026-08-12")
///     .count("prints", 34)
///     .line();
/// assert_eq!(
///     line,
///     r#"{"event_code":"BACKTEST_SESSION_DONE","session":"WINQ26 2026-08-12","prints":34}"#
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Event {
    buffer: String,
}

impl Event {
    /// Start an event with its stable code.
    #[must_use]
    pub fn new(code: &str) -> Self {
        let mut buffer = String::with_capacity(128);
        buffer.push_str("{\"event_code\":");
        push_string(&mut buffer, code);
        Self { buffer }
    }

    /// Add a string field.
    #[must_use]
    pub fn text(mut self, key: &str, value: &str) -> Self {
        self.key(key);
        push_string(&mut self.buffer, value);
        self
    }

    /// Add a whole-number field.
    #[must_use]
    pub fn count(mut self, key: &str, value: u64) -> Self {
        self.key(key);
        let _ = write!(self.buffer, "{value}");
        self
    }

    /// Add a signed whole-number field (an epoch millisecond, an elapsed
    /// duration).
    #[must_use]
    pub fn integer(mut self, key: &str, value: i64) -> Self {
        self.key(key);
        let _ = write!(self.buffer, "{value}");
        self
    }

    /// Add an exact decimal field. Decimals serialize as JSON numbers with
    /// the digits they carry — no float round trip on the way out.
    #[must_use]
    pub fn decimal(mut self, key: &str, value: Decimal) -> Self {
        self.key(key);
        let _ = write!(self.buffer, "{}", value.normalize());
        self
    }

    /// Add a decimal that may be absent. `None` is JSON `null` — the
    /// machine-readable spelling of the report's `n/a`, and never a zero
    /// standing in for an unknown.
    #[must_use]
    pub fn maybe_decimal(mut self, key: &str, value: Option<Decimal>) -> Self {
        match value {
            Some(value) => return self.decimal(key, value),
            None => {
                self.key(key);
                self.buffer.push_str("null");
            }
        }
        self
    }

    /// Add a boolean field.
    #[must_use]
    pub fn flag(mut self, key: &str, value: bool) -> Self {
        self.key(key);
        self.buffer.push_str(if value { "true" } else { "false" });
        self
    }

    /// The finished line, without a trailing newline.
    #[must_use]
    pub fn line(self) -> String {
        let mut buffer = self.buffer;
        buffer.push('}');
        buffer
    }

    /// Write the finished line to stderr.
    pub fn emit(self) {
        eprintln!("{}", self.line());
    }

    fn key(&mut self, key: &str) {
        self.buffer.push(',');
        push_string(&mut self.buffer, key);
        self.buffer.push(':');
    }
}

/// Append a JSON string literal, escaped per RFC 8259.
fn push_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // Everything below 0x20 must be escaped; \u form is the only
            // spelling that covers the ones without a short escape.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    #[test]
    fn a_path_with_backslashes_and_quotes_stays_valid_json() {
        // Windows paths are the reason this matters: every session file the
        // harness names on this desk contains backslashes.
        let line = Event::new("BACKTEST_SESSION_FAILED")
            .text("file", r#"C:\replay\WIN"Q26"\a.csv"#)
            .text("detail", "line 1:\n\tbad")
            .line();
        assert_eq!(
            line,
            r#"{"event_code":"BACKTEST_SESSION_FAILED","file":"C:\\replay\\WIN\"Q26\"\\a.csv","detail":"line 1:\n\tbad"}"#
        );
    }

    #[test]
    fn an_absent_ratio_is_null_not_zero() {
        // The stderr twin of the report's `n/a`: a machine reading this must
        // not learn "profit factor 0" from a run that had no losses.
        let line = Event::new("BACKTEST_SESSION_DONE")
            .maybe_decimal("profit_factor", None)
            .maybe_decimal(
                "expectancy_points",
                Some(Decimal::from_str("1.50").expect("a decimal")),
            )
            .line();
        assert_eq!(
            line,
            r#"{"event_code":"BACKTEST_SESSION_DONE","profit_factor":null,"expectancy_points":1.5}"#
        );
    }

    #[test]
    fn control_characters_take_the_unicode_escape() {
        let line = Event::new("X").text("k", "a\u{1}b").line();
        assert_eq!(line, r#"{"event_code":"X","k":"a\u0001b"}"#);
    }
}
