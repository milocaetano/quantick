//! How far the window reaches into the UI crate's production code, rationed.
//!
//! The target shape for a control port is Ports and Adapters with
//! `QuantickApp` as the composition root: a port is implemented by the owner
//! of the state it serves, a split-borrow adapter implements it only when
//! that state spans app-owned types, and `QuantickApp` builds adapters but
//! implements no domain methods. Three shapes coexisted on that path and an
//! agent copies the nearest one, so every new impl on the window was a
//! precedent for the next.
//!
//! So the reach is a ratchet. In `crates/app/src` production code it counts:
//!
//! - every `impl` block, inherent or trait, whose self type is the window:
//!   `QuantickApp`, `ControlWindow`, or `dyn ControlPort`, which
//!   `ControlWindow` names;
//! - every function outside those blocks with a parameter holding either by
//!   reference (`&`, `&mut`, `&'a mut`, `&mut impl`, any path prefix);
//! - every struct or enum with a field holding either by reference.
//!
//! Production code is what [`production`] leaves: comments and literal
//! contents blanked by the impl-spread lexer's mask, test files skipped, and
//! only an *item* under a bare `#[cfg(test)]` blanked. That lexer's own test
//! spans also swallow a `#[cfg(test)]` field or argument and whatever follows
//! it, and treat `cfg(any(feature = "...", test))` as test-only; either would
//! hide production reach, so this view does neither.
//!
//! The total must stay at the one signed ceiling, [`LABEL`] in
//! [`BASELINE_FILE`], on the shared [`Policy`]. The slack is zero: a reach
//! that falls is a finding until `--tighten` writes the new ceiling, so a
//! method moved to its owner cannot quietly come back.

use std::fs;
use std::path::Path;

use crate::Finding;
use crate::impl_spread::lex;
use crate::ratchet::{Policy, Unmeasured};
use crate::size;

/// The one signed ceiling and its budget.
pub const BASELINE_FILE: &str = "crates/guards/window-reach-baseline.txt";

/// The baseline entry holding the ceiling.
pub const LABEL: &str = "window-reach";

/// The tree the reach is counted in.
const SOURCE: &str = "crates/app/src";

/// The names the window goes by: the app, its control alias, and the trait
/// object that alias stands for.
const WINDOW: [&str; 3] = ["QuantickApp", "ControlWindow", "ControlPort"];

/// What the guard asks for when the reach grows.
pub const REMEDY: &str = "The window reaches further into crates/app than its signed ceiling. \
    The target shape is Ports and Adapters with QuantickApp as the composition root: a control \
    port is implemented by the owner of the state it serves, and the port asks for that owner — \
    as AlertDefinitionsPort::alert_runtime hands over quantick_stores::alerts::AlertRuntime. Only \
    when the state spans app-owned types does a split-borrow adapter implement the port, as \
    LayoutAdapter does in crates/app/src/app/layout_wiring.rs. QuantickApp builds adapters and \
    implements no domain methods: no new impl block for QuantickApp or ControlWindow, no new \
    function taking either by reference, no new struct holding either. Give the behaviour its \
    owner instead. A deliberate raise is a new file in crates/guards/window-reach-baseline.d/ \
    named for the branch, `window-reach +N` and `!budget +N` with a comment saying why, never an \
    edit to the baseline. A reach that fell needs no argument: \
    `cargo run -p quantick-guards -- --tighten` writes the new ceiling.";

/// What the guard asks for when the budget sits above the ceiling.
pub const BUDGET_SLACK_REMEDY: &str = "The window-reach ceiling fell and the !budget has not \
    caught up. Nothing has to be argued: `cargo run -p quantick-guards -- --tighten` writes the \
    new total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "crates/guards/window-reach-baseline.txt could not be read as \
    data, so the reach was not checked. Every line is blank, a `#` comment, the one \
    `!budget <count>` directive, or `window-reach <count>`. Fix the line the finding names.";

