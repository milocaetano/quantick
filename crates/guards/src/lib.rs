//! Repository guards for the things the compiler cannot see.
//!
//! Five rules hold in this repo that no amount of `cargo build` can check: a
//! file may not silently absorb a crate ([`size`]), a crate's modules may not
//! weld themselves into a cycle ([`cycle`]), everything written into a
//! tracked file is English ([`language`]), sources are UTF-8 without a BOM
//! and without welded doc comments ([`encoding`]), and a test's temporary
//! directory is minted by its crate's scratch module rather than spelled by
//! hand ([`scratch`]). Each is a rule `CLAUDE.md` states and each fails
//! invisibly — fmt, clippy, build and the whole suite stay green while it is
//! broken.
//!
//! # Why this is a crate rather than three test files
//!
//! It used to be three test files under `crates/app/tests/`. None of them
//! imported anything from `quantick-app` — they read files and count lines,
//! and `std` is the whole dependency — but an integration test living in a
//! crate's `tests/` directory makes cargo build that crate first. So asking
//! the cheapest question in the repo meant building the largest crate in it:
//! four minutes of link for five seconds of work, on a warm `target/`.
//!
//! That cost was not paid once. The size ratchet is *designed* to fire while
//! you work — that is what a ratchet is for — and each firing sent the author
//! back through a full build to read one number. A guard that expensive to
//! consult is one that gets consulted late, which is exactly when its finding
//! costs the most to act on.
//!
//! So the guards moved into a crate with no dependencies at all. `cargo test
//! --workspace` still runs them and CI is unchanged; what changed is that
//! `cargo test -p quantick-guards` is now a question you can afford to ask
//! after every edit, and the [`bin`](../quantick_guards/index.html) beside
//! this library answers it for a single file in milliseconds, which is what
//! the edit-time hook in `.claude/hooks/` calls.
//!
//! The rules did not soften. Every threshold, every keyword and every
//! grandfathered path came across unchanged; the only guard whose reach grew
//! is [`encoding`], which now sees every crate rather than only the one it
//! was born inside.

pub mod context;
pub mod cycle;
pub mod encoding;
pub mod generated;
pub mod language;
pub mod ratchet;
pub mod scratch;
pub mod size;
#[cfg(test)]
pub mod tempdir;

use std::path::{Path, PathBuf};

/// The workspace root, from this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/guards sits two levels below the workspace root")
        .to_path_buf()
}

/// One violation, carrying the instruction that fixes *it*.
///
/// The remedy rides on the finding rather than on the guard, because the size
/// guard has three classes of violation whose fixes are unrelated: a file over
/// its ceiling is a capability that docked by editing the trunk, a debt total
/// over budget is a raise nobody paid for, and a baseline that does not parse
/// is a typo in a data file. A wrong remedy is worse than a terse one — it is
/// followed.
///
/// The first attempt made the remedy a function of the finding *strings*,
/// which meant classifying a violation by sniffing its own prose for
/// substrings. That is the duplicated-constant defect this repository files
/// against code: reword a message and the classifier silently starts handing
/// out the wrong instruction, with nothing to fail. Attaching the remedy where
/// the finding is *built* makes the two impossible to separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The violation, already formatted for output.
    pub line: String,
    /// What to do about this one.
    pub remedy: &'static str,
}

impl Finding {
    /// Build a finding. Taking the remedy at construction is the whole point:
    /// there is no way to produce a violation without saying how to fix it.
    pub fn new(line: impl Into<String>, remedy: &'static str) -> Self {
        Self {
            line: line.into(),
            remedy,
        }
    }
}

/// Every distinct remedy across a set of findings, in the order the findings
/// raised them, each appearing once.
///
/// Order is the order to act in, and it comes from the findings rather than
/// from a fixed list, so a guard that grows a fourth class needs no edit here.
pub fn remedies(findings: &[Finding]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for finding in findings {
        if !out.contains(&finding.remedy) {
            out.push(finding.remedy);
        }
    }
    out
}

/// The half of a guard that rations a measurable quantity and can therefore
/// lower its own recorded numbers.
///
/// Carried on the [`Guard`] rather than listed a second time in the binary.
/// The first version of the cycle ratchet was registered in three places —
/// [`GUARDS`], the tighten list in `main.rs`, and the CI test file — and only
/// the third had a drift check. A ratchet added to the registry and forgotten
/// in the binary tightens nothing, silently, which is the failure shape this
/// crate exists to remove.
pub struct Ratchet {
    /// Lower every entry whose measurement has fallen, and the budget with
    /// them. One line per entry rewritten.
    pub tighten: fn(&Path) -> Result<Vec<String>, String>,
    /// The baseline it rewrites, workspace-relative, for the success line.
    pub baseline_file: &'static str,
    /// The gap below the budget it tolerates, for the nothing-to-do line.
    pub budget_slack: usize,
}

/// One guard, so the binary and the tests name the same things in the same
/// order.
pub struct Guard {
    /// How the guard is named in output.
    pub name: &'static str,
    /// Every violation across the whole repository.
    pub check: fn(&Path) -> Vec<Finding>,
    /// Every violation in one file, for the edit-time hook.
    pub check_file: fn(&Path, &str) -> Vec<Finding>,
    /// Present only for a guard built on [`ratchet::Policy`]. `None` for the
    /// scans — [`language`] and [`encoding`] have no numbers to lower.
    pub ratchet: Option<Ratchet>,
}

/// Every guard this crate runs.
pub const GUARDS: &[Guard] = &[
    Guard {
        name: "size",
        check: size::check,
        check_file: size::check_file,
        ratchet: Some(Ratchet {
            tighten: size::tighten,
            baseline_file: size::BASELINE_FILE,
            budget_slack: size::BUDGET_SLACK,
        }),
    },
    Guard {
        name: "context",
        check: context::check,
        check_file: context::check_file,
        ratchet: Some(Ratchet {
            tighten: context::tighten,
            baseline_file: context::BASELINE_FILE,
            budget_slack: context::BUDGET_SLACK,
        }),
    },
    Guard {
        name: "cycle",
        check: cycle::check,
        check_file: cycle::check_file,
        ratchet: Some(Ratchet {
            tighten: cycle::tighten,
            baseline_file: cycle::BASELINE_FILE,
            budget_slack: cycle::BUDGET_SLACK,
        }),
    },
    Guard {
        name: "scratch",
        check: scratch::check,
        check_file: scratch::check_file,
        ratchet: None,
    },
    Guard {
        name: "language",
        check: language::check,
        check_file: language::check_file,
        ratchet: None,
    },
    Guard {
        name: "encoding",
        check: encoding::check,
        check_file: encoding::check_file,
        ratchet: None,
    },
    Guard {
        // No ratchet: a generated file has no number to lower. It either
        // matches the code it was rendered from or it does not, and the fix is
        // always to regenerate rather than to record a new ceiling.
        name: "generated",
        check: generated::check,
        check_file: generated::check_file,
        ratchet: None,
    },
];
