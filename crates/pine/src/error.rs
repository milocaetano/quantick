//! [`PineError`] — one error type for every stage, AI-first.
//!
//! Every failure carries a **stable code** (greppable, documented in the
//! dialect reference), a **byte span** into the source, and a human message.
//! Rendering resolves the span to `file:line:col` lazily against the source
//! text, so spans stay cheap to carry through every stage.
//!
//! Loading collects *all* errors rather than stopping at the first — a
//! script with three problems reports three (§3.4 of the plan).

/// Byte range into the source text. Half-open: `start..end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// First byte of the offending text.
    pub start: usize,
    /// One past the last byte.
    pub end: usize,
}

impl Span {
    /// A span covering `start..end`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// An empty span at one position (end-of-file errors).
    #[must_use]
    pub const fn at(position: usize) -> Self {
        Self {
            start: position,
            end: position,
        }
    }

    /// The 1-based line and column of `self.start` within `source`.
    /// Columns count Unicode scalar values, not bytes — editors agree.
    ///
    /// Total by construction: a start that lands inside a multi-byte
    /// character is floored to the boundary before it rather than slicing
    /// there. Rendering a diagnostic is the last thing standing between a bad
    /// script and a crash, so it may not panic on a span it dislikes.
    #[must_use]
    pub fn line_col(&self, source: &str) -> (usize, usize) {
        let mut start = self.start.min(source.len());
        while start > 0 && !source.is_char_boundary(start) {
            start -= 1;
        }
        let upto = &source[..start];
        let line = upto.matches('\n').count() + 1;
        let line_start = upto.rfind('\n').map_or(0, |i| i + 1);
        let col = upto[line_start..].chars().count() + 1;
        (line, col)
    }
}

/// Stable error codes — the contract scripts and logs are debugged against.
/// Never renumber or reuse; the dialect reference catalogs them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// Malformed token: bad number, unterminated string, stray character.
    PineLex,
    /// The parser expected different syntax.
    PineSyntax,
    /// Inconsistent indentation (a block must be strictly deeper).
    PineIndent,
    /// A construct outside the dialect (`request.*`, `strategy.*`, …).
    PineNoSecurity,
    /// `timeframe.*` — activity-sampled charts have no timeframes.
    PineNoTimeframe,
    /// `strategy.*` — backtesting is a non-goal of v1.
    PineNoStrategy,
    /// Collections (`array.* matrix.* map.* table.*`) are not in v1.
    PineNoCollections,
    /// Calendar builtins / time-coordinate drawing are not in v1.
    PineNoCalendar,
    /// Any other rejected construct (`plotcandle`, `xloc.bar_time`, …).
    PineUnsupported,
    /// A name that resolves to nothing (with a did-you-mean note).
    PineUnknownName,
    /// Wrong argument count/kind for a builtin.
    PineArity,
    /// `input.*` arguments must be constant-foldable.
    PineInputNotConst,
    /// A windowed kernel's `len` must fold to a positive integer.
    PineSeriesLength,
    /// A stateful `ta.*` call inside a loop body (call-site identity would
    /// silently diverge from Pine; we refuse honestly).
    PineStatefulInLoop,
    /// Recursion in user functions (not supported).
    PineRecursion,
    /// `//@version` other than 5.
    PineVersion,
    /// Runtime: type/kind error during evaluation.
    PineType,
    /// Runtime: a `for`/`while` exceeded the per-bar iteration budget.
    PineLoopBudget,
}

