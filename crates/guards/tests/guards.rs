//! The seven repository guards, as the tests CI runs.
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
const TESTED: [&str; 7] = [
    "size",
    "language",
    "encoding",
    "context",
    "cycle",
    "generated",
    "scratch",
];

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
         by a #[test] above, so the next guard needs the next test."
    );
}

#[test]
fn no_context_file_grows_past_its_recorded_ceiling() {
    assert_clean(TESTED[3]);
}

/// The one this repository learned the hard way twice: a refactor that
/// welds two modules into a cycle leaves fmt, clippy, the build and the
/// whole suite green. This is the test that stops being green.
#[test]
fn no_crate_grows_a_module_cycle_past_its_recorded_ceiling() {
    assert_clean(TESTED[4]);
}

/// The generated indexes — the capability inventory and the hook registry —
/// must still say what the code says. A hand edit to either, a capability
/// registered without regenerating, or a hook read without being declared,
/// fails here.
#[test]
fn the_generated_indexes_match_the_code_they_describe() {
    assert_clean(TESTED[5]);
}

/// The one that keeps a red honest: a test naming its own temporary folder
/// after a process id inherits a reused id's leftovers and fails on the
/// previous run's files, while leaving the folder behind for ever. Neither
/// half is visible to fmt, clippy, the build or the suite.
#[test]
fn no_test_mints_a_temporary_path_outside_its_scratch_module() {
    assert_clean(TESTED[6]);
}

// --- `--report`, the mode that measures rather than judges -------------------
//
// The guard tests above ask whether the repository is within its ceilings.
// These ask whether the report describing it can be trusted to say the same
// thing twice, and whether its two hand-written counting rules — a struct's
// fields, and a site that survives into production source — count what the
// mission said they should.

use std::process::Command;

use quantick_guards::{report, size};

/// One `pub struct` whose fields sit at one indent, wrapped in the things
/// that have to be ignored around them: a doc comment, an attribute, a
/// generic parameter on the header, a nested type whose own fields sit
/// deeper, and a trailing item after the closing brace.
fn fixture_struct(fields: usize) -> String {
    let mut source = String::from(
        "//! A module.\n\
         \n\
         /// A type.\n\
         #[derive(Debug)]\n\
         pub struct Wide<T> {\n\
         \x20   /// The first one, documented.\n\
         \x20   pub kind: T,\n\
         \x20   pub(crate) shared: usize,\n",
    );
    for index in 0..fields {
        source.push_str(&format!("    field_{index}: usize,\n"));
    }
    source.push_str(
        "    nested: Inner,\n\
         }\n\
         \n\
         struct Inner {\n\
         \x20   deep: usize,\n\
         }\n",
    );
    source
}

/// The rule the mission stated: a field is a `name:` line at one indent
/// inside a `pub struct` body. Everything at another indent, and everything
/// without a colon, is not a field.
#[test]
fn a_struct_is_as_wide_as_its_fields_at_one_indent() {
    let source = fixture_struct(report::WIDE_STRUCT_FIELDS);
    let production = size::production_source(&source);
    // Three hand-written fields — `kind`, `shared` and `nested` — plus the
    // generated ones. The doc comment, the attribute, the generic header, the
    // closing braces and the private `Inner` all count for nothing.
    assert_eq!(
        report::wide_structs(&production),
        vec![("Wide".to_owned(), report::WIDE_STRUCT_FIELDS + 3)]
    );
}

/// The threshold is a threshold. A struct one field short of it is ordinary,
/// and an ordinary struct on this list would bury the four types the sprint
/// is actually arguing about.
#[test]
fn a_struct_below_the_field_threshold_is_not_wide() {
    let source = fixture_struct(report::WIDE_STRUCT_FIELDS - 4);
    let production = size::production_source(&source);
    assert!(report::wide_structs(&size::production_source(&source)).is_empty());
    // …and the counting itself still ran, rather than the fixture being
    // malformed in a way that would pass this test for the wrong reason.
    assert!(production.iter().any(|line| line.contains("field_0")));
}

