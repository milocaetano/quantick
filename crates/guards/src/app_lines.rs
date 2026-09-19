//! The UI crate's absolute size, rationed.
//!
//! The number the refactor sprints were graded on was `app`'s *share* of the
//! workspace, `crate.lines.app_percent`. A share is a proxy that can be moved
//! without touching the trunk: it falls when any other crate grows, so a
//! branch that adds 5,000 lines to `pine` and none anywhere else reads as an
//! architectural improvement. The file-size ratchet cannot see it either —
//! splitting a file lowers every per-file number while the crate stays the
//! size it was.
//!
//! What the goal actually asks is that `app` get smaller. So this guard caps
//! the absolute count: the production lines of every tracked file under
//! `crates/app/`, as [`size`] counts them, recorded once as the `crates/app`
//! entry of [`BASELINE_FILE`] on the shared [`Policy`]. Over the ceiling
//! fails; more than [`SLACK`] below it asks for `--tighten`; a raise is two
//! signed edits a reviewer can argue with.
//!
//! The measurement is the one `--report` prints as `crate.lines.app`, and a
//! test in `tests/guards.rs` holds the two rows equal, so the report and the
//! ratchet cannot drift into two definitions of "the size of `app`".

use std::path::Path;

use crate::Finding;
use crate::ratchet::{self, Policy, Unmeasured};
use crate::size;

/// The recorded ceiling and its budget.
pub const BASELINE_FILE: &str = "crates/guards/app-lines-baseline.txt";

/// The one baseline entry: the UI crate.
pub const ENTRY: &str = "crates/app";

/// The tree the total is taken over. The trailing slash keeps a sibling such
/// as a future `crates/app-core` out of the count.
const SOURCE: &str = "crates/app/";

/// How far below its ceiling the total may sit before the entry must be
/// tightened: the size and UI-free ratchets' number, for their reason.
pub const SLACK: usize = size::SLACK;

/// What the guard asks for when the total is over, or far under, its ceiling.
pub const REMEDY: &str = "Over the ceiling, the UI crate grew. New code that is not drawing \
    belongs in a crate below `app`; code that is drawing docks as a new file against an existing \
    port, and pays for itself by moving as many lines out of crates/app in the same change. A \
    deliberate raise is the `crates/app` entry and the !budget in \
    crates/guards/app-lines-baseline.txt, both raised and signed with a reason in the same \
    change. A total that fell needs no argument: `cargo run -p quantick-guards -- --tighten` \
    writes the new number.";

/// What the guard asks for when the budget has fallen far below its entry.
pub const BUDGET_SLACK_REMEDY: &str = "The recorded ceiling fell and the !budget has not caught \
    up. Nothing has to be argued: `cargo run -p quantick-guards -- --tighten` writes the new \
    total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "crates/guards/app-lines-baseline.txt could not be read as \
    data, so no ceiling was checked. Every line is blank, a `#` comment, the one \
    `!budget <count>` directive, or `crates/app <count>`. Fix the line the finding names.";

/// What the guard asks for when the total could not be taken at all.
pub const UNMEASURED_REMEDY: &str = "The size of crates/app could not be taken, so no ceiling \
    was checked: a path under crates/app could not be listed, read or decoded. Fix the path the \
    finding names. Until then the guard is not reporting a clean tree; it is reporting that it \
    could not look.";

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
    unit: "production lines",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// The rationed total today: production lines under [`SOURCE`]. An error
/// when the walk could not measure every path there, never a smaller number.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    size::measure_under(root, SOURCE).map(|counts| ratchet::total(&counts))
}

/// The total against the recorded ceiling and its budget.
pub fn check(root: &Path) -> Vec<Finding> {
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    match measured(root) {
        Ok(total) => POLICY.against(&recorded, &[(ENTRY.to_owned(), total)], 0, &|path| {
            path == ENTRY
        }),
        Err(unmeasured) => vec![Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY)],
    }
}

/// The edit-time question. The quantity is a sum over the whole crate, so a
/// write to any tracked file under it — or to the baseline — asks the whole
/// question; anything else is out of scope.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    let in_scope =
        (relative.starts_with(SOURCE) && size::tracked(relative)) || relative == BASELINE_FILE;
    if in_scope { check(root) } else { Vec::new() }
}

