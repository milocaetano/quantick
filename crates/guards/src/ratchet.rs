//! The ratchet mechanism, once, for every guard that rations a measurable
//! quantity per file.
//!
//! [`size`](crate::size) rations production lines of Rust;
//! [`context`](crate::context) rations the bytes a file adds to every Claude
//! session. The two measure different things over different trees, but
//! everything between the measurement and the finding is identical: a data
//! file of recorded ceilings, a threshold below which a file needs no entry, a
//! slack that asks for a ceiling to be tightened when a file shrinks, a
//! `!budget` line capping the sum of every ceiling so growth is
//! pay-as-you-go, and a `--tighten` that only ever moves numbers down.
//!
//! That shared half used to be one copy inside `size.rs`. Writing it a second
//! time for the context guard would have been the duplicated-constant defect
//! this repository files against its own code — two baseline parsers drifting
//! apart, two wordings of the same finding, and a `--tighten` that fixed one
//! file format correctly and the other by accident. So the shared half is a
//! [`Policy`]: a third ratchet is a constant, not a copy.
//!
//! What a policy does *not* own is measurement. Walking `crates/` for `.rs`
//! files and counting the lines that ship in the binary has nothing in common
//! with reading a known list of markdown files, and a pair of `fn` pointers
//! wide enough to cover both would have been a worse abstraction than two
//! honest scans. Each guard measures its own tree and hands the counts here.

use std::fs;
use std::path::Path;

use crate::Finding;

/// The line in a baseline that caps the *sum* of every recorded ceiling.
///
/// A directive rather than a comment because the parser strips comments, and
/// a budget the parser cannot see is one that silently stops existing the day
/// somebody reflows the file.
pub const BUDGET_DIRECTIVE: &str = "!budget";

/// One recorded ceiling, with the position that lets [`Policy::tighten`]
/// rewrite it.
#[derive(Debug)]
pub struct Entry {
    /// Workspace-relative path, with forward slashes.
    pub path: String,
    /// What this file has been signed for.
    pub ceiling: usize,
    /// Index into the baseline file's lines, so a rewrite touches the number
    /// and leaves every comment where its author put it.
    pub line: usize,
}

/// The cap on the sum of every recorded ceiling, with the position that lets
/// [`Policy::tighten`] rewrite it.
#[derive(Debug)]
pub struct Budget {
    /// The signed total.
    pub allowed: usize,
    /// Index into the baseline file's lines.
    pub line: usize,
}

/// Everything a baseline file states: the per-file ceilings, and the cap on
/// their total.
#[derive(Debug)]
pub struct Baseline {
    /// Every `path ceiling` pair, in file order.
    pub entries: Vec<Entry>,
    /// Absent only when the directive is missing, which is itself a finding.
    /// Parsed as an option rather than defaulted, because a default would
    /// make deleting the line the cheapest way past the budget — the guard
    /// would hand out its own bypass.
    pub budget: Option<Budget>,
}

impl Baseline {
    /// The recorded debt: what the repository has signed for, not what its
    /// files currently measure. Deliberately the ceilings rather than the
    /// counts — the budget rations *permission* to be large, so a file
    /// sitting under its ceiling still spends the whole entry until the entry
    /// is tightened, and [`Policy::slack`] is what bounds that gap.
    pub fn recorded(&self) -> usize {
        self.entries.iter().map(|entry| entry.ceiling).sum()
    }

    /// The entry for one path, if it has one.
    pub fn entry(&self, path: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.path == path)
    }
}

/// One ratchet's numbers and wordings: everything that differs between the
/// size guard and the context guard, and nothing that does not.
pub struct Policy {
    /// The recorded ceilings, as a workspace-relative path.
    pub baseline_file: &'static str,
    /// The measurement above which a file must carry a baseline entry. Files
    /// below it are not the problem the guard exists for, and tracking them
    /// would turn every ordinary edit into a baseline update — the reliable
    /// way to get a guard disabled.
    pub threshold: usize,
    /// How far below its ceiling a tracked file may sit before the entry must
    /// be tightened.
    pub slack: usize,
    /// How far below the budget the recorded total may sit before the budget
    /// itself has to come down. Wider than [`Policy::slack`] on purpose: this
    /// number tracks every entry at once, so ordinary tightening moves it
    /// constantly, and a budget needing a rewrite on every extraction is a
    /// budget people delete.
    pub budget_slack: usize,
    /// What is being counted, as it reads inside a finding — `production
    /// lines`, `bytes of context`.
    pub unit: &'static str,
    /// What the guard asks for when a file is over its ceiling.
    pub remedy: &'static str,
    /// What it asks for when the recorded total is over budget.
    pub budget_remedy: &'static str,
    /// What it asks for when the recorded total has fallen *below* budget.
    /// Good news, with the number already computed, so it is one command
    /// rather than an argument — and deliberately not
    /// [`Policy::budget_remedy`], which would tell an author to pay for a
    /// raise they never made.
    pub budget_slack_remedy: &'static str,
    /// What it asks for when the baseline itself cannot be read as data.
    /// Neither of the others applies: nothing docked in the trunk and no
    /// raise was made, so both would send an author to restructure code over
    /// a typo in a data file.
    pub baseline_remedy: &'static str,
}

