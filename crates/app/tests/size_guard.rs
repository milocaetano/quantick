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
//! green through a Portuguese comment (`language_guard.rs`) or a codepage
//! round-trip (`source_encoding_guard.rs`). So the rule is enforced the same
//! way those are: mechanically, because a rule that lives only in a skill's
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
//! same commit.
//!
//! # Raising a ceiling is allowed — it just has to be signed
//!
//! This guard does not forbid growth; it forbids *invisible* growth. There
//! are two honest ways past a failure. The first, which the failure message
//! asks for, is to put the new code in its own module behind a port, as
//! `new-extension` describes. The second is to raise the number here on
//! purpose, with a comment saying why. That stays legitimate: a reviewer sees
//! a one-line diff saying "this file is allowed to be bigger now" and can
//! argue with it, which is precisely what a silent +400 lines inside a
//! 36,000-line file never let anyone do.

use std::fs;
use std::path::{Path, PathBuf};

/// Production lines above which a file must carry a [`BASELINE`] entry. Files
/// below it are not the problem this guard exists for, and tracking them
/// would turn every ordinary edit into a baseline update — the reliable way
/// to get a guard disabled.
const THRESHOLD: usize = 1_500;

/// How far below its ceiling a tracked file may sit before the entry must be
/// tightened. Generous enough that ordinary churn stays quiet, small enough
/// that a real extraction cannot leave room for a whole feature behind it.
const SLACK: usize = 200;

