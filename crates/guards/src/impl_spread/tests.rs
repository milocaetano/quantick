use super::*;
use crate::scratch_dir::ScratchDir;
use std::fs;
use std::path::PathBuf;

// --- The port of `measure.py`, against the shared corpus ---------------------
//
// `tools/outside_score/fixtures/expected.tsv` is read here and by
// `tools/outside_score/test_measure.py`, so the two definitions are pinned to
// one table rather than each to a hand-copied number.

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/outside_score/fixtures")
}

fn impls_of(source: &str) -> Vec<String> {
    lex::inherent_impls(&lex::production(source))
}

#[test]
fn the_shared_corpus_measures_as_the_script_counts_it() {
    let dir = corpus();
    let table = fs::read_to_string(dir.join("expected.tsv")).expect("the corpus table is readable");
    let rows: Vec<&str> = table
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(!rows.is_empty(), "the corpus table holds no row");
    for row in rows {
        let columns: Vec<&str> = row.split('\t').collect();
        let [name, lines, impls] = columns[..] else {
            panic!("`{row}` is not `file<TAB>lines<TAB>impls`");
        };
        let source = fs::read_to_string(dir.join(name)).expect("a corpus file is readable");
        let production = lex::production(&source);
        let lines: usize = lines.parse().expect("the line column is a count");
        assert_eq!(
            lex::code_lines(&production),
            lines,
            "{name}: production lines"
        );
        let expected: Vec<String> = match impls {
            "-" => Vec::new(),
            names => names.split(',').map(str::to_owned).collect(),
        };
        assert_eq!(lex::inherent_impls(&production), expected, "{name}: impls");
    }
}

#[test]
fn an_arrow_in_a_bound_does_not_close_the_generics() {
    // The `>` of `->` once closed `<F: Fn() -> bool>` early and named the
    // type `bool`.
    assert_eq!(impls_of("impl<F: Fn() -> bool> Pane<F> {\n}\n"), ["Pane"]);
    assert_eq!(
        impls_of("impl<F: Fn(u8) -> Option<u8>, G> Gate<F, G> {\n}\n"),
        ["Gate"]
    );
}

#[test]
fn test_files_are_the_scripts_test_files() {
    assert!(lex::is_test_file("crates/app/tests/flow.rs"));
    assert!(lex::is_test_file("crates/app/src/pane/tests.rs"));
    assert!(lex::is_test_file("crates/app/src/view_tests.rs"));
    assert!(lex::is_test_file("crates/app/src/pane_tests/draw.rs"));
    assert!(!lex::is_test_file("crates/app/src/testsuite.rs"));
}

#[test]
fn a_nested_crate_is_keyed_by_its_own_directory() {
    let root = ScratchDir::new("impl-spread-crate-of");
    fs::create_dir_all(root.join("crates/viewer/re_view/src")).expect("creatable");
    fs::write(root.join("crates/viewer/re_view/Cargo.toml"), "[package]\n").expect("writable");
    assert_eq!(
        crate_of(&root, "crates/viewer/re_view/src/lib.rs").as_deref(),
        Some("crates/viewer/re_view")
    );
    assert_eq!(
        crate_of(&root, "crates/viewer/src/lib.rs").as_deref(),
        Some("crates/viewer")
    );
    assert_eq!(crate_of(&root, "crates/stray.rs"), None);
}

// --- The ratchet, over scratch workspaces -----------------------------------

/// One `impl Hub` block, three code lines.
fn hub_impl(index: usize) -> String {
    format!("impl Hub {{\n    fn part_{index}(&self) {{}}\n}}\n")
}

/// Crate `name` holding `Hub` with its inherent impls in `files` files, at
/// exactly `lines` production lines in all.
fn add_crate(root: &Path, name: &str, files: usize, lines: usize) {
    let src = root.join(format!("crates/{name}/src"));
    fs::create_dir_all(&src).expect("source is creatable");
    let filler = lines - 1 - 3 * files;
    let mut lib = String::from("pub struct Hub;\n");
    for index in 0..filler {
        lib.push_str(&format!("pub const C{index}: u8 = 0;\n"));
    }
    fs::write(src.join("lib.rs"), lib).expect("source is writable");
    for index in 0..files {
        fs::write(src.join(format!("part_{index}.rs")), hub_impl(index))
            .expect("source is writable");
    }
}

/// A workspace with crate `big` over the line, `Hub` spread over `files`
/// files, and a baseline recording `ceiling`.
fn tree(files: usize, ceiling: usize) -> ScratchDir {
    let root = ScratchDir::new("impl-spread");
    fs::create_dir_all(root.join("crates/guards")).expect("guards dir is creatable");
    add_crate(&root, "big", files, QUALIFYING_LINES + 1);
    fs::write(
        root.join(BASELINE_FILE),
        format!("!budget {ceiling}\n{LABEL} {ceiling}\n"),
    )
    .expect("baseline is writable");
    root
}

