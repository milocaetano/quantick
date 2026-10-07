//! How many files one type's inherent `impl` blocks may span, rationed.
//!
//! The size ratchet caps a file and cannot see a type. A type that absorbed a
//! subsystem satisfies it by spreading: one more `impl ChartPane { … }` in one
//! more file keeps every file under 1,500 lines while the type grows without
//! bound, and every file that holds a piece of it reaches into the same
//! private state. Splitting a hub that way is file-size proxy gaming — the
//! files shrink and the coupling does not. The outside-score rubric grades
//! exactly this (A3 in `docs/quality/outside-score-rubric.md`: the largest
//! `impl_spread` among crates over 20,000 production lines), and nothing
//! failed when it grew.
//!
//! So it is a ratchet now. For every crate over [`QUALIFYING_LINES`]
//! production lines, each type is counted by the distinct production files
//! that hold an inherent `impl` block of it, and the widest count must stay
//! at or under the one signed ceiling, [`LABEL`] in [`BASELINE_FILE`], on the
//! shared [`Policy`]. Every type over the ceiling is its own finding. The
//! slack is zero: a spread that falls is a finding until `--tighten` writes
//! the new ceiling, so a file folded back cannot quietly be split out again.
//!
//! # What is counted
//!
//! The definition is `tools/outside_score/measure.py`'s, ported in [`lex`]
//! so the guard and the score never disagree about one tree: test files and
//! `#[cfg(test)]` items are out, a crate's production lines are its non-blank
//! code lines once comments and literals are blanked, and an inherent impl is
//! an `impl` at the start of a line whose header holds no `for`. Trait impls
//! are not counted — implementing a trait in the module that needs it is a
//! port, the shape the remedy asks for — and neither is `impl Trait` in an
//! argument or return type. The type is the header's last path segment with
//! generics, `&`, `mut` and `dyn` stripped, so `impl dyn Port` counts as
//! `Port`. A crate is the directory under `crates/`, or the nearest nested
//! directory holding its own `Cargo.toml`, as the script decides it.
//!
//! Small crates are left out on purpose: the hub this guard exists for lives
//! in a large crate, and a ceiling shared with every helper type of every
//! small crate would be a number nobody can act on.

mod lex;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::{Baseline, Entry, Policy, Unmeasured};
use crate::size;

/// The one signed ceiling and its budget.
pub const BASELINE_FILE: &str = "crates/guards/impl-spread-baseline.txt";

/// The baseline entry holding the ceiling: the most files any one type's
/// inherent impls may span in a qualifying crate.
pub const LABEL: &str = "impl-spread";

/// Production lines a crate must exceed before its types are counted — the
/// rubric's line for a large crate.
pub const QUALIFYING_LINES: usize = 20_000;

/// The tree every crate is taken from.
const SOURCE: &str = "crates/";

/// What the guard asks for when a type spans more files than the ceiling.
pub const REMEDY: &str = "A type whose inherent impl blocks span more files than the ceiling is \
    a hub split by file rather than by owner: every file holding a piece of it reaches into the \
    same private state, and the size ratchet cannot see that because each file stays small. Give \
    the new behaviour an owner type of its own, or a trait the type implements at a port in the \
    module that needs it, or fold the new impl into a file that already holds one. A deliberate \
    raise is a new file in crates/guards/impl-spread-baseline.d/ named for the branch, \
    `impl-spread +N` and `!budget +N` with a comment saying why, never an edit to the baseline. \
    A spread that fell needs no argument: `cargo run -p quantick-guards -- --tighten` writes the \
    new ceiling.";

/// What the guard asks for when the budget sits above the ceiling.
pub const BUDGET_SLACK_REMEDY: &str = "The impl-spread ceiling fell and the !budget has not caught \
    up. Nothing has to be argued: `cargo run -p quantick-guards -- --tighten` writes the new \
    total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "crates/guards/impl-spread-baseline.txt could not be read as \
    data, so no spread was checked. Every line is blank, a `#` comment, the one \
    `!budget <count>` directive, or `impl-spread <files>`. Fix the line the finding names.";

