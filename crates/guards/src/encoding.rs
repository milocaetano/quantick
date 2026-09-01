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
//! happened once on `crates/app`, which is why this guard exists.
//!
//! The second guard here is the same shape of damage from the other common
//! tool: a scripted edit that joins two lines without the newline between
//! them, welding one doc comment onto the end of another. The result is still
//! a legal comment, so nothing downstream objects — rustfmt never reflows
//! comments and clippy never reads them — while `rustdoc` renders the two
//! sentences as one and the second item silently loses its documentation.
//! It also happened on `crates/app`, in a branch that made a dozen scripted
//! edits to one 9,000-line file.
//!
//! # Scope
//!
//! Every crate, not just the one where the damage first landed. The guard was
//! born inside `crates/app/tests/` and could only see that crate from there;
//! the codepage round-trip it exists to catch is a property of the editing
//! tool, not of the crate being edited, so the same accident in `pine` or
//! `control` was always just as possible and just as invisible. Moving the
//! guard to a crate of its own is what made the wider scan cost nothing.

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
/// `.txt` earns its place for one file: `crates/guards/size-baseline.txt`,
/// which `--tighten` rewrites by machine. A rewrite is precisely how a
/// codepage round-trip enters a file, and the em dashes in its rationale are
/// precisely what such a round-trip mangles.
const SCANNED: &[&str] = &["rs", "pine", "txt"];

/// The UTF-8 byte-order mark. Legal UTF-8, but nothing in this repo carries
/// one, and a tool that adds one has usually changed the encoding too.
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// The two files whose non-ASCII content is the mechanism rather than prose,
/// and which the wider scan reaches for the first time. This one lists the
/// mangled byte sequences it hunts for; the language guard's fixtures spell
/// their keywords in the accented uppercase that a `grep -i` recipe loses,
/// which is the difference its test pins. Both would be false positives, and a
/// guard that cries wolf is a guard somebody disables.
const ALLOWED: &[&str] = &[
    "crates/guards/src/encoding.rs",
    "crates/guards/src/language.rs",
];

fn scan(dir: &Path, root: &Path, violations: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // Reported, not skipped. The guard this replaced panicked here; a
        // quiet return would let a permission error or a locked tree produce
        // a green encoding verdict over sources nobody opened, which is the
        // one outcome this guard family exists to make impossible.
        Err(e) => {
            let relative = dir
                .strip_prefix(root)
                .unwrap_or(dir)
                .to_string_lossy()
                .replace('\\', "/");
            violations.push(format!("{relative}/: directory could not be listed: {e}"));
            return;
        }
    };
    for entry in entries {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            scan(&path, root, violations);
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if !in_scope(&relative) {
            continue;
        }
        inspect(&path, &relative, violations);
    }
}

/// Whether a workspace-relative path is one this guard reads. The single
/// owner of that question, called by the walker and by [`check_file`], so the
/// suite and the edit-time hook can never disagree about what is in scope —
/// the hook used to report findings under `target/` that the whole-repo scan
/// skipped, which is an advisory an author cannot make go away.
fn in_scope(relative: &str) -> bool {
    relative.starts_with("crates/")
        && !relative.split('/').any(|part| part == "target")
        && relative
            .rsplit_once('.')
            .is_some_and(|(_, ext)| SCANNED.contains(&ext))
}

/// The per-file half of the scan, shared with [`check_file`] so the
/// whole-repo run and the edit-time hook read the same file the same way.
fn inspect(path: &Path, relative: &str, violations: &mut Vec<String>) {
    let Ok(bytes) = fs::read(path) else {
        return;
    };
    if bytes.starts_with(BOM) {
        violations.push(format!("{relative}: starts with a UTF-8 BOM"));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        violations.push(format!("{relative}: is not valid UTF-8"));
        return;
    };
    // Hoisted out of the loop: the answer cannot change within a file, and
    // asking per line was ~a million redundant slice scans across the repo.
    // It also puts the exemption where its scope is legible — mojibake only,
    // never the BOM or the welded-join check below.
    let exempt_from_mojibake = ALLOWED.contains(&relative);
    for (line_no, line) in text.lines().enumerate() {
        if welded_doc_comments(line) {
            violations.push(format!(
                "{relative}:{}: two doc comments welded onto one line — an edit joined them \
                 without a newline",
                line_no + 1,
            ));
        }
        // The exemption is scoped to this loop and no further. It exists
        // because two files hold mangled byte sequences *as data*; it is not
        // a reason to stop checking them for a BOM or a welded join, and the
        // file most likely to be edited by the scripted tooling this guard
        // polices is this one.
        if exempt_from_mojibake {
            continue;
        }
        for pattern in MOJIBAKE {
            if line.contains(pattern) {
                violations.push(format!(
                    "{relative}:{}: `{pattern}` — a codepage round-trip mangled this line",
                    line_no + 1,
                ));
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
pub fn welded_doc_comments(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("/// ") else {
        return false;
    };
    rest.split_once("///")
        .is_some_and(|(before, _)| before.trim_end().ends_with(['.', ',', ';', ':', '—', ')']))
}

/// What the guard asks for beyond the list of violations.
pub const REMEDY: &str = "Source files were rewritten in the wrong encoding. Write them as UTF-8 \
                          without a BOM — in PowerShell, use \
                          [System.IO.File]::WriteAllBytes or an editor, never Set-Content.";

/// Every encoding accident found under `crates`.
pub fn check(root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    scan(&root.join("crates"), root, &mut violations);
    violations
}

/// The same check for one file. A path outside `crates/`, or with an
/// extension the guard does not read, reports nothing.
pub fn check_file(root: &Path, relative: &str) -> Vec<String> {
    if !in_scope(relative) {
        return Vec::new();
    }
    let mut violations = Vec::new();
    inspect(&root.join(relative), relative, &mut violations);
    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The join this catches, and the one it must not: a doc comment that
    /// quotes a path or a line of code has a `///` in its prose too, and
    /// flagging that would make the guard the thing people work around.
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
}
