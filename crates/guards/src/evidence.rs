//! Guard that keeps agent execution evidence and mission files out of Git.
//!
//! Every mission used to leave its goal file behind as
//! `.claude/GOAL-archive-<slug>.md`, and every capture, log and review
//! dossier beside it under an evidence folder. By 2026-09-15 those records
//! were 31 MB of a 56 MB pack: more than half of what every clone downloads,
//! none of it read by the code, the build or the next contributor. The PR
//! that removed them also changed the skills to stop writing them, and
//! `main` merged three new archives and nine new evidence files while that
//! PR was still open. Prose did not hold the rule, so this guard does.
//!
//! The results a later reader needs stay on the PR and its linked issue:
//! the mission summary, the review reports and CI. Raw output stays in a
//! temporary directory or a CI artifact.
//!
//! # What counts
//!
//! A path is refused when Git would carry it: tracked already, or untracked
//! and not ignored, so the next `git add -A` would commit it. An ignored
//! local file is working state and passes — `.claude/GOAL.md` is where a
//! mission lives while it runs.
//!
//! # What this cannot see
//!
//! It judges paths, not content. Evidence pasted into a maintained document
//! under another name passes; that is `arch-review`'s call to make.

use std::path::Path;
use std::process::Command;

use crate::Finding;

/// Directories whose only purpose was holding execution evidence.
const EVIDENCE_DIRS: &[&str] = &[
    ".claude/evidence/",
    "docs/evidence/",
    "docs/workflow/evidence/",
];

/// Mission files: the live `GOAL.md` and every archive or source record
/// named after it, directly under `.claude/`.
const GOAL_PREFIX: &str = ".claude/GOAL";

/// The file name every per-task evidence dossier under `docs/` was given.
const DOSSIER_SUFFIX: &str = "-evidence.md";

/// Documents that carry the dossier suffix and stay, each for a reason.
///
/// The control-plane history is curated archaeology: `docs/README.md`
/// indexes it as the reasoning behind the published contract, and it is
/// frozen. The Windows authority record describes maintained tests rather
/// than one run. A new entry needs the same kind of reason; a record of one
/// run is never one.
const ALLOWED: &[&str] = &[
    "docs/control-plane/history/pr3-gateway-evidence.md",
    "docs/control-plane/history/pr4-mcp-evidence.md",
    "docs/control-plane/history/pr5a-events-evidence.md",
    "docs/control-plane/history/pr5b-annotate-evidence.md",
    "docs/control-plane/history/pr5c-evidence.md",
    "docs/control-plane/windows-authority-evidence.md",
];

/// What the guard asks for.
pub const REMEDY: &str = "Execution evidence and mission files do not belong in Git. Remove the \
                          path from the index (`git rm --cached <path>`, or `git rm` to drop it), \
                          keep raw output in a temporary directory or a CI artifact, and put the \
                          result — verdicts, numbers, artifact links — in the PR body. The \
                          mission's goal goes in the PR body, under its summary block.";

/// Whether a workspace-relative path names evidence or a mission file. The
/// single owner of that question, so the scan and the edit-time hook cannot
/// disagree about scope.
pub fn forbidden(relative: &str) -> bool {
    if ALLOWED.contains(&relative) {
        return false;
    }
    let goal_file = relative
        .strip_prefix(GOAL_PREFIX)
        .is_some_and(|rest| !rest.contains('/'));
    goal_file
        || EVIDENCE_DIRS.iter().any(|dir| relative.starts_with(dir))
        || (relative.starts_with("docs/") && relative.ends_with(DOSSIER_SUFFIX))
}

/// Run `git` in `root` and return its standard output, or why it failed.
fn git(root: &Path, args: &[&str]) -> Result<(i32, Vec<u8>), String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|e| format!("git could not be run: {e}"))?;
    Ok((output.status.code().unwrap_or(-1), output.stdout))
}

fn finding(relative: &str) -> Finding {
    Finding::new(
        format!("{relative}: execution evidence or a mission file Git would carry"),
        REMEDY,
    )
}

/// Every tracked path, and every untracked one Git would add, that is
/// evidence. A repository Git cannot list is a finding rather than a pass:
/// a guard that goes quiet over files nobody listed reads exactly like clean.
pub fn check(root: &Path) -> Vec<Finding> {
    let listing = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    );
    let stdout = match listing {
        Ok((0, stdout)) => stdout,
        Ok((code, _)) => {
            return vec![Finding::new(
                format!("git ls-files exited {code}: the paths Git carries could not be listed"),
                REMEDY,
            )];
        }
        Err(reason) => return vec![Finding::new(reason, REMEDY)],
    };
    let mut paths: Vec<String> = stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .filter(|path| forbidden(path))
        .collect();
    paths.sort();
    paths.dedup();
    paths.iter().map(|path| finding(path)).collect()
}