/// Lower the entry, and the budget with it, once the total has fallen more
/// than [`SLACK`] below. Refused when the crate could not be measured whole.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let total = measured(root).map_err(|unmeasured| format!("app-lines: {unmeasured}"))?;
    POLICY.tighten(root, &[(ENTRY.to_owned(), total)], 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch_dir::ScratchDir;
    use std::fs;

    /// `count` production lines.
    fn lines(count: usize) -> String {
        (0..count)
            .map(|index| format!("fn f{index}() {{}}\n"))
            .collect()
    }

    /// A scratch workspace whose `crates/app` holds `total` production lines
    /// split over a source file and a build script, a test module that must
    /// not count, and a baseline recording `ceiling`.
    fn tree(total: usize, ceiling: usize) -> ScratchDir {
        let root = ScratchDir::new("app-lines");
        fs::create_dir_all(root.join("crates/app/src")).expect("app source is creatable");
        fs::create_dir_all(root.join("crates/app-core/src")).expect("sibling is creatable");
        fs::create_dir_all(root.join("crates/guards")).expect("guards dir is creatable");
        let half = total / 2;
        fs::write(
            root.join("crates/app/src/lib.rs"),
            format!(
                "{}#[cfg(test)]\nmod tests {{\n{}}}\n",
                lines(half),
                lines(50)
            ),
        )
        .expect("source is writable");
        fs::write(root.join("crates/app/build.rs"), lines(total - half))
            .expect("build script is writable");
        // A sibling whose name starts with `app` is not the UI crate.
        fs::write(root.join("crates/app-core/src/lib.rs"), lines(500))
            .expect("sibling is writable");
        fs::write(
            root.join(BASELINE_FILE),
            format!("!budget {ceiling}\n{ENTRY} {ceiling}\n"),
        )
        .expect("baseline is writable");
        root
    }

    #[test]
    fn the_total_is_every_production_line_of_the_crate_and_nothing_beside_it() {
        let root = tree(300, 300);
        assert_eq!(measured(root.path()), Ok(300));
        assert!(check(root.path()).is_empty());
    }

    #[test]
    fn growth_past_the_ceiling_fails() {
        let root = tree(301, 300);
        let findings = check(root.path());
        assert!(
            findings
                .iter()
                .any(|f| f.line.contains("301 production lines, ceiling 300 (+1)")),
            "{findings:?}"
        );
    }

    #[test]
    fn a_shrink_past_the_slack_asks_for_the_ceiling_to_follow_it() {
        let root = tree(1000 - SLACK - 1, 1000);
        let findings = check(root.path());
        assert!(
            findings
                .iter()
                .any(|f| f.line.contains("tighten the entry")),
            "{findings:?}"
        );
        tighten(root.path()).expect("tighten runs");
        assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
    }

    #[test]
    fn a_missing_entry_is_a_finding_not_a_free_pass() {
        let root = tree(10, 10);
        fs::write(root.join(BASELINE_FILE), "!budget 10\n").expect("baseline is writable");
        assert!(!check(root.path()).is_empty());
    }

    #[test]
    fn a_missing_crate_is_unmeasured_rather_than_zero() {
        let root = tree(10, 10);
        fs::remove_dir_all(root.join("crates/app")).expect("crate is removable");
        let findings = check(root.path());
        assert!(
            findings.iter().all(|f| f.remedy == UNMEASURED_REMEDY) && !findings.is_empty(),
            "{findings:?}"
        );
    }

    #[test]
    fn only_the_crate_and_its_baseline_are_in_scope_at_edit_time() {
        let root = tree(301, 300);
        assert!(!check_file(root.path(), "crates/app/src/lib.rs").is_empty());
        assert!(!check_file(root.path(), BASELINE_FILE).is_empty());
        assert!(check_file(root.path(), "crates/app-core/src/lib.rs").is_empty());
        assert!(check_file(root.path(), "crates/app/tests/it.rs").is_empty());
    }
}
