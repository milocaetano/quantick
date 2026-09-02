//! A ratchet for the files that are injected into a Claude session.
//!
//! `CLAUDE.md` is read at the start of every session; a skill's `SKILL.md` is
//! read whole the moment that skill is invoked. Those bytes are paid for on
//! every turn that follows, before a single line of the repository has been
//! looked at — and unlike the code, nothing has ever rationed them.
//!
//! The result was measurable. By the time this guard was written,
//! `arch-review`'s skill had reached 48 KB and `mission`'s 33 KB, so a
//! `/mission → /arch-review → /delivery-review → /ship` cycle spent about
//! 28,000 tokens on instructions before starting work. `CLAUDE.md` had gone
//! the same way and was cut by two thirds in one branch. Both were the same
//! failure the [`size`](crate::size) guard exists for, in a different file
//! extension: something valuable grows a paragraph at a time, every paragraph
//! is defensible, and nobody is looking at the total.
//!
//! So the rule is the size guard's rule, over the other tree. A context file
//! has a ceiling, growth past it must be signed in the baseline with a
//! reason, and a budget caps the whole tracked weight — every ceiling plus
//! every tracked file too small to need one, so adding weight anywhere means
//! taking comparable weight out somewhere else. That second half is what
//! stops the obvious dodge: splitting a tracked file into sub-threshold
//! pieces moves the prose a session loads not at all, and a budget of
//! ceilings alone would have called it a saving. The mechanism is
//! [`ratchet`](crate::ratchet), shared with the size guard; this module
//! contributes the scan and the numbers.
//!
//! # Why bytes rather than lines
//!
//! Lines are what the size guard counts, and they are the wrong unit here.
//! These files are prose wrapped at 80 columns, so a line is worth whatever
//! the author's editor did, and moving a paragraph out of a table into a
//! sentence changes the line count without changing what a session pays.
//! Bytes track the cost being rationed — roughly four to a token for English
//! prose — and they cannot be gamed by reflowing.
//!
//! # What is in scope
//!
//! Exactly the files a session loads: `CLAUDE.md`, `AGENTS.md` (which
//! `CLAUDE.md` delegates the crate map to, so a cut that moves weight there
//! must still be paid for), and every `.md` under `.claude/skills/`. The goal
//! files under `.claude/` are not in scope — a `GOAL.md` is a record of one
//! mission, read deliberately and never at start-up, and rationing it would
//! push authors to write down less of what they were asked for.

use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::{Baseline, Policy};

/// Bytes above which a context file must carry a baseline entry.
///
/// About 2,500 tokens: large enough that no ordinary skill needs an entry,
/// small enough that the three files this guard was written for all do. A
/// file over it is not forbidden — it is asked to say, in the baseline, why
/// every session should pay for it.
pub const THRESHOLD: usize = 10_000;

/// How far below its ceiling a tracked file may sit before the entry must be
/// tightened. Wider than the size guard's slack in absolute terms and much
/// tighter in relative ones: editing a sentence moves a markdown file by tens
/// of bytes, and only a real cut moves it by a thousand.
pub const SLACK: usize = 1_000;

/// The recorded ceilings, as a workspace-relative path.
pub const BASELINE_FILE: &str = "crates/guards/context-baseline.txt";

/// How far below the budget the tracked total may sit before the budget
/// itself has to come down.
///
/// It bounds one direction only. The other is [`BUDGET_HEADROOM`], and this
/// guard needs both where the size guard needs one: its budget counts
/// *measured* bytes, not permissions alone, so an ordinary sentence moves the
/// total where a `.rs` edit under its ceiling moves nothing.
pub const BUDGET_SLACK: usize = 4_000;

/// How far *over* the budget the tracked total may go before it is a finding.
///
/// Without it the baseline ships with zero tolerance upward: a one-word
/// addition anywhere under `.claude/skills/`, `CLAUDE.md` or `AGENTS.md`
/// fails `cargo test -p quantick-guards` and forces a baseline edit. That is
/// the failure [`THRESHOLD`]'s own doc comment warns about — every ordinary
/// edit becoming a baseline update is the reliable way to get a guard
/// disabled — reintroduced through the budget instead of the threshold.
///
/// Deliberately smaller than [`BUDGET_SLACK`], and deliberately not zero. A
/// paragraph is roughly 300 bytes, so this absorbs a few of them and nothing
/// like a new section; the ratchet still catches every real growth, and
/// `--tighten` pulls the number back down whenever prose leaves.
pub const BUDGET_HEADROOM: usize = 2_000;

