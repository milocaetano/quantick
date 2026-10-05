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
//!
//! A raise is written as a file of its own, never as an edit to the baseline.
//! Beside every baseline sits a raises directory ([`Policy::raises_dir`]):
//! each file in it holds signed deltas, `crates/app +239` and `!budget +239`,
//! with the comment saying why, and the parser adds them to the numbers the
//! baseline records. A baseline of one total used to take an edit to the same
//! two lines from every branch that grew `app` — nine of fifteen merged pull
//! requests in a row — so any two open at once conflicted. A new file per
//! branch cannot conflict with another branch's new file. `--tighten` writes
//! its cuts the same way, so after a ratchet lands its baseline is never
//! rewritten.

use std::fs;
use std::path::Path;

use crate::Finding;

/// The line in a baseline that caps the *sum* of every recorded ceiling.
///
/// A directive rather than a comment because the parser strips comments, and
/// a budget the parser cannot see is one that silently stops existing the day
/// somebody reflows the file.
pub const BUDGET_DIRECTIVE: &str = "!budget";

/// What a baseline's raises directory is called, in place of the baseline's
/// `.txt`: `app-lines-baseline.txt` takes its raises from
/// `app-lines-baseline.d/`.
pub const RAISES_SUFFIX: &str = ".d";

/// What a guard's own walk measured, summed.
///
/// One owner for the arithmetic; each guard's `measured` reaches it through
/// [`complete_total`] over its own `measure`. The copies this replaced were
/// byte-identical, in a change that elsewhere removed a duplicated constant
/// for exactly this reason — and the failure they invited is quiet: a guard
/// summing its counts differently from its siblings makes [`crate::report`]
/// print totals that are not comparable, with nothing to fail.
///
/// Deliberately the *measurement* and not [`Baseline::recorded`]. The budget
/// caps what the repository has signed for; this is what its files actually
/// weigh, and the gap between the two is the debt `--tighten` has yet to
/// write off.
pub fn total(counts: &[(String, usize)]) -> usize {
    counts.iter().map(|(_, count)| count).sum()
}

/// [`total`], refused when the walk did not measure every path it tracks.
///
/// A sum over a walk that skipped something is smaller than the tree, and
/// smaller is the flattering direction: `--report` printed 0 root lines for
/// the extension boundary whenever its scan failed (#365), which reads as
/// "excellent" and cannot be told apart from "measured nothing". Each line of
/// `missed` names one path or directory the walk could not measure; any at
/// all turns the total into a failure the caller has to print as one.
pub fn complete_total(counts: &[(String, usize)], missed: &[String]) -> Result<usize, Unmeasured> {
    if missed.is_empty() {
        return Ok(total(counts));
    }
    Err(Unmeasured {
        missed: missed.to_vec(),
    })
}

/// A measurement that could not be taken, and why.
///
/// Typed rather than a message so a caller can enumerate what was missed —
/// tell a missing root from one unreadable file — without parsing prose;
/// [`Display`](std::fmt::Display) gives the sentence `--report` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmeasured {
    /// One `"  <path>: <reason>"` line per path or directory the walk could
    /// not measure, in the order the walk met them.
    pub missed: Vec<String>,
}

impl std::fmt::Display for Unmeasured {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} path(s) could not be measured:\n{}",
            self.missed.len(),
            self.missed.join("\n")
        )
    }
}

/// One recorded ceiling, with the position that lets [`Policy::tighten`]
/// rewrite it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Workspace-relative path, with forward slashes.
    pub path: String,
    /// What this file has been signed for: the baseline's number plus every
    /// raise.
    pub ceiling: usize,
    /// Index into the baseline file's lines, so a rewrite touches the number
    /// and leaves every comment where its author put it. `None` for an entry
    /// only a raise file names.
    pub line: Option<usize>,
}