/// What the guard asks for when the tree could not be scanned whole.
pub const UNMEASURED_REMEDY: &str = "The window's reach could not be taken: a path under \
    crates/app/src could not be listed, read or decoded. Fix the path the finding names. Until \
    then the guard is not reporting a clean tree; it is reporting that it could not look.";

/// This guard's ratchet: the shared mechanism, with this guard's wording.
pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    // Zero: with no entry, any reach is unsigned.
    threshold: 0,
    // Zero both ways: one more impl on the window is a precedent, not noise.
    slack: 0,
    budget_slack: 0,
    budget_headroom: 0,
    unit: "places the window reaches",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// Every production `.rs` file under `dir`, sorted; directories that could
/// not be listed go to `missed`, never read as empty.
fn files(root: &Path, dir: &str, found: &mut Vec<String>, missed: &mut Vec<String>) {
    let listed = fs::read_dir(root.join(dir)).and_then(|entries| {
        entries
            .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
            .collect::<std::io::Result<Vec<_>>>()
    });
    let mut names = match listed {
        Ok(names) => names,
        Err(e) => {
            missed.push(format!("  {dir}/: directory could not be listed: {e}"));
            return;
        }
    };
    names.sort();
    for name in names {
        let path = format!("{dir}/{name}");
        if root.join(&path).is_dir() {
            if !matches!(name.as_str(), "tests" | "target") {
                files(root, &path, found, missed);
            }
        } else if name.ends_with(".rs") && !lex::is_test_file(&path) {
            found.push(path);
        }
    }
}

/// The reach of one file's production view.
pub fn reach_of(source: &str) -> usize {
    let text = production(source);
    let text = text.as_slice();
    let window_impls = window_impls(text);
    let inside = |at: usize| {
        window_impls
            .iter()
            .any(|(from, to)| *from <= at && at <= *to)
    };
    let functions = words(text, "fn")
        .filter(|at| !inside(*at))
        .filter_map(|at| parameters(text, at))
        .filter(|params| holds_window(params))
        .count();
    let holders = words(text, "struct")
        .chain(words(text, "enum"))
        .filter_map(|at| fields(text, at))
        .filter(|body| holds_window(body))
        .count();
    window_impls.len() + functions + holders
}

/// The items a bare `#[cfg(test)]` may stand on: blanking one of these
/// removes test code, while a field, argument or statement under the same
/// attribute stays counted rather than risk hiding what follows it.
const ITEMS: [&str; 12] = [
    "mod",
    "fn",
    "impl",
    "struct",
    "enum",
    "use",
    "const",
    "static",
    "type",
    "trait",
    "unsafe",
    "macro_rules",
];

/// One file's production view as bytes: comments and literal contents
/// blanked, then every item under a bare `#[cfg(test)]`, newlines kept.
fn production(source: &str) -> Vec<u8> {
    let source: Vec<char> = source.replace("\r\n", "\n").chars().collect();
    let masked: String = lex::mask(&source).into_iter().collect();
    let mut text = masked.into_bytes();
    let mut from = 0;
    while let Some(at) = find_bytes(&text, b"#[", from) {
        from = at + 2;
        let Some(end) = closing_bracket(&text, at + 1) else {
            continue;
        };
        let inner: String = String::from_utf8_lossy(&text[at + 2..end])
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if inner != "cfg(test)" {
            continue;
        }
        if let Some(item_end) = item_end(&text, end + 1) {
            for byte in &mut text[at..=item_end] {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            from = item_end + 1;
        }
    }
    text
}

fn find_bytes(text: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    text.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| from + offset)
}