impl Policy {
    /// Read the ceilings and the budget. Comments and blank lines are
    /// skipped; anything else must be `path ceiling` or the
    /// [`BUDGET_DIRECTIVE`] line, because a typo silently dropping an entry
    /// would leave a file unguarded and looking green.
    pub fn baseline(&self, root: &Path) -> Result<Baseline, String> {
        let file = root.join(self.baseline_file);
        let text = fs::read_to_string(&file)
            .map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
        let name = self.baseline_file;
        let mut entries = Vec::new();
        let mut budget: Option<Budget> = None;
        for (line, raw) in text.lines().enumerate() {
            let content = raw.split('#').next().unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }
            if let Some(rest) = content.strip_prefix(BUDGET_DIRECTIVE) {
                let allowed = rest.trim().parse::<usize>().map_err(|e| {
                    format!("{name}:{}: `{}` is not a count: {e}", line + 1, rest.trim())
                })?;
                // Two budgets is not a harmless duplicate: whichever one loses
                // is a cap somebody wrote and nothing enforces, and the file
                // gives no hint which that was.
                if let Some(first) = &budget {
                    return Err(format!(
                        "{name}:{}: a second `{BUDGET_DIRECTIVE}` — the first is on line {}, and \
                         only one of them could ever be the cap",
                        line + 1,
                        first.line + 1
                    ));
                }
                budget = Some(Budget { allowed, line });
                continue;
            }
            let (path, ceiling) = content
                .rsplit_once(char::is_whitespace)
                .ok_or_else(|| format!("{name}:{}: expected `path ceiling`", line + 1))?;
            let ceiling = ceiling
                .parse::<usize>()
                .map_err(|e| format!("{name}:{}: `{ceiling}` is not a count: {e}", line + 1))?;
            let path = path.trim().to_owned();
            // A path listed twice is the same defect as a second `!budget`,
            // one level down: [`Baseline::entry`] answers from the first, so
            // the second is a ceiling somebody wrote and nothing enforces,
            // while [`Baseline::recorded`] sums both and charges the budget
            // twice. Refused rather than picked, because the file gives no
            // hint which of the two was meant.
            if let Some(first) = entries.iter().find(|e: &&Entry| e.path == path) {
                return Err(format!(
                    "{name}:{}: `{path}` is already recorded on line {} — only one of the two \
                     ceilings could ever be enforced",
                    line + 1,
                    first.line + 1
                ));
            }
            entries.push(Entry {
                path,
                ceiling,
                line,
            });
        }
        Ok(Baseline { entries, budget })
    }

    /// A baseline that would not parse, worded as the finding an author acts
    /// on. Kept here so both guards and both of their entry points say it the
    /// same way.
    pub fn unparsed(&self, problem: &str) -> Finding {
        Finding::new(format!("  {problem}"), self.baseline_remedy)
    }

    /// How one measured file stands against its recorded ceiling. The single
    /// place the three verdicts are worded, so a whole-repo scan and the
    /// single-file check the edit-time hook runs can never disagree about the
    /// same file.
    pub fn verdict(&self, entry: Option<&Entry>, path: &str, actual: usize) -> Option<Finding> {
        let unit = self.unit;
        match entry {
            Some(entry) if actual > entry.ceiling => Some(Finding::new(
                format!(
                    "  {path}: {actual} {unit}, ceiling {} (+{})",
                    entry.ceiling,
                    actual - entry.ceiling
                ),
                self.remedy,
            )),
            Some(entry) if entry.ceiling.saturating_sub(actual) > self.slack => Some(Finding::new(
                format!(
                    "  {path}: down to {actual} from {} — good news, tighten the entry to \
                         {actual}",
                    entry.ceiling
                ),
                self.remedy,
            )),
            None if actual > self.threshold => Some(Finding::new(
                format!(
                    "  {path}: {actual} {unit}, over the {} threshold and absent from the \
                     baseline — add `{path} {actual}`",
                    self.threshold
                ),
                self.remedy,
            )),
            _ => None,
        }
    }

    /// An entry whose file the scan no longer sees.
    pub fn stale(&self, path: &str) -> Finding {
        Finding::new(
            format!("  {path}: in the baseline but no longer scanned — drop the stale entry"),
            self.remedy,
        )
    }

    /// How the recorded total stands against the budget.
    ///
    /// It reads the baseline alone and never the files, which is what makes
    /// the rule pay-as-you-go rather than a second size check. Growth reaches
    /// this function only once somebody has written a raise down — so a
    /// branch that grows a file and *does not* raise its ceiling is caught by
    /// [`Policy::verdict`], and one that raises the ceiling honestly is
    /// caught here unless it paid for the raise by extraction.
    pub fn budget_verdict(&self, recorded: &Baseline) -> Option<Finding> {
        let name = self.baseline_file;
        let total = recorded.recorded();
        let Some(budget) = &recorded.budget else {
            return Some(Finding::new(
                format!(
                    "  {name}: no `{BUDGET_DIRECTIVE}` line — the recorded ceilings total {total} \
                     and nothing caps them. Restore the directive at {total} or lower; deleting \
                     it is the one edit that switches pay-as-you-go off for every file at once"
                ),
                self.budget_remedy,
            ));
        };
        if total > budget.allowed {
            return Some(Finding::new(
                format!(
                    "  {name}:{}: the recorded ceilings total {total}, over the \
                     {BUDGET_DIRECTIVE} of {} (+{}) — this branch raised a ceiling without \
                     lowering another",
                    budget.line + 1,
                    budget.allowed,
                    total - budget.allowed
                ),
                self.budget_remedy,
            ));
        }
        if budget.allowed.saturating_sub(total) > self.budget_slack {
            return Some(Finding::new(
                format!(
                    "  {name}:{}: the recorded ceilings total {total}, down from the \
                     {BUDGET_DIRECTIVE} of {} — good news, tighten the budget to {total}",
                    budget.line + 1,
                    budget.allowed
                ),
                self.budget_slack_remedy,
            ));
        }
        None
    }

    /// Every way a set of measurements and the recorded baseline disagree:
    /// each file against its ceiling, the total against the budget, and each
    /// entry whose file the scan no longer sees.
    ///
    /// `seen` answers "is this path still there?" rather than "did it
    /// measure?". A file that exists but could not be decoded or opened is
    /// present, not gone, and telling the author to drop its entry would
    /// delete a ceiling over a live file — after which it is re-added at
    /// whatever size it has since grown to, laundering a raise through the
    /// guard's own instructions.
    pub fn against(
        &self,
        recorded: &Baseline,
        counts: &[(String, usize)],
        seen: &dyn Fn(&str) -> bool,
    ) -> Vec<Finding> {
        let mut findings: Vec<Finding> = counts
            .iter()
            .filter_map(|(path, actual)| self.verdict(recorded.entry(path), path, *actual))
            .collect();
        findings.extend(self.budget_verdict(recorded));
        findings.extend(
            recorded
                .entries
                .iter()
                .filter(|entry| !seen(&entry.path))
                .map(|entry| self.stale(&entry.path)),
        );
        findings
    }

    /// Apply the one direction that never needs an argument: a file that
    /// shrank more than [`Policy::slack`] below its ceiling has its entry
    /// rewritten to the size it actually is. Growth is untouched — that is
    /// the decision a human signs.
    ///
    /// Returns one line per entry rewritten.
    pub fn tighten(&self, root: &Path, counts: &[(String, usize)]) -> Result<Vec<String>, String> {
        let recorded = self.baseline(root)?;
        let file = root.join(self.baseline_file);
        let text = fs::read_to_string(&file)
            .map_err(|e| format!("{} is unreadable: {e}", file.display()))?;
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let mut applied = Vec::new();

        // The total as it will stand once every rewrite below has been
        // applied, accumulated as they are decided rather than re-read
        // afterwards: the rewritten text is not parsed again, so this is the
        // only place the new sum exists.
        let mut tightened_total = 0;

        for entry in &recorded.entries {
            let Some((_, actual)) = counts.iter().find(|(path, _)| path == &entry.path) else {
                // An entry with no measured file keeps its ceiling and still
                // spends it. The check reports it as stale; dropping it from
                // the total here would let a deleted file's budget quietly
                // finance the next raise.
                tightened_total += entry.ceiling;
                continue;
            };
            if entry.ceiling.saturating_sub(*actual) <= self.slack {
                tightened_total += entry.ceiling;
                continue;
            }
            tightened_total += *actual;
            applied.push(format!("  {}: {} -> {actual}", entry.path, entry.ceiling));
            lines[entry.line] = rewrite(&lines[entry.line], &entry.path, *actual);
        }

        // The budget follows the ceilings down, and **only** down. Letting
        // this raise the number would turn `--tighten` into the bypass the
        // whole mechanism is built to deny: a branch over budget would run
        // the command the failure message recommends and have its raise
        // signed by a tool instead of by a person.
        //
        // And only once the gap is wide enough to *be* a finding — the same
        // test `budget_verdict` applies. Lowering on any gap at all would
        // revoke headroom somebody deliberately signed for.
        if let Some(budget) = &recorded.budget
            && budget.allowed.saturating_sub(tightened_total) > self.budget_slack
        {
            applied.push(format!(
                "  {BUDGET_DIRECTIVE}: {} -> {tightened_total}",
                budget.allowed
            ));
            lines[budget.line] = rewrite(&lines[budget.line], BUDGET_DIRECTIVE, tightened_total);
        }

        if !applied.is_empty() {
            let mut out = lines.join("\n");
            out.push('\n');
            fs::write(&file, out).map_err(|e| format!("{} is unwritable: {e}", file.display()))?;
        }
        Ok(applied)
    }
}