fn raise(root: &Path, text: &str) {
    let dir = root.join(POLICY.raises_dir());
    fs::create_dir_all(&dir).expect("raises dir is creatable");
    fs::write(dir.join("feat-spread.txt"), text).expect("raise is writable");
}

fn lines(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| f.line.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_spread_at_the_ceiling_is_clean() {
    let root = tree(3, 3);
    assert!(check(&root).is_empty(), "{:?}", check(&root));
    assert_eq!(measured(&root), Ok(3));
}

#[test]
fn one_file_past_the_ceiling_fails_under_the_baseline_key_and_names_the_type() {
    let root = tree(4, 3);
    let found = lines(&check(&root));
    // The path slot is the key a raise names, so `impl-spread +1` is the
    // raise a copy of the finding leads to; the type follows.
    assert!(
        found.starts_with(
            "  impl-spread: 4 files holding one type's inherent impls, ceiling 3 (+1) — \
             big::Hub spans crates/big/src/part_0.rs, "
        ),
        "{found}"
    );
    raise(&root, "impl-spread +1\n!budget +1\n");
    assert!(check(&root).is_empty(), "{:?}", check(&root));
}

#[test]
fn a_signed_raise_with_its_budget_is_accepted() {
    let root = tree(4, 3);
    raise(&root, "# Why the spread grew.\nimpl-spread +1\n");
    let found = lines(&check(&root));
    assert!(found.contains("over the !budget of 3"), "{found}");
    raise(
        &root,
        "# Why the spread grew.\nimpl-spread +1\n!budget +1\n",
    );
    assert!(check(&root).is_empty(), "{:?}", check(&root));
}

#[test]
fn a_spread_that_fell_must_be_tightened_and_tighten_lowers_it() {
    let root = tree(2, 3);
    let found = lines(&check(&root));
    assert!(found.contains("impl-spread: down to 2 from 3"), "{found}");
    tighten(&root).expect("tighten runs");
    let recorded = POLICY.baseline(&root).expect("parses");
    assert_eq!(recorded.entry(LABEL).map(|entry| entry.ceiling), Some(2));
    assert_eq!(recorded.budget.map(|budget| budget.allowed), Some(2));
    assert!(check(&root).is_empty(), "{:?}", check(&root));
    // Once tightened, the file cannot quietly come back.
    fs::write(root.join("crates/big/src/part_2.rs"), hub_impl(2)).expect("writable");
    let found = lines(&check(&root));
    assert!(found.contains("(+1) — big::Hub spans"), "{found}");
}

#[test]
fn crates_at_or_under_the_line_are_ignored() {
    let root = tree(3, 3);
    add_crate(&root, "small", 9, QUALIFYING_LINES);
    assert!(check(&root).is_empty(), "{:?}", check(&root));
    // One line more and the same spread counts.
    let lib = root.join("crates/small/src/lib.rs");
    let mut source = fs::read_to_string(&lib).expect("readable");
    source.push_str("pub const ONE_MORE: u8 = 0;\n");
    fs::write(&lib, source).expect("writable");
    let found = lines(&check(&root));
    assert!(
        found.contains("impl-spread: 9 files") && found.contains("small::Hub spans"),
        "{found}"
    );
}

#[test]
fn trait_impls_impl_trait_and_test_code_do_not_widen_a_spread() {
    let root = tree(3, 3);
    let src = root.join("crates/big/src");
    for (name, source) in [
        ("display.rs", "impl std::fmt::Display for Hub {\n}\n"),
        ("from.rs", "impl<T> From<T> for Hub {\n}\n"),
        ("take.rs", "pub fn take(hub: impl Into<Hub>) {}\n"),
        (
            "fixture.rs",
            "#[cfg(test)]\nimpl Hub {\n    fn t(&self) {}\n}\n",
        ),
        ("hub_tests.rs", "impl Hub {\n}\n"),
        ("note.rs", "// impl Hub {\n"),
    ] {
        fs::write(src.join(name), source).expect("writable");
    }
    assert!(check(&root).is_empty(), "{:?}", check(&root));
    assert_eq!(measured(&root), Ok(3));
}

#[test]
fn a_qualifying_spread_with_no_entry_is_unsigned() {
    let root = tree(3, 3);
    fs::write(root.join(BASELINE_FILE), "!budget 0\n").expect("writable");
    let found = lines(&check(&root));
    assert!(found.contains("absent from the baseline"), "{found}");
}

#[test]
fn any_entry_but_the_ceiling_is_stale() {
    let root = tree(3, 3);
    fs::write(
        root.join(BASELINE_FILE),
        format!("!budget 4\n{LABEL} 3\napp::Hub 1\n"),
    )
    .expect("writable");
    let found = lines(&check(&root));
    assert!(
        found.contains("app::Hub: in the baseline but no longer"),
        "{found}"
    );
}

#[test]
fn nested_crates_sharing_a_leaf_name_are_two_crates() {
    let root = tree(3, 3);
    for parent in ["left", "right"] {
        add_crate(&root, &format!("{parent}/view"), 2, QUALIFYING_LINES + 1);
        fs::write(
            root.join(format!("crates/{parent}/view/Cargo.toml")),
            "[package]\n",
        )
        .expect("writable");
    }
    // Merged by leaf name, `view::Hub` would span four files.
    assert_eq!(measured(&root), Ok(3));
    assert!(check(&root).is_empty(), "{:?}", check(&root));
}

// --- The edit-time path agrees with the whole tree ---------------------------

/// `check`'s lines that an edit inside crate `leaf` can change: its own
/// types over the ceiling and every tree-wide line, not another crate's type.
fn restricted(root: &Path, leaf: &str) -> Vec<String> {
    check(root)
        .into_iter()
        .map(|finding| finding.line)
        .filter(|line| !line.contains(" spans ") || line.contains(&format!(" {leaf}::")))
        .collect()
}

/// Every source file of crate `leaf`, the ones holding no impl included,
/// answers at edit time exactly what the whole tree says about that crate.
fn assert_agree(root: &Path, leaf: &str) {
    let expected = restricted(root, leaf);
    let src = root.join(format!("crates/{leaf}/src"));
    let mut names: Vec<String> = fs::read_dir(&src)
        .expect("listable")
        .map(|entry| {
            entry
                .expect("readable")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    for name in names {
        let relative = format!("crates/{leaf}/src/{name}");
        let at_edit: Vec<String> = check_file(root, &relative)
            .into_iter()
            .map(|finding| finding.line)
            .collect();
        assert_eq!(at_edit, expected, "{relative}");
    }
}

#[test]
fn check_file_agrees_with_check_at_over_and_under_the_ceiling() {
    for (files, ceiling) in [(3, 3), (4, 3), (2, 3)] {
        let root = tree(files, ceiling);
        assert!(!check(&root).is_empty() || files == ceiling);
        assert_agree(&root, "big");
    }
}

#[test]
fn check_file_reports_a_fall_from_a_file_holding_no_impl() {
    // `big` drops to the line: nothing qualifies, the ceiling fell to zero,
    // and the file that did it holds no impl.
    let root = tree(3, 3);
    let lib = root.join("crates/big/src/lib.rs");
    let source = fs::read_to_string(&lib).expect("readable");
    let shorter = source.replacen("pub const C0: u8 = 0;\n", "", 1);
    fs::write(&lib, shorter).expect("writable");
    let found = lines(&check(&root));
    assert!(found.contains("impl-spread: down to 0 from 3"), "{found}");
    assert_agree(&root, "big");
}

#[test]
fn check_file_reports_a_crate_crossing_the_line_from_a_file_holding_no_impl() {
    let root = tree(3, 3);
    add_crate(&root, "small", 9, QUALIFYING_LINES);
    assert_agree(&root, "small");
    let lib = root.join("crates/small/src/lib.rs");
    let mut source = fs::read_to_string(&lib).expect("readable");
    source.push_str("pub const ONE_MORE: u8 = 0;\n");
    fs::write(&lib, source).expect("writable");
    assert!(lines(&check(&root)).contains("small::Hub spans"));
    assert_agree(&root, "small");
    // The other crate's file is not answerable for `small::Hub`.
    assert_agree(&root, "big");
}

#[test]
fn check_file_sees_a_fall_held_by_another_crate() {
    // `small` holds the widest spread; once it narrows, an edit anywhere sees
    // the ceiling fall, and `big` alone below the ceiling settles nothing.
    let root = tree(2, 3);
    add_crate(&root, "small", 3, QUALIFYING_LINES + 1);
    assert!(check(&root).is_empty(), "{:?}", check(&root));
    assert_agree(&root, "big");
    // Same three lines, no impl: `small` still qualifies.
    fs::write(
        root.join("crates/small/src/part_2.rs"),
        "pub const A: u8 = 0;\npub const B: u8 = 0;\npub const C: u8 = 0;\n",
    )
    .expect("writable");
    assert!(lines(&check(&root)).contains("down to 2 from 3"));
    assert_agree(&root, "big");
    assert_agree(&root, "small");
}

#[test]
fn check_file_on_a_crate_deleted_whole_answers_as_the_tree_does() {
    let root = tree(3, 3);
    let expected = lines(&check(&root));
    let at_edit = lines(&check_file(&root, "crates/gone/src/lib.rs"));
    assert_eq!(at_edit, expected);
}

#[test]
fn a_vanished_entry_is_skipped_and_other_errors_stay_errors() {
    use std::io::{Error, ErrorKind};
    assert_eq!(skip_vanished(Ok(7)).unwrap(), Some(7));
    let gone = skip_vanished::<u8>(Err(Error::from(ErrorKind::NotFound)));
    assert_eq!(gone.unwrap(), None);
    let denied = skip_vanished::<u8>(Err(Error::from(ErrorKind::PermissionDenied)));
    assert_eq!(denied.unwrap_err().kind(), ErrorKind::PermissionDenied);
}