/// The cap on the sum of every recorded ceiling, with the position that lets
/// [`Policy::tighten`] rewrite it.
#[derive(Debug, Clone)]
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
    /// Every raise file applied, workspace-relative, in name order.
    pub raise_files: Vec<String>,
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
    /// How far *over* the budget the total may go before it is a finding.
    ///
    /// Zero for a ratchet whose budget sums permissions: a ceiling raise is
    /// deliberate, so there is nothing to absorb. Non-zero for one that sums
    /// measured bytes, where an ordinary edit moves the total and a budget
    /// that fails on every sentence is a budget somebody deletes.
    pub budget_headroom: usize,
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
                    first.line.map_or(0, |at| at + 1)
                ));
            }
            entries.push(Entry {
                path,
                ceiling,
                line: Some(line),
            });
        }
        let raise_files = self.apply_raises(root, &mut entries, &mut budget)?;
        Ok(Baseline {
            entries,
            budget,
            raise_files,
        })
    }

    /// The directory of raise files beside [`Policy::baseline_file`],
    /// workspace-relative.
    pub fn raises_dir(&self) -> String {
        let stem = self
            .baseline_file
            .strip_suffix(".txt")
            .unwrap_or(self.baseline_file);
        format!("{stem}{RAISES_SUFFIX}")
    }

    /// Whether a workspace-relative path is this ratchet's data — the
    /// baseline or one of its raise files — so an edit to either asks the
    /// edit-time hook the whole question.
    pub fn owns(&self, relative: &str) -> bool {
        relative == self.baseline_file
            || relative
                .strip_prefix(&self.raises_dir())
                .is_some_and(|rest| rest.starts_with('/'))
    }

    /// Add every raise file's deltas to the entries and the budget, in file
    /// name order so the result never depends on how the directory lists.
    /// Returns the files applied. A missing directory is no raises.
    ///
    /// Each line is `path +N`, `path -N` or `!budget +N`. The sign is
    /// required: a bare number would read as a ceiling, and a ceiling written
    /// in a raise file would be added to the one it meant to replace.
    fn apply_raises(
        &self,
        root: &Path,
        entries: &mut Vec<Entry>,
        budget: &mut Option<Budget>,
    ) -> Result<Vec<String>, String> {
        let dir = self.raises_dir();
        let listing = match fs::read_dir(root.join(&dir)) {
            Ok(listing) => listing,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("{dir} is unreadable: {e}")),
        };
        // Anything but a flat `.txt` file is refused rather than skipped: a
        // raise saved under the wrong name, or under `fix/` because the
        // branch name has a slash, would otherwise be a signed raise the
        // guard silently does not apply.
        let mut names = Vec::new();
        for item in listing {
            let item = item.map_err(|e| format!("{dir} is unreadable: {e}"))?;
            let name = item.file_name().to_string_lossy().into_owned();
            if !item.path().is_file() || !name.ends_with(".txt") {
                return Err(format!(
                    "{dir}/{name}: a raise file is a flat `<branch>.txt`, with any `/` in the \
                     branch spelled `-`"
                ));
            }
            names.push(name);
        }
        names.sort();
        let mut applied = Vec::new();
        for name in names {
            let relative = format!("{dir}/{name}");
            let text = fs::read_to_string(root.join(&relative))
                .map_err(|e| format!("{relative} is unreadable: {e}"))?;
            for (line, raw) in text.lines().enumerate() {
                let content = raw.split('#').next().unwrap_or("").trim();
                if content.is_empty() {
                    continue;
                }
                let at = format!("{relative}:{}", line + 1);
                let (label, delta) = content
                    .rsplit_once(char::is_whitespace)
                    .ok_or_else(|| format!("{at}: expected `path +N` or `path -N`"))?;
                let label = label.trim();
                let raise = delta.strip_prefix('+');
                let cut = delta.strip_prefix('-');
                let Some(amount) = raise.or(cut) else {
                    return Err(format!(
                        "{at}: `{delta}` has no sign — a raise file states a change, `+N` or `-N`"
                    ));
                };
                let amount = amount
                    .parse::<usize>()
                    .map_err(|e| format!("{at}: `{delta}` is not a count: {e}"))?;
                let shift = |from: usize| {
                    if raise.is_some() {
                        Some(from + amount)
                    } else {
                        from.checked_sub(amount)
                    }
                };
                let below_zero = || format!("{at}: `{label} {delta}` takes it below zero");
                if label == BUDGET_DIRECTIVE {
                    let Some(budget) = budget.as_mut() else {
                        return Err(format!(
                            "{at}: a `{BUDGET_DIRECTIVE}` change, but {} has no `{BUDGET_DIRECTIVE}` \
                             line to change",
                            self.baseline_file
                        ));
                    };
                    budget.allowed = shift(budget.allowed).ok_or_else(below_zero)?;
                    continue;
                }
                let index = match entries.iter().position(|entry| entry.path == label) {
                    Some(index) => index,
                    None => {
                        entries.push(Entry {
                            path: label.to_owned(),
                            ceiling: 0,
                            line: None,
                        });
                        entries.len() - 1
                    }
                };
                let entry = &mut entries[index];
                entry.ceiling = shift(entry.ceiling).ok_or_else(below_zero)?;
            }
            applied.push(relative);
        }
        // A cut to zero is how a raise file drops an entry, so a deleted
        // file's ceiling can go without an edit to the baseline.
        entries.retain(|entry| entry.ceiling > 0);
        Ok(applied)
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
                     baseline — add `{path} +{actual}` in a raise file in {}/",
                    self.threshold,
                    self.raises_dir()
                ),
                self.remedy,
            )),
            _ => None,
        }
    }

    /// An entry whose file the scan no longer sees.
    pub fn stale(&self, path: &str) -> Finding {
        Finding::new(
            format!(
                "  {path}: in the baseline but no longer scanned — drop the stale entry with \
                 `{path} -<its ceiling>` in a raise file in {}/",
                self.raises_dir()
            ),
            self.remedy,
        )
    }

    /// How the total stands against the budget.
    ///
    /// `unrecorded` is what the guard measured in files that carry no entry.
    /// A guard passes `0` to keep the budget a pure statement of signed
    /// permissions; it passes a real sum when a file below the threshold
    /// still costs something the budget is meant to see.
    ///
    /// That choice is the difference between a cap and a bypass. With `0`,
    /// growth reaches this function only once somebody has written a raise
    /// down — a branch that grows a file without raising its ceiling is
    /// caught by [`Policy::verdict`] instead. But it also means a tracked
    /// file can be split into pieces that each sit under the threshold, and
    /// the recorded total *falls* while the real weight does not move. The
    /// context ratchet was built on a branch that did exactly that by
    /// accident: 37,490 of the 49,281 bytes it removed from three files
    /// landed in sub-threshold siblings, and the budget applauded. Counting
    /// the unrecorded remainder closes that, at the cost of a total that
    /// drifts with ordinary edits — which is what [`Policy::budget_slack`]
    /// absorbs.
    pub fn budget_verdict(&self, recorded: &Baseline, unrecorded: usize) -> Option<Finding> {
        let name = self.baseline_file;
        let total = recorded.recorded() + unrecorded;
        let Some(budget) = &recorded.budget else {
            return Some(Finding::new(
                format!(
                    "  {name}: no `{BUDGET_DIRECTIVE}` line — the tracked total is {total} \
                     and nothing caps it. Restore the directive at {total} or lower; deleting \
                     it is the one edit that switches pay-as-you-go off for every file at once"
                ),
                self.budget_remedy,
            ));
        };
        if total > budget.allowed + self.budget_headroom {
            return Some(Finding::new(
                format!(
                    "  {}: the tracked total is {total}, over the \
                     {BUDGET_DIRECTIVE} of {} (+{}) — this branch added weight without taking \
                     any away",
                    self.budget_source(recorded, budget),
                    budget.allowed,
                    total - budget.allowed
                ),
                self.budget_remedy,
            ));
        }
        if budget.allowed.saturating_sub(total) > self.budget_slack {
            return Some(Finding::new(
                format!(
                    "  {}: the tracked total is {total}, down from the \
                     {BUDGET_DIRECTIVE} of {} — good news, tighten the budget to {total}",
                    self.budget_source(recorded, budget),
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
        unrecorded: usize,
        seen: &dyn Fn(&str) -> bool,
    ) -> Vec<Finding> {
        let mut findings: Vec<Finding> = counts
            .iter()
            .filter_map(|(path, actual)| self.verdict(recorded.entry(path), path, *actual))
            .collect();
        findings.extend(self.budget_verdict(recorded, unrecorded));
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
    /// cut to the size it actually is. Growth is untouched — that is the
    /// decision a human signs.
    ///
    /// The cut is written as a raise file of its own, never as an edit to the
    /// baseline. Cuts compose the way the code changes behind them do: two
    /// branches that each take lines out of a file each write their own
    /// delta, and the merged ceiling is the merged file. A rewritten number
    /// would instead be two edits to one line. The file is named for the
    /// checked-out branch, and a second run on the branch appends to it.
    ///
    /// Returns one line per number cut.
    pub fn tighten(
        &self,
        root: &Path,
        counts: &[(String, usize)],
        unrecorded: usize,
    ) -> Result<Vec<String>, String> {
        self.tighten_where(root, counts, unrecorded, &|_| true)
    }

    /// [`Policy::tighten`] for a ratchet whose budget covers only some of its
    /// entries: every entry is still tightened, but only those `budgeted`
    /// accepts count toward the budget written down.
    pub fn tighten_where(
        &self,
        root: &Path,
        counts: &[(String, usize)],
        unrecorded: usize,
        budgeted: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, String> {
        let recorded = self.baseline(root)?;
        let mut applied = Vec::new();
        let mut cuts = Vec::new();

        // The total as it will stand once every cut below has been applied.
        // Seeded with what the guard measured outside the baseline, so the
        // number written here is the same total `budget_verdict` compares.
        let mut tightened_total = unrecorded;

        for entry in &recorded.entries {
            // An entry with no measured file keeps its ceiling and still
            // spends it. The check reports it as stale; dropping it from the
            // total here would let a deleted file's budget quietly finance
            // the next raise.
            let lowered = counts
                .iter()
                .find(|(path, _)| path == &entry.path)
                .map(|(_, actual)| *actual)
                .filter(|actual| entry.ceiling.saturating_sub(*actual) > self.slack);
            if budgeted(&entry.path) {
                tightened_total += lowered.unwrap_or(entry.ceiling);
            }
            if let Some(actual) = lowered {
                applied.push(format!("  {}: {} -> {actual}", entry.path, entry.ceiling));
                cuts.push(format!("{} -{}", entry.path, entry.ceiling - actual));
            }
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
            cuts.push(format!(
                "{BUDGET_DIRECTIVE} -{}",
                budget.allowed - tightened_total
            ));
        }

        if cuts.is_empty() {
            return Ok(applied);
        }
        let dir = root.join(self.raises_dir());
        fs::create_dir_all(&dir).map_err(|e| format!("{} is unwritable: {e}", dir.display()))?;
        let file = dir.join(format!("{}.txt", branch_slug(root)));
        let mut text = fs::read_to_string(&file).unwrap_or_default();
        text.push_str("# Written by `cargo run -p quantick-guards -- --tighten`: these shrank.\n");
        for cut in &cuts {
            text.push_str(cut);
            text.push('\n');
        }
        fs::write(&file, text).map_err(|e| format!("{} is unwritable: {e}", file.display()))?;
        Ok(applied)
    }

    /// Where a budget finding's number comes from: the baseline's line, and
    /// every raise file that moved it, so an author who opens that line and
    /// reads a different number knows where the rest is.
    fn budget_source(&self, recorded: &Baseline, budget: &Budget) -> String {
        let at = format!("{}:{}", self.baseline_file, budget.line + 1);
        match recorded.raise_files.len() {
            0 => at,
            raised => format!("{at} plus {raised} raise file(s) in {}/", self.raises_dir()),
        }
    }
}

/// The checked-out branch as a raise file's name, `/` spelled `-`, read from
/// the worktree's own `HEAD`; `tighten` when there is no branch to name.
///
/// Read rather than asked of `git`, because this crate has no dependencies
/// and runs no processes. A worktree's `.git` is a file pointing at its git
/// directory; the main checkout's is the directory itself.
fn branch_slug(root: &Path) -> String {
    let dot_git = root.join(".git");
    let git_dir = match fs::read_to_string(&dot_git) {
        Ok(pointer) => pointer
            .trim()
            .strip_prefix("gitdir:")
            .map(|dir| Path::new(dir.trim()).to_path_buf()),
        Err(_) => Some(dot_git),
    };
    git_dir
        .and_then(|dir| fs::read_to_string(dir.join("HEAD")).ok())
        .and_then(|head| {
            head.trim()
                .strip_prefix("ref: refs/heads/")
                .map(|branch| branch.replace('/', "-"))
        })
        .unwrap_or_else(|| "tighten".to_owned())
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
        budget_headroom: 0,
        unit: "widgets",
        remedy: "over",
        budget_remedy: "budget",
        budget_slack_remedy: "budget slack",
        baseline_remedy: "syntax",
    };

    /// A scratch workspace holding one baseline file.
    fn workspace(baseline: &str) -> crate::scratch_dir::ScratchDir {
        let dir = crate::scratch_dir::ScratchDir::new("ratchet");
        fs::write(dir.path().join("baseline.txt"), baseline).expect("baseline is writable");
        dir
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
                    line: Some(0),
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
            line: Some(0),
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
            .budget_verdict(&recorded, 0)
            .expect("no budget is a finding");
        assert!(finding.line.contains("nothing caps it"), "{}", finding.line);
    }

    #[test]
    fn a_raise_that_was_not_paid_for_is_over_budget() {
        let dir = workspace("!budget 50\nsrc/a.md 40\nsrc/b.md 30\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY.budget_verdict(&recorded, 0).expect("over budget");
        assert!(
            finding
                .line
                .contains("total is 70, over the !budget of 50 (+20)"),
            "{}",
            finding.line
        );
        assert_eq!(finding.remedy, POLICY.budget_remedy);
    }

    #[test]
    fn a_total_far_under_budget_asks_for_the_cap_to_follow_it_down() {
        let dir = workspace("!budget 200\nsrc/a.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY.budget_verdict(&recorded, 0).expect("under budget");
        assert_eq!(finding.remedy, POLICY.budget_slack_remedy);
    }

    #[test]
    fn an_entry_whose_file_the_scan_no_longer_sees_is_stale() {
        let dir = workspace("!budget 40\nsrc/gone.md 40\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let findings = POLICY.against(&recorded, &[], 0, &|_| false);
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
        let findings = POLICY.against(&recorded, &[], 0, &|path| path == "src/binary.md");
        assert!(
            !findings.iter().any(|f| f.line.contains("stale")),
            "{findings:?}"
        );
    }

    /// A ceiling and the budget as the guard reads them, raises applied.
    fn effective(dir: &crate::scratch_dir::ScratchDir, path: &str) -> (Option<usize>, usize) {
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let ceiling = recorded.entry(path).map(|entry| entry.ceiling);
        (ceiling, recorded.budget.expect("budget").allowed)
    }

    #[test]
    fn tighten_writes_a_cut_and_leaves_the_baseline_alone() {
        let before = "!budget 100\nsrc/a.md 90  # signed for the parser\n";
        let dir = workspace(before);
        let applied = POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 20)], 0)
            .expect("tighten runs");
        let text = fs::read_to_string(dir.path().join("baseline.txt")).expect("readable");
        assert_eq!(text, before, "the baseline is never rewritten");
        let cut = fs::read_to_string(dir.path().join("baseline.d/tighten.txt")).expect("cut");
        assert!(cut.contains("src/a.md -70\n!budget -80\n"), "{cut}");
        assert_eq!(effective(&dir, "src/a.md"), (Some(20), 20));
        assert!(
            applied.iter().any(|line| line.contains("90 -> 20")),
            "{applied:?}"
        );
    }

    #[test]
    fn a_second_tighten_appends_to_the_branch_file() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 60)], 0)
            .expect("first tighten runs");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 30)], 0)
            .expect("second tighten runs");
        assert_eq!(effective(&dir, "src/a.md"), (Some(30), 30));
    }

    #[test]
    fn tighten_never_raises_a_ceiling() {
        let dir = workspace("!budget 100\nsrc/a.md 40\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 900)], 0)
            .expect("tighten runs");
        assert_eq!(effective(&dir, "src/a.md").0, Some(40));
    }

    #[test]
    fn tighten_follows_the_budget_down_but_only_past_the_budget_slack() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 10)], 0)
            .expect("tighten runs");
        assert_eq!(effective(&dir, "src/a.md").1, 10);
    }

    #[test]
    fn tighten_leaves_a_budget_alone_when_the_gap_is_headroom_somebody_signed() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 60)], 0)
            .expect("tighten runs");
        // 60 is 40 under the budget, inside BUDGET_SLACK of 50.
        assert_eq!(effective(&dir, "src/a.md"), (Some(60), 100));
    }

    #[test]
    fn an_entry_with_no_measurement_still_spends_its_ceiling_against_the_budget() {
        let dir = workspace("!budget 100\nsrc/a.md 90\nsrc/gone.md 10\n");
        POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 10)], 0)
            .expect("tighten runs");
        // 10 measured plus the 10 the vanished entry still holds.
        assert_eq!(effective(&dir, "src/gone.md"), (Some(10), 20));
    }

    /// One raise file beside the scratch baseline.
    fn raise(dir: &crate::scratch_dir::ScratchDir, name: &str, text: &str) {
        let raises = dir.path().join("baseline.d");
        let file = raises.join(name);
        let parent = file.parent().expect("a raise file has a directory");
        fs::create_dir_all(parent).expect("raises directory is creatable");
        fs::write(file, text).expect("raise file is writable");
    }

    #[test]
    fn a_raise_file_adds_to_the_entry_and_the_budget_it_names() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "feat-x.txt", "# why\nsrc/a.md +20\n!budget +20\n");
        raise(&dir, "feat-y.txt", "src/a.md +5\n!budget +5\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        assert_eq!(recorded.entry("src/a.md").expect("entry").ceiling, 115);
        assert_eq!(recorded.budget.expect("budget").allowed, 125);
        assert_eq!(
            recorded.raise_files,
            ["baseline.d/feat-x.txt", "baseline.d/feat-y.txt"]
        );
    }

    #[test]
    fn a_raise_file_may_trade_one_entry_for_another_and_add_a_new_one() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "trade.txt", "src/a.md -30\nsrc/b.md +30\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        assert_eq!(recorded.entry("src/a.md").expect("a").ceiling, 60);
        let added = recorded.entry("src/b.md").expect("b");
        assert_eq!((added.ceiling, added.line), (30, None));
        assert!(POLICY.budget_verdict(&recorded, 0).is_none());
    }

    #[test]
    fn a_raise_without_a_sign_is_refused_rather_than_read_as_a_ceiling() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "x.txt", "src/a.md 120\n");
        let problem = POLICY.baseline(dir.path()).expect_err("unsigned");
        assert!(
            problem.contains("baseline.d/x.txt:1") && problem.contains("has no sign"),
            "{problem}"
        );
    }

    #[test]
    fn a_cut_below_zero_and_a_budget_change_with_no_budget_are_refused() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "x.txt", "src/a.md -91\n");
        let problem = POLICY.baseline(dir.path()).expect_err("below zero");
        assert!(problem.contains("below zero"), "{problem}");

        let dir = workspace("src/a.md 90\n");
        raise(&dir, "x.txt", "!budget +1\n");
        let problem = POLICY.baseline(dir.path()).expect_err("no budget line");
        assert!(problem.contains("no `!budget` line"), "{problem}");
    }

    #[test]
    fn the_ratchet_owns_its_baseline_and_its_raise_files_only() {
        assert_eq!(POLICY.raises_dir(), "baseline.d");
        assert!(POLICY.owns("baseline.txt"));
        assert!(POLICY.owns("baseline.d/feat-x.txt"));
        assert!(!POLICY.owns("baseline.dx/feat-x.txt"));
        assert!(!POLICY.owns("src/a.md"));
    }

    #[test]
    fn tighten_with_nothing_to_lower_writes_nothing() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "x.txt", "src/a.md +10\n!budget +10\n");
        let applied = POLICY
            .tighten(dir.path(), &[("src/a.md".into(), 100)], 0)
            .expect("tighten runs");
        assert!(applied.is_empty(), "{applied:?}");
        assert!(!dir.path().join("baseline.d/tighten.txt").exists());
    }

    #[test]
    fn tighten_cuts_a_raised_entry_from_its_raised_ceiling() {
        let dir = workspace("!budget 200\nsrc/a.md 90\nsrc/b.md 100\n");
        raise(&dir, "x.txt", "# signed for x\nsrc/b.md +10\n!budget +10\n");
        raise(&dir, "y.txt", "src/c.md +5  # new file\n!budget +5\n");
        POLICY
            .tighten(
                dir.path(),
                &[
                    ("src/a.md".into(), 20),
                    ("src/b.md".into(), 110),
                    ("src/c.md".into(), 5),
                ],
                0,
            )
            .expect("tighten runs");
        assert!(dir.path().join("baseline.d/x.txt").is_file());
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        // 20 + 110 + 5 = 135, 80 under the raised budget of 215.
        assert_eq!(recorded.recorded(), 135);
        assert_eq!(recorded.budget.expect("budget").allowed, 135);
    }

    #[test]
    fn a_cut_to_zero_drops_the_entry() {
        let dir = workspace("!budget 100\nsrc/a.md 90\nsrc/gone.md 10\n");
        raise(&dir, "x.txt", "src/gone.md -10\n!budget -10\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        assert!(recorded.entry("src/gone.md").is_none());
        assert!(
            POLICY
                .against(&recorded, &[], 0, &|path| path == "src/a.md")
                .is_empty()
        );
    }

    #[test]
    fn a_nested_or_misnamed_raise_file_is_refused_rather_than_skipped() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "fix/branch.txt", "src/a.md +1\n");
        let problem = POLICY.baseline(dir.path()).expect_err("nested");
        assert!(
            problem.contains("baseline.d/fix: a raise file is a flat"),
            "{problem}"
        );

        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "feat-x.md", "src/a.md +1\n");
        let problem = POLICY.baseline(dir.path()).expect_err("not .txt");
        assert!(problem.contains("baseline.d/feat-x.md"), "{problem}");
    }

    #[test]
    fn a_budget_finding_names_the_raise_files_behind_its_number() {
        let dir = workspace("!budget 100\nsrc/a.md 90\n");
        raise(&dir, "x.txt", "src/a.md +30\n!budget +5\n");
        let recorded = POLICY.baseline(dir.path()).expect("baseline parses");
        let finding = POLICY.budget_verdict(&recorded, 0).expect("over budget");
        assert!(
            finding
                .line
                .contains("baseline.txt:1 plus 1 raise file(s) in baseline.d/"),
            "{}",
            finding.line
        );
    }

    /// A walk that missed a path has no total. The sum of what it did see is
    /// the flattering number, and returning it is how `--report` came to print
    /// 0 for a scan that had failed outright.
    #[test]
    fn a_walk_that_missed_a_path_has_no_total() {
        let counts = [("src/a.md".to_owned(), 7), ("src/b.md".to_owned(), 5)];
        assert_eq!(complete_total(&counts, &[]), Ok(12));
        let missed = ["  src/c.md: could not be read: denied".to_owned()];
        let failure = complete_total(&counts, &missed).expect_err("a missed path is a failure");
        assert_eq!(
            failure.missed,
            missed.to_vec(),
            "the failure lists what was missed"
        );
        assert_eq!(
            failure.to_string(),
            "1 path(s) could not be measured:
  src/c.md: could not be read: denied"
        );
    }
}