/// The directory every skill lives under.
///
/// Carries its trailing slash so a prefix test is a directory test. Without
/// it, `.claude/skills-old/notes.md` starts with `.claude/skills` and would be
/// rationed as a skill — and, worse, an entry recorded for such a file would
/// never be found by [`measure`], which walks the real directory, so the
/// guard would report its own scan as a stale entry.
const SKILLS: &str = ".claude/skills/";

/// The context files that are not skills, relative to the workspace root.
const ROOT_FILES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];

/// What the guard asks for when a context file is over its ceiling.
pub const REMEDY: &str = "A context file over its ceiling is a cost every session pays before it \
                          reads any code. The fix is the one PR #279 applied to CLAUDE.md: keep \
                          every operative rule, state each once, and move the reasoning out — to \
                          docs/agentic-development.md for the working rules, or to a \
                          references/ file beside the skill for detail that only some runs need. \
                          A skill's references/ are read on demand, so a dimension or a step that \
                          most reviews waive costs nothing until it is in scope. Raising a \
                          ceiling on purpose is still allowed: change the number in \
                          crates/guards/context-baseline.txt and say why in a comment. A file \
                          that shrank needs no argument — `cargo run -p quantick-guards -- \
                          --tighten` writes the new number.";

/// What the guard asks for when the recorded total is over budget.
pub const BUDGET_REMEDY: &str = "The context budget is what a session's instructions weigh in \
                                 total: every recorded ceiling, plus the measured bytes of every \
                                 tracked file too small to need one. Both halves are deliberate. \
                                 Individually signed raises cannot say whether the instructions \
                                 are getting cheaper — every paragraph added to a skill was \
                                 defensible on its own, which is how one reached 48 KB. And a \
                                 budget of ceilings alone would pay a branch for splitting a \
                                 large file into sub-threshold pieces, which moves the prose a \
                                 session loads not at all. So growth is pay-as-you-go: a branch \
                                 that adds weight takes comparable weight out somewhere else, and \
                                 moving prose between context files buys nothing. Raising the \
                                 budget line itself stays available and is the escape hatch on \
                                 purpose: it is one number, in one place, that a reviewer watches \
                                 move.";

/// What the guard asks for when the recorded total has fallen below budget.
pub const BUDGET_SLACK_REMEDY: &str = "The recorded ceilings now total less than the budget caps them at, which means prose left a \
     context file and the cap has not caught up. Nothing has to be argued: `cargo run -p \
     quantick-guards -- --tighten` writes the new total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "The baseline could not be read as data, so no ceiling was checked at all — this is a syntax \
     finding, not a size one. Every line in crates/guards/context-baseline.txt is blank, a `#` \
     comment, the one `!budget <count>` directive, or a `<path> <count>` pair. Fix the line the \
     finding names and the guard resumes. Until then it is not reporting a lean set of \
     instructions, it is reporting that it could not look.";

/// This guard's ratchet.
pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    threshold: THRESHOLD,
    slack: SLACK,
    budget_slack: BUDGET_SLACK,
    budget_headroom: BUDGET_HEADROOM,
    unit: "bytes of context",
    remedy: REMEDY,
    budget_remedy: BUDGET_REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// Whether a workspace-relative path is a file a session loads.
///
/// The single owner of that question, called by the walk below *and* by
/// [`check_file`], because those are the two surfaces the edit-time hook
/// trusts to say the same thing.
pub fn tracked(relative: &str) -> bool {
    ROOT_FILES.contains(&relative) || (relative.starts_with(SKILLS) && relative.ends_with(".md"))
}

/// What a walk of the context tree found.
pub struct Measured {
    /// Byte counts by workspace-relative path, sorted.
    pub counts: Vec<(String, usize)>,
    /// Paths that exist and could not be read at all. Reported rather than
    /// skipped: a file the guard cannot open is not a file it has cleared,
    /// and silence there is indistinguishable from a clean result.
    pub unreadable: Vec<String>,
    /// Directory prefixes the walk could not list, with a trailing slash.
    ///
    /// Kept apart from the message in [`Measured::unreadable`] because the
    /// two answer different questions. The message names the directory; the
    /// entries at risk name *files inside it*, and no substring of one is the
    /// other. Without this, an unlistable `.claude/skills/ui-harness/` made
    /// every ceiling under it report as a stale entry — whose stated remedy
    /// is to delete it, after which the file is re-added at whatever size it
    /// has since grown to. That is the raise laundered through the guard's
    /// own instructions that [`Policy::against`] exists to refuse.
    pub blind: Vec<String>,
}

