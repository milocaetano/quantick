use super::*;
use crate::scratch_dir::ScratchDir;
use std::fs;

/// A file that names the UI library in code, and so costs nothing here.
const UI_FILE: &str =
    "use eframe::egui;\n\npub fn draw(ui: &mut egui::Ui) {\n    ui.label(\"x\");\n}\n";

/// `count` production lines that never name the UI library.
fn ui_free(count: usize) -> String {
    (0..count)
        .map(|index| format!("fn f{index}() {{}}\n"))
        .collect()
}

/// A scratch workspace holding `crates/app/src` with `files`, the baseline
/// recording `ceiling` for both the entry and the budget, and `exemptions`.
fn tree(files: &[(&str, String)], ceiling: usize, exemptions: &str) -> ScratchDir {
    let root = ScratchDir::new("ui-free");
    fs::create_dir_all(root.join("crates/app/src")).expect("app source is creatable");
    fs::create_dir_all(root.join("crates/guards")).expect("guards dir is creatable");
    for (path, source) in files {
        write(&root, path, source);
    }
    write_baseline(&root, ceiling, ceiling);
    fs::write(root.join(EXEMPTIONS_FILE), exemptions).expect("exemptions are writable");
    root
}

fn write(root: &ScratchDir, path: &str, source: &str) {
    let path = root.join("crates/app/src").join(path);
    fs::create_dir_all(path.parent().expect("a source has a parent")).expect("dir is creatable");
    fs::write(path, source).expect("source is writable");
}

fn write_baseline(root: &ScratchDir, entry: usize, budget: usize) {
    fs::write(
        root.join(BASELINE_FILE),
        format!("# fixture\n!budget {budget}\n{ENTRY} {entry}\n"),
    )
    .expect("baseline is writable");
}