/// A struct that is not `pub`, and a struct with no brace-delimited body, are
/// both outside the rule — the first because a private type is not a surface,
/// the second because a tuple or unit struct has no fields of this shape.
#[test]
fn only_a_public_brace_bodied_struct_declares_fields() {
    let mut source = String::from("struct Private {\n");
    for index in 0..report::WIDE_STRUCT_FIELDS {
        source.push_str(&format!("    field_{index}: usize,\n"));
    }
    source.push_str("}\n\npub struct Tuple(usize, usize);\npub struct Unit;\n");
    let production = size::production_source(&source);
    assert!(report::wide_structs(&production).is_empty());
}

/// The property the whole report rests on: a site inside a test module is not
/// a site the repository still carries. Counting it would score the branch
/// that moved tests out of a production file as no improvement at all.
#[test]
fn a_site_inside_a_test_module_is_not_counted() {
    let allow_index = report::SITES
        .iter()
        .position(|(label, _)| *label == "site.allow")
        .expect("the allow site is registered");
    let process_index = report::SITES
        .iter()
        .position(|(label, _)| *label == "site.process_id")
        .expect("the process-id site is registered");

    let source = "\
#[allow(dead_code)]
pub fn live() -> u32 {
    std::process::id()
}

#[cfg(test)]
mod tests {
    #[allow(unused)]
    fn scratch() -> String {
        format!(\"{}\", std::process::id())
    }
}
";
    let production = size::production_source(source);
    let counts = report::site_counts(&production);
    assert_eq!(counts[allow_index], 1, "only the production `allow` counts");
    assert_eq!(
        counts[process_index], 1,
        "only the production `process::id()` counts"
    );

    // The same file with its test module deleted must count identically —
    // which is what makes the number a measure of production code rather than
    // of how a file happens to be laid out.
    let without_tests: String = source
        .lines()
        .take_while(|line| !line.starts_with("#[cfg(test)]"))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(
        report::site_counts(&size::production_source(&without_tests)),
        counts
    );
}

/// The mode's whole point. Two runs over the same tree must produce the same
/// bytes, or a diff between a report taken before a merge and one taken after
/// describes the reporter rather than the merge.
///
/// Run as the command rather than as [`report::render`], because the wiring
/// between the two is part of what has to hold: a mode that printed a summary
/// line or a path from the build machine would pass a library-level check and
/// fail the one use the report has.
#[test]
fn the_report_is_byte_identical_across_runs() {
    let first = run_report();
    let second = run_report();
    assert_eq!(
        first, second,
        "two --report runs over the same tree disagreed"
    );
    // A report of nothing is byte-identical too, and would pass the check
    // above while saying nothing at all.
    assert!(
        first.lines().count() > 20,
        "the report printed {} line(s); it should carry one per number",
        first.lines().count()
    );
    assert!(
        first.lines().all(|line| line.matches('\t').count() == 1),
        "every row is one label, one tab, one value"
    );
}

/// The modes are alternatives. `--report` with anything beside it is refused
/// rather than ignored, because a mistyped invocation that exits 0 having
/// done half of what was asked is the failure the usage string was rewritten
/// to prevent.
#[test]
fn the_report_mode_refuses_extra_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_quantick-guards"))
        .args(["--report", "--tighten"])
        .output()
        .expect("the guards binary runs");
    assert!(
        !output.status.success(),
        "an extra argument must not exit 0"
    );
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("--tighten"),
        "the refusal names the unconsumed argument: {complaint}"
    );
    assert!(
        complaint.contains("--report"),
        "the usage string offers the mode that was asked for: {complaint}"
    );
}

/// Run the command and hand back its stdout, failing loudly on a non-zero
/// exit — `--report` measures the tree and never judges it, so the only way
/// it can fail is by not working.
fn run_report() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_quantick-guards"))
        .arg("--report")
        .output()
        .expect("the guards binary runs");
    assert!(
        output.status.success(),
        "--report exited {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("the report is UTF-8")
}