/// The `]` closing the attribute bracket opened at `open`.
fn closing_bracket(text: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in text[open..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// The last byte of the item starting after an attribute at `from`, if it
/// is one of [`ITEMS`]: further attributes and a visibility are skipped, and
/// the item ends at its body's closing brace or its `;`.
fn item_end(text: &[u8], from: usize) -> Option<usize> {
    let mut at = skip_space(text, from);
    while text[at..].starts_with(b"#[") {
        at = skip_space(text, closing_bracket(text, at + 1)? + 1);
    }
    if text[at..].starts_with(b"pub") && text.get(at + 3).is_some_and(|b| !is_ident(*b)) {
        at = skip_space(text, at + 3);
        if text.get(at) == Some(&b'(') {
            at = skip_space(text, closing(text, at)? + 1);
        }
    }
    let word_end = at + text[at..].iter().take_while(|b| is_ident(**b)).count();
    let word = std::str::from_utf8(&text[at..word_end]).ok()?;
    if !ITEMS.contains(&word) {
        return None;
    }
    let mut depth = 0usize;
    for (offset, byte) in text[word_end..].iter().enumerate() {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Some(word_end + offset),
            b'{' if depth == 0 => return closing(text, word_end + offset),
            _ => {}
        }
    }
    None
}

/// The whole reach of `crates/app/src`.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    let (mut found, mut missed) = (Vec::new(), Vec::new());
    // A missing source tree is a scan that failed, never a reach of zero.
    files(root, SOURCE, &mut found, &mut missed);
    let mut total = 0;
    for path in &found {
        match fs::read(root.join(path)).map(String::from_utf8) {
            Ok(Ok(source)) => total += reach_of(&source),
            Ok(Err(_)) => missed.push(format!("  {path}: does not decode as UTF-8")),
            Err(e) => missed.push(format!("  {path}: could not be read: {e}")),
        }
    }
    if missed.is_empty() {
        Ok(total)
    } else {
        Err(Unmeasured { missed })
    }
}

/// Every way the tree and the recorded ceiling disagree.
pub fn check(root: &Path) -> Vec<Finding> {
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let reach = match measured(root) {
        Ok(reach) => reach,
        Err(unmeasured) => {
            return vec![Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY)];
        }
    };
    let mut findings: Vec<Finding> = POLICY
        .verdict(recorded.entry(LABEL), LABEL, reach)
        .into_iter()
        .collect();
    findings.extend(POLICY.budget_verdict(&recorded, 0));
    findings.extend(
        recorded
            .entries
            .iter()
            .filter(|entry| entry.path != LABEL)
            .map(|entry| POLICY.stale(&entry.path)),
    );
    findings
}

/// The edit-time question: the whole check, for an edit that can move the
/// reach — a production file under `crates/app/src` or the baseline.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    let counted =
        relative.starts_with(SOURCE) && size::tracked(relative) && !lex::is_test_file(relative);
    if counted || POLICY.owns(relative) {
        check(root)
    } else {
        Vec::new()
    }
}

/// Lower the ceiling to the reach, and the budget with it, when it fell.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let reach = measured(root).map_err(|unmeasured| format!("window-reach: {unmeasured}"))?;
    POLICY.tighten(root, &[(LABEL.to_owned(), reach)], 0)
}