/// The recorded ceiling, in production lines, for every file over
/// [`THRESHOLD`]. These are the sizes on the day the guard was written, not
/// targets: each entry is debt, and the only direction one should ever move
/// is down. `app.rs` at the top of the list is the reason the file exists.
const BASELINE: &[(&str, usize)] = &[
    // `app.rs` is the reason this file exists. Tightened twice now. The
    // guard's own change took the agent popup, the acknowledgement toast and
    // the Save-as box out to `src/surfaces/`; the second batch took the
    // indicator-preview watermark, the appearance window, the footprint
    // settings window, the market dialog and the arming dialog with its alarm
    // section — 423 production lines and nine struct fields — and gave back
    // the environment the port hands them instead.
    ("crates/app/src/app.rs", 11228),
    // The five entries below `pane.rs` were invisible to the first version of
    // this guard, which stopped counting at the first `#[cfg(test)]` of any
    // kind: `gateway.rs` scored 72 lines of its 4,142, `drawings/mod.rs` 221
    // of 2,283. They are recorded at today's true size, not at a target.
    // `paper_trading.rs` came down two lines when its private toast — a
    // second acknowledgement lane in the same place on a different clock —
    // was converged onto the window's `ToastSurface`; what left was mostly
    // drawing code, and what stayed is the outbox that replaced it.
    ("crates/app/src/paper_trading.rs", 8859),
    ("crates/app/src/pane.rs", 7757),
    ("crates/app/src/tab.rs", 4401),
    ("crates/app/src/control/gateway.rs", 4142),
    ("crates/app/src/orderflow_render.rs", 3075),
    ("crates/app/src/orderflow_view.rs", 2485),
    ("crates/app/src/drawings/mod.rs", 2283),
    ("crates/app/src/footprint_render.rs", 2234),
    ("crates/feed-mt5/src/stream.rs", 2142),
    ("crates/app/src/toolrail.rs", 2114),
    ("crates/app/src/orderflow/projection.rs", 1850),
    ("crates/app/src/control/contract.rs", 1718),
    ("crates/app/src/control/evidence.rs", 1708),
    ("crates/pine/src/eval.rs", 1669),
    ("crates/app/src/app/layout_wiring.rs", 1557),
    ("crates/pine/src/compile.rs", 1540),
    ("crates/app/src/orderflow/config.rs", 1523),
];

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
fn production_lines(source: &str) -> usize {
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
/// forward slashes so [`BASELINE`] entries read the same on every platform.
/// `tests/` is skipped whole: test code is asked for, not rationed.
fn scan(dir: &Path, root: &Path, found: &mut Vec<(String, usize)>) {
    for entry in fs::read_dir(dir).expect("source dir is readable") {
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

#[test]
fn no_tracked_file_grows_past_its_recorded_ceiling() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .to_path_buf();

    let mut found = Vec::new();
    scan(&root.join("crates"), &root, &mut found);
    found.sort();

    let mut violations = Vec::new();

    for (path, actual) in &found {
        match BASELINE.iter().find(|(tracked, _)| tracked == path) {
            Some((_, ceiling)) if actual > ceiling => violations.push(format!(
                "  {path}: {actual} production lines, ceiling {ceiling} (+{})",
                actual - ceiling
            )),
            Some((_, ceiling)) if ceiling.saturating_sub(*actual) > SLACK => {
                violations.push(format!(
                    "  {path}: down to {actual} from {ceiling} — good news, tighten the entry to \
                     {actual}"
                ))
            }
            None if *actual > THRESHOLD => violations.push(format!(
                "  {path}: {actual} production lines, over the {THRESHOLD} threshold and absent \
                 from BASELINE — add (\"{path}\", {actual})"
            )),
            _ => {}
        }
    }

    for (path, _) in BASELINE {
        if !found.iter().any(|(scanned, _)| scanned == path) {
            violations.push(format!(
                "  {path}: in BASELINE but no longer scanned — drop the stale entry"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "file sizes moved away from their recorded baseline:\n{}\n\nA file over its ceiling means \
         a capability docked by editing the trunk instead of by adding a module. The fix asked \
         for is the one in the new-extension skill: give the capability its own file and a port \
         to dock into, so the edit here is a registration line rather than a body. Raising a \
         ceiling on purpose is still allowed — change the number and say why in a comment, so a \
         reviewer argues with a visible decision instead of missing an invisible one.",
        violations.join("\n")
    );
}

/// The test module does not count, so test code may grow without moving a
/// ceiling. This is the whole reason the guard counts production lines rather
/// than the file's length.
#[test]
fn production_lines_skips_the_test_module() {
    let source = "fn ship() {}\n\n#[cfg(test)]\nmod tests {\n    fn a() {}\n    fn b() {}\n}\n";
    assert_eq!(production_lines(source), 2);
}

/// The case that made the first implementation worthless: a `#[cfg(test)] use`
/// above the production code. Stopping at it scored `control/gateway.rs` at 72
/// lines of 4,566, so the guard reported nothing while the file was free to
/// grow without limit.
#[test]
fn production_lines_counts_past_a_cfg_test_use() {
    let source = "use std::fs;\n#[cfg(test)]\nuse std::io;\n\nfn ship() {}\nfn also_ships() {}\n";
    // Six lines, less the attribute and the `use` it governs.
    assert_eq!(production_lines(source), 4);
}

/// The same hole in its other spelling: a `#[cfg(test)]` helper *function*
/// above the test module, which is what hid 1,250 production lines of
/// `paper_trading.rs` behind a ceiling of 7,611.
#[test]
fn production_lines_counts_past_a_cfg_test_helper() {
    let source = concat!(
        "fn ship() {}\n",
        "#[cfg(test)]\n",
        "fn helper() {\n",
        "    let _ = 1;\n",
        "}\n",
        "fn ships_too() {}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn t() {}\n",
        "}\n",
    );
    assert_eq!(production_lines(source), 2);
}

/// An attribute stack above the test item must not swallow the item itself.
#[test]
fn production_lines_steps_over_stacked_attributes() {
    let source = concat!(
        "fn ship() {}\n",
        "#[cfg(test)]\n",
        "#[allow(clippy::pedantic)]\n",
        "mod tests {\n",
        "    fn t() {}\n",
        "}\n",
    );
    assert_eq!(production_lines(source), 1);
}

/// A `#[cfg(test)]` on a field or method *inside* an item is indented, and
/// `app.rs` carries dozens of them. They govern something already inside a
/// counted item, so only column-0 attributes are considered.
#[test]
fn production_lines_ignores_an_indented_cfg_test() {
    let source =
        "struct App {\n    #[cfg(test)]\n    probe: bool,\n}\n\n#[cfg(test)]\nmod tests {\n}\n";
    assert_eq!(production_lines(source), 5);
}

/// A file with no test module at all counts whole, so a pure-production file
/// cannot slip under the threshold by omitting tests.
///
/// The trailing newline is a terminator, not a line: `str::lines` yields three
/// items here, and a baseline generated by a script that splits on `\n`
/// instead would sit one line high on exactly the files that have no test
/// module — a line of unearned headroom, granted quietly. That happened while
/// this guard was being written, to `layout_wiring.rs` and `compile.rs`, and
/// this case is what caught it.
#[test]
fn production_lines_counts_a_file_with_no_test_module_whole() {
    assert_eq!(production_lines("a\nb\nc\n"), 3);
    assert_eq!(production_lines("a\nb\nc"), 3);
}

/// The lookup takes the first matching entry, so a duplicated path would leave
/// the second — usually the tighter one a later branch added — dead, with
/// nothing saying so. This file is meant to be edited often, which is exactly
/// how two merges end up appending the same path.
#[test]
fn baseline_names_each_file_once() {
    let mut seen: Vec<&str> = BASELINE.iter().map(|(path, _)| *path).collect();
    let before = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(before, seen.len(), "BASELINE lists a path more than once");
}