/// What the guard asks for when the tree could not be scanned whole.
pub const UNMEASURED_REMEDY: &str = "The impl spreads could not be taken, so no type was checked: \
    a path under crates/ could not be listed, read or decoded. Fix the path the finding names. \
    Until then the guard is not reporting a clean tree; it is reporting that it could not look.";

/// This guard's ratchet: the shared mechanism, with this guard's wording.
pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    // Zero: with no entry, any qualifying type with an impl is unsigned.
    threshold: 0,
    // Zero both ways: one file is a design decision, not noise to absorb.
    slack: 0,
    budget_slack: 0,
    budget_headroom: 0,
    unit: "files holding its inherent impls",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// One type of a qualifying crate and the production files holding its
/// inherent impls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spread {
    /// The crate, as [`crate_of`] names it.
    pub krate: String,
    /// The type, by its last path segment.
    pub name: String,
    /// Workspace-relative paths, sorted.
    pub files: Vec<String>,
}

impl Spread {
    /// `crate::Type`, as findings name it.
    pub fn label(&self) -> String {
        format!("{}::{}", self.krate, self.name)
    }
}

/// What one crate's production files add up to.
#[derive(Default)]
struct Tally {
    lines: usize,
    impls: BTreeMap<String, BTreeSet<String>>,
}

/// The crate a workspace-relative `.rs` path belongs to: the nearest
/// directory below `crates/<name>/` holding a `Cargo.toml`, else `<name>`.
/// `None` for a file directly under `crates/`, which belongs to none.
fn crate_of(root: &Path, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.strip_prefix(SOURCE)?.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    for depth in (2..parts.len()).rev() {
        let dir = format!("{SOURCE}{}", parts[..depth].join("/"));
        if root.join(&dir).join("Cargo.toml").is_file() {
            return Some(parts[depth - 1].to_owned());
        }
    }
    Some(parts[0].to_owned())
}

/// Every crate under `prefix` with its production lines and inherent impls.
/// An error when anything there could not be measured, never a smaller
/// tally: a file the scan missed is a file a type could spread into unseen.
fn tally(root: &Path, prefix: &str) -> Result<BTreeMap<String, Tally>, Unmeasured> {
    let mut crates: BTreeMap<String, Tally> = BTreeMap::new();
    let mut missed = Vec::new();
    for (path, _) in size::measure_under(root, prefix)? {
        if lex::is_test_file(&path) {
            continue;
        }
        let Some(krate) = crate_of(root, &path) else {
            continue;
        };
        let source = match fs::read_to_string(root.join(&path)) {
            Ok(source) => source,
            Err(e) => {
                missed.push(format!("  {path}: could not be read: {e}"));
                continue;
            }
        };
        let production = lex::production(&source);
        let tally = crates.entry(krate).or_default();
        tally.lines += lex::code_lines(&production);
        for name in lex::inherent_impls(&production) {
            tally.impls.entry(name).or_default().insert(path.clone());
        }
    }
    if !missed.is_empty() {
        return Err(Unmeasured { missed });
    }
    Ok(crates)
}

/// Every type of every crate over [`QUALIFYING_LINES`], widest first, then
/// by crate and type name.
fn spreads_of(crates: BTreeMap<String, Tally>) -> Vec<Spread> {
    let mut spreads: Vec<Spread> = crates
        .into_iter()
        .filter(|(_, tally)| tally.lines > QUALIFYING_LINES)
        .flat_map(|(krate, tally)| {
            tally.impls.into_iter().map(move |(name, files)| Spread {
                krate: krate.clone(),
                name,
                files: files.into_iter().collect(),
            })
        })
        .collect();
    spreads.sort_by(|a, b| {
        b.files
            .len()
            .cmp(&a.files.len())
            .then_with(|| a.krate.cmp(&b.krate))
            .then_with(|| a.name.cmp(&b.name))
    });
    spreads
}