impl Measured {
    /// Whether the walk saw this path at all — measured, present but
    /// unreadable, or inside a directory it could not look into. A file that
    /// exists but could not be read is present, not gone, and reporting its
    /// entry stale would delete a ceiling over a live file.
    fn seen(&self, path: &str) -> bool {
        self.counts.iter().any(|(scanned, _)| scanned == path)
            || self
                .unreadable
                .iter()
                .any(|line| line.starts_with(&format!("  {path}: ")))
            || self.blind.iter().any(|dir| path.starts_with(dir.as_str()))
    }
}

/// Add one file's size to the measurement, or the reason it could not be
/// taken.
fn weigh(path: &Path, relative: String, found: &mut Measured) {
    match fs::metadata(path) {
        Ok(meta) => found.counts.push((relative, meta.len() as usize)),
        Err(e) => found
            .unreadable
            .push(format!("  {relative}: could not be read: {e}")),
    }
}

/// Every `.md` under a skills directory, recursively, so a skill's
/// `references/` are weighed alongside its `SKILL.md`. A reference file is
/// cheap only because it is read on demand; one that grows without bound is
/// the same debt moved one directory down.
fn walk(dir: &Path, root: &Path, found: &mut Measured) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // A directory that cannot be listed is not a directory that came back
        // clean: a permission error would otherwise produce a green run over
        // files nobody looked at, and make every baseline entry look stale.
        Err(e) => {
            let relative = relative_to(root, dir);
            found
                .unreadable
                .push(format!("  {relative}/: directory could not be listed: {e}"));
            found.blind.push(format!("{relative}/"));
            return;
        }
    };
    for entry in entries {
        let path = match entry {
            Ok(entry) => entry.path(),
            // The error carries no name, so the file it stood for cannot be
            // recorded — which is exactly the shape that made an unlistable
            // directory report every ceiling under it as stale. The directory
            // is marked blind for the same reason: a walk that lost an entry
            // has not cleared this directory, and no ceiling inside it may be
            // called gone on the strength of a scan that missed a name.
            Err(e) => {
                let relative = relative_to(root, dir);
                found
                    .unreadable
                    .push(format!("  {relative}/: entry unreadable: {e}"));
                found.blind.push(format!("{relative}/"));
                continue;
            }
        };
        if path.is_dir() {
            walk(&path, root, found);
            continue;
        }
        let relative = relative_to(root, &path);
        if tracked(&relative) {
            weigh(&path, relative, found);
        }
    }
}

/// A path under the workspace root, with forward slashes so baseline entries
/// read the same on every platform.
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Byte counts for every context file, sorted by path.
pub fn measure(root: &Path) -> Measured {
    let mut found = Measured {
        counts: Vec::new(),
        unreadable: Vec::new(),
        blind: Vec::new(),
    };
    for name in ROOT_FILES {
        let path = root.join(name);
        // Not `is_file()`, which answers `false` for a file that exists and
        // whose metadata could not be read — the walk would then drop it
        // silently, and a dropped file is a stale entry, whose stated remedy
        // deletes a live ceiling. Only a genuine absence is silent here.
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => found.counts.push((name.to_owned(), meta.len() as usize)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => found
                .unreadable
                .push(format!("  {name}: could not be read: {e}")),
        }
    }
    let skills = root.join(SKILLS);
    if skills.is_dir() {
        walk(&skills, root, &mut found);
    }
    found.counts.sort();
    found
}

/// The bytes the scan measured in files that carry no baseline entry.
///
/// This is what makes the budget a cap rather than a bypass. Without it, a
/// tracked file split into pieces that each sit under [`THRESHOLD`] leaves
/// the recorded total *lower* while a session loads exactly as much prose —
/// and `--tighten` would then write the smaller number down as progress. The
/// branch that built this guard did that by accident: of the 49,281 bytes it
/// removed from three skills, 37,490 landed in sub-threshold `references/`
/// siblings.
///
/// A file with an entry is excluded because its ceiling already speaks for
/// it, and counting both would charge it twice.
fn unrecorded(recorded: &Baseline, found: &Measured) -> usize {
    found
        .counts
        .iter()
        .filter(|(path, _)| recorded.entry(path).is_none())
        .map(|(_, bytes)| bytes)
        .sum()
}

