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

/// The line in the baseline that caps the *sum* of every recorded ceiling.
///
/// A directive rather than a comment because the parser strips comments, and a
/// budget the parser cannot see is one that silently stops existing the day
/// somebody reflows the file.
pub const BUDGET_DIRECTIVE: &str = "!budget";

/// How far below the budget the recorded total may sit before the budget
/// itself has to come down.
///
/// Wider than [`SLACK`] on purpose. This number tracks every entry at once, so
/// ordinary tightening moves it constantly, and a budget needing a rewrite on
/// every extraction is a budget people delete. Narrow enough that it cannot
/// bank a feature: [`SLACK`] is what one file may hide, and the total is not
/// allowed to hide much more.
pub const BUDGET_SLACK: usize = 500;

/// What the guard asks for when the recorded total is over budget. Quoted by
/// the test and the binary alike, for the same reason as [`REMEDY`].
pub const BUDGET_REMEDY: &str = "The debt budget is the sum of every recorded ceiling — the one \
                                 number that says whether this repository's largest files are \
                                 getting better or worse. Individually signed raises cannot \
                                 answer that: eighteen entries each raised `for this branch` read \
                                 as eighteen reasonable decisions and one lost repository. So \
                                 growth is pay-as-you-go. A branch needing a ceiling raised moves \
                                 comparable code out of some debt file in the same change, and \
                                 the total does not move; extract, and both numbers fall on their \
                                 own. Raising the budget line itself stays available and is the \
                                 escape hatch on purpose — it is one number, in one place, that a \
                                 reviewer watches move, which a +115 buried among eighteen \
                                 entries never was.";

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

/// Which remedies the findings in hand actually call for.
///
/// The two are not interchangeable, and picking wrong is worse than saying
/// nothing: [`REMEDY`] tells an author to carve a module, which does not help a
/// branch whose files are each within their ceiling and whose *total* moved,
/// while [`BUDGET_REMEDY`] tells them to pay for a raise they may not have
/// made. A run that produced both gets both, in the order to act in — fix the
/// file, then settle the total.
pub fn remedies(findings: &[String]) -> Vec<&'static str> {
    let is_budget =
        |line: &String| line.contains(BUDGET_DIRECTIVE) || line.contains("nothing caps them");
    let mut out = Vec::new();
    // Everything that is not the budget's is one [`REMEDY`] speaks to: a file
    // over its ceiling, a stale entry, an unreadable path. Derived by
    // subtraction rather than by matching each wording, so a finding added
    // later arrives with a remedy instead of with silence — and silence here
    // reads exactly like "nothing to do".
    if findings.iter().any(|line| !is_budget(line)) {
        out.push(REMEDY);
    }
    if findings.iter().any(is_budget) {
        out.push(BUDGET_REMEDY);
    }
    out
}

/// One recorded ceiling, with the position that lets [`tighten`] rewrite it.
struct Entry {
    path: String,
    ceiling: usize,
    /// Index into the baseline file's lines, so a rewrite touches the number
    /// and leaves every comment where its author put it.
    line: usize,
}

/// The cap on the sum of every recorded ceiling, with the position that lets
/// [`tighten`] rewrite it.
struct Budget {
    allowed: usize,
    line: usize,
}

/// Everything the baseline file states: the per-file ceilings, and the cap on
/// their total.
struct Baseline {
    entries: Vec<Entry>,
    /// Absent only when the directive is missing, which is itself a finding.
    /// Parsed as an option rather than defaulted, because a default would make
    /// deleting the line the cheapest way past the budget — the guard would
    /// hand out its own bypass.
    budget: Option<Budget>,
}

impl Baseline {
    /// The recorded debt: what the repository has signed for, not what its
    /// files currently measure. Deliberately the ceilings rather than the
    /// counts — the budget rations *permission* to be large, so a file sitting
    /// under its ceiling still spends the whole entry until the entry is
    /// tightened, and [`SLACK`] is what bounds that gap.
    fn recorded(&self) -> usize {
        self.entries.iter().map(|entry| entry.ceiling).sum()
    }
}

