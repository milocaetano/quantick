use super::*;
use crate::scratch_dir::ScratchDir;
use std::fs;

// --- The port of `measure.py`, against the script's own fixtures ------------
//
// The expected numbers below are the ones `tools/outside_score/test_measure.py`
// asserts for the same text, so the two definitions are pinned to each other
// and not only each to itself.

/// `test_measure.py`'s `LIB`, verbatim.
const LIB: &str = r#####"//! A crate. The word fn in a comment is not a function. {
use std::fmt;

pub(crate) fn tricky<'a>(s: &'a str) -> usize {
    let raw = r#"fn fake() { "quoted" }"#; let c = '{'; let q = '\'';
    let b = b'}'; let text = "unbalanced { brace and fn inside";
    s.len() + raw.len() + text.len() + (c as usize) + (q as usize) + (b as usize)
}

/* block /* nested { */ comment } */
pub(super) fn short() -> u8 {
    let v: Option<u8> = None;
    v.unwrap()
}

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Square;

impl Square {
    pub fn side(&self) -> f64 { 1.0 }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "sq") }
}

pub fn evens() -> impl Iterator<Item = u8> {
    (0..4).filter(|n| n % 2 == 0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_test_is_not_production() {
        let _ = "QUANTICK_TEST_ONLY";
        panic!("never counted");
    }
}
"#####;

fn lines_of(source: &str) -> usize {
    lex::code_lines(&lex::production(source))
}

fn impls_of(source: &str) -> Vec<String> {
    lex::inherent_impls(&lex::production(source))
}

#[test]
fn the_scripts_fixture_measures_as_the_script_counts_it() {
    // "LIB: 23 lines of code outside comments and the cfg(test) module".
    assert_eq!(lines_of(LIB), 23);
    // `impl fmt::Display for Square` and `-> impl Iterator` are not counted.
    assert_eq!(impls_of(LIB), vec!["Square".to_owned()]);
}

#[test]
fn inner_any_and_all_cfg_test_are_test_code_and_not_test_is_not() {
    assert_eq!(
        lines_of("#![cfg(test)]\npub fn helper() {\n    let _ = 1;\n}\n"),
        0
    );
    assert_eq!(
        lines_of(
            "#[cfg(any(test, feature = \"fake\"))]\npub fn fake() -> u8 {\n    \
             Some(1).unwrap()\n}\n"
        ),
        0
    );
    assert_eq!(
        lines_of("#[cfg(all(unix, test))]\npub fn late() -> u8 {\n    Some(2).unwrap()\n}\n"),
        0
    );
    assert_eq!(lines_of("#[cfg(not(test))]\npub fn live() {\n}\n"), 3);
}

// The two below were run through `measure.py`'s `inherent_impls` and
// `code_lines`; the expected values are what the script returned.

#[test]
fn an_impl_header_is_named_by_its_last_segment_without_generics_or_dyn() {
    let source = "\
impl dyn Shape {\n}\n\
impl<T: Clone> Holder<T> where T: Send {\n}\n\
impl crate::view::Pane {\n}\n\
impl<'a> &'a mut Cursor {\n}\n\
impl &mut Slot {\n}\n\
unsafe impl Buffer {\n}\n";
    // A header that still opens with a lifetime names nothing, as in the
    // script.
    assert_eq!(
        impls_of(source),
        ["Shape", "Holder", "Pane", "Slot", "Buffer"].map(str::to_owned)
    );
    assert_eq!(lines_of(source), 12);
}

#[test]
fn trait_impls_impl_trait_comments_and_literals_name_no_type() {
    let source = "\
impl std::fmt::Display for Hub {\n}\n\
impl<T> From<T> for Hub {\n}\n\
unsafe impl Send for Hub {}\n\
pub fn take(hub: impl Into<Hub>) {}\n\
pub fn make() -> impl Fn() -> u8 {\n    || 1\n}\n\
// impl Hub {\n\
/* impl Hub { */\n\
const TEXT: &str = \"\nimpl Hub {\n\";\n";
    assert!(impls_of(source).is_empty(), "{:?}", impls_of(source));
    assert_eq!(lines_of(source), 11);
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
fn a_nested_crate_is_named_by_its_own_manifest() {
    let root = ScratchDir::new("impl-spread-crate-of");
    fs::create_dir_all(root.join("crates/viewer/re_view/src")).expect("creatable");
    fs::write(root.join("crates/viewer/re_view/Cargo.toml"), "[package]\n").expect("writable");
    assert_eq!(
        crate_of(&root, "crates/viewer/re_view/src/lib.rs").as_deref(),
        Some("re_view")
    );
    assert_eq!(
        crate_of(&root, "crates/viewer/src/lib.rs").as_deref(),
        Some("viewer")
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
fn one_file_past_the_ceiling_fails_and_names_the_type() {
    let root = tree(4, 3);
    let found = lines(&check(&root));
    assert!(
        found.contains("big::Hub: 4 files holding its inherent impls, ceiling 3 (+1)"),
        "{found}"
    );
    assert!(
        !check_file(&root, "crates/big/src/part_0.rs").is_empty(),
        "the edit-time check sees the spread from a file holding one of its impls"
    );
    assert!(
        check_file(&root, "crates/big/src/lib.rs").is_empty(),
        "a file holding none of the type's impls is not the one to blame"
    );
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
    assert!(found.contains("big::Hub: 3 files"), "{found}");
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
    assert!(found.contains("small::Hub: 9 files"), "{found}");
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
