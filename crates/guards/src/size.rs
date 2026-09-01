//! Ratchet guard for the size of the trunk.
//!
//! `arch-review` dimension 1 asks whether a new capability can dock as a new
//! file plus one registration line, and recent features answered yes
//! honestly — `pointer_compass.rs` really is a new file. `app.rs` still grew
//! from 108 lines to over 36,000 in the five weeks after it was created,
//! monotonically, never once shrinking. The review measures the leaf and
//! never the trunk: nothing asks *where the registration lines accumulate*.
//! They accumulate in `QuantickApp` and in its constructor, because a
//! capability with no port is docked by hand — a field, an init, a draw call,
//! a hotkey — and every one of those four edits lands in the same file.
//!
//! Nothing else in the repo can see that. fmt, clippy, build and the whole
//! suite stay green while one file absorbs a crate, exactly as they stay
//! green through a Portuguese comment ([`crate::language`]) or a codepage
//! round-trip ([`crate::encoding`]). So the rule is enforced the same way
//! those are: mechanically, because a rule that lives only in a skill's
//! judgement drifts. `CLAUDE.md` already says as much about worktrees —
//! "enforced by hooks, not by memory".
//!
//! # What is measured, and what is deliberately not
//!
//! **Production lines only** — everything before a file's test module. Two
//! thirds of `app.rs` is `#[cfg(test)]`, and dimension 4 asks for exactly
//! that. A guard counting total lines would fire on a well-tested change and
//! teach the author to write fewer tests, which is worse than the disease.
//! Test code may grow without limit, and files under `tests/` are not tracked
//! at all, for the same reason.
//!
//! Keeping unit tests beside the code is the Rust convention, not an accident
//! of this repo: a child module sees its parent's private items, so a test in
//! the same file reaches a private function without widening its visibility,
//! and `#[cfg(test)]` compiles the whole module out of the shipped binary.
//! The convention assumes files stay navigable, which is the assumption this
//! guard restores. When a surface moves out to its own module, its tests move
//! with it — one extraction, both halves.
//!
//! # The ratchet has teeth in both directions
//!
//! A tracked file may not grow past its recorded ceiling, and may not sit far
//! *below* it either: slack left unclaimed is only headroom for the next
//! feature to refill silently, which is how the debt was run up the first
//! time. Shrinking a file is therefore expected to tighten its entry in the
//! same commit — and because that direction is always good news with the
//! right number already computed, [`tighten`] applies it for you.
//!
//! # Raising a ceiling is allowed — it just has to be signed
//!
//! This guard does not forbid growth; it forbids *invisible* growth. There
//! are two honest ways past a failure. The first, which the failure message
//! asks for, is to put the new code in its own module behind a port, as
//! `new-extension` describes. The second is to raise the number in
//! `size-baseline.txt` on purpose, with a comment saying why. That stays
//! legitimate: a reviewer sees a one-line diff saying "this file is allowed
//! to be bigger now" and can argue with it, which is precisely what a silent
//! +400 lines inside a 36,000-line file never let anyone do.

use std::fs;
use std::path::Path;

/// Production lines above which a file must carry a baseline entry. Files
/// below it are not the problem this guard exists for, and tracking them
/// would turn every ordinary edit into a baseline update — the reliable way
/// to get a guard disabled.
pub const THRESHOLD: usize = 1_500;

/// How far below its ceiling a tracked file may sit before the entry must be
/// tightened. Generous enough that ordinary churn stays quiet, small enough
/// that a real extraction cannot leave room for a whole feature behind it.
pub const SLACK: usize = 200;

/// The recorded ceilings, as a workspace-relative path.
pub const BASELINE_FILE: &str = "crates/guards/size-baseline.txt";

/// What the guard asks for beyond the list of violations, quoted by both the
/// test and the binary so a reader sees the same instruction either way.
pub const REMEDY: &str = "A file over its ceiling means a capability docked by editing the trunk \
                          instead of by adding a module. The fix asked for is the one in the \
                          new-extension skill: give the capability its own file and a port to \
                          dock into, so the edit here is a registration line rather than a body. \
                          Raising a ceiling on purpose is still allowed — change the number in \
                          crates/guards/size-baseline.txt and say why in a comment, so a reviewer \
                          argues with a visible decision instead of missing an invisible one. A \
                          file that shrank needs no argument at all: `cargo run -p \
                          quantick-guards -- --tighten` writes the new number.";

/// One recorded ceiling, with the position that lets [`tighten`] rewrite it.
struct Entry {
    path: String,
    ceiling: usize,
    /// Index into the baseline file's lines, so a rewrite touches the number
    /// and leaves every comment where its author put it.
    line: usize,
}