/// Read the ceilings and the budget. Comments and blank lines are skipped;
/// anything else must be `path ceiling` or the [`BUDGET_DIRECTIVE`] line,
/// because a typo silently dropping an entry would leave a file unguarded and
/// looking green.
fn baseline(root: &Path) -> Result<Baseline, String> {
    let file = root.join(BASELINE_FILE);
    let text =
        fs::read_to_string(&file).map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
    let mut entries = Vec::new();
    let mut budget: Option<Budget> = None;
    for (line, raw) in text.lines().enumerate() {
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        if let Some(rest) = content.strip_prefix(BUDGET_DIRECTIVE) {
            let allowed = rest.trim().parse::<usize>().map_err(|e| {
                format!(
                    "{BASELINE_FILE}:{}: `{}` is not a count: {e}",
                    line + 1,
                    rest.trim()
                )
            })?;
            // Two budgets is not a harmless duplicate: whichever one loses is
            // a cap somebody wrote and nothing enforces, and the file gives no
            // hint which that was.
            if let Some(first) = &budget {
                return Err(format!(
                    "{BASELINE_FILE}:{}: a second `{BUDGET_DIRECTIVE}` — the first is on line {}, \
                     and only one of them could ever be the cap",
                    line + 1,
                    first.line + 1
                ));
            }
            budget = Some(Budget { allowed, line });
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
    Ok(Baseline { entries, budget })
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

/// Whether a workspace-relative path is one this guard tracks.
///
/// The single owner of that question, called by the walker below *and* by
/// [`check_file`], because those are the two surfaces the edit-time hook
/// trusts to say the same thing. `tests/` is out of scope because test code
/// is asked for rather than rationed; `target/` because it holds build output
/// and vendored sources, and asking an author to record a generated file in
/// the ratchet is how a guard becomes noise. The sibling guards skip
/// `target/` too — this one did not, until a review pointed at the
/// divergence.
fn tracked(relative: &str) -> bool {
    relative.starts_with("crates/")
        && relative.ends_with(".rs")
        && !relative
            .split('/')
            .any(|part| part == "tests" || part == "target")
}

/// What a walk of `crates/` found.
pub struct Measured {
    /// Production-line counts by workspace-relative path, sorted.
    pub counts: Vec<(String, usize)>,
    /// Paths that exist and could not be read at all. Reported rather than
    /// skipped: a file the guard cannot open is not a file it has cleared,
    /// and silence there is indistinguishable from a clean result.
    pub unreadable: Vec<String>,
    /// Tracked paths that were read but do not decode as UTF-8. Counted as
    /// *seen* even though they carry no line count, because the alternative
    /// is worse than useless: a file missing from [`Measured::counts`] looks
    /// to the stale-entry check like a file that no longer exists, and the
    /// remedy that check prints is "drop the stale entry" — which deletes the
    /// ceiling, after which the file is re-added at whatever size it has since
    /// grown to. The ratchet would have laundered a raise through its own
    /// instructions. The encoding guard reports what is actually wrong with
    /// these; this guard only has to avoid lying about them.
    pub undecodable: Vec<String>,
}

/// Every tracked `.rs` file under `crates`, as workspace-relative paths with
/// forward slashes so baseline entries read the same on every platform.
fn scan(dir: &Path, root: &Path, found: &mut Measured) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A directory that cannot be listed is not a directory that came back
        // clean. The original guard panicked here; returning quietly would
        // let a permission error or a locked tree produce a green run over
        // sources nobody looked at, which is the outcome this whole family of
        // guards exists to make impossible.
        Err(e) => {
            let relative = dir
                .strip_prefix(root)
                .unwrap_or(dir)
                .to_string_lossy()
                .replace('\\', "/");
            found
                .unreadable
                .push(format!("  {relative}/: directory could not be listed: {e}"));
            return;
        }
    };
    for entry in entries {
        let path = entry.expect("dir entry is readable").path();
        let relative = path
            .strip_prefix(root)
            .expect("scanned path sits under the workspace root")
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            if matches!(
                path.file_name().and_then(|n| n.to_str()),
                Some("tests" | "target")
            ) {
                continue;
            }
            scan(&path, root, found);
            continue;
        }
        if !tracked(&relative) {
            continue;
        }
        match fs::read(&path) {
            // Not valid UTF-8 is the encoding guard's finding, not this
            // one's, and it is skipped here rather than reported twice.
            // This used to be an `.expect`, which aborted the whole process
            // on the one input the encoding guard exists for — so the guard
            // that would have explained the file never got to run.
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(source) => found.counts.push((relative, production_lines(&source))),
                Err(_) => found.undecodable.push(relative),
            },
            Err(e) => found
                .unreadable
                .push(format!("  {relative}: could not be read: {e}")),
        }
    }
}

