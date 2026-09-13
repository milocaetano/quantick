//! UI-free code in the UI crate, rationed.
//!
//! `app` is the only crate allowed to know the UI library, and the only one
//! the [`headless`](crate::headless) guard does not scan. So a module that
//! never names the library — a paper account, a gateway server, an
//! idempotency store — can live in `app` for ever with no wall-clock rule, no
//! UI rule and no second consumer, and every change to it rebuilds the
//! largest crate in the workspace. Nothing stopped that code from
//! accumulating: `--report` measured it and enforced nothing.
//!
//! This guard caps it. The quantity is the production lines of every
//! `crates/app/src` file whose production source never names `egui` or
//! `eframe` in code, summed, minus the files [`EXEMPTIONS_FILE`] signs for.
//! It is recorded once, as the `crates/app` entry of [`BASELINE_FILE`], on the
//! same [`Policy`] the size and cycle ratchets run on: over the ceiling fails,
//! more than [`SLACK`] below it asks for `--tighten`, and a raise is two
//! signed edits a reviewer can argue with.
//!
//! # Why a file is charged whole
//!
//! The question is whether a module could leave `app`, and that is a
//! property of the file: a single `ui.label` keeps the file's other lines
//! next to the painter that calls them. Counting lines without the name
//! instead scored 97% of `app` as portable — `ui.label(...)` does not spell
//! the library — which would have capped the size of the UI crate rather than
//! the logic hiding in it.
//!
//! # What counts as naming the library
//!
//! Exactly what the headless guard forbids below `app` for the UI toolkit,
//! asked through its own matcher ([`headless::forbidden_on`], entries marked
//! [`headless::UI_TOOLKIT`]): a whole identifier, comments stripped with
//! quotes respected. One list and one matcher, so the two guards cannot
//! disagree — this one never tells an author to move a file into a headless
//! crate that the other would reject. So `timeframe` is not `eframe`, and a
//! comment is not a reference: `//! drawn with egui elsewhere` is prose, and
//! if it counted, one comment line would take a whole file off the books.
//! The test is over [`size::production_source`], so a UI name only a test
//! module carries does not make a file UI code either.
//!
//! The rule errs toward charging: a UI file that reaches the library only
//! through a re-export (`use crate::prelude::Ui`) is counted as UI-free. That
//! is the direction a ratchet may err in, and the fix is to name the import.
//!
//! The rule is lexical, so the other direction exists too: a name that is
//! not a use — an unused `use eframe::egui as _;`, the name inside a block
//! comment or a string — keeps a file off the books. No scan short of the
//! compiler tells a use from a mention; the diff shows the line, and that is
//! a reviewer's finding.

use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::{Baseline, Policy, Unmeasured};
use crate::{headless, size};

/// The recorded ceiling and its budget.
pub const BASELINE_FILE: &str = "crates/guards/ui-free-baseline.txt";

/// The signed exceptions: `<path> <reason>` per line.
pub const EXEMPTIONS_FILE: &str = "crates/guards/ui-free-exemptions.txt";

/// The one baseline entry: the UI crate, whose UI-free total is rationed.
pub const ENTRY: &str = "crates/app";

/// The source tree the total is taken over.
const SOURCE: &str = "crates/app/src/";

/// How far below its ceiling the total may sit before the entry must be
/// tightened. The size ratchet's number, for the size ratchet's reason: a
/// ceiling that must move on every deleted line is a ceiling that turns
/// every unrelated change into a baseline conflict.
pub const SLACK: usize = 200;

/// What the guard asks for when the total is over, or far under, its ceiling.
pub const REMEDY: &str = "Over the ceiling, the UI crate gained code that never names the UI \
    library. Such code belongs in a headless crate below `app`, where the headless guard scans \
    it and backtest and the bot can reuse it — move it there, or move as many UI-free lines out \
    of crates/app in the same change. A file only `app` can hold is exempted by one line in \
    crates/guards/ui-free-exemptions.txt with its reason; a deliberate raise is the `crates/app` \
    entry and the !budget in crates/guards/ui-free-baseline.txt, both raised and signed in the \
    same change. A total that fell needs no argument: `cargo run -p quantick-guards -- \
    --tighten` writes the new number.";