/// Every way the recorded baseline and the context files on disk disagree.
pub fn check(root: &Path) -> Vec<Finding> {
    // Checked before anything is measured, because a missing skills directory
    // measures as *empty* — and an empty measurement makes every baseline
    // entry look stale, whose stated remedy is to delete it. That one edit
    // switches the ratchet off for every instruction file at once.
    let skills = root.join(SKILLS);
    if !skills.is_dir() {
        return vec![Finding::new(
            format!(
                "  {} is not a readable directory — there is nothing to measure, and every \
                 baseline entry would otherwise be reported stale",
                skills.display()
            ),
            BASELINE_REMEDY,
        )];
    }
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let found = measure(root);
    let mut violations: Vec<Finding> = found
        .unreadable
        .iter()
        // BASELINE_REMEDY, not REMEDY: an author who hit a permission error
        // is not an author who wrote too much prose, and `Finding`'s own doc
        // comment argues that a wrong remedy is worse than a terse one
        // because it gets followed.
        .map(|line| Finding::new(line.clone(), BASELINE_REMEDY))
        .collect();
    violations.extend(POLICY.against(
        &recorded,
        &found.counts,
        unrecorded(&recorded, &found),
        &|path| found.seen(path),
    ));
    violations
}

/// The same verdict for one file, without walking the tree — what the
/// edit-time hook calls after a write.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    // The baseline is not a context file, so `tracked` rejects it — but it is
    // where a raise is actually written, and a hook that saw every skill edit
    // while missing the one edit that spends the budget would report the
    // symptom and never the act.
    if relative == BASELINE_FILE {
        return match POLICY.baseline(root) {
            // The budget covers files with no entry, so answering it needs the
            // scan. It is fifteen `stat` calls over known paths, not a walk
            // of the repository, and a hook that answered from the baseline
            // alone would call a split clean that the suite calls a
            // violation.
            Ok(recorded) => {
                let found = measure(root);
                POLICY
                    .budget_verdict(&recorded, unrecorded(&recorded, &found))
                    .into_iter()
                    .collect()
            }
            Err(problem) => vec![POLICY.unparsed(&problem)],
        };
    }
    if !tracked(relative) {
        return Vec::new();
    }
    let actual = match fs::metadata(root.join(relative)) {
        Ok(meta) => meta.len() as usize,
        Err(e) => {
            // BASELINE_REMEDY here too, matching `check`. The two surfaces
            // must not attach different instructions to the same condition:
            // the author sees whichever ran, and one of them would send them
            // to cut prose over a permission error.
            return vec![Finding::new(
                format!("  {relative}: could not be read: {e}"),
                BASELINE_REMEDY,
            )];
        }
    };
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let mut findings: Vec<Finding> = POLICY
        .verdict(recorded.entry(relative), relative, actual)
        .into_iter()
        .collect();
    // And the budget, which is the whole point of counting unrecorded files.
    // A file under the threshold has no ceiling to cross, so `verdict` is
    // always silent about it — and that is the ordinary edit here, not the
    // edge case. Without this the hook reports clean on exactly the write
    // that puts the tree over budget, and the author meets it minutes later
    // as a failing suite with no idea which edit did it.
    let found = measure(root);
    findings.extend(POLICY.budget_verdict(&recorded, unrecorded(&recorded, &found)));
    findings
}

