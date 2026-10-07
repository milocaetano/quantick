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
//! so the guard and the score agree on every tree whose crates have distinct
//! directory names, as this tree's do: test files and
//! `#[cfg(test)]` items are out, a crate's production lines are its non-blank
//! code lines once comments and literals are blanked, and an inherent impl is
//! an `impl` at the start of a line whose header holds no `for`. Trait impls
//! are not counted — implementing a trait in the module that needs it is a
//! port, the shape the remedy asks for — and neither is `impl Trait` in an
//! argument or return type. The type is the header's last path segment with
//! generics, `&`, `mut` and `dyn` stripped, so `impl dyn Port` counts as
//! `Port`. A crate is the directory under `crates/`, or the nearest nested
//! directory holding its own `Cargo.toml`, as the script decides it. It is
//! keyed by that directory and named by its leaf, so two nested crates that
//! share a leaf name stay two crates here; the script merges them under one
//! row, a shape this tree does not have. The shared corpus in
//! `tools/outside_score/fixtures/` pins the per-file definition: both test
//! suites read its `expected.tsv`.
//!
//! The edit-time [`check_file`] answers with the whole-tree verdict
//! restricted to the edited file's crate, never a narrower question, so the
//! hook and CI cannot disagree about a fall or a crate crossing the line.
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
    unit: "files holding one type's inherent impls",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// One type of a qualifying crate and the production files holding its
/// inherent impls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spread {
    /// The crate's directory, workspace-relative (`crates/app`), as
    /// [`crate_of`] finds it: two nested crates sharing a leaf name are two
    /// crates, not one.
    pub krate: String,
    /// The type, by its last path segment.
    pub name: String,
    /// Workspace-relative paths, sorted.
    pub files: Vec<String>,
}

impl Spread {
    /// `crate::Type`, the crate by its directory's leaf name, as findings
    /// name it.
    pub fn label(&self) -> String {
        let leaf = self.krate.rsplit('/').next().unwrap_or(&self.krate);
        format!("{leaf}::{}", self.name)
    }
}

/// What one crate's production files add up to.
#[derive(Default)]
struct Tally {
    lines: usize,
    impls: BTreeMap<String, BTreeSet<String>>,
}

impl Tally {
    /// The widest spread this crate puts into the verdict: zero unless it
    /// is over [`QUALIFYING_LINES`].
    fn widest(&self) -> usize {
        if self.lines <= QUALIFYING_LINES {
            return 0;
        }
        self.impls.values().map(BTreeSet::len).max().unwrap_or(0)
    }
}

/// The crate directory a workspace-relative `.rs` path belongs to: the
/// nearest directory below `crates/<name>/` holding a `Cargo.toml`, else
/// `crates/<name>`. `None` for a file directly under `crates/`, which belongs
/// to none.
fn crate_of(root: &Path, path: &str) -> Option<String> {
    let parts: Vec<&str> = path.strip_prefix(SOURCE)?.split('/').collect();
    if parts.len() < 2 {
        return None;
    }
    for depth in (2..parts.len()).rev() {
        let dir = format!("{SOURCE}{}", parts[..depth].join("/"));
        if root.join(&dir).join("Cargo.toml").is_file() {
            return Some(dir);
        }
    }
    Some(format!("{SOURCE}{}", parts[0]))
}

/// One crate's candidate production files, each with its size in bytes.
type Files = Vec<(String, u64)>;

/// The candidate production files a walk found, by crate directory, and
/// every directory it could not list. Nothing is read yet.
#[derive(Default)]
struct Inventory {
    crates: BTreeMap<String, Files>,
    missed: Vec<String>,
}

impl Inventory {
    /// The crates, or every directory the walk could not list: a partial
    /// inventory is the flattering one.
    fn complete(self) -> Result<BTreeMap<String, Files>, Unmeasured> {
        if self.missed.is_empty() {
            Ok(self.crates)
        } else {
            Err(Unmeasured {
                missed: self.missed,
            })
        }
    }
}