fn lines(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|finding| finding.line.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn code_naming_either_ui_identifier_makes_a_file_ui_code() {
    assert!(names_ui(&["use eframe::egui::Ui;"]));
    assert!(names_ui(&["    let red = egui::Color32::RED;"]));
    assert!(names_ui(&["fn run() -> eframe::Result<()> {"]));
    assert!(!names_ui(&["fn ship() {}", "struct Plain;"]));
}

/// A name that merely contains one of the identifiers is not a reference:
/// `timeframe` ends in `eframe`, and a substring test charged every file
/// that names a timeframe as UI code.
#[test]
fn an_identifier_inside_a_longer_word_is_not_the_ui_library() {
    assert!(!names_ui(&["let timeframe = Timeframe::M1;"]));
    assert!(!names_ui(&["fn set_regui() {}"]));
    assert!(names_ui(&["use egui_extras::TableBuilder;"]));
    assert!(names_ui(&["let x = (egui::Pos2::ZERO, 1);"]));
}

/// A comment is not a reference to the library. Were it one, `// egui` at
/// the top of a file would take every line in it off the books.
#[test]
fn a_comment_naming_the_ui_library_does_not_make_a_file_ui_code() {
    assert!(!names_ui(&[
        "//! Painted with egui elsewhere.",
        "fn ship() {}"
    ]));
    assert!(!names_ui(&["    // egui would draw this", "fn ship() {}"]));
    assert!(!names_ui(&["/// See eframe.", "fn ship() {}"]));
    assert!(!names_ui(&["fn ship() {} // egui", "struct Plain;"]));
}

#[test]
fn a_tree_at_its_ceiling_is_clean_and_ui_files_cost_nothing() {
    let root = tree(
        &[("chart.rs", UI_FILE.to_owned()), ("free.rs", ui_free(10))],
        10,
        "",
    );
    assert_eq!(check(&root), Vec::new());
    assert_eq!(measured(&root), Ok(10));
}

/// Issue #443's A1, as a fixture: a new UI-free production file fails the
/// ratchet, and the same tree passes again once as many UI-free lines leave
/// another file. The edit-time surface says what the whole-tree one says.
#[test]
fn a_new_ui_free_file_fails_until_as_many_lines_leave_another() {
    let root = tree(
        &[("chart.rs", UI_FILE.to_owned()), ("free.rs", ui_free(10))],
        10,
        "",
    );
    write(&root, "account/new.rs", &ui_free(4));

    let findings = check(&root);
    let found = lines(&findings);
    assert!(found.contains("crates/app: 14"), "{found}");
    assert!(found.contains("ceiling 10 (+4)"), "{found}");
    assert_eq!(findings[0].remedy, REMEDY);
    assert_eq!(check_file(&root, "crates/app/src/account/new.rs"), findings);

    write(&root, "free.rs", &ui_free(6));
    assert_eq!(check(&root), Vec::new());
}

/// The rule charges a file whole: the day its last UI line goes, every line
/// in it is portable, and the total says so.
#[test]
fn a_file_that_drops_its_last_ui_line_is_charged_whole() {
    let root = tree(&[("chart.rs", UI_FILE.to_owned())], 0, "");
    assert_eq!(check(&root), Vec::new());
    write(&root, "chart.rs", "pub fn draw() {}\npub fn label() {}\n");
    let found = lines(&check(&root));
    assert!(found.contains("crates/app: 2"), "{found}");
}

#[test]
fn test_items_are_not_production_lines() {
    let source = format!(
        "{}#[cfg(test)]\nmod tests {{\n    fn a() {{}}\n    fn b() {{}}\n}}\n",
        ui_free(3)
    );
    let root = tree(&[("free.rs", source)], 3, "");
    assert_eq!(measured(&root), Ok(3));
    assert_eq!(check(&root), Vec::new());
}

/// The UI test is over production source too: a UI name only a test module
/// carries does not make the file UI code.
#[test]
fn a_ui_name_inside_a_test_module_does_not_make_a_file_ui_code() {
    let source = format!(
        "{}#[cfg(test)]\nmod tests {{\n    use eframe::egui;\n}}\n",
        ui_free(3)
    );
    let root = tree(&[("free.rs", source)], 3, "");
    assert_eq!(measured(&root), Ok(3));
}

#[test]
fn a_file_outside_the_app_source_is_not_counted() {
    let root = tree(&[("free.rs", ui_free(2))], 2, "");
    fs::create_dir_all(root.join("crates/app/tests")).expect("tests dir");
    fs::write(root.join("crates/app/tests/it.rs"), ui_free(50)).expect("test file");
    fs::create_dir_all(root.join("crates/engine/src")).expect("engine dir");
    fs::write(root.join("crates/engine/src/lib.rs"), ui_free(50)).expect("engine file");
    assert_eq!(measured(&root), Ok(2));
    assert_eq!(check(&root), Vec::new());
}

#[test]
fn an_exempt_file_is_not_charged() {
    let root = tree(
        &[("scratch.rs", ui_free(30)), ("free.rs", ui_free(2))],
        2,
        "# comment\ncrates/app/src/scratch.rs test scratch directories, required by the scratch guard\n",
    );
    assert_eq!(measured(&root), Ok(2));
    assert_eq!(check(&root), Vec::new());
}

#[test]
fn an_exemption_without_a_reason_is_a_finding() {
    let root = tree(
        &[("scratch.rs", ui_free(3))],
        0,
        "crates/app/src/scratch.rs\n",
    );
    let findings = check(&root);
    let found = lines(&findings);
    assert!(found.contains("ui-free-exemptions.txt:1"), "{found}");
    assert!(found.contains("no reason"), "{found}");
    assert_eq!(findings[0].remedy, EXEMPTION_REMEDY);
    // A malformed line exempts nothing, so a typo cannot widen the list.
    assert!(found.contains("crates/app: 3"), "{found}");
}

#[test]
fn an_exemption_outside_the_app_source_is_a_finding() {
    let root = tree(
        &[],
        0,
        "crates/engine/src/lib.rs headless by construction\n",
    );
    let found = lines(&check(&root));
    assert!(found.contains("not under crates/app/src/"), "{found}");
}

#[test]
fn an_exemption_listed_twice_is_a_finding() {
    let root = tree(
        &[("scratch.rs", ui_free(3))],
        0,
        "crates/app/src/scratch.rs one\ncrates/app/src/scratch.rs two\n",
    );
    let found = lines(&check(&root));
    assert!(found.contains("already exempt on line 1"), "{found}");
}

/// An exemption nobody prunes is a permission granted to code that is gone,
/// which the next author reads as precedent.
#[test]
fn an_exemption_for_a_file_that_is_gone_is_stale() {
    let root = tree(&[], 0, "crates/app/src/gone.rs was wiring once\n");
    let found = lines(&check(&root));
    assert!(found.contains("crates/app/src/gone.rs"), "{found}");
    assert!(found.contains("delete the line"), "{found}");
}

#[test]
fn an_exemption_for_a_file_that_names_the_ui_library_is_stale() {
    let root = tree(
        &[("chart.rs", UI_FILE.to_owned())],
        0,
        "crates/app/src/chart.rs wiring\n",
    );
    let found = lines(&check(&root));
    assert!(found.contains("names the UI library"), "{found}");
}

#[test]
fn a_missing_exemption_file_is_a_finding_and_no_measurement() {
    let root = tree(&[("free.rs", ui_free(2))], 2, "");
    fs::remove_file(root.join(EXEMPTIONS_FILE)).expect("exemptions removable");
    assert!(!check(&root).is_empty());
    assert!(measured(&root).is_err());
}

#[test]
fn a_raise_nobody_paid_for_is_over_budget() {
    let root = tree(&[("free.rs", ui_free(14))], 10, "");
    write_baseline(&root, 14, 10);
    let found = lines(&check(&root));
    assert!(
        found.contains("total is 14, over the !budget of 10"),
        "{found}"
    );
}

#[test]
fn a_total_far_below_its_ceiling_asks_for_tighten_and_tighten_lowers_both() {
    let root = tree(&[("free.rs", ui_free(10))], 10 + SLACK + 1, "");
    let found = lines(&check(&root));
    assert!(found.contains("tighten the entry to 10"), "{found}");

    let applied = tighten(&root).expect("tighten runs");
    assert_eq!(applied.len(), 2, "{applied:?}");
    let text = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline readable");
    assert!(text.contains("crates/app 10"), "{text}");
    assert!(text.contains("!budget 10"), "{text}");
    assert!(text.contains("# fixture"), "comments survive: {text}");
    assert_eq!(check(&root), Vec::new());
}

/// An entry lowered by hand leaves the budget behind; the finding sends the
/// author to `--tighten`, not to the over-ceiling remedy.
#[test]
fn a_budget_left_far_above_the_entry_asks_to_follow_it_down() {
    let root = tree(&[("free.rs", ui_free(10))], 10, "");
    write_baseline(&root, 10, 10 + SLACK + 1);
    let findings = check(&root);
    assert_eq!(findings.len(), 1, "{}", lines(&findings));
    assert_eq!(findings[0].remedy, BUDGET_SLACK_REMEDY);
}

#[test]
fn a_baseline_that_does_not_parse_says_so_rather_than_passing() {
    let root = tree(&[("free.rs", ui_free(10))], 10, "");
    fs::write(root.join(BASELINE_FILE), "!budget 10\ncrates/app lots\n").expect("writable");
    let findings = check(&root);
    assert_eq!(findings.len(), 1, "{}", lines(&findings));
    assert_eq!(findings[0].remedy, BASELINE_REMEDY);
}

#[test]
fn a_total_within_the_slack_needs_no_tightening() {
    let root = tree(&[("free.rs", ui_free(10))], 10 + SLACK, "");
    assert_eq!(check(&root), Vec::new());
}

#[test]
fn tighten_never_raises_the_ceiling() {
    let root = tree(&[("free.rs", ui_free(10))], 5, "");
    assert_eq!(tighten(&root), Ok(Vec::new()));
    let text = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline readable");
    assert!(text.contains("crates/app 5"), "{text}");
}

/// A tree whose app source cannot be walked has no total, and a partial
/// walk is the flattering number: tightening on it would write a ceiling
/// the real tree is already over.
#[test]
fn a_tree_without_app_source_has_no_measurement_and_tightens_nothing() {
    let root = tree(&[], 7 + SLACK + 1, "");
    fs::remove_dir_all(root.join("crates/app")).expect("app removable");
    assert!(measured(&root).is_err());
    let findings = check(&root);
    assert_eq!(findings.len(), 1, "{}", lines(&findings));
    assert_eq!(findings[0].remedy, UNMEASURED_REMEDY);
    assert!(tighten(&root).is_err());
    let text = fs::read_to_string(root.join(BASELINE_FILE)).expect("baseline readable");
    assert!(
        text.contains(&format!("crates/app {}", 7 + SLACK + 1)),
        "{text}"
    );
}

#[test]
fn only_app_source_and_the_two_data_files_trigger_the_edit_time_check() {
    let root = tree(&[("free.rs", ui_free(4))], 2, "");
    assert!(!check_file(&root, "crates/app/src/free.rs").is_empty());
    assert!(!check_file(&root, BASELINE_FILE).is_empty());
    assert!(!check_file(&root, EXEMPTIONS_FILE).is_empty());
    assert!(check_file(&root, "crates/engine/src/lib.rs").is_empty());
    assert!(check_file(&root, "crates/app/tests/it.rs").is_empty());
    assert!(check_file(&root, "docs/notes.md").is_empty());
}