/// Every type of every qualifying crate in the workspace, widest first.
pub fn spreads(root: &Path) -> Result<Vec<Spread>, Unmeasured> {
    Ok(spreads_of(tally(root, SOURCE)?))
}

/// The widest spread among qualifying crates; zero when no crate qualifies.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    Ok(widest(&spreads(root)?))
}

fn widest(spreads: &[Spread]) -> usize {
    spreads.first().map_or(0, |spread| spread.files.len())
}

/// One finding per type spanning more files than the ceiling allows.
fn over<'a>(entry: &Entry, spreads: impl Iterator<Item = &'a Spread>) -> Vec<Finding> {
    spreads
        .filter(|spread| spread.files.len() > entry.ceiling)
        .filter_map(|spread| POLICY.verdict(Some(entry), &spread.label(), spread.files.len()))
        .collect()
}

/// Every type over the ceiling by name, or — when none is — the ceiling
/// against the widest spread, which asks for an entry or a tighten.
fn spread_findings(recorded: &Baseline, spreads: &[Spread]) -> Vec<Finding> {
    let entry = recorded.entry(LABEL);
    let widest = widest(spreads);
    match entry {
        Some(entry) if widest > entry.ceiling => over(entry, spreads.iter()),
        _ => POLICY.verdict(entry, LABEL, widest).into_iter().collect(),
    }
}

/// Every way the tree and the recorded ceiling disagree: each type over it,
/// a ceiling to tighten or to add, the budget, and any entry but [`LABEL`].
pub fn check(root: &Path) -> Vec<Finding> {
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let spreads = match spreads(root) {
        Ok(spreads) => spreads,
        Err(unmeasured) => {
            return vec![Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY)];
        }
    };
    let mut findings = spread_findings(&recorded, &spreads);
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

/// The edit-time question. A source file is answered for its own crate: any
/// type over the ceiling that this file holds an impl of. An edit to the
/// baseline re-runs the whole check.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if POLICY.owns(relative) {
        return check(root);
    }
    if !size::tracked(relative) || lex::is_test_file(relative) {
        return Vec::new();
    }
    let (Some(top), Some(krate)) = (
        relative
            .strip_prefix(SOURCE)
            .and_then(|rest| rest.split('/').next()),
        crate_of(root, relative),
    ) else {
        return Vec::new();
    };
    // A file holding no inherent impl cannot widen a spread, and answering
    // that from the file alone keeps most edits off the crate-wide scan. A
    // file that does not decode is the encoding guard's finding.
    let Ok(source) = fs::read_to_string(root.join(relative)) else {
        return Vec::new();
    };
    if lex::inherent_impls(&lex::production(&source)).is_empty() {
        return Vec::new();
    }
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    // A tree that does not measure is the whole-tree check's finding.
    let Ok(crates) = tally(root, &format!("{SOURCE}{top}/")) else {
        return Vec::new();
    };
    let spreads: Vec<Spread> = spreads_of(crates)
        .into_iter()
        .filter(|spread| spread.krate == krate)
        .collect();
    match recorded.entry(LABEL) {
        Some(entry) => over(
            entry,
            spreads
                .iter()
                .filter(|spread| spread.files.iter().any(|file| file == relative)),
        ),
        None => POLICY
            .verdict(None, LABEL, widest(&spreads))
            .into_iter()
            .collect(),
    }
}

/// Lower the ceiling to the widest spread, and the budget with it, when the
/// spread fell. Refused when the tree could not be scanned whole.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let widest = measured(root).map_err(|unmeasured| format!("impl-spread: {unmeasured}"))?;
    POLICY.tighten(root, &[(LABEL.to_owned(), widest)], 0)
}

#[cfg(test)]
mod tests;