/// Lower any entry whose file has shrunk past [`SLACK`].
///
/// Refuses on an incomplete scan. `--tighten` writes the budget *down*, and
/// only down, so a file the walk could not see is one whose bytes are missing
/// from the total that gets written — permanently, and with nothing left to
/// notice it by. Pointed at a tree holding one skill instead of ten, the
/// first version wrote `!budget: 231950 -> 164886` and every honest run
/// afterwards read 67k over budget, with the remedy telling the author to
/// delete prose nobody had added. `check` already refuses the same
/// conditions; this is the surface that *writes*, so it refuses harder.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let skills = root.join(SKILLS);
    if !skills.is_dir() {
        return Err(format!(
            "{} is not a readable directory — refusing to tighten from a scan that measured \
             nothing, which would write a budget missing every skill",
            skills.display()
        ));
    }
    let found = measure(root);
    if !found.unreadable.is_empty() {
        return Err(format!(
            "refusing to tighten: the scan could not read {} path(s), whose bytes would go \
             missing from the total written down:\n{}",
            found.unreadable.len(),
            found.unreadable.join("\n")
        ));
    }
    let recorded = POLICY.baseline(root)?;
    // The check that actually catches a partial tree. A directory that lists
    // fine but holds three of ten skills is *readable* — the walk succeeds and
    // simply measures less — so neither test above sees it, and the budget
    // written from it is short by every file that was not there. The signal is
    // the baseline itself: an entry whose file the scan did not find means
    // either a deleted file, which is a human's decision to drop, or a tree
    // that is not the one the baseline describes. Both make this the wrong
    // moment to write a number down.
    let missing: Vec<&str> = recorded
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .filter(|path| !found.seen(path))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "refusing to tighten: {} recorded file(s) were not found by the scan, so this tree is \
             not the one the baseline describes and the total written would be short by every \
             file that is missing — {}",
            missing.len(),
            missing.join(", ")
        ));
    }
    let unrecorded = unrecorded(&recorded, &found);
    POLICY.tighten(root, &found.counts, unrecorded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_root;

    #[test]
    fn the_repository_is_within_its_context_budget() {
        let findings = check(&workspace_root());
        assert!(
            findings.is_empty(),
            "context ratchet:\n{}",
            findings
                .iter()
                .map(|f| f.line.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn the_files_a_session_loads_are_the_files_in_scope() {
        assert!(tracked("CLAUDE.md"));
        assert!(tracked("AGENTS.md"));
        assert!(tracked(".claude/skills/mission/SKILL.md"));
        assert!(tracked(
            ".claude/skills/ui-harness/references/hook-registry.md"
        ));
    }

    #[test]
    fn a_goal_file_is_a_record_of_one_mission_and_is_not_rationed() {
        assert!(!tracked(".claude/GOAL.md"));
        assert!(!tracked(".claude/GOAL-archive-mission-tiers.md"));
    }

    #[test]
    fn a_directory_that_merely_starts_like_the_skills_one_is_not_in_scope() {
        // The prefix test is a directory test. A file recorded under a name
        // the walk would never reach reports as a stale entry for ever.
        assert!(!tracked(".claude/skills-old/notes.md"));
        assert!(!tracked(".claude/skillsets.md"));
    }

    #[test]
    fn code_and_documentation_belong_to_other_guards() {
        assert!(!tracked("crates/app/src/app.rs"));
        assert!(!tracked("docs/agentic-development.md"));
        assert!(!tracked(".claude/hooks/README.md"));
    }

    #[test]
    fn the_scan_finds_claude_md_and_every_skill() {
        let found = measure(&workspace_root());
        assert!(
            found.counts.iter().any(|(path, _)| path == "CLAUDE.md"),
            "CLAUDE.md is the file every session loads"
        );
        let skills = found
            .counts
            .iter()
            .filter(|(path, _)| path.ends_with("/SKILL.md"))
            .count();
        assert!(skills >= 8, "found only {skills} skills");
    }

    #[test]
    fn every_measured_file_has_a_size() {
        let found = measure(&workspace_root());
        assert!(
            found.counts.iter().all(|(_, bytes)| *bytes > 0),
            "a tracked context file measured as empty"
        );
    }

    #[test]
    fn check_file_agrees_with_the_scan_about_every_tracked_file() {
        let root = workspace_root();
        let whole = check(&root);
        for (path, _) in measure(&root).counts {
            let single = check_file(&root, &path);
            // Anchored on the finding's own `"  {path}: "` prefix rather than
            // matched as a substring: one tracked path being a substring of
            // another would otherwise attribute the wrong file's violation,
            // which is the hole the size guard's twin test already closed.
            let prefix = format!("  {path}: ");
            let from_scan: Vec<&str> = whole
                .iter()
                .filter(|f| f.line.starts_with(&prefix))
                .map(|f| f.line.as_str())
                .collect();
            let from_file: Vec<&str> = single.iter().map(|f| f.line.as_str()).collect();
            assert_eq!(
                from_scan, from_file,
                "the two surfaces disagree about {path}"
            );
        }
    }

    #[test]
    fn an_entry_inside_a_directory_the_walk_could_not_list_is_not_stale() {
        // The directory failure names the directory; the entries at risk name
        // files inside it. Reporting those stale tells the author to delete a
        // live ceiling, and the file is then re-added at whatever size it has
        // grown to — the guard laundering a raise through its own remedy.
        let found = Measured {
            counts: Vec::new(),
            unreadable: vec![
                "  .claude/skills/ui-harness/: directory could not be listed: denied".to_owned(),
            ],
            blind: vec![".claude/skills/ui-harness/".to_owned()],
        };
        assert!(found.seen(".claude/skills/ui-harness/SKILL.md"));
        assert!(found.seen(".claude/skills/ui-harness/references/hook-registry.md"));
        assert!(!found.seen(".claude/skills/mission/SKILL.md"));
    }

    #[test]
    fn an_untracked_path_is_silent_rather_than_clean() {
        assert!(check_file(&workspace_root(), "crates/app/src/app.rs").is_empty());
    }

    #[test]
    fn the_baseline_is_checked_by_the_hook_that_sees_it_edited() {
        // The budget verdict is the whole reason the baseline is answerable
        // at all: it is the file a raise is written into.
        let findings = check_file(&workspace_root(), BASELINE_FILE);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn splitting_a_tracked_file_into_sub_threshold_pieces_buys_nothing() {
        // The bypass the budget's unrecorded half exists to close. `one` is
        // cut from 12,000 to 2,000 and the 10,000 bytes reappear as a
        // reference file below the threshold, so no ceiling is needed for it
        // and the recorded total falls by 10,000. Nothing a session loads
        // moved, and the guard must say so.
        let root = scratch("split-bypass", 12_000, 10_000, 22_000);
        assert!(check(&root).is_empty(), "the scratch tree starts clean");

        let one = root.join(".claude/skills/one/SKILL.md");
        fs::write(&one, "x".repeat(2_000)).expect("scratch skill is writable");
        fs::create_dir_all(root.join(".claude/skills/one/references"))
            .expect("scratch dirs are creatable");
        fs::write(
            root.join(".claude/skills/one/references/detail.md"),
            "x".repeat(9_000),
        )
        .expect("scratch reference is writable");
        // The entry is tightened, and the budget is lowered to the sum of the
        // ceilings the way a budget of permissions alone would have it —
        // 2,000 + 10,000. That is the bypass: it claims a 10,000-byte saving
        // for a split that removed 1,000 real bytes.
        let baseline = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        fs::write(
            root.join(BASELINE_FILE),
            baseline
                .replace(
                    ".claude/skills/one/SKILL.md 12000",
                    ".claude/skills/one/SKILL.md 2000",
                )
                .replace("!budget 22000", "!budget 12000"),
        )
        .expect("scratch baseline is writable");

        let findings = check(&root);
        assert!(
            findings.iter().any(
                |f| f.line.contains("the tracked total is 21000") && f.line.contains("(+9000)")
            ),
            "the 9,000 bytes that moved into a sub-threshold file must still be \
             charged: {findings:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tighten_refuses_a_tree_it_could_not_fully_measure() {
        // The finding that made this a refusal: `--tighten` writes the budget
        // down and only down, so a file the walk never saw is bytes missing
        // from the number it writes — permanently, with nothing left to
        // notice it by.
        let root = scratch("tighten-refuses", 12_000, 10_000, 22_000);
        fs::remove_dir_all(root.join(".claude/skills")).expect("scratch skills are removable");

        let problem = tighten(&root).expect_err("an unmeasurable tree is refused");
        assert!(problem.contains("refusing to tighten"), "{problem}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tighten_refuses_a_tree_that_is_not_the_one_the_baseline_describes() {
        // The partial tree the first refusal missed: every directory lists
        // fine, so nothing is *unreadable* — there is simply less of it. The
        // walk succeeds, measures a fraction, and `--tighten` would write that
        // fraction down as the budget. Reproduced against the real repository
        // by pointing the binary at a copy holding one skill of ten: it wrote
        // `!budget: 231950 -> 164886`, permanently, and every honest run
        // afterwards read 67k over budget.
        let root = scratch("tighten-partial", 12_000, 10_000, 22_000);
        fs::remove_dir_all(root.join(".claude/skills/two")).expect("scratch skill is removable");

        let problem = tighten(&root).expect_err("a partial tree is refused");
        assert!(
            problem.contains("not the one the baseline describes")
                && problem.contains(".claude/skills/two/SKILL.md"),
            "{problem}"
        );

        let baseline = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(baseline.contains("!budget 22000"), "{baseline}");

        // And the baseline is untouched — a refusal that had already written
        // would be no refusal at all.
        let baseline = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline is readable");
        assert!(baseline.contains("!budget 22000"), "{baseline}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_sentence_added_to_a_context_file_is_absorbed_rather_than_a_finding() {
        // A budget over measured bytes moves on every ordinary edit. With no
        // headroom, a one-word addition fails the suite and forces a baseline
        // commit — which is how a guard gets switched off.
        let root = scratch("headroom", 12_000, 10_000, 22_000);
        assert!(check(&root).is_empty(), "the scratch tree starts clean");

        // Grown in a file with no ceiling, so only the budget can answer —
        // a file over its own entry is the per-file ratchet's business, and
        // that one has no headroom by design.
        fs::create_dir_all(root.join(".claude/skills/one/references"))
            .expect("scratch dirs are creatable");
        let note = root.join(".claude/skills/one/references/detail.md");
        fs::write(&note, "x".repeat(300)).expect("scratch reference is writable");
        assert!(
            check(&root).is_empty(),
            "300 bytes must be absorbed, not filed"
        );

        // And the budget still bites on real growth.
        fs::write(&note, "x".repeat(3_000)).expect("scratch reference is writable");
        assert!(!check(&root).is_empty(), "3,000 bytes is not churn");
        let _ = fs::remove_dir_all(&root);
    }

    /// A scratch workspace whose baseline names two skill files, so a raise on
    /// one can be paid for — or not — by the other.
    fn scratch(test: &str, first: usize, second: usize, budget: usize) -> std::path::PathBuf {
        // Process-unique, like `ratchet::tests::tempdir`: two worktrees running
        // the suite at once — the workflow `CLAUDE.md` prescribes — would
        // otherwise have one run `remove_dir_all` the other's fixture
        // mid-test.
        let root =
            std::env::temp_dir().join(format!("quantick-context-{test}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("crates/guards")).expect("scratch dirs are creatable");
        fs::create_dir_all(root.join(".claude/skills/one")).expect("scratch dirs are creatable");
        fs::create_dir_all(root.join(".claude/skills/two")).expect("scratch dirs are creatable");
        fs::write(
            root.join(BASELINE_FILE),
            format!(
                "!budget {budget}\n\
                 .claude/skills/one/SKILL.md {first}\n\
                 .claude/skills/two/SKILL.md {second}\n"
            ),
        )
        .expect("scratch baseline is writable");
        // Each file sits exactly at its ceiling, so the only finding a test
        // can see is the budget's.
        fs::write(root.join(".claude/skills/one/SKILL.md"), "x".repeat(first))
            .expect("scratch skill is writable");
        fs::write(root.join(".claude/skills/two/SKILL.md"), "x".repeat(second))
            .expect("scratch skill is writable");
        root
    }

    /// The same invariant as `check_file_agrees_with_the_scan_about_every_
    /// tracked_file`, for the one path that scan cannot reach — and asserted
    /// over a tree that is genuinely over budget, because agreement on two
    /// empty lists proves nothing. The failure worth catching is the hook
    /// calling a raise clean while the suite calls it a violation.
    #[test]
    fn check_file_agrees_with_the_scan_about_the_baseline() {
        let root = scratch("budget-agree", 12_000, 11_000, 20_000);
        let prefix = format!("  {BASELINE_FILE}:");
        let from_scan: Vec<Finding> = check(&root)
            .into_iter()
            .filter(|f| f.line.starts_with(&prefix))
            .collect();

        assert_eq!(from_scan.len(), 1, "the scratch tree is over budget");
        assert_eq!(
            check_file(&root, BASELINE_FILE),
            from_scan,
            "the hook and the suite disagree about the baseline"
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// A raise paid for by a cut somewhere else clears the guard: the rule
    /// bounds the total, it does not freeze any one file.
    #[test]
    fn a_raise_paid_for_by_a_cut_elsewhere_is_allowed() {
        let root = scratch("budget-paid", 12_000, 10_000, 22_000);
        let findings = check(&root);
        assert!(findings.is_empty(), "{findings:?}");
        let _ = fs::remove_dir_all(&root);
    }
}
