//! Grep guard for source-file encoding.
//!
//! Rust sources here are UTF-8 without a BOM, and their comments and UI
//! strings are full of `—`, `·`, `×` and `§`. A tool that rewrites a file in
//! the system codepage — a PowerShell `Set-Content`/`Out-File` without
//! `-Encoding utf8` is all it takes — turns every one of those into mojibake
//! (`â€"`, `Â·`, `Ã—`, `Â§`) and may prepend a BOM.
//!
//! Nothing else in the repo can see that: mojibake is valid UTF-8, so fmt,
//! clippy, build and the whole suite stay green while the chart shows
//! `"Native Â· 1Ã—"` and the diff fills with hundreds of lines of noise. It
//! happened once on this crate, which is why this test exists.
//!
//! The second guard here is the same shape of damage from the other common
//! tool: a scripted edit that joins two lines without the newline between
//! them, welding one doc comment onto the end of another. The result is still
//! a legal comment, so nothing downstream objects — rustfmt never reflows
//! comments and clippy never reads them — while `rustdoc` renders the two
//! sentences as one and the second item silently loses its documentation.
//! It also happened on this crate, in a branch that made a dozen scripted
//! edits to one 9,000-line file.

use std::fs;
use std::path::Path;

/// Byte sequences a codepage round-trip leaves behind. Each is the UTF-8
/// encoding of a character that cannot occur in this repo's sources on its
/// own: `Â`, `Ã`, `â` and `Ð` only ever appear here as the first byte of a
/// mangled multi-byte character.
const MOJIBAKE: &[&str] = &["Â", "Ã", "â€", "âœ", "â†", "Ð"];

/// Extensions worth scanning. `.pine` is here for the same reason `.rs` is,
/// and arguably more: `crates/app/scripts/*.pine` hold the repo's densest
/// non-ASCII prose — trader-facing headers full of `—`, plus input titles
/// like `min body (×average)` that the settings dialog renders verbatim. A
/// folder-wide rewrite mangles a script and its byte-identical corpus copy
/// together, so the pin test that compares the two stays green while the
/// dialog fills with `Ã—`.
const SCANNED: &[&str] = &["rs", "pine"];

/// The UTF-8 byte-order mark. Legal UTF-8, but nothing in this repo carries
/// one, and a tool that adds one has usually changed the encoding too.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn scan(dir: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("source dir is readable") {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            scan(&path, violations);
            continue;
        }
        if path
            .extension()
            .is_none_or(|e| !SCANNED.iter().any(|ext| e == *ext))
        {
            continue;
        }
        let bytes = fs::read(&path).expect("source file is readable");
        if bytes.starts_with(BOM) {
            violations.push(format!("{}: starts with a UTF-8 BOM", path.display()));
        }
        let Ok(text) = String::from_utf8(bytes) else {
            violations.push(format!("{}: is not valid UTF-8", path.display()));
            continue;
        };
        for (line_no, line) in text.lines().enumerate() {
            if welded_doc_comments(line) {
                violations.push(format!(
                    "{}:{}: two doc comments welded onto one line — an edit joined                      them without the newline between",
                    path.display(),
                    line_no + 1,
                ));
            }
            for pattern in MOJIBAKE {
                if line.contains(pattern) {
                    violations.push(format!(
                        "{}:{}: `{}` — a codepage round-trip mangled this line",
                        path.display(),
                        line_no + 1,
                        pattern
                    ));
                }
            }
        }
    }
}

/// Whether `line` carries a second `///` after the first one's text.
///
/// Only *doc* comments, and only a second `///` that follows visible text on
/// the same line: `//// ` is a legal (if unusual) comment and a bare `///`
/// inside a doc comment's own prose — a path, a quoted line of Rust — has
/// non-space before it either way, so the check reads "one comment ends and
/// another begins", which is a thing no editor writes and only a bad join
/// produces.
fn welded_doc_comments(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("/// ") else {
        return false;
    };
    rest.split_once("///")
        .is_some_and(|(before, _)| before.trim_end().ends_with(['.', ',', ';', ':', '—', ')']))
}

#[test]
fn sources_are_utf8_without_a_bom_or_mojibake() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    scan(&crate_dir.join("src"), &mut violations);
    scan(&crate_dir.join("scripts"), &mut violations);
    assert!(
        violations.is_empty(),
        "source files were rewritten in the wrong encoding (write them as UTF-8 without a \
         BOM — in PowerShell, use [System.IO.File]::WriteAllBytes or an editor, never \
         Set-Content):\n{}",
        violations.join("\n")
    );
}

/// The join this catches, and the one it must not: a doc comment that quotes
/// a path or a line of code has a `///` in its prose too, and flagging that
/// would make the guard the thing people work around.
#[test]
fn a_welded_join_is_told_apart_from_a_doc_comment_that_quotes_one() {
    assert!(welded_doc_comments(
        "    /// A sentence that ended.    /// And the next item's own line."
    ));
    assert!(welded_doc_comments(
        "/// Ends with a comma,   /// then another."
    ));
    assert!(!welded_doc_comments(
        "/// A comment mentioning `///` inline."
    ));
    assert!(!welded_doc_comments(
        "    /// An ordinary line of documentation."
    ));
    assert!(!welded_doc_comments("// not a doc comment /// at all"));
}