impl ErrorCode {
    /// Every code, so a guard can iterate them instead of restating the list.
    ///
    /// The doc-drift test used to hand-list all of these; a variant added and
    /// forgotten there was simply not checked, which is the habit that test
    /// exists to replace. Adding a variant now fails `as_str`'s exhaustive
    /// match, and this array sits beside it.
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::PineLex,
        ErrorCode::PineSyntax,
        ErrorCode::PineIndent,
        ErrorCode::PineNoSecurity,
        ErrorCode::PineNoTimeframe,
        ErrorCode::PineNoStrategy,
        ErrorCode::PineNoCollections,
        ErrorCode::PineNoCalendar,
        ErrorCode::PineUnsupported,
        ErrorCode::PineUnknownName,
        ErrorCode::PineArity,
        ErrorCode::PineInputNotConst,
        ErrorCode::PineSeriesLength,
        ErrorCode::PineStatefulInLoop,
        ErrorCode::PineRecursion,
        ErrorCode::PineVersion,
        ErrorCode::PineType,
        ErrorCode::PineLoopBudget,
    ];

    /// The stable string form (logs, tests, the dialect reference).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::PineLex => "PINE_LEX",
            ErrorCode::PineSyntax => "PINE_SYNTAX",
            ErrorCode::PineIndent => "PINE_INDENT",
            ErrorCode::PineNoSecurity => "PINE_NO_SECURITY",
            ErrorCode::PineNoTimeframe => "PINE_NO_TIMEFRAME",
            ErrorCode::PineNoStrategy => "PINE_NO_STRATEGY",
            ErrorCode::PineNoCollections => "PINE_NO_COLLECTIONS",
            ErrorCode::PineNoCalendar => "PINE_NO_CALENDAR",
            ErrorCode::PineUnsupported => "PINE_UNSUPPORTED",
            ErrorCode::PineUnknownName => "PINE_UNKNOWN_NAME",
            ErrorCode::PineArity => "PINE_ARITY",
            ErrorCode::PineInputNotConst => "PINE_INPUT_NOT_CONST",
            ErrorCode::PineSeriesLength => "PINE_SERIES_LENGTH",
            ErrorCode::PineStatefulInLoop => "PINE_STATEFUL_IN_LOOP",
            ErrorCode::PineRecursion => "PINE_RECURSION",
            ErrorCode::PineVersion => "PINE_VERSION",
            ErrorCode::PineType => "PINE_TYPE",
            ErrorCode::PineLoopBudget => "PINE_LOOP_BUDGET",
        }
    }
}

/// One diagnosed problem: code + span + message (+ optional notes such as
/// "did you mean `close`?").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PineError {
    /// The stable code.
    pub code: ErrorCode,
    /// Where in the source.
    pub span: Span,
    /// What went wrong, in plain words.
    pub message: String,
    /// Extra hints, one per line in the rendering.
    pub notes: Vec<String>,
}

impl PineError {
    /// A bare error with no notes.
    #[must_use]
    pub fn new(code: ErrorCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            code,
            span,
            message: message.into(),
            notes: Vec::new(),
        }
    }

    /// Attach a hint line.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// The human rendering:
    /// `file.pine:12:8: request.security is not supported … (PINE_NO_SECURITY)`.
    #[must_use]
    pub fn render(&self, file: &str, source: &str) -> String {
        let (line, col) = self.span.line_col(source);
        let mut out = format!(
            "{file}:{line}:{col}: {} ({})",
            self.message,
            self.code.as_str()
        );
        for note in &self.notes {
            out.push_str("\n  note: ");
            out.push_str(note);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_is_one_based_and_counts_chars() {
        let source = "abc\ndef ghi\n";
        assert_eq!(Span::new(0, 1).line_col(source), (1, 1));
        assert_eq!(Span::new(4, 5).line_col(source), (2, 1));
        assert_eq!(Span::new(8, 9).line_col(source), (2, 5));
    }

    #[test]
    fn line_col_survives_a_span_inside_a_character() {
        // A file ending in a multi-byte character without a trailing newline
        // produces an end-of-line span at `len - 1`: the middle of that
        // character. Rendering it must report a position, not panic.
        let source = "x = // ré";
        let inside = source.len() - 1;
        assert!(!source.is_char_boundary(inside), "the case under test");
        assert_eq!(Span::at(inside).line_col(source), (1, 9));
    }

    #[test]
    fn render_matches_the_documented_shape() {
        let source = "//@version=5\nrequest.security(x)\n";
        let start = source.find("request").unwrap();
        let err = PineError::new(
            ErrorCode::PineNoSecurity,
            Span::new(start, start + 7),
            "request.security is not supported on activity-sampled charts",
        )
        .with_note("tick/volume/dollar bars have no timeframes to request");
        let rendered = err.render("file.pine", source);
        assert!(
            rendered.starts_with("file.pine:2:1: request.security is not supported"),
            "{rendered}"
        );
        assert!(rendered.contains("(PINE_NO_SECURITY)"), "{rendered}");
        assert!(rendered.contains("note: tick/volume"), "{rendered}");
    }
}