/// Production-line counts for every scanned file, sorted by path.
pub fn measure(root: &Path) -> Measured {
    let mut found = Measured {
        counts: Vec::new(),
        unreadable: Vec::new(),
        undecodable: Vec::new(),
    };
    scan(&root.join("crates"), root, &mut found);
    found.counts.sort();
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

/// How the recorded total stands against the budget.
///
/// Kept beside [`verdict`] and worded once, for the same reason: this is the
/// finding an author acts on, and two wordings of it would drift.
///
/// It reads the baseline alone and never the files, which is what makes the
/// rule pay-as-you-go rather than a second size check. Growth reaches this
/// function only once somebody has written a raise down — so a branch that
/// grows a file and *does not* raise its ceiling is caught by [`verdict`] as
/// it always was, and one that raises the ceiling honestly is caught here
/// unless it paid for the raise by extraction.
fn budget_verdict(recorded: &Baseline) -> Option<String> {
    let total = recorded.recorded();
    let Some(budget) = &recorded.budget else {
        return Some(format!(
            "  {BASELINE_FILE}: no `{BUDGET_DIRECTIVE}` line — the recorded ceilings total \
             {total} and nothing caps them. Restore the directive at {total} or lower; deleting \
             it is the one edit that switches pay-as-you-go off for every file at once"
        ));
    };
    if total > budget.allowed {
        return Some(format!(
            "  {BASELINE_FILE}:{}: the recorded ceilings total {total}, over the \
             {BUDGET_DIRECTIVE} of {} (+{}) — this branch raised a ceiling without lowering \
             another",
            budget.line + 1,
            budget.allowed,
            total - budget.allowed
        ));
    }
    if budget.allowed.saturating_sub(total) > BUDGET_SLACK {
        return Some(format!(
            "  {BASELINE_FILE}:{}: the recorded ceilings total {total}, down from the \
             {BUDGET_DIRECTIVE} of {} — good news, tighten the budget to {total}",
            budget.line + 1,
            budget.allowed
        ));
    }
    None
}

/// Every way the recorded baseline and the files on disk disagree.
pub fn check(root: &Path) -> Vec<String> {
    // Checked before anything is measured, because an unreadable `crates/`
    // measures as *empty* — and an empty measurement makes every baseline
    // entry look stale. The guard would then print eighteen findings whose
    // stated remedy is to delete the entries, which is the one edit that
    // switches the ratchet off on every large file in the repo.
    let sources = root.join("crates");
    if !sources.is_dir() {
        return vec![format!(
            "  {} is not a readable directory — there is nothing to measure, and every baseline \
             entry would otherwise be reported stale",
            sources.display()
        )];
    }
    let recorded = match baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![format!("  {problem}")],
    };
    let entries = &recorded.entries;
    let found = measure(root);
    let mut violations = found.unreadable.clone();

    for (path, actual) in &found.counts {
        let entry = entries.iter().find(|entry| &entry.path == path);
        violations.extend(verdict(entry, path, *actual));
    }

    violations.extend(budget_verdict(&recorded));

    for entry in entries {
        // "Seen" is wider than "counted". A file that was found but could not
        // be decoded or opened is present, not gone, and telling the author to
        // drop its entry would delete a ceiling over a file that still
        // exists — after which it is re-added at whatever size it has grown
        // to, laundering a raise through the guard's own instructions.
        let seen = found
            .counts
            .iter()
            .any(|(scanned, _)| scanned == &entry.path)
            || found.undecodable.contains(&entry.path)
            || found
                .unreadable
                .iter()
                .any(|line| line.contains(entry.path.as_str()));
        if !seen {
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
/// A path [`tracked`] rejects reports nothing — the same silence [`check`]
/// gives it. A tracked path that cannot be read reports *that*, rather than
/// nothing: the hook prints whatever comes back, and an empty result is what
/// an author reads as an all-clear.
pub fn check_file(root: &Path, relative: &str) -> Vec<String> {
    // The baseline is not a source file, so [`tracked`] rejects it — but it is
    // the file a raise is actually written into, and the edit-time hook seeing
    // every `.rs` change while missing the one edit that spends the budget
    // would report the symptom and never the act. Answered from the baseline
    // alone, with no file walk.
    if relative == BASELINE_FILE {
        return match baseline(root) {
            Ok(recorded) => budget_verdict(&recorded).into_iter().collect(),
            Err(problem) => vec![format!("  {problem}")],
        };
    }
    if !tracked(relative) {
        return Vec::new();
    }
    let bytes = match fs::read(root.join(relative)) {
        Ok(bytes) => bytes,
        Err(e) => return vec![format!("  {relative}: could not be read: {e}")],
    };
    // Not valid UTF-8 belongs to the encoding guard, which runs beside this
    // one over the same file and words it better.
    let Ok(source) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    let recorded = match baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![format!("  {problem}")],
    };
    let entry = recorded.entries.iter().find(|e| e.path == relative);
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
    let recorded = baseline(root)?;
    let entries = &recorded.entries;
    let found = measure(root);
    let file = root.join(BASELINE_FILE);
    let text =
        fs::read_to_string(&file).map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let mut applied = Vec::new();

    // The total as it will stand once every rewrite below has been applied,
    // accumulated as they are decided rather than re-read afterwards: the
    // rewritten text is not parsed again, so this is the only place the new
    // sum exists.
    let mut tightened_total = 0;

    for entry in entries {
        let Some((_, actual)) = found.counts.iter().find(|(path, _)| path == &entry.path) else {
            // An entry with no measured file keeps its ceiling and still
            // spends it. `check` reports it as stale; dropping it from the
            // total here would let a deleted file's budget quietly finance
            // the next raise.
            tightened_total += entry.ceiling;
            continue;
        };
        if entry.ceiling.saturating_sub(*actual) <= SLACK {
            tightened_total += entry.ceiling;
            continue;
        }
        tightened_total += *actual;
        // A trailing comment is carried across. The file header advertises
        // `#` and the parser honours it anywhere on the line, so an author
        // may well have written the justification for a ceiling *beside* it —
        // and that justification is the whole doctrine of this guard. A
        // rewrite that dropped it would delete the signed decision while
        // reporting only that a number went down.
        let trailing = lines[entry.line]
            .find('#')
            .map(|at| lines[entry.line][at..].to_owned());
        lines[entry.line] = match trailing {
            Some(comment) => format!("{} {actual}  {comment}", entry.path),
            None => format!("{} {actual}", entry.path),
        };
        applied.push(format!("  {}: {} -> {actual}", entry.path, entry.ceiling));
    }

    // The budget follows the ceilings down, and **only** down. Letting this
    // raise the number would turn `--tighten` into the bypass the whole
    // mechanism is built to deny: a branch over budget would run the command
    // the failure message recommends and have its raise signed by a tool
    // instead of by a person. So the strict `<` is the load-bearing character
    // in this function.
    if let Some(budget) = &recorded.budget
        && tightened_total < budget.allowed
    {
        let trailing = lines[budget.line]
            .find('#')
            .map(|at| lines[budget.line][at..].to_owned());
        lines[budget.line] = match trailing {
            Some(comment) => format!("{BUDGET_DIRECTIVE} {tightened_total}  {comment}"),
            None => format!("{BUDGET_DIRECTIVE} {tightened_total}"),
        };
        applied.push(format!(
            "  {BUDGET_DIRECTIVE}: {} -> {tightened_total}",
            budget.allowed
        ));
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

    /// A file with no test module counts whole.
    ///
    /// The trailing newline is a terminator, not a line: `str::lines` yields
    /// three items here, and a baseline generated by a script that splits on
    /// `\n` instead would sit one line high on exactly the files that have no
    /// test module — a line of unearned headroom, granted quietly. That
    /// happened while this guard was being written, to `layout_wiring.rs` and
    /// `compile.rs`, and this case is what caught it.
    #[test]
    fn production_lines_counts_every_line_when_there_is_no_test_module() {
        assert_eq!(production_lines("a\nb\nc\n"), 3);
        assert_eq!(production_lines("a\nb\nc"), 3);
    }

    /// The parse the data file bought, and the two ways it could go wrong: a
    /// comment read as an entry, or a trailing comment read into the count.
    #[test]
    fn baseline_parsing_ignores_comments() {
        let entries = baseline(&workspace_root())
            .expect("the baseline file parses")
            .entries;
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
        let entries = baseline(&workspace_root())
            .expect("the baseline file parses")
            .entries;
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

    /// A throwaway workspace: a baseline naming one source file, and that
    /// file. Named after its test rather than after the process id, because
    /// a reused pid leaves a populated directory behind and the test then
    /// fails on the previous run's contents.
    fn scratch(test: &str, ceiling: usize, lines: usize) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("quantick-guards-{test}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::create_dir_all(root.join("crates/probe/src")).expect("scratch dirs are creatable");
        fs::write(
            root.join(BASELINE_FILE),
            format!(
                "# an own-line comment the rewrite must not eat\n\
                 {BUDGET_DIRECTIVE} {ceiling}  # seeded at the one ceiling below\n\
                 crates/probe/src/big.rs {ceiling}  # and a trailing one, raised on purpose\n"
            ),
        )
        .expect("scratch baseline is writable");
        fs::write(root.join("crates/probe/src/big.rs"), "x\n".repeat(lines))
            .expect("scratch source is writable");
        root
    }

    /// The direction that needs no argument, applied. The ceiling comes down
    /// to the size the file actually is, and the comment above it survives —
    /// the rewrite works line by line precisely so the rationale a reviewer
    /// left behind is not the price of a tightening.
    #[test]
    fn tighten_lowers_a_ceiling_and_keeps_the_comments() {
        let root = scratch("tighten-lowers", 5_000, 100);
        let applied = tighten(&root).expect("the scratch baseline parses");

        assert_eq!(
            applied.len(),
            2,
            "the entry, and the budget that follows it down: {applied:?}"
        );
        let written = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(
            written.contains("crates/probe/src/big.rs 100"),
            "the ceiling was not lowered to the measured size: {written}"
        );
        assert!(
            written.contains("# an own-line comment the rewrite must not eat"),
            "the rewrite dropped an own-line comment: {written}"
        );
        // The case the first version lost: the parser accepts a comment
        // *beside* an entry, so the rewrite has to put it back. Testing only
        // the own-line spelling passed while this one silently deleted the
        // justification a reviewer had signed.
        assert!(
            written.contains("# and a trailing one, raised on purpose"),
            "the rewrite dropped a trailing comment: {written}"
        );
        assert!(
            written.contains(&format!("{BUDGET_DIRECTIVE} 100")),
            "the budget did not follow the ceiling down: {written}"
        );
        assert!(check(&root).is_empty(), "the scratch tree is clean after");
        let _ = fs::remove_dir_all(&root);
    }

    /// Growth is never automated. A file over its ceiling is the decision a
    /// reviewer has to be able to argue with, so `--tighten` must leave it
    /// exactly where it is rather than quietly writing the bigger number —
    /// which would hand back the invisibility the ratchet exists to remove.
    #[test]
    fn tighten_never_raises_a_ceiling() {
        let root = scratch("tighten-never-raises", 10, 100);
        let applied = tighten(&root).expect("the scratch baseline parses");

        assert!(
            applied.is_empty(),
            "growth must not be applied: {applied:?}"
        );
        let written = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(
            written.contains("crates/probe/src/big.rs 10"),
            "the ceiling moved: {written}"
        );
        assert_eq!(check(&root).len(), 1, "the violation is still reported");
        let _ = fs::remove_dir_all(&root);
    }

    /// The two surfaces that must never disagree. `check` walks the repo and
    /// `check_file` reads one file, and the edit-time hook trusts the second
    /// to say what the suite would have said. A file the hook calls clean
    /// while the suite calls it over its ceiling is worse than no hook: it
    /// reports an all-clear the author then acts on.
    #[test]
    fn check_file_agrees_with_the_whole_repo_scan() {
        let root = workspace_root();
        // Scanned once, outside the loop. The first draft called `check`
        // per file and took 13.7s, which would have made `cargo test -p
        // quantick-guards` slower than the thing this crate exists to speed
        // up — a test that quietly spends the win it is meant to protect.
        let whole = check(&root);
        // Every path the walk *saw*, not only the ones it could count. The
        // first version iterated `counts`, which by construction excludes the
        // files the two surfaces could actually disagree about — it proved
        // the invariant everywhere except where it had broken.
        let measured = measure(&root);
        let seen = measured
            .counts
            .iter()
            .map(|(path, _)| path.clone())
            .chain(measured.undecodable.iter().cloned());
        for path in seen {
            // Anchored on the finding's own `"  {path}: "` prefix rather than
            // matched as a substring: one tracked path being a substring of
            // another would otherwise attribute the wrong file's violation.
            let prefix = format!("  {path}: ");
            let from_scan: Vec<String> = whole
                .iter()
                .filter(|line| line.starts_with(&prefix))
                .cloned()
                .collect();
            assert_eq!(
                check_file(&root, &path),
                from_scan,
                "the single-file check and the repository scan disagree about {path}"
            );
        }
    }

    /// What the hook hands the binary is whatever path was just written, and
    /// most of them are not tracked. Each must come back silent rather than
    /// panicking on a missing file or scoring a test module. `target/` is in
    /// the list because the walker skips it, and a hook that reported a
    /// vendored source would be reporting something the suite never will.
    #[test]
    fn check_file_ignores_what_the_guard_does_not_track() {
        let root = workspace_root();
        for path in [
            "docs/README.md",
            "crates/guards/tests/guards.rs",
            "crates/app/target/debug/build/probe/src/vendored.rs",
            "Cargo.toml",
        ] {
            assert!(
                check_file(&root, path).is_empty(),
                "{path} is not tracked by the size guard and must report nothing"
            );
        }
    }

    /// A tracked path the guard cannot open is not a path it has cleared.
    /// The hook prints whatever comes back and nothing else, so returning an
    /// empty list here would put an all-clear in front of an author over a
    /// file that was never read — a root or path mismatch reading exactly
    /// like a clean result.
    #[test]
    fn check_file_says_so_when_a_tracked_path_cannot_be_read() {
        let findings = check_file(&workspace_root(), "crates/app/src/does_not_exist.rs");
        assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
        assert!(
            findings[0].contains("could not be read"),
            "the finding must say the file was unreadable: {findings:?}"
        );
    }

    /// A tracked file the guard cannot decode is *present*, and must never be
    /// reported as a stale baseline entry. The remedy that finding prints is
    /// "drop the stale entry", and an author who follows it deletes the
    /// ceiling, fixes the encoding, and gets the file re-added at whatever
    /// size it has since grown to — the ratchet laundering a raise through
    /// its own instructions.
    #[test]
    fn a_file_that_does_not_decode_is_not_reported_as_a_stale_entry() {
        let root = std::env::temp_dir().join("quantick-guards-undecodable");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::create_dir_all(root.join("crates/probe/src")).expect("scratch dirs are creatable");
        fs::write(
            root.join(BASELINE_FILE),
            "!budget 1600\ncrates/probe/src/big.rs 1600\n",
        )
        .expect("scratch baseline is writable");
        // Latin-1, so it is legal bytes and illegal UTF-8.
        fs::write(root.join("crates/probe/src/big.rs"), b"// caf\xe9\n")
            .expect("scratch source is writable");

        let measured = measure(&root);
        assert!(
            measured
                .undecodable
                .contains(&"crates/probe/src/big.rs".to_owned()),
            "the file should be recorded as seen-but-undecodable: {:?}",
            measured.undecodable
        );
        let findings = check(&root);
        assert!(
            !findings.iter().any(|line| line.contains("stale entry")),
            "an undecodable file must not be called a stale entry: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A root with no `crates/` measures as empty, and an empty measurement
    /// makes every baseline entry look stale. The guard must say what is
    /// actually wrong instead of printing eighteen findings whose remedy —
    /// delete the entries — would switch the ratchet off repo-wide.
    #[test]
    fn a_missing_sources_directory_is_named_rather_than_read_as_stale_entries() {
        let root = std::env::temp_dir().join("quantick-guards-missing-sources");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::write(
            root.join(BASELINE_FILE),
            "!budget 1600\ncrates/probe/src/big.rs 1600\n",
        )
        .expect("scratch baseline is writable");
        fs::remove_dir_all(root.join("crates")).expect("the sources directory is removable");

        let findings = check(&root);
        assert_eq!(findings.len(), 1, "expected one finding, got {findings:?}");
        assert!(
            findings[0].contains("nothing to measure"),
            "the finding must name the missing directory: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A throwaway workspace whose baseline names two debt files, so a raise
    /// on one can be paid for — or not — by the other. The single-entry
    /// [`scratch`] cannot express pay-as-you-go at all: with one ceiling, the
    /// total and the ceiling are the same number and every raise is a raise.
    fn scratch_pair(test: &str, first: usize, second: usize, budget: usize) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("quantick-guards-{test}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::create_dir_all(root.join("crates/probe/src")).expect("scratch dirs are creatable");
        fs::write(
            root.join(BASELINE_FILE),
            format!(
                "{BUDGET_DIRECTIVE} {budget}\n\
                 crates/probe/src/one.rs {first}\n\
                 crates/probe/src/two.rs {second}\n"
            ),
        )
        .expect("scratch baseline is writable");
        // Each file sits exactly at its ceiling, so nothing here trips the
        // per-file verdict and the only finding a test can see is the budget's.
        fs::write(root.join("crates/probe/src/one.rs"), "x\n".repeat(first))
            .expect("scratch source is writable");
        fs::write(root.join("crates/probe/src/two.rs"), "x\n".repeat(second))
            .expect("scratch source is writable");
        root
    }

    /// The failing case the budget exists for, and the one the per-file ratchet
    /// cannot see: a ceiling raised honestly, with a comment, and nothing given
    /// back. Every individual entry is within its ceiling — before the budget,
    /// `check` had no finding to make here at all.
    #[test]
    fn a_raise_that_pays_for_nothing_is_over_budget() {
        let root = scratch_pair("budget-unpaid", 2_100, 1_600, 3_600);
        let findings = check(&root);

        assert_eq!(findings.len(), 1, "expected only the budget: {findings:?}");
        assert!(
            findings[0].contains("+100") && findings[0].contains("without lowering another"),
            "the finding must name the overage and the act: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// The passing case, and the whole point of the rule: the same +100 on one
    /// file, paid for by -100 on another. The debt did not move, so the guard
    /// has nothing to say — a branch that extracts as it grows is never
    /// blocked, which is what keeps this from being a freeze.
    #[test]
    fn a_raise_paid_for_by_an_extraction_is_allowed() {
        let root = scratch_pair("budget-paid", 2_100, 1_500, 3_600);
        let findings = check(&root);
        assert!(
            findings.is_empty(),
            "paying for a raise must clear the guard: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// The escape hatch, working. A branch with genuinely nothing to extract
    /// raises the budget line itself and ships — the rule bounds the debt, it
    /// does not block the work. What it costs is visibility: one number, in one
    /// place, that a reviewer watches move.
    #[test]
    fn raising_the_budget_line_is_a_way_through() {
        let root = scratch_pair("budget-raised", 2_100, 1_600, 3_700);
        let findings = check(&root);
        assert!(
            findings.is_empty(),
            "a signed raise of the budget must be allowed: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Deleting the directive must not be cheaper than obeying it. Defaulting
    /// to "no budget" would have made the guard hand out its own bypass, so the
    /// absence is a finding that names the total nothing is capping.
    #[test]
    fn a_missing_budget_is_a_finding_rather_than_no_budget() {
        let root = scratch_pair("budget-missing", 2_100, 1_600, 3_700);
        let baseline_path = root.join(BASELINE_FILE);
        let text = fs::read_to_string(&baseline_path).expect("baseline is readable");
        let without: String = text
            .lines()
            .filter(|line| !line.starts_with(BUDGET_DIRECTIVE))
            .map(|line| format!("{line}\n"))
            .collect();
        fs::write(&baseline_path, without).expect("baseline is writable");

        let findings = check(&root);
        assert_eq!(findings.len(), 1, "expected one finding: {findings:?}");
        assert!(
            findings[0].contains("3700") && findings[0].contains("nothing caps them"),
            "the finding must name the uncapped total: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// Two budgets is not a harmless duplicate. Whichever one loses is a cap
    /// somebody wrote and nothing enforces, and the file gives no hint which —
    /// so the parse refuses rather than picking.
    #[test]
    fn a_second_budget_directive_is_refused() {
        let root = scratch_pair("budget-twice", 2_100, 1_600, 3_700);
        let baseline_path = root.join(BASELINE_FILE);
        let text = fs::read_to_string(&baseline_path).expect("baseline is readable");
        fs::write(&baseline_path, format!("{text}{BUDGET_DIRECTIVE} 99999\n"))
            .expect("baseline is writable");

        let findings = check(&root);
        assert_eq!(findings.len(), 1, "expected one finding: {findings:?}");
        assert!(
            findings[0].contains("a second"),
            "the parse must refuse a duplicate: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// The sharpest failure this mechanism could have had. `--tighten` is the
    /// command the guard's own remedy tells an author to reach for, so if it
    /// also raised the budget, the recommended fix would sign the raise a
    /// person was supposed to sign. It must leave an over-budget baseline
    /// exactly as it found it.
    #[test]
    fn tighten_never_raises_the_budget() {
        let root = scratch_pair("budget-tighten-up", 2_100, 1_600, 3_600);
        let applied = tighten(&root).expect("the scratch baseline parses");

        assert!(applied.is_empty(), "nothing was under slack: {applied:?}");
        let written = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(
            written.contains(&format!("{BUDGET_DIRECTIVE} 3600")),
            "the budget moved to cover the overage: {written}"
        );
        assert_eq!(
            check(&root).len(),
            1,
            "the branch is still over budget after --tighten"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A stale entry still spends its ceiling. Refunding it into the budget the
    /// moment its file stops being scanned would let a deleted file finance the
    /// next raise — the same laundering the stale-entry remedy already avoids.
    #[test]
    fn a_stale_entry_still_spends_its_ceiling() {
        let root = scratch_pair("budget-stale", 2_100, 1_600, 3_700);
        fs::remove_file(root.join("crates/probe/src/two.rs")).expect("the source is removable");

        let applied = tighten(&root).expect("the scratch baseline parses");
        assert!(applied.is_empty(), "nothing was under slack: {applied:?}");
        let written = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(
            written.contains(&format!("{BUDGET_DIRECTIVE} 3700")),
            "the vanished file's ceiling was refunded into the budget: {written}"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