fn is_ident(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Every offset where `word` stands as a whole word.
fn words<'a>(text: &'a [u8], word: &'a str) -> impl Iterator<Item = usize> + 'a {
    let needle = word.as_bytes();
    (0..text.len().saturating_sub(needle.len() - 1)).filter(move |&at| {
        text[at..].starts_with(needle)
            && (at == 0 || !is_ident(text[at - 1]))
            && text.get(at + needle.len()).is_none_or(|b| !is_ident(*b))
    })
}

/// The offset of the bracket closing the one opened at `open`.
fn closing(text: &[u8], open: usize) -> Option<usize> {
    let (opener, closer) = (
        text[open],
        match text[open] {
            b'{' => b'}',
            b'(' => b')',
            b'<' => b'>',
            _ => return None,
        },
    );
    let mut depth = 0usize;
    for (offset, byte) in text[open..].iter().enumerate() {
        if *byte == opener {
            depth += 1;
        } else if *byte == closer && !(closer == b'>' && text[open + offset - 1] == b'-') {
            depth -= 1;
            if depth == 0 {
                return Some(open + offset);
            }
        }
    }
    None
}

fn skip_space(text: &[u8], mut at: usize) -> usize {
    while text.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

/// The spans, header to closing brace, of every `impl` block whose self type
/// is the window. An `impl` counts only at the start of a line (after an
/// optional `unsafe`), so `impl Trait` in a signature never does.
fn window_impls(text: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for at in words(text, "impl") {
        let line_start = text[..at]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |newline| newline + 1);
        let prefix = String::from_utf8_lossy(&text[line_start..at]);
        if !matches!(prefix.trim(), "" | "unsafe") {
            continue;
        }
        let mut header = skip_space(text, at + 4);
        if text.get(header) == Some(&b'<') {
            let Some(end) = closing(text, header) else {
                continue;
            };
            header = end + 1;
        }
        let Some(brace) = text[header..].iter().position(|b| *b == b'{') else {
            continue;
        };
        let brace = header + brace;
        if text[header..brace].contains(&b';') {
            continue;
        }
        let mut head = &text[header..brace];
        if let Some(cut) = words(head, "where").next() {
            head = &head[..cut];
        }
        let self_type = match words(head, "for").last() {
            Some(cut) => &head[cut + 3..],
            None => head,
        };
        if names_window(self_type) {
            spans.push((at, closing(text, brace).unwrap_or(text.len())));
        }
    }
    spans
}

/// Whether an impl's self type is the window, by reference or not.
fn names_window(ty: &[u8]) -> bool {
    let at = skip_space(ty, 0);
    if ty.get(at) == Some(&b'&') {
        names_window_at(ty, at + 1)
    } else {
        names_window_at(ty, at)
    }
}

/// Whether the type at `at` — after an optional lifetime, `mut`, `dyn`,
/// `impl` and an opening parenthesis — is a path ending in one of [`WINDOW`].
fn names_window_at(text: &[u8], mut at: usize) -> bool {
    at = skip_space(text, at);
    if text.get(at) == Some(&b'\'') {
        at += 1;
        while text.get(at).is_some_and(|b| is_ident(*b)) {
            at += 1;
        }
        at = skip_space(text, at);
    }
    loop {
        let before = at;
        if text.get(at) == Some(&b'(') {
            at = skip_space(text, at + 1);
        }
        for keyword in [&b"mut"[..], b"dyn", b"impl"] {
            if text[at..].starts_with(keyword)
                && text.get(at + keyword.len()).is_some_and(|b| !is_ident(*b))
            {
                at = skip_space(text, at + keyword.len());
            }
        }
        if at == before {
            break;
        }
    }
    let start = at;
    while text.get(at).is_some_and(|b| is_ident(*b) || *b == b':') {
        at += 1;
    }
    let path = String::from_utf8_lossy(&text[start..at]);
    let last = path.rsplit("::").next().unwrap_or("");
    WINDOW.contains(&last)
}

/// The parameter list of the function named at `at` (the `fn` keyword), or
/// `None` for a function-pointer type, which names no function.
fn parameters(text: &[u8], at: usize) -> Option<&[u8]> {
    let name = skip_space(text, at + 2);
    if !text.get(name).is_some_and(|b| is_ident(*b)) {
        return None;
    }
    let mut open = name;
    while text.get(open).is_some_and(|b| is_ident(*b)) {
        open += 1;
    }
    open = skip_space(text, open);
    if text.get(open) == Some(&b'<') {
        open = skip_space(text, closing(text, open)? + 1);
    }
    if text.get(open) != Some(&b'(') {
        return None;
    }
    Some(&text[open..=closing(text, open)?])
}

/// The body of the struct or enum named at `at` (its keyword); `None` for a
/// unit struct.
fn fields(text: &[u8], at: usize) -> Option<&[u8]> {
    let body = text[at..]
        .iter()
        .position(|b| matches!(b, b'{' | b'(' | b';'))
        .map(|offset| at + offset)?;
    if text[body] == b';' {
        return None;
    }
    Some(&text[body..=closing(text, body)?])
}

/// Whether `text` holds the window by reference anywhere: `&`, then what
/// [`names_window_at`] accepts.
fn holds_window(text: &[u8]) -> bool {
    text.iter()
        .enumerate()
        .any(|(at, byte)| *byte == b'&' && names_window_at(text, at + 1))
}

#[cfg(test)]
mod tests;
