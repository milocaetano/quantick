//! The four repository guards, as the tests CI runs.
//!
//! Each is a thin shell over the library: the logic, its rationale and its
//! unit tests live in the module the failure names, and this file exists so
//! `cargo test --workspace` fails on a violation exactly as it did when these
//! were three files under `crates/app/tests/`. Nothing about the enforcement
//! moved — only the four minutes of `quantick-app` that used to stand between
//! an author and the answer.

use quantick_guards::{GUARDS, remedies, workspace_root};

/// The guards this file tests, in the order the tests below declare them.
///
/// Indexed by the tests rather than written beside them, so the drift check
/// cannot be satisfied by appending a name. A contributor who adds a guard and
/// only edits this list gets a compile error at the index that has no test,
/// instead of a green suite over a guard CI never runs — which is the failure
/// the check exists to prevent, and which a hand-kept list of names invites by
/// making "add the string" the obvious fix.
const TESTED: [&str; 4] = ["size", "language", "encoding", "context"];

/// Run one named guard and fail with everything it found.
fn assert_clean(name: &str) {
    let guard = GUARDS
        .iter()
        .find(|guard| guard.name == name)
        .unwrap_or_else(|| panic!("`{name}` is a registered guard"));
    let violations = (guard.check)(&workspace_root());
    assert!(
        violations.is_empty(),
        "{name} guard: {} finding(s)\n{}\n\n{}",
        violations.len(),
        violations
            .iter()
            .map(|f| f.line.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        remedies(&violations).join("\n\n")
    );
}

#[test]
fn no_tracked_file_grows_past_its_recorded_ceiling() {
    assert_clean(TESTED[0]);
}

#[test]
fn tracked_files_are_written_in_english() {
    assert_clean(TESTED[1]);
}

#[test]
fn sources_are_utf8_without_a_bom_or_mojibake() {
    assert_clean(TESTED[2]);
}

/// The registry is what the binary and these tests share; a guard added to
/// one and forgotten in the other would run in the command and never in CI,
/// which is the failure mode that looks green.
#[test]
fn every_guard_in_the_registry_has_a_test_here() {
    let missing: Vec<&str> = GUARDS
        .iter()
        .map(|guard| guard.name)
        .filter(|name| !TESTED.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "guards with no test in this file: {missing:?} — each would run in the binary and never \
         in CI. Widening TESTED alone will not do: its length is fixed and every slot is indexed \
         by a #[test] above, so a fourth guard needs a fourth test."
    );
}

#[test]
fn no_context_file_grows_past_its_recorded_ceiling() {
    assert_clean(TESTED[3]);
}