/// What the guard asks for when the budget has fallen far below the entry it
/// caps: good news with the number already computed, and deliberately not
/// [`REMEDY`], which would tell an author to pay for a raise never made.
pub const BUDGET_SLACK_REMEDY: &str = "The recorded ceiling fell and the !budget has not caught \
    up. Nothing has to be argued: `cargo run -p quantick-guards -- --tighten` writes the new \
    total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "crates/guards/ui-free-baseline.txt could not be read as data, \
    so no ceiling was checked. Every line is blank, a `#` comment, the one `!budget <count>` \
    directive, or `crates/app <count>`. Fix the line the finding names.";

/// What the guard asks for when the total could not be taken at all. Neither
/// ceiling remedy applies: nothing grew and nothing shrank that anyone knows.
pub const UNMEASURED_REMEDY: &str = "The UI-free total could not be taken, so no ceiling was \
    checked: a path under crates/app/src could not be listed, read or decoded. Fix the path the \
    finding names. Until then the guard is not reporting a clean tree; it is reporting that it \
    could not look.";

/// What the guard asks for when an exemption line is malformed or stale.
pub const EXEMPTION_REMEDY: &str = "Every line in crates/guards/ui-free-exemptions.txt is blank, a \
    `#` comment, or `<path> <reason>`: a crates/app/src file that must be UI-free and must live \
    in `app`, and why. An exemption whose file is gone or now names the UI library permits \
    nothing — delete the line.";

/// This guard's ratchet: the shared mechanism, with this guard's wording.
pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    // Zero: the one entry is always required, so a missing `crates/app` line
    // is a finding rather than a free pass.
    threshold: 0,
    slack: SLACK,
    // One entry, so the budget moves exactly when the entry does.
    budget_slack: SLACK,
    // Zero: the budget sums signed permissions, and a raise is an act.
    budget_headroom: 0,
    unit: "UI-free production lines",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// Whether any code of this production source names the UI library, as the
/// headless guard's matcher reads it (see the module doc).
pub fn names_ui(production: &[&str]) -> bool {
    production.iter().any(|line| {
        headless::forbidden_on(line)
            .iter()
            .any(|forbidden| forbidden.because == headless::UI_TOOLKIT)
    })
}

/// One signed exemption.
struct Exemption {
    path: String,
    /// One-based line in [`EXEMPTIONS_FILE`].
    line: usize,
}

/// Read the exemptions. A malformed line is a finding and no exemption, so a
/// typo can never widen the list.
fn exemptions(root: &Path) -> Result<(Vec<Exemption>, Vec<Finding>), String> {
    let text = fs::read_to_string(root.join(EXEMPTIONS_FILE))
        .map_err(|e| format!("  {EXEMPTIONS_FILE} is unreadable: {e}"))?;
    let mut found: Vec<Exemption> = Vec::new();
    let mut findings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let (path, reason) = content
            .split_once(char::is_whitespace)
            .unwrap_or((content, ""));
        let problem = if reason.trim().is_empty() {
            Some(format!("exempts {path} with no reason"))
        } else if !path.starts_with(SOURCE) || !path.ends_with(".rs") {
            Some(format!("exempts {path}, which is not under {SOURCE}"))
        } else {
            found
                .iter()
                .find(|known| known.path == path)
                .map(|first| format!("{path} is already exempt on line {}", first.line))
        };
        match problem {
            Some(problem) => findings.push(Finding::new(
                format!("  {EXEMPTIONS_FILE}:{line}: {problem}"),
                EXEMPTION_REMEDY,
            )),
            None => found.push(Exemption {
                path: path.to_owned(),
                line,
            }),
        }
    }
    Ok((found, findings))
}

