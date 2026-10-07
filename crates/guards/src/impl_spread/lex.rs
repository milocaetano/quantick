//! The production view `tools/outside_score/measure.py` takes of a Rust file,
//! ported function for function so the ratchet and the rubric count the same
//! thing.
//!
//! The rubric row this guard holds (`crate.<name>.impl_spread`, A3 in
//! `docs/quality/outside-score-rubric.md`) is defined by that script, so its
//! definition is the one to match: a lexer that blanks comments and literal
//! contents, then blanks every `#[cfg(test)]` item by brace matching. That is
//! not [`crate::size::production_source`], whose line rule keeps comments and
//! blank lines and sees only a column-0 `#[cfg(test)]`; reusing it would make
//! the guard and the score disagree on the same tree, which is the divergence
//! this port exists to rule out. Each function names the Python one it
//! mirrors, and works on `char`s because the script indexes code points.

/// `is_test_file`: a file only tests compile — under a `tests` directory, a
/// `tests.rs` or `*_tests.rs` sidecar, or inside a `*_tests` directory.
pub(super) fn is_test_file(relative: &str) -> bool {
    let parts: Vec<&str> = relative.split('/').collect();
    let Some((name, dirs)) = parts.split_last() else {
        return false;
    };
    dirs.contains(&"tests")
        || *name == "tests.rs"
        || name.ends_with("_tests.rs")
        || dirs.iter().any(|dir| dir.ends_with("_tests"))
}

/// The source as code points with comments, literal contents and every
/// test-only item blanked to spaces, newlines kept: `blank_spans(mask(src),
/// test_spans(...))`.
pub(super) fn production(source: &str) -> Vec<char> {
    let source: Vec<char> = source.replace("\r\n", "\n").chars().collect();
    let mut code = mask(&source);
    for (from, to) in test_spans(&code) {
        blank(&mut code, from, to);
    }
    code
}

/// `code_lines`: lines holding anything but whitespace.
pub(super) fn code_lines(text: &[char]) -> usize {
    text.split(|c| *c == '\n')
        .filter(|line| line.iter().any(|c| !c.is_whitespace()))
        .count()
}

/// `inherent_impls`: the type name of every inherent (non-trait) impl block,
/// in source order.
///
/// An `impl` counts only at the start of a line (after an optional `unsafe`),
/// so `impl Trait` in an argument or return position never does; a header
/// holding `for` is a trait impl; generics, a `where` clause, `&`, `mut` and
/// `dyn` are stripped, and the last path segment names the type.
pub(super) fn inherent_impls(production: &[char]) -> Vec<String> {
    let mut names = Vec::new();
    let mut from = 0;
    while let Some(at) = find_word(production, "impl", from) {
        from = at + 4;
        let line_start = production[..at]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |newline| newline + 1);
        let prefix: String = production[line_start..at].iter().collect();
        if !matches!(prefix.trim(), "" | "unsafe") {
            continue;
        }
        let Some(brace) = find(production, '{', from) else {
            continue;
        };
        if find(production, ';', from).is_some_and(|semi| semi < brace) {
            continue;
        }
        let header = strip_leading_generics(&production[from..brace]);
        let header = match find_word(header, "where", 0) {
            Some(cut) => &header[..cut],
            None => header,
        };
        if find_word(header, "for", 0).is_some() {
            continue;
        }
        if let Some(name) = type_name(header) {
            names.push(name);
        }
    }
    names
}

/// The last path segment of an impl header's type, once `&`, `mut`, `dyn`
/// and generic arguments are gone, if it starts with an identifier.
fn type_name(header: &[char]) -> Option<String> {
    let header: String = header.iter().collect();
    let header: Vec<char> = header.trim().trim_start_matches('&').chars().collect();
    let head: String = without_mut_and_dyn(&header).into_iter().collect();
    let head = head.split('<').next().unwrap_or("");
    let last = head
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .last()?;
    let name: String = last
        .chars()
        .enumerate()
        .take_while(|(index, c)| {
            c.is_ascii_alphabetic() || *c == '_' || (*index > 0 && c.is_ascii_digit())
        })
        .map(|(_, c)| c)
        .collect();
    (!name.is_empty()).then_some(name)
}

