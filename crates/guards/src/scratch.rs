//! Grep guard for temporary directories nobody owns.
//!
//! A test that spells `std::env::temp_dir()` itself gets two things wrong that
//! nothing else in the build can see.
//!
//! It names the folder after `std::process::id()`, because that is the only
//! unique-looking number `std` offers for free. A process id is *reused* —
//! Windows hands one back within minutes — so the folder a later run creates
//! is the folder an earlier run left behind, and the test asserts on the
//! previous run's files. The failure looks exactly like the change under
//! test. Three `paper_trading` tests failed that way, and the same shape has
//! taken app tests down in CI on `main`.
//!
//! And it removes nothing. `%TEMP%` on the host that runs this suite daily
//! held 450,227 `quantick-*` entries on 2026-09-04.
//!
//! Neither is visible to fmt, clippy, build or the suite: the leak is silent
//! by construction, and the collision only fires when the operating system
//! happens to reuse a number. So the rule is mechanical instead — **only a
//! crate's own scratch module may ask for the temporary directory**, and
//! those modules carry a run token no other run can reproduce plus a `Drop`
//! that removes the tree.
//!
//! # What this cannot see
//!
//! It reads text, not types: a helper that returns a path from an allowed
//! module and never removes it passes. The guard makes the *one* place where
//! temporary paths are minted explicit and reviewable, which is what makes
//! the removal reviewable too — not a proof that every test cleans up.

use std::fs;
use std::path::Path;

use crate::Finding;

/// What a test-side path has to go through. Every entry is a module whose
/// whole job is minting scratch paths that remove themselves; each carries
/// its own run token, because a crate that cannot depend on another (the
/// `guards` crate depends on nothing at all) cannot share one.
const SCRATCH_MODULES: &[&str] = &[
    "crates/app/src/scratch.rs",
    "crates/control-local/src/scratch.rs",
    "crates/guards/src/tempdir.rs",
    // `mcp` needs two: an integration test links the crate as a dependency
    // and cannot see its `#[cfg(test)]` items, so the unit tests and the
    // tests under `tests/` cannot share one module.
    "crates/mcp/src/scratch.rs",
    "crates/mcp/tests/common/mod.rs",
    "crates/replay/src/scratch.rs",
];

/// Files outside a scratch module that may still read the temporary
/// directory. Empty, and meant to stay that way: an entry here is a place the
/// rule does not reach, so each one needs a reason beside it saying why the
/// path it mints cannot leak and when the entry goes.
const ALLOWED: &[&str] = &[];

/// The call this guard hunts for.
///
/// The bare call rather than `env::temp_dir()`, because the qualified spelling
/// is only one of the ways to write it: `use std::env::temp_dir;` and a bare
/// `temp_dir()`, or `std::env :: temp_dir()`, are the same call and were both
/// invisible to a needle that insisted on the path. Matching the call itself
/// costs one rule — no source outside a scratch module may name a function
/// called `temp_dir` — which is why `chart_layers`'s local helper of that name
/// was renamed rather than exempted.
///
/// Written split so the guard's own source does not match it; the file would
/// otherwise be its own first finding.
const NEEDLE: &str = concat!("temp_", "dir(");

fn scan(dir: &Path, root: &Path, violations: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // Reported, not skipped: a permission error or a locked tree must not
        // produce a green verdict over sources nobody opened.
        Err(e) => {
            let relative = relative_to(dir, root);
            violations.push(format!("{relative}/: directory could not be listed: {e}"));
            return;
        }
    };
    for entry in entries {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            scan(&path, root, violations);
            continue;
        }
        let relative = relative_to(&path, root);
        if !in_scope(&relative) {
            continue;
        }
        inspect(&path, &relative, violations);
    }
}

/// A workspace-relative path with forward slashes, so a finding reads the
/// same on either platform.
fn relative_to(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether a workspace-relative path is one this guard reads. The single
/// owner of that question, called by the walker and by [`check_file`], so the
/// whole-repo run and the edit-time hook can never disagree about scope.
fn in_scope(relative: &str) -> bool {
    relative.starts_with("crates/")
        && relative.ends_with(".rs")
        && !relative.split('/').any(|part| part == "target")
        && !SCRATCH_MODULES.contains(&relative)
        && !ALLOWED.contains(&relative)
}

/// The per-file half of the scan, shared with [`check_file`].
fn inspect(path: &Path, relative: &str, violations: &mut Vec<String>) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for (line_no, line) in text.lines().enumerate() {
        // Comments are prose, and this guard's own documentation names the
        // call it hunts for. A commented-out call leaks nothing either.
        if line.trim_start().starts_with("//") {
            continue;
        }
        if line.contains(NEEDLE) {
            violations.push(format!(
                "{relative}:{}: asks for the temporary directory directly",
                line_no + 1,
            ));
        }
    }
}

/// What the guard asks for beyond the list of violations.
pub const REMEDY: &str = "A temporary path was minted outside its crate's scratch module. Take it \
                          from that module instead — `ScratchDir`/`ScratchFile` where a test can \
                          hold the value, `thread_dir` where the path is handed to something the \
                          test does not own — so it carries a run token and is removed.";