/// Read the ceilings. Comments and blank lines are skipped; anything else
/// must be `path ceiling`, because a typo silently dropping an entry would
/// leave a file unguarded and looking green.
fn baseline(root: &Path) -> Result<Vec<Entry>, String> {
    let file = root.join(BASELINE_FILE);
    let text =
        fs::read_to_string(&file).map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
    let mut entries = Vec::new();
    for (line, raw) in text.lines().enumerate() {
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let (path, ceiling) = content
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| format!("{BASELINE_FILE}:{}: expected `path ceiling`", line + 1))?;
        let ceiling = ceiling.parse::<usize>().map_err(|e| {
            format!(
                "{BASELINE_FILE}:{}: `{ceiling}` is not a count: {e}",
                line + 1
            )
        })?;
        entries.push(Entry {
            path: path.trim().to_owned(),
            ceiling,
            line,
        });
    }
    Ok(entries)
}

/// Lines of a source file that ship in the binary: every line that is not part
/// of a top-level `#[cfg(test)]` item.
///
/// The obvious implementation — stop counting at the first `#[cfg(test)]` — was
/// written first and was wrong in a way that mattered: fifteen files in this
/// repo carry a top-level `#[cfg(test)] use`, `#[cfg(test)] const` or
/// `#[cfg(test)] fn` *above* their test module, and truncating there scored
/// `control/gateway.rs` at 72 lines of 4,566 and `drawings/mod.rs` at 221 of
/// 4,455. The guard was blind on the largest files in the repo, and one
/// `#[cfg(test)] use` at the top of `app.rs` would have switched it off there
/// too. So the count skips each such item and keeps going.
///
/// Only column-0 attributes are considered. A `#[cfg(test)]` on a field or a
/// method inside an `impl` is indented and governs something that is already
/// inside an item being counted.
///
/// Test code above the module therefore costs nothing, and production code
/// below it is counted — which is the direction a ratchet should err in.
pub fn production_lines(source: &str) -> usize {
    let lines: Vec<&str> = source.lines().collect();
    let mut production = 0;
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != "#[cfg(test)]" {
            production += 1;
            index += 1;
            continue;
        }
        // Step over the attribute, then any further attributes or doc lines,
        // to reach the item itself.
        index += 1;
        while index < lines.len()
            && (lines[index].starts_with("#[")
                || lines[index].starts_with("//")
                || lines[index].is_empty())
        {
            index += 1;
        }
        let Some(item) = lines.get(index) else { break };
        // `use …;` and `mod tests;` end on their own line; everything else
        // opens a block that closes on a brace back in column 0.
        if item.ends_with(';') {
            index += 1;
            continue;
        }
        index += 1;
        while index < lines.len() && lines[index] != "}" && lines[index] != "};" {
            index += 1;
        }
        index += 1;
    }
    production
}

/// Every tracked `.rs` file under `crates`, as workspace-relative paths with
/// forward slashes so baseline entries read the same on every platform.
/// `tests/` is skipped whole: test code is asked for, not rationed.
fn scan(dir: &Path, root: &Path, found: &mut Vec<(String, usize)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("dir entry is readable").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "tests") {
                continue;
            }
            scan(&path, root, found);
            continue;
        }
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("source file is readable UTF-8");
        let relative = path
            .strip_prefix(root)
            .expect("scanned path sits under the workspace root")
            .to_string_lossy()
            .replace('\\', "/");
        found.push((relative, production_lines(&source)));
    }
}

/// Production-line counts for every scanned file, sorted by path.
pub fn measure(root: &Path) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    scan(&root.join("crates"), root, &mut found);
    found.sort();
    found
}

/// How one measured file stands against its recorded ceiling. The single
/// place the three verdicts are worded, so the whole-repo scan and the
/// single-file check the edit-time hook runs can never disagree about the
/// same file.
fn verdict(entry: Option<&Entry>, path: &str, actual: usize) -> Option<String> {
    match entry {
        Some(entry) if actual > entry.ceiling => Some(format!(
            "  {path}: {actual} production lines, ceiling {} (+{})",
            entry.ceiling,
            actual - entry.ceiling
        )),
        Some(entry) if entry.ceiling.saturating_sub(actual) > SLACK => Some(format!(
            "  {path}: down to {actual} from {} — good news, tighten the entry to {actual}",
            entry.ceiling
        )),
        None if actual > THRESHOLD => Some(format!(
            "  {path}: {actual} production lines, over the {THRESHOLD} threshold and absent from \
             the baseline — add `{path} {actual}`"
        )),
        _ => None,
    }
}