/// An entry that vanished between `read_dir` and its stat (an editor's atomic
/// save, a temp file) is no entry, not a directory that cannot be listed.
/// Any other error stays an error.
fn skip_vanished<T>(entry: std::io::Result<T>) -> std::io::Result<Option<T>> {
    match entry {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Every `.rs` file under `dir` that is not a test file, filed under
/// `krate`. A subdirectory holding its own `Cargo.toml` is another crate:
/// walked as one when `nested`, skipped otherwise. `target/` is build output
/// and every file under `tests/` is a test file, so neither is entered.
fn inventory(root: &Path, dir: &str, krate: &str, nested: bool, found: &mut Inventory) {
    let listed = fs::read_dir(root.join(dir)).and_then(|entries| {
        entries
            .filter_map(|entry| {
                skip_vanished((|| {
                    let entry = entry?;
                    Ok((
                        entry.file_name(),
                        entry.file_type()?,
                        entry.metadata()?.len(),
                    ))
                })())
                .transpose()
            })
            .collect::<std::io::Result<Vec<_>>>()
    });
    let mut entries = match listed {
        Ok(entries) => entries,
        Err(e) => {
            found
                .missed
                .push(format!("  {dir}/: directory could not be listed: {e}"));
            return;
        }
    };
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind, len) in entries {
        let name = name.to_string_lossy();
        let path = format!("{dir}/{name}");
        if kind.is_dir() {
            if matches!(name.as_ref(), "target" | "tests") {
                continue;
            }
            if !root.join(&path).join("Cargo.toml").is_file() {
                inventory(root, &path, krate, nested, found);
            } else if nested {
                inventory(root, &path, &path, nested, found);
            }
        } else if name.ends_with(".rs") && !lex::is_test_file(&path) {
            found
                .crates
                .entry(krate.to_owned())
                .or_default()
                .push((path, len));
        }
    }
}

/// Every crate of the workspace, each a directory under `crates/` or a
/// nested one holding its own `Cargo.toml`.
fn inventory_all(root: &Path) -> Inventory {
    let mut found = Inventory::default();
    let top = SOURCE.trim_end_matches('/');
    match fs::read_dir(root.join(top)) {
        Ok(entries) => {
            let mut dirs = Vec::new();
            for entry in entries {
                match entry.and_then(|entry| Ok((entry.file_name(), entry.file_type()?))) {
                    Ok((name, kind)) if kind.is_dir() && name != "target" && name != "tests" => {
                        dirs.push(format!("{SOURCE}{}", name.to_string_lossy()));
                    }
                    Ok(_) => {}
                    Err(e) => found
                        .missed
                        .push(format!("  {SOURCE}: directory could not be listed: {e}")),
                }
            }
            dirs.sort();
            for dir in dirs {
                inventory(root, &dir, &dir, true, &mut found);
            }
        }
        Err(e) => found
            .missed
            .push(format!("  {SOURCE}: directory could not be listed: {e}")),
    }
    found
}

/// One crate's production lines and inherent impls, each file read once.
/// An error when a file could not be read or decoded, never a smaller tally:
/// a file the scan missed is a file a type could spread into unseen.
fn tally(root: &Path, files: &[(String, u64)]) -> Result<Tally, Unmeasured> {
    let mut tally = Tally::default();
    let mut missed = Vec::new();
    for (path, _) in files {
        let source = match fs::read(root.join(path)).map(String::from_utf8) {
            Ok(Ok(source)) => source,
            Ok(Err(_)) => {
                missed.push(format!("  {path}: does not decode as UTF-8"));
                continue;
            }
            Err(e) => {
                missed.push(format!("  {path}: could not be read: {e}"));
                continue;
            }
        };
        let production = lex::production(&source);
        tally.lines += lex::code_lines(&production);
        for name in lex::inherent_impls(&production) {
            tally.impls.entry(name).or_default().insert(path.clone());
        }
    }
    if !missed.is_empty() {
        return Err(Unmeasured { missed });
    }
    Ok(tally)
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
fn spreads(root: &Path) -> Result<Vec<Spread>, Unmeasured> {
    let mut crates = BTreeMap::new();
    for (krate, files) in inventory_all(root).complete()? {
        crates.insert(krate, tally(root, &files)?);
    }
    Ok(spreads_of(crates))
}

/// The widest spread among qualifying crates; zero when no crate qualifies.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    Ok(widest(&spreads(root)?))
}

fn widest(spreads: &[Spread]) -> usize {
    spreads.first().map_or(0, |spread| spread.files.len())
}

/// A type over the ceiling. The path slot holds [`LABEL`], the baseline key a
/// raise names, so a copy of the finding leads to a valid raise; the type and
/// its files follow in the message.
fn over(entry: &Entry, spread: &Spread) -> Option<Finding> {
    let found = POLICY.verdict(Some(entry), LABEL, spread.files.len())?;
    Some(Finding::new(
        format!(
            "{} — {} spans {}",
            found.line,
            spread.label(),
            spread.files.join(", ")
        ),
        found.remedy,
    ))
}

/// Every type over the ceiling that `keep` admits, or — when none is over —
/// the ceiling against the widest spread, which asks for an entry or a
/// tighten. The single place the spread verdict is taken, for both
/// [`check`] and [`check_file`].
fn spread_findings(
    recorded: &Baseline,
    spreads: &[Spread],
    keep: impl Fn(&Spread) -> bool,
) -> Vec<Finding> {
    let entry = recorded.entry(LABEL);
    let widest = widest(spreads);
    match entry {
        Some(entry) if widest > entry.ceiling => spreads
            .iter()
            .filter(|spread| keep(spread))
            .filter_map(|spread| over(entry, spread))
            .collect(),
        _ => POLICY.verdict(entry, LABEL, widest).into_iter().collect(),
    }
}

fn unmeasured_finding(unmeasured: &Unmeasured) -> Finding {
    Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY)
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
        Err(unmeasured) => return vec![unmeasured_finding(&unmeasured)],
    };
    let mut findings = spread_findings(&recorded, &spreads, |_| true);
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