/// Every production file under [`SOURCE`], with whether it names the UI
/// library and its production lines, sorted by path.
///
/// The walk and the definition of production are the size guard's, so the
/// two ratchets can never disagree about which lines ship. An error rather
/// than a smaller list when anything under `crates/app` could not be read:
/// a partial walk is the flattering number.
fn files(root: &Path) -> Result<Vec<(String, bool, usize)>, Unmeasured> {
    if !root.join(SOURCE).is_dir() {
        return Err(Unmeasured {
            missed: vec![format!("  {SOURCE}: not a readable directory")],
        });
    }
    let walk = size::measure(root);
    // A file or directory under the source tree, as the walk reported it.
    let mut missed: Vec<String> = walk
        .unreadable
        .iter()
        .filter(|line| line.trim_start().starts_with(SOURCE))
        .cloned()
        .collect();
    // An ancestor the walk could not list hides the whole tree.
    missed.extend(
        walk.blind
            .iter()
            .filter(|dir| SOURCE.starts_with(dir.as_str()) && !dir.starts_with(SOURCE))
            .map(|dir| format!("  {dir}: directory could not be listed")),
    );
    missed.extend(
        walk.undecodable
            .iter()
            .filter(|path| path.starts_with(SOURCE))
            .map(|path| format!("  {path}: does not decode as UTF-8")),
    );
    let mut files = Vec::new();
    for (path, lines) in walk
        .counts
        .iter()
        .filter(|(path, _)| path.starts_with(SOURCE))
    {
        match fs::read_to_string(root.join(path)) {
            Ok(source) => files.push((
                path.clone(),
                names_ui(&size::production_source(&source)),
                *lines,
            )),
            Err(e) => missed.push(format!("  {path}: could not be read: {e}")),
        }
    }
    if missed.is_empty() {
        Ok(files)
    } else {
        Err(Unmeasured { missed })
    }
}

/// The total the ratchet rations: UI-free production lines under
/// [`SOURCE`], less the exempt files.
fn total(files: &[(String, bool, usize)], exempt: &[Exemption]) -> usize {
    files
        .iter()
        .filter(|(path, names_ui, _)| !names_ui && !exempt.iter().any(|e| &e.path == path))
        .map(|(_, _, lines)| lines)
        .sum()
}

/// The rationed total today, for [`crate::report`] and the registry. An
/// error when the walk or the exemption list could not be read, never a
/// smaller number.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    let files = files(root)?;
    let (exempt, _) = exemptions(root).map_err(|problem| Unmeasured {
        missed: vec![problem],
    })?;
    Ok(total(&files, &exempt))
}

/// Every way the tree, the exemptions and the recorded ceiling disagree.
pub fn check(root: &Path) -> Vec<Finding> {
    let recorded: Baseline = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let (exempt, mut findings) = match exemptions(root) {
        Ok(read) => read,
        Err(problem) => return vec![Finding::new(problem, EXEMPTION_REMEDY)],
    };
    let files = match files(root) {
        Ok(files) => files,
        Err(unmeasured) => {
            findings.push(Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY));
            return findings;
        }
    };
    for exemption in &exempt {
        let stale = match files.iter().find(|(path, ..)| path == &exemption.path) {
            None => Some("is not a production file under crates/app/src"),
            Some((_, true, _)) => Some("names the UI library"),
            Some(_) => None,
        };
        if let Some(stale) = stale {
            findings.push(Finding::new(
                format!(
                    "  {EXEMPTIONS_FILE}:{}: {} {stale} — delete the line",
                    exemption.line, exemption.path
                ),
                EXEMPTION_REMEDY,
            ));
        }
    }
    let counts = [(ENTRY.to_owned(), total(&files, &exempt))];
    findings.extend(POLICY.against(&recorded, &counts, 0, &|path| path == ENTRY));
    findings
}

/// The edit-time question. The quantity is a sum over the whole source
/// tree, so a write to any file under it — or to either data file — asks
/// the whole question; anything else is out of scope.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    let in_scope = (relative.starts_with(SOURCE) && size::tracked(relative))
        || relative == BASELINE_FILE
        || relative == EXEMPTIONS_FILE;
    if in_scope { check(root) } else { Vec::new() }
}

/// Lower the entry, and the budget with it, once the total has fallen more
/// than [`SLACK`] below. Refused when the tree could not be measured whole:
/// a partial walk would write a ceiling the real tree is already over.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let total = measured(root).map_err(|unmeasured| format!("app-ui-free: {unmeasured}"))?;
    POLICY.tighten(root, &[(ENTRY.to_owned(), total)], 0)
}

#[cfg(test)]
mod tests;