/// Rewrite one baseline line to a new number, carrying any trailing comment
/// across.
///
/// The file header advertises `#` and the parser honours it anywhere on the
/// line, so an author may well have written the justification for a ceiling
/// *beside* it — and that justification is the whole doctrine of these
/// guards. A rewrite that dropped it would delete the signed decision while
/// reporting only that a number went down.
fn rewrite(line: &str, label: &str, number: usize) -> String {
    match line.find('#').map(|at| &line[at..]) {
        Some(comment) => format!("{label} {number}  {comment}"),
        None => format!("{label} {number}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const POLICY: Policy = Policy {
        baseline_file: "baseline.txt",
        threshold: 100,
        slack: 10,
        budget_slack: 50,
        unit: "widgets",
        remedy: "over",
        budget_remedy: "budget",
        budget_slack_remedy: "budget slack",
        baseline_remedy: "syntax",
    };

    /// A scratch workspace holding one baseline file.
    fn workspace(baseline: &str) -> tempdir::TempDir {
        let dir = tempdir::TempDir::new();
        fs::write(dir.path().join("baseline.txt"), baseline).expect("baseline is writable");
        dir
    }

    /// The smallest temporary directory that removes itself, so these tests
    /// stay inside the no-dependency rule the whole crate is built on.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::{fs, process};

        pub struct TempDir(PathBuf);

        static NEXT: AtomicUsize = AtomicUsize::new(0);

        impl TempDir {
            pub fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "quantick-ratchet-{}-{}",
                    process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::create_dir_all(&path).expect("temp dir is creatable");
                Self(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_comment_and_a_blank_line_are_not_entries() {
        let dir = workspace("# a note\n\n!budget 100\nsrc/a.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        assert_eq!(recorded.entries.len(), 1);
        assert_eq!(recorded.recorded(), 40);
        assert_eq!(recorded.budget.expect("budget parsed").allowed, 100);
    }

    #[test]
    fn a_second_budget_line_is_refused_rather_than_silently_losing_one() {
        let dir = workspace("!budget 100\n!budget 200\n");
        let problem = POLICY.baseline(dir.path()).expect_err("two budgets");
        assert!(problem.contains("a second `!budget`"), "{problem}");
    }

    #[test]
    fn a_path_recorded_twice_is_refused_rather_than_charged_twice() {
        let dir = workspace("!budget 100\nsrc/a.md 40\nsrc/a.md 60\n");
        let problem = POLICY.baseline(dir.path()).expect_err("two entries");
        assert!(
            problem.contains("is already recorded on line 2"),
            "{problem}"
        );
    }

    #[test]
    fn a_count_that_is_not_a_number_names_its_line() {
        let dir = workspace("!budget 100\nsrc/a.md lots\n");
        let problem = POLICY.baseline(dir.path()).expect_err("bad count");
        assert!(problem.contains("baseline.txt:2"), "{problem}");
    }

    #[test]
    fn the_unit_appears_in_the_verdict_so_a_finding_says_what_it_counted() {
        let over = POLICY
            .verdict(
                Some(&Entry {
                    path: "a".into(),
                    ceiling: 10,
                    line: 0,
                }),
                "a",
                12,
            )
            .expect("over its ceiling");
        assert!(
            over.line.contains("12 widgets, ceiling 10 (+2)"),
            "{}",
            over.line
        );
    }

    #[test]
    fn a_file_under_the_threshold_needs_no_entry() {
        assert!(POLICY.verdict(None, "a", 100).is_none());
        assert!(POLICY.verdict(None, "a", 101).is_some());
    }

    #[test]
    fn a_file_that_shrank_past_the_slack_asks_for_a_tighter_entry() {
        let entry = Entry {
            path: "a".into(),
            ceiling: 50,
            line: 0,
        };
        assert!(POLICY.verdict(Some(&entry), "a", 40).is_none());
        let finding = POLICY
            .verdict(Some(&entry), "a", 39)
            .expect("more than slack below");
        assert!(
            finding.line.contains("tighten the entry to 39"),
            "{}",
            finding.line
        );
    }

    #[test]
    fn a_missing_budget_line_is_a_finding_rather_than_a_default() {
        let dir = workspace("src/a.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY
            .budget_verdict(&recorded)
            .expect("no budget is a finding");
        assert!(
            finding.line.contains("nothing caps them"),
            "{}",
            finding.line
        );
    }

    #[test]
    fn a_raise_that_was_not_paid_for_is_over_budget() {
        let dir = workspace("!budget 50\nsrc/a.md 40\nsrc/b.md 30\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY.budget_verdict(&recorded).expect("over budget");
        assert!(
            finding
                .line
                .contains("total 70, over the !budget of 50 (+20)"),
            "{}",
            finding.line
        );
        assert_eq!(finding.remedy, POLICY.budget_remedy);
    }

    #[test]
    fn a_total_far_under_budget_asks_for_the_cap_to_follow_it_down() {
        let dir = workspace("!budget 200\nsrc/a.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY.budget_verdict(&recorded).expect("under budget");
        assert_eq!(finding.remedy, POLICY.budget_slack_remedy);
    }

    #[test]
    fn an_entry_whose_file_the_scan_no_longer_sees_is_stale() {
        let dir = workspace("!budget 40\nsrc/gone.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let findings = POLICY.against(&recorded, &[], &|_| false);
        assert!(
            findings
                .iter()
                .any(|f| f.line.contains("drop the stale entry")),
            "{findings:?}"
        );
    }

    #[test]
    fn a_file_that_exists_but_did_not_measure_is_seen_and_keeps_its_ceiling() {
        let dir = workspace("!budget 40\nsrc/binary.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let findings = POLICY.against(&recorded, &[], &|path| path == "src/binary.md");
        assert!(
            !findings.iter().any(|f| f.line.contains("stale")),
            "{findings:?}"
        );
    }

    #[test]
    fn tighten_lowers_an_entry_and_keeps_the_comment_beside_it() {
        let dir = workspace("!budget 100\nsrc/a.md 90  # signed for the parser\n");
        let applied = POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 20)])
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        assert!(
            text.contains("src/a.md 20  # signed for the parser"),
            "{text}"
        );
        assert!(
            applied.iter().any(|line| line.contains("90 -> 20")),
            "{applied:?}"
        );
    }

    #[test]
    fn tighten_never_raises_a_ceiling() {
        let dir = workspace("!budget 100\nsrc/a.md 40\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 900)])
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        assert!(text.contains("src/a.md 40"), "{text}");
    }

    #[test]
    fn tighten_follows_the_budget_down_but_only_past_the_budget_slack() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 10)])
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        assert!(text.contains("!budget 10"), "{text}");
    }

    #[test]
    fn tighten_leaves_a_budget_alone_when_the_gap_is_headroom_somebody_signed() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 60)])
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        // 60 is 40 under the budget, inside BUDGET_SLACK of 50.
        assert!(text.contains("!budget 100"), "{text}");
    }

    #[test]
    fn an_entry_with_no_measurement_still_spends_its_ceiling_against_the_budget() {
        let dir = workspace("!budget 100\nsrc/a.md 90\nsrc/gone.md 10\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 10)])
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        // 10 measured plus the 10 the vanished entry still holds.
        assert!(text.contains("!budget 20"), "{text}");
    }
}
