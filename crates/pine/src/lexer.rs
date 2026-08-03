//! Indentation-aware lexer.
//!
//! Physical lines become logical lines Python-style: `Newline` ends a
//! statement, `Indent`/`Dedent` bracket blocks, and a physical line ending
//! while brackets are unclosed *or* after a binary operator / comma / `=>`
//! **continues** the logical line. This accepts normal Pine formatting
//! without cloning TradingView's "continuation must not be a multiple of 4"
//! rule — a documented divergence.
//!
//! A block must be *strictly deeper* than its parent, and a dedent must land
//! exactly on an enclosing level; anything else is `PINE_INDENT` with the
//! offending span. Tabs count as [`TAB_WIDTH`] columns (document users who
//! mix tabs and spaces get honest errors, not silent misparses).
//!
//! Every token carries its byte [`Span`]; every later stage reports through
//! spans, so the lexer is the single owner of position math.

use crate::error::{ErrorCode, PineError, Span};

/// Columns one tab advances (a fixed convention, documented in the dialect
/// reference; mixing tabs and spaces inconsistently trips `PINE_INDENT`).
const TAB_WIDTH: usize = 4;

/// One lexical token kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// Integer literal.
    Int(i64),
    /// Float literal (also integers too large for i64 — documented).
    Float(f64),
    /// String literal, escapes resolved.
    Str(String),
    /// `#RRGGBB[AA]` literal, packed RGBA (alpha defaults to 0xFF).
    ColorLit(u32),
    /// Identifier (namespaces arrive as `Ident Dot Ident`).
    Ident(String),

    // Keywords.
    /// `var`.
    KwVar,
    /// `varip`.
    KwVarip,
    /// `if`.
    KwIf,
    /// `else`.
    KwElse,
    /// `for`.
    KwFor,
    /// `to`.
    KwTo,
    /// `by`.
    KwBy,
    /// `while`.
    KwWhile,
    /// `switch`.
    KwSwitch,
    /// `and`.
    KwAnd,
    /// `or`.
    KwOr,
    /// `not`.
    KwNot,
    /// `true`.
    KwTrue,
    /// `false`.
    KwFalse,
    /// `na`.
    KwNa,

    // Operators and punctuation.
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `%`
    Percent,
    /// `=`
    Eq,
    /// `:=`
    ColonEq,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `?`
    Question,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// `.`
    Dot,
    /// `=>`
    Arrow,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `[`
    LBracket,
    /// `]`
    RBracket,

    // Layout.
    /// End of a logical line.
    Newline,
    /// A strictly deeper block starts.
    Indent,
    /// A block ended.
    Dedent,
    /// End of input (after closing Newline/Dedents).
    Eof,
}

/// A token with its source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// What it is.
    pub tok: Tok,
    /// Where it is.
    pub span: Span,
}

/// The lexer's output: the token stream plus the side-channel directive.
#[derive(Debug)]
pub struct LexOutput {
    /// The token stream, ending in [`Tok::Eof`].
    pub tokens: Vec<Token>,
    /// `//@version=N`, if the script declared one.
    pub version: Option<(u32, Span)>,
}