/// The same check for one path. Git is asked only about a path already in
/// scope, so the hook stays in milliseconds for every other edit.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !forbidden(relative) {
        return Vec::new();
    }
    // `check-ignore` answers 0 for an ignored path and 1 for one Git would
    // carry; a tracked path is never reported as ignored.
    match git(root, &["check-ignore", "-q", "--", relative]) {
        Ok((0, _)) => Vec::new(),
        Ok((1, _)) => vec![finding(relative)],
        Ok((code, _)) => vec![Finding::new(
            format!("{relative}: git check-ignore exited {code}, so the path could not be judged"),
            REMEDY,
        )],
        Err(reason) => vec![Finding::new(format!("{relative}: {reason}"), REMEDY)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn mission_files_and_evidence_folders_are_in_scope() {
        for path in [
            ".claude/GOAL.md",
            ".claude/GOAL-archive-mission-tiers.md",
            ".claude/GOAL-source-campaign-recovery.md",
            ".claude/evidence/run/capture.png",
            "docs/evidence/right-drag/four-checks.txt",
            "docs/workflow/evidence/exercises-a.md",
            "docs/quality/incremental-lane-evidence.md",
        ] {
            assert!(forbidden(path), "{path} should be refused");
        }
    }

    #[test]
    fn maintained_documents_and_product_code_are_not() {
        for path in [
            ".claude/skills/mission/SKILL.md",
            ".claude/skills/GOAL-notes/SKILL.md",
            "docs/control-plane/screenshot-evidence-proof.md",
            "docs/control-plane/history/pr5c-evidence.md",
            "docs/quality/quantick-score-rubric.md",
            "crates/control/src/evidence.rs",
            "tools/evidence/capture_screenshot_evidence.py",
        ] {
            assert!(!forbidden(path), "{path} should pass");
        }
    }

    /// The whole rule on a real repository: a tracked archive and an
    /// untracked dossier Git would add are findings, the ignored live goal is
    /// not, and the hook answers the same as the scan for each of them.
    #[test]
    fn only_what_git_would_carry_is_refused_and_the_hook_agrees() {
        let root = crate::scratch_dir::ScratchDir::new("evidence-guard");
        let (code, _) = git(root.path(), &["init", "-q"]).expect("git runs");
        assert_eq!(code, 0, "git init succeeds");
        fs::create_dir_all(root.join(".claude")).expect("dirs are creatable");
        fs::create_dir_all(root.join("docs/quality")).expect("dirs are creatable");
        fs::write(root.join(".gitignore"), ".claude/GOAL.md\n").expect("writable");
        fs::write(root.join(".claude/GOAL.md"), "live goal\n").expect("writable");
        fs::write(root.join(".claude/GOAL-archive-old.md"), "archive\n").expect("writable");
        fs::write(root.join("docs/quality/run-evidence.md"), "dossier\n").expect("writable");
        fs::write(root.join("docs/quality/rubric.md"), "kept\n").expect("writable");
        let (code, _) =
            git(root.path(), &["add", ".claude/GOAL-archive-old.md"]).expect("git runs");
        assert_eq!(code, 0, "git add succeeds");

        let lines: Vec<String> = check(root.path()).into_iter().map(|f| f.line).collect();
        assert_eq!(
            lines,
            [
                ".claude/GOAL-archive-old.md: execution evidence or a mission file Git would carry",
                "docs/quality/run-evidence.md: execution evidence or a mission file Git would carry",
            ],
        );
        for relative in [
            ".claude/GOAL.md",
            ".claude/GOAL-archive-old.md",
            "docs/quality/run-evidence.md",
            "docs/quality/rubric.md",
        ] {
            let from_scan: Vec<_> = check(root.path())
                .into_iter()
                .filter(|f| f.line.starts_with(&format!("{relative}:")))
                .collect();
            assert_eq!(
                check_file(root.path(), relative),
                from_scan,
                "the hook and the scan disagree about {relative}"
            );
        }
    }

    /// A tree Git cannot list is a finding, never a silent pass.
    #[test]
    fn a_tree_git_cannot_list_is_reported() {
        // The scratch directory lives under the system temporary directory,
        // which is not inside any repository.
        let root = crate::scratch_dir::ScratchDir::new("evidence-guard-no-repo");
        let lines: Vec<String> = check(root.path()).into_iter().map(|f| f.line).collect();
        assert_eq!(lines.len(), 1, "{lines:#?}");
        assert!(lines[0].contains("could not be listed"), "{}", lines[0]);
    }

    #[test]
    fn the_workspace_itself_is_clean() {
        let findings = check(&crate::workspace_root());
        assert!(findings.is_empty(), "{findings:#?}");
    }
}