/// Every unowned temporary path found under `crates`.
pub fn check(root: &Path) -> Vec<Finding> {
    let mut violations = Vec::new();
    scan(&root.join("crates"), root, &mut violations);
    // One class of violation, one remedy.
    violations
        .into_iter()
        .map(|v| Finding::new(v, REMEDY))
        .collect()
}

/// The same check for one file. A path outside `crates/`, a scratch module,
/// or a non-Rust file reports nothing.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !in_scope(relative) {
        return Vec::new();
    }
    let mut violations = Vec::new();
    inspect(&root.join(relative), relative, &mut violations);
    violations
        .into_iter()
        .map(|v| Finding::new(v, REMEDY))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole rule, on a workspace of three files: an ordinary source that
    /// asks for the temporary directory is a finding, the scratch module that
    /// exists to ask is not, and neither is a file that never asks.
    #[test]
    fn only_a_scratch_module_may_ask_for_the_temporary_directory() {
        let root = crate::tempdir::TempDir::new("scratch-guard");
        fs::create_dir_all(root.join("crates/app/src")).expect("scratch dirs are creatable");
        fs::write(
            root.join("crates/app/src/scratch.rs"),
            format!("fn dir() {{ std::{NEEDLE}; }}\n"),
        )
        .expect("the scratch module is writable");
        fs::write(
            root.join("crates/app/src/leaky.rs"),
            format!("#[test]\nfn t() {{\n    let d = std::{NEEDLE}.join(\"x\");\n}}\n"),
        )
        .expect("the leaky test is writable");
        fs::write(
            root.join("crates/app/src/clean.rs"),
            "fn t() { let d = crate::scratch::ScratchDir::new(\"x\"); }\n",
        )
        .expect("the clean test is writable");

        let findings = check(root.path());
        assert_eq!(
            findings.len(),
            1,
            "one file asks and should not: {findings:#?}"
        );
        assert!(
            findings[0].line.starts_with("crates/app/src/leaky.rs:3:"),
            "the finding names the file and the line: {}",
            findings[0].line
        );
    }

    /// The spelling does not save a leak. A qualified call, an imported bare
    /// one, and a spaced-out path are the same call, and a needle that
    /// insisted on `env::` saw only the first — so the regression this guard
    /// exists to prevent could come back by changing an import.
    #[test]
    fn every_spelling_of_the_call_is_caught() {
        let root = crate::tempdir::TempDir::new("scratch-guard-spellings");
        fs::create_dir_all(root.join("crates/app/src")).expect("scratch dirs are creatable");
        // Built from `NEEDLE` rather than spelled out: a fixture that names
        // the call literally makes this file its own first finding.
        fs::write(
            root.join("crates/app/src/spellings.rs"),
            format!(
                "use std::env::temp_dir;
                 fn a() {{ let _ = std::env::{NEEDLE}); }}
                 fn b() {{ let _ = {NEEDLE}); }}
                 fn c() {{ let _ = std::env :: {NEEDLE}); }}
"
            ),
        )
        .expect("the source is writable");

        let lines: Vec<String> = check(root.path())
            .into_iter()
            .map(|finding| finding.line)
            .collect();
        assert_eq!(lines.len(), 3, "one per call, not per import: {lines:#?}");
        for (index, line) in lines.iter().enumerate() {
            assert!(
                line.starts_with(&format!("crates/app/src/spellings.rs:{}:", index + 2)),
                "the calls are on lines 2, 3 and 4: {line}"
            );
        }
    }

    /// The edit-time hook and the whole-repo scan answer the same for one
    /// file, including for a file the scan deliberately skips — a hook that
    /// reports what the suite does not is an advisory nobody can clear.
    #[test]
    fn check_file_agrees_with_the_scan_about_every_file() {
        let root = crate::tempdir::TempDir::new("scratch-guard-agree");
        fs::create_dir_all(root.join("crates/app/src")).expect("scratch dirs are creatable");
        for name in ["scratch.rs", "leaky.rs"] {
            fs::write(
                root.join(format!("crates/app/src/{name}")),
                format!("fn dir() {{ std::{NEEDLE}; }}\n"),
            )
            .expect("the source is writable");
        }
        for name in ["scratch.rs", "leaky.rs"] {
            let relative = format!("crates/app/src/{name}");
            let from_scan: Vec<_> = check(root.path())
                .into_iter()
                .filter(|finding| finding.line.starts_with(&relative))
                .collect();
            assert_eq!(
                check_file(root.path(), &relative),
                from_scan,
                "the hook and the scan disagree about {relative}"
            );
        }
    }

    /// The guard's own source names the call it hunts for, and must not
    /// report itself for doing so.
    #[test]
    fn the_guard_does_not_find_itself() {
        let findings = check(&crate::workspace_root());
        assert!(
            !findings
                .iter()
                .any(|finding| finding.line.starts_with("crates/guards/src/scratch.rs")),
            "the guard reported its own source: {findings:#?}"
        );
    }

    /// The rule as it actually stands in this repository: every temporary
    /// path is minted in a scratch module. The guard is worth nothing if the
    /// workspace it guards does not pass it.
    #[test]
    fn the_workspace_itself_is_clean() {
        let findings = check(&crate::workspace_root());
        assert!(findings.is_empty(), "{findings:#?}");
    }
}