/// Every way the recorded baseline and the files on disk disagree.
pub fn check(root: &Path) -> Vec<String> {
    let entries = match baseline(root) {
        Ok(entries) => entries,
        Err(problem) => return vec![format!("  {problem}")],
    };
    let found = measure(root);
    let mut violations = Vec::new();

    for (path, actual) in &found {
        let entry = entries.iter().find(|entry| &entry.path == path);
        violations.extend(verdict(entry, path, *actual));
    }

    for entry in &entries {
        if !found.iter().any(|(scanned, _)| scanned == &entry.path) {
            violations.push(format!(
                "  {}: in the baseline but no longer scanned — drop the stale entry",
                entry.path
            ));
        }
    }

    violations
}

/// The same verdict for one file, without walking the repo. This is what the
/// edit-time hook calls: it reads one source file and the baseline, so it
/// answers in milliseconds rather than in the seconds a full scan takes.
///
/// A path outside `crates/`, or under a `tests/` directory, is not tracked
/// and reports nothing — the same silence [`check`] gives it.
pub fn check_file(root: &Path, relative: &str) -> Vec<String> {
    if !relative.starts_with("crates/")
        || !relative.ends_with(".rs")
        || relative.split('/').any(|part| part == "tests")
    {
        return Vec::new();
    }
    let Ok(source) = fs::read_to_string(root.join(relative)) else {
        return Vec::new();
    };
    let entries = match baseline(root) {
        Ok(entries) => entries,
        Err(problem) => return vec![format!("  {problem}")],
    };
    let entry = entries.iter().find(|entry| entry.path == relative);
    verdict(entry, relative, production_lines(&source))
        .into_iter()
        .collect()
}

/// Apply the one direction that never needs an argument: a file that shrank
/// more than [`SLACK`] below its ceiling has its entry rewritten to the size
/// it actually is. Growth is untouched — that is the decision a human signs.
///
/// Returns one line per entry rewritten.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let entries = baseline(root)?;
    let found = measure(root);
    let file = root.join(BASELINE_FILE);
    let text =
        fs::read_to_string(&file).map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut applied = Vec::new();

    for entry in &entries {
        let Some((_, actual)) = found.iter().find(|(path, _)| path == &entry.path) else {
            continue;
        };
        if entry.ceiling.saturating_sub(*actual) <= SLACK {
            continue;
        }
        lines[entry.line] = format!("{} {actual}", entry.path);
        applied.push(format!("  {}: {} -> {actual}", entry.path, entry.ceiling));
    }

    if !applied.is_empty() {
        let mut out = lines.join("\n");
        out.push('\n');
        fs::write(&file, out).map_err(|e| format!("{} is unwritable: {e}", file.display()))?;
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_root;

    /// The test module does not count, so test code may grow without moving a
    /// ceiling. This is the whole reason the guard counts production lines
    /// rather than the file's length.
    #[test]
    fn production_lines_skips_the_test_module() {
        let source = "fn ship() {}\n\n#[cfg(test)]\nmod tests {\n    fn a() {}\n    fn b() {}\n}\n";
        assert_eq!(production_lines(source), 2);
    }

    /// The case that made the first implementation worthless: a
    /// `#[cfg(test)] use` above the production code. Stopping at it scored
    /// `control/gateway.rs` at 72 lines of 4,566, so the guard reported
    /// nothing while the file was free to grow without limit.
    #[test]
    fn production_lines_counts_past_a_cfg_test_use() {
        let source =
            "use std::fs;\n#[cfg(test)]\nuse std::io;\n\nfn ship() {}\nfn also_ships() {}\n";
        // Six lines, less the attribute and the `use` it governs.
        assert_eq!(production_lines(source), 4);
    }

    #[test]
    fn production_lines_counts_every_line_when_there_is_no_test_module() {
        assert_eq!(production_lines("a\nb\nc\n"), 3);
        assert_eq!(production_lines("a\nb\nc"), 3);
    }

    /// The parse the data file bought, and the two ways it could go wrong: a
    /// comment read as an entry, or a trailing comment read into the count.
    #[test]
    fn baseline_parsing_ignores_comments() {
        let entries = baseline(&workspace_root()).expect("the baseline file parses");
        assert!(
            entries.iter().any(|e| e.path == "crates/app/src/app.rs"),
            "app.rs is the entry the guard was written for"
        );
        assert!(
            entries.iter().all(|e| !e.path.starts_with('#')),
            "a comment line was read as an entry"
        );
    }

    #[test]
    fn the_baseline_lists_no_path_twice() {
        let entries = baseline(&workspace_root()).expect("the baseline file parses");
        let mut seen: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            before,
            seen.len(),
            "the baseline lists a path more than once"
        );
    }
}