/// `re.sub(r"\b(?:mut|dyn)\s+", "", ...)`.
fn without_mut_and_dyn(text: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let keyword = (at == 0 || !is_ident_char(text[at - 1]))
            && (starts_with(text, at, "mut") || starts_with(text, at, "dyn"))
            && text.get(at + 3).is_some_and(|c| c.is_whitespace());
        if keyword {
            at += 3;
            while text.get(at).is_some_and(|c| c.is_whitespace()) {
                at += 1;
            }
            continue;
        }
        out.push(text[at]);
        at += 1;
    }
    out
}

/// `strip_leading_generics`: the header after a leading `<...>`, or the
/// header unchanged when it does not open with one. Index 0 is the `<`, so a
/// `>` always has a character before it.
fn strip_leading_generics(header: &[char]) -> &[char] {
    let start = header
        .iter()
        .position(|c| !c.is_whitespace())
        .unwrap_or(header.len());
    let trimmed = &header[start..];
    if trimmed.first() != Some(&'<') {
        return header;
    }
    let mut depth = 0i64;
    for (index, c) in trimmed.iter().enumerate() {
        match c {
            '<' => depth += 1,
            // The `>` of a `->` in a bound such as `F: Fn() -> bool` closes
            // nothing.
            '>' if trimmed[index - 1] != '-' => {
                depth -= 1;
                if depth == 0 {
                    return &trimmed[index + 1..];
                }
            }
            _ => {}
        }
    }
    trimmed
}

/// `is_ident_char`: what Python's `str.isalnum()` or `_` accepts.
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn starts_with(text: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, expected)| text.get(at + offset) == Some(&expected))
}

/// `str.find` for one character.
fn find(text: &[char], needle: char, from: usize) -> Option<usize> {
    text.get(from..)?
        .iter()
        .position(|c| *c == needle)
        .map(|offset| from + offset)
}

/// The first whole-word `word` (`\bword\b`) at or after `from`.
fn find_word(text: &[char], word: &str, from: usize) -> Option<usize> {
    (from..text.len()).find(|at| is_word_at(text, *at, word))
}

fn is_word_at(text: &[char], at: usize, word: &str) -> bool {
    let end = at + word.chars().count();
    starts_with(text, at, word)
        && (at == 0 || !is_ident_char(text[at - 1]))
        && text.get(end).is_none_or(|c| !is_ident_char(*c))
}

/// Every character in `from..to` except a newline, as a space.
fn blank(out: &mut [char], from: usize, to: usize) {
    let to = to.min(out.len());
    for c in &mut out[from.min(to)..to] {
        if *c != '\n' {
            *c = ' ';
        }
    }
}

/// `raw_start`: for a raw string opening at `at` (`r"`, `r#"`, `br#"`,
/// `cr"`), its content start and hash count.
fn raw_start(source: &[char], at: usize) -> Option<(usize, usize)> {
    let mut index = at;
    if matches!(source[index], 'b' | 'c') {
        index += 1;
    }
    if source.get(index) != Some(&'r') {
        return None;
    }
    index += 1;
    let mut hashes = 0;
    while source.get(index) == Some(&'#') {
        hashes += 1;
        index += 1;
    }
    (source.get(index) == Some(&'"')).then_some((index + 1, hashes))
}