/// The spreads [`check`]'s verdict needs to be answered for crate `own`.
///
/// `own` is always tallied: an edit there can add an impl, fold one away, or
/// carry the crate across [`QUALIFYING_LINES`] either way. When its widest
/// spread already reaches the ceiling, no other crate can change what is said
/// about it, so nothing else is read. Otherwise the verdict may be the
/// tree's — a ceiling that fell, or one that is missing — and that needs the
/// widest spread anywhere, so the other crates are tallied largest first
/// until one reaches the ceiling, which settles it, or all have been.
fn spreads_for(root: &Path, own: &str, entry: Option<&Entry>) -> Result<Vec<Spread>, Unmeasured> {
    let reaches = |widest: usize| entry.is_some_and(|entry| widest >= entry.ceiling);
    let mut found = Inventory::default();
    // A crate deleted whole holds nothing, as the whole-tree walk finds it.
    if root.join(own).is_dir() {
        inventory(root, own, own, false, &mut found);
    }
    let files = found.complete()?.remove(own).unwrap_or_default();
    let own_tally = tally(root, &files)?;
    let settled = reaches(own_tally.widest());
    let mut crates = BTreeMap::from([(own.to_owned(), own_tally)]);
    if settled {
        return Ok(spreads_of(crates));
    }
    let mut others: Vec<(u64, String, Files)> = inventory_all(root)
        .complete()?
        .into_iter()
        .filter(|(krate, _)| krate != own)
        .map(|(krate, files)| (files.iter().map(|(_, len)| len).sum(), krate, files))
        .collect();
    others.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, krate, files) in others {
        let other = tally(root, &files)?;
        let settled = reaches(other.widest());
        crates.insert(krate, other);
        if settled {
            break;
        }
    }
    Ok(spreads_of(crates))
}

/// The edit-time question: exactly [`check`]'s spread verdict, restricted to
/// what an edit to `relative` can change — every type of its crate over the
/// ceiling, and the tree's ceiling to tighten or add. Types of other crates
/// over the ceiling are theirs to answer for. A file that cannot widen or
/// narrow a spread — a test file, one outside `crates/` — answers nothing;
/// any other source file is answered whether or not it holds an impl, since
/// its lines alone can carry its crate across [`QUALIFYING_LINES`]. An edit
/// to the baseline re-runs the whole check.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if POLICY.owns(relative) {
        return check(root);
    }
    if !size::tracked(relative) || lex::is_test_file(relative) {
        return Vec::new();
    }
    let Some(own) = crate_of(root, relative) else {
        return Vec::new();
    };
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    match spreads_for(root, &own, recorded.entry(LABEL)) {
        Ok(spreads) => spread_findings(&recorded, &spreads, |spread| spread.krate == own),
        Err(unmeasured) => vec![unmeasured_finding(&unmeasured)],
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