/// Lex `source` completely, collecting every error (a script with three bad
/// tokens reports three).
///
/// # Errors
///
/// Returns every [`PineError`] found; the token stream is not returned when
/// lexing failed (later stages need a clean stream).
pub fn lex(source: &str) -> Result<LexOutput, Vec<PineError>> {
    let mut lexer = Lexer::new(source);
    lexer.run();
    if lexer.errors.is_empty() {
        Ok(LexOutput {
            tokens: lexer.tokens,
            version: lexer.version,
        })
    } else {
        Err(lexer.errors)
    }
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    errors: Vec<PineError>,
    version: Option<(u32, Span)>,
    /// Open `(`/`[` count; a newline inside brackets continues the line.
    bracket_depth: usize,
    /// Indentation column stack; the root level is 0.
    indents: Vec<usize>,
    /// Whether any significant token was emitted on the current logical line.
    line_has_content: bool,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            pos: 0,
            tokens: Vec::new(),
            errors: Vec::new(),
            version: None,
            bracket_depth: 0,
            indents: vec![0],
            line_has_content: false,
        }
    }

    fn run(&mut self) {
        // The first line's indentation counts too (a script must start at
        // column zero; anything deeper is a stray indent).
        self.handle_line_start();
        while self.pos < self.bytes.len() {
            let c = self.bytes[self.pos];
            match c {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\n' => {
                    self.pos += 1;
                    if self.continues_logical_line() {
                        // Same statement: swallow the newline, and the next
                        // line's leading whitespace is plain whitespace.
                        self.skip_blank_prefix();
                    } else {
                        self.end_logical_line();
                        self.handle_line_start();
                    }
                }
                b'/' if self.peek(1) == Some(b'/') => self.comment(),
                _ => self.token(),
            }
        }
        self.end_logical_line();
        // Close every open block at EOF.
        while self.indents.len() > 1 {
            self.indents.pop();
            self.push(Tok::Dedent, Span::at(self.pos));
        }
        self.push(Tok::Eof, Span::at(self.pos));
    }

    /// After a swallowed (continuation) newline: move past indentation and
    /// blank/comment-only lines without touching the indent stack.
    fn skip_blank_prefix(&mut self) {
        loop {
            while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\r')) {
                self.pos += 1;
            }
            match self.bytes.get(self.pos) {
                Some(b'\n') => self.pos += 1,
                Some(b'/') if self.peek(1) == Some(b'/') && !self.is_version_comment() => {
                    self.comment();
                }
                _ => return,
            }
        }
    }

    /// At the start of a fresh logical line: skip blank/comment-only lines,
    /// then measure indentation and emit Indent/Dedent.
    fn handle_line_start(&mut self) {
        loop {
            let line_start = self.pos;
            let mut column = 0usize;
            loop {
                match self.bytes.get(self.pos) {
                    Some(b' ') => {
                        column += 1;
                        self.pos += 1;
                    }
                    Some(b'\t') => {
                        column += TAB_WIDTH;
                        self.pos += 1;
                    }
                    Some(b'\r') => self.pos += 1,
                    _ => break,
                }
            }
            match self.bytes.get(self.pos) {
                // Blank line: no indentation meaning at all.
                Some(b'\n') => {
                    self.pos += 1;
                    continue;
                }
                None => return,
                // Comment-only line: also meaningless for layout (but the
                // version directive is still read).
                Some(b'/') if self.peek(1) == Some(b'/') => {
                    self.comment();
                    continue;
                }
                _ => {
                    self.apply_indentation(column, Span::new(line_start, self.pos));
                    return;
                }
            }
        }
    }

    fn apply_indentation(&mut self, column: usize, span: Span) {
        let current = *self.indents.last().expect("indent stack is never empty");
        if column > current {
            self.indents.push(column);
            self.push(Tok::Indent, span);
            return;
        }
        while column < *self.indents.last().expect("indent stack is never empty") {
            self.indents.pop();
            self.push(Tok::Dedent, span);
        }
        if column != *self.indents.last().expect("indent stack is never empty") {
            self.errors.push(PineError::new(
                ErrorCode::PineIndent,
                span,
                "dedent does not match any enclosing indentation level",
            ));
            // Recover: force-adopt the stray level so one bad line doesn't
            // cascade errors down the file.
            self.indents.push(column);
        }
    }

    /// Whether the just-ended physical line continues logically.
    fn continues_logical_line(&self) -> bool {
        if self.bracket_depth > 0 {
            return true;
        }
        matches!(
            self.tokens.last().map(|t| &t.tok),
            Some(
                Tok::Plus
                    | Tok::Minus
                    | Tok::Star
                    | Tok::Slash
                    | Tok::Percent
                    | Tok::Eq
                    | Tok::ColonEq
                    | Tok::EqEq
                    | Tok::NotEq
                    | Tok::Lt
                    | Tok::Le
                    | Tok::Gt
                    | Tok::Ge
                    | Tok::Question
                    | Tok::Colon
                    | Tok::Comma
                    | Tok::KwAnd
                    | Tok::KwOr
                    | Tok::KwNot
            )
        )
        // `=>` is deliberately NOT a continuation: a line ending in `=>` is
        // the multi-line function-body form, and the next indented line must
        // open a block, not continue the header.
    }

    fn end_logical_line(&mut self) {
        if self.line_has_content {
            self.push_raw(Tok::Newline, Span::at(self.pos.saturating_sub(1)));
            self.line_has_content = false;
        }
    }

    fn comment(&mut self) {
        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'\n' {
            self.pos += 1;
        }
        let text = &self.source[start..self.pos];
        if let Some(rest) = text.strip_prefix("//@version=") {
            let value = rest.trim();
            match value.parse::<u32>() {
                Ok(v) if self.version.is_none() => {
                    self.version = Some((v, Span::new(start, self.pos)));
                }
                Ok(_) => {}
                Err(_) => self.errors.push(PineError::new(
                    ErrorCode::PineVersion,
                    Span::new(start, self.pos),
                    format!("malformed version directive `{text}`"),
                )),
            }
        }
    }

    fn is_version_comment(&self) -> bool {
        self.source[self.pos..].starts_with("//@version=")
    }

    fn token(&mut self) {
        let start = self.pos;
        let c = self.bytes[self.pos];
        match c {
            b'0'..=b'9' => self.number(),
            b'"' | b'\'' => self.string(c),
            b'#' => self.color(),
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => self.ident(),
            _ => {
                self.pos += 1;
                let two = self.bytes.get(self.pos).copied();
                let tok = match (c, two) {
                    (b':', Some(b'=')) => {
                        self.pos += 1;
                        Some(Tok::ColonEq)
                    }
                    (b'=', Some(b'=')) => {
                        self.pos += 1;
                        Some(Tok::EqEq)
                    }
                    (b'=', Some(b'>')) => {
                        self.pos += 1;
                        Some(Tok::Arrow)
                    }
                    (b'!', Some(b'=')) => {
                        self.pos += 1;
                        Some(Tok::NotEq)
                    }
                    (b'<', Some(b'=')) => {
                        self.pos += 1;
                        Some(Tok::Le)
                    }
                    (b'>', Some(b'=')) => {
                        self.pos += 1;
                        Some(Tok::Ge)
                    }
                    (b'+', _) => Some(Tok::Plus),
                    (b'-', _) => Some(Tok::Minus),
                    (b'*', _) => Some(Tok::Star),
                    (b'/', _) => Some(Tok::Slash),
                    (b'%', _) => Some(Tok::Percent),
                    (b'=', _) => Some(Tok::Eq),
                    (b'<', _) => Some(Tok::Lt),
                    (b'>', _) => Some(Tok::Gt),
                    (b'?', _) => Some(Tok::Question),
                    (b':', _) => Some(Tok::Colon),
                    (b',', _) => Some(Tok::Comma),
                    (b'.', _) => Some(Tok::Dot),
                    (b'(', _) => {
                        self.bracket_depth += 1;
                        Some(Tok::LParen)
                    }
                    (b')', _) => {
                        self.bracket_depth = self.bracket_depth.saturating_sub(1);
                        Some(Tok::RParen)
                    }
                    (b'[', _) => {
                        self.bracket_depth += 1;
                        Some(Tok::LBracket)
                    }
                    (b']', _) => {
                        self.bracket_depth = self.bracket_depth.saturating_sub(1);
                        Some(Tok::RBracket)
                    }
                    _ => None,
                };
                match tok {
                    Some(tok) => self.push(tok, Span::new(start, self.pos)),
                    None => {
                        // The byte matched nothing, so it may be the first of
                        // a multi-byte character — an accented letter, a dash
                        // or the non-breaking space that rides along with any
                        // copy-paste from a web page. Advance by the whole
                        // scalar (the same idiom `string` uses) before
                        // slicing: `&str` indexing inside a character panics,
                        // and the one function whose job is to turn bad input
                        // into an honest error must never do that.
                        let ch = self.source[start..]
                            .chars()
                            .next()
                            .expect("start is a char boundary and in bounds");
                        self.pos = start + ch.len_utf8();
                        self.errors.push(PineError::new(
                            ErrorCode::PineLex,
                            Span::new(start, self.pos),
                            format!("unexpected character `{ch}`"),
                        ));
                    }
                }
            }
        }
    }

    fn number(&mut self) {
        let start = self.pos;
        let mut saw_dot = false;
        let mut saw_exp = false;
        while let Some(&c) = self.bytes.get(self.pos) {
            match c {
                b'0'..=b'9' => self.pos += 1,
                // A dot only joins the number when a digit follows —
                // otherwise it is the member/history operator (`5.foo` does
                // not occur, but `close[1].0` misparses would).
                b'.' if !saw_dot && !saw_exp => {
                    if matches!(self.bytes.get(self.pos + 1), Some(b'0'..=b'9')) {
                        saw_dot = true;
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                b'e' | b'E' if !saw_exp => {
                    let next = self.bytes.get(self.pos + 1);
                    let next2 = self.bytes.get(self.pos + 2);
                    let exp_ok = matches!(next, Some(b'0'..=b'9'))
                        || (matches!(next, Some(b'+' | b'-'))
                            && matches!(next2, Some(b'0'..=b'9')));
                    if exp_ok {
                        saw_exp = true;
                        self.pos += 2; // consume `e` and sign-or-digit
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        let text = &self.source[start..self.pos];
        let span = Span::new(start, self.pos);
        if saw_dot || saw_exp {
            match text.parse::<f64>() {
                Ok(v) => self.push(Tok::Float(v), span),
                Err(e) => self.errors.push(PineError::new(
                    ErrorCode::PineLex,
                    span,
                    format!("malformed number `{text}`: {e}"),
                )),
            }
        } else {
            match text.parse::<i64>() {
                Ok(v) => self.push(Tok::Int(v), span),
                // Too big for i64: promote to float rather than reject —
                // Pine numbers are float64 anyway (documented).
                Err(_) => match text.parse::<f64>() {
                    Ok(v) => self.push(Tok::Float(v), span),
                    Err(e) => self.errors.push(PineError::new(
                        ErrorCode::PineLex,
                        span,
                        format!("malformed number `{text}`: {e}"),
                    )),
                },
            }
        }
    }

    fn string(&mut self, quote: u8) {
        let start = self.pos;
        self.pos += 1;
        let mut value = String::new();
        loop {
            match self.bytes.get(self.pos) {
                None | Some(b'\n') => {
                    self.errors.push(PineError::new(
                        ErrorCode::PineLex,
                        Span::new(start, self.pos),
                        "unterminated string literal",
                    ));
                    return;
                }
                Some(&c) if c == quote => {
                    self.pos += 1;
                    break;
                }
                Some(b'\\') => {
                    let escaped = self.bytes.get(self.pos + 1).copied();
                    match escaped {
                        Some(b'n') => value.push('\n'),
                        Some(b't') => value.push('\t'),
                        Some(b'\\') => value.push('\\'),
                        Some(b'"') => value.push('"'),
                        Some(b'\'') => value.push('\''),
                        _ => {
                            self.errors.push(PineError::new(
                                ErrorCode::PineLex,
                                Span::new(self.pos, self.pos + 2),
                                "unknown escape sequence",
                            ));
                        }
                    }
                    self.pos += 2;
                }
                Some(_) => {
                    // Multi-byte UTF-8 advances by the full scalar.
                    let ch = self.source[self.pos..]
                        .chars()
                        .next()
                        .expect("in-bounds char");
                    value.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        self.push(Tok::Str(value), Span::new(start, self.pos));
    }

    fn color(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while matches!(
            self.bytes.get(self.pos),
            Some(b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
        ) {
            self.pos += 1;
        }
        let hex = &self.source[start + 1..self.pos];
        let span = Span::new(start, self.pos);
        let packed = match hex.len() {
            6 => u32::from_str_radix(hex, 16)
                .ok()
                .map(|rgb| (rgb << 8) | 0xFF),
            8 => u32::from_str_radix(hex, 16).ok(),
            _ => None,
        };
        match packed {
            Some(rgba) => self.push(Tok::ColorLit(rgba), span),
            None => self.errors.push(PineError::new(
                ErrorCode::PineLex,
                span,
                format!("malformed color literal `#{hex}` (expected #RRGGBB or #RRGGBBAA)"),
            )),
        }
    }

    fn ident(&mut self) {
        let start = self.pos;
        while matches!(
            self.bytes.get(self.pos),
            Some(b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
        ) {
            self.pos += 1;
        }
        let text = &self.source[start..self.pos];
        let tok = match text {
            "var" => Tok::KwVar,
            "varip" => Tok::KwVarip,
            "if" => Tok::KwIf,
            "else" => Tok::KwElse,
            "for" => Tok::KwFor,
            "to" => Tok::KwTo,
            "by" => Tok::KwBy,
            "while" => Tok::KwWhile,
            "switch" => Tok::KwSwitch,
            "and" => Tok::KwAnd,
            "or" => Tok::KwOr,
            "not" => Tok::KwNot,
            "true" => Tok::KwTrue,
            "false" => Tok::KwFalse,
            "na" => Tok::KwNa,
            _ => Tok::Ident(text.to_owned()),
        };
        self.push(tok, Span::new(start, self.pos));
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    /// Push a significant token (marks the logical line non-empty).
    fn push(&mut self, tok: Tok, span: Span) {
        self.line_has_content = true;
        self.push_raw(tok, span);
    }

    fn push_raw(&mut self, tok: Tok, span: Span) {
        self.tokens.push(Token { tok, span });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Tok> {
        lex(source)
            .expect("fixture lexes")
            .tokens
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn a_flat_script_is_lines_of_tokens() {
        let toks = kinds("a = 1\nb = a + 2.5\n");
        assert_eq!(
            toks,
            vec![
                Tok::Ident("a".into()),
                Tok::Eq,
                Tok::Int(1),
                Tok::Newline,
                Tok::Ident("b".into()),
                Tok::Eq,
                Tok::Ident("a".into()),
                Tok::Plus,
                Tok::Float(2.5),
                Tok::Newline,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn blocks_emit_indent_and_dedent() {
        let toks = kinds("if x\n    y = 1\nz = 2\n");
        assert_eq!(
            toks,
            vec![
                Tok::KwIf,
                Tok::Ident("x".into()),
                Tok::Newline,
                Tok::Indent,
                Tok::Ident("y".into()),
                Tok::Eq,
                Tok::Int(1),
                Tok::Newline,
                Tok::Dedent,
                Tok::Ident("z".into()),
                Tok::Eq,
                Tok::Int(2),
                Tok::Newline,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn eof_closes_every_open_block() {
        let toks = kinds("if x\n    if y\n        z = 1");
        let dedents = toks.iter().filter(|t| **t == Tok::Dedent).count();
        assert_eq!(dedents, 2);
        assert_eq!(toks.last(), Some(&Tok::Eof));
    }

    #[test]
    fn unclosed_brackets_continue_the_line() {
        let toks = kinds("f(a,\n   b)\n");
        assert!(
            !toks[..toks.len() - 2].contains(&Tok::Newline),
            "no newline inside the call: {toks:?}"
        );
        assert!(!toks.contains(&Tok::Indent), "no block: {toks:?}");
    }

    #[test]
    fn trailing_operator_continues_the_line() {
        let toks = kinds("a = 1 +\n     2\n");
        assert_eq!(
            toks,
            vec![
                Tok::Ident("a".into()),
                Tok::Eq,
                Tok::Int(1),
                Tok::Plus,
                Tok::Int(2),
                Tok::Newline,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn blank_and_comment_lines_do_not_disturb_blocks() {
        let toks = kinds("if x\n    a = 1\n\n    // note\n    b = 2\n");
        let indents = toks.iter().filter(|t| **t == Tok::Indent).count();
        let dedents = toks.iter().filter(|t| **t == Tok::Dedent).count();
        assert_eq!(indents, 1, "{toks:?}");
        assert_eq!(dedents, 1, "{toks:?}");
    }

    #[test]
    fn a_bad_dedent_is_a_pine_indent_error() {
        let errors = lex("if x\n        a = 1\n    b = 2\n").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, ErrorCode::PineIndent);
        let (line, _) = errors[0].span.line_col("if x\n        a = 1\n    b = 2\n");
        assert_eq!(line, 3);
    }

    #[test]
    fn version_directive_is_read_from_a_comment() {
        let out = lex("//@version=5\nplot(close)\n").expect("lexes");
        assert_eq!(out.version.map(|(v, _)| v), Some(5));
    }

    #[test]
    fn literals_cover_the_dialect() {
        let toks = kinds("x = \"hi\\n\" + 'y'\nc = #FF00AA\nd = #FF00AA80\ne = 1.5e3\n");
        assert!(toks.contains(&Tok::Str("hi\n".into())));
        assert!(toks.contains(&Tok::Str("y".into())));
        assert!(toks.contains(&Tok::ColorLit(0xFF00_AAFF)));
        assert!(toks.contains(&Tok::ColorLit(0xFF00_AA80)));
        assert!(toks.contains(&Tok::Float(1500.0)));
    }

    #[test]
    fn history_operator_tokens_after_ident() {
        let toks = kinds("y = close[1]\n");
        assert_eq!(
            toks,
            vec![
                Tok::Ident("y".into()),
                Tok::Eq,
                Tok::Ident("close".into()),
                Tok::LBracket,
                Tok::Int(1),
                Tok::RBracket,
                Tok::Newline,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn multiple_errors_are_all_reported() {
        let errors = lex("a = \"unterminated\nb = #ZZZ\n").unwrap_err();
        assert!(errors.len() >= 2, "{errors:?}");
    }

    #[test]
    fn spans_point_at_the_token() {
        let source = "abc = 42\n";
        let out = lex(source).expect("lexes");
        let ident = &out.tokens[0];
        assert_eq!(&source[ident.span.start..ident.span.end], "abc");
        let int = &out.tokens[2];
        assert_eq!(&source[int.span.start..int.span.end], "42");
    }
}