/// `mask`: comments and literal contents blanked to spaces, newlines kept.
fn mask(source: &[char]) -> Vec<char> {
    let n = source.len();
    let mut out = source.to_vec();
    let mut at = 0;
    while at < n {
        let c = source[at];
        let starts_word = at == 0 || !is_ident_char(source[at - 1]);
        let next_is = |offset: usize, want: char| source.get(at + offset) == Some(&want);
        if starts_with(source, at, "//") {
            let end = find(source, '\n', at).unwrap_or(n);
            blank(&mut out, at, end);
            at = end;
        } else if starts_with(source, at, "/*") {
            let (mut depth, mut end) = (1, at + 2);
            while end < n && depth > 0 {
                if starts_with(source, end, "/*") {
                    depth += 1;
                    end += 2;
                } else if starts_with(source, end, "*/") {
                    depth -= 1;
                    end += 2;
                } else {
                    end += 1;
                }
            }
            blank(&mut out, at, end);
            at = end;
        } else if let Some((start, hashes)) = (matches!(c, 'r' | 'b' | 'c') && starts_word)
            .then(|| raw_start(source, at))
            .flatten()
        {
            let end = (start..n)
                .find(|from| {
                    source[*from] == '"' && (1..=hashes).all(|k| source.get(from + k) == Some(&'#'))
                })
                .unwrap_or(n);
            blank(&mut out, start, end);
            at = end + 1 + hashes;
        } else if c == '"' || (matches!(c, 'b' | 'c') && starts_word && next_is(1, '"')) {
            let start = if c == '"' { at + 1 } else { at + 2 };
            let mut end = start;
            while end < n && source[end] != '"' {
                end += if source[end] == '\\' { 2 } else { 1 };
            }
            blank(&mut out, start, end);
            at = end + 1;
        } else if c == '\'' || (c == 'b' && starts_word && next_is(1, '\'')) {
            let open = if c == '\'' { at + 1 } else { at + 2 };
            if open < n && source[open] == '\\' {
                let close = find(source, '\'', open + 2).unwrap_or(n);
                blank(&mut out, open, close);
                at = close + 1;
            } else if open + 1 < n && source[open + 1] == '\'' {
                blank(&mut out, open, open + 1);
                at = open + 2;
            } else {
                // A lifetime or a loop label.
                at = open;
            }
        } else {
            at += 1;
        }
    }
    out
}

/// `TEST_CFG`, matched just after a `cfg(`: `test`, or `test` anywhere at
/// the top level of `all(...)` / `any(...)`, with one level of nested group
/// such as `not(test)` consumed whole. The end of the match, if any.
fn test_cfg_end(code: &[char], after_open: usize) -> Option<usize> {
    let mut at = after_open;
    while code.get(at).is_some_and(|c| c.is_whitespace()) {
        at += 1;
    }
    if is_word_at(code, at, "test") {
        return Some(at + 4);
    }
    if !(starts_with(code, at, "all(") || starts_with(code, at, "any(")) {
        return None;
    }
    at += 4;
    while at < code.len() {
        if is_word_at(code, at, "test") {
            return Some(at + 4);
        }
        match code[at] {
            '(' => {
                let inner = (at + 1..code.len()).find(|k| matches!(code[*k], '(' | ')'))?;
                if code[inner] != ')' {
                    return None;
                }
                at = inner + 1;
            }
            ')' => return None,
            _ => at += 1,
        }
    }
    None
}

/// Every non-overlapping `opener` + [`test_cfg_end`] match, as offset ranges.
fn test_attributes(code: &[char], opener: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at < code.len() {
        if starts_with(code, at, opener)
            && let Some(end) = test_cfg_end(code, at + opener.chars().count())
        {
            found.push((at, end));
            at = end;
        } else {
            at += 1;
        }
    }
    found
}

/// `test_spans`: the offset range of every `#[cfg(test)]` item, attribute
/// included; an inner `#![cfg(test)]` makes the whole file test code.
fn test_spans(code: &[char]) -> Vec<(usize, usize)> {
    if !test_attributes(code, "#![cfg(").is_empty() {
        return vec![(0, code.len())];
    }
    let mut spans = Vec::new();
    for (start, end) in test_attributes(code, "#[cfg(") {
        let mut item = find(code, ']', end).map_or(end, |close| close + 1);
        loop {
            while code.get(item).is_some_and(|c| c.is_whitespace()) {
                item += 1;
            }
            if !starts_with(code, item, "#[") {
                break;
            }
            // An attribute that never closes does not compile; stop there
            // rather than loop as the script would.
            let Some(close) = find(code, ']', item) else {
                break;
            };
            item = close + 1;
        }
        spans.push((start, item_end(code, item)));
    }
    spans
}

/// `item_end`: past the item's closing brace or its `;`.
fn item_end(code: &[char], start: usize) -> usize {
    let mut parens = 0i64;
    for (index, c) in code.iter().enumerate().skip(start) {
        match c {
            '(' | '[' => parens += 1,
            ')' | ']' => parens -= 1,
            ';' if parens == 0 => return index + 1,
            '{' if parens == 0 => return match_brace(code, index),
            _ => {}
        }
    }
    code.len()
}

/// `match_brace`: just past the brace closing the one at `open`.
fn match_brace(code: &[char], open: usize) -> usize {
    let mut depth = 0i64;
    for (index, c) in code.iter().enumerate().skip(open) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    code.len()
}
