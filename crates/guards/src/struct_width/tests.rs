use super::*;
use crate::scratch_dir::ScratchDir;
use std::fs;

/// A struct named `name` at `visibility` with `fields` fields at one indent,
/// plus the things that must not count: a doc comment, an attribute, a
/// blank line and a nested-indent line.
fn declare(visibility: &str, name: &str, fields: usize) -> String {
    let mut source = format!("/// A type.\n{visibility}struct {name}<T> {{\n");
    for index in 0..fields {
        source.push_str(&format!(
            "    /// Field {index}.\n    #[serde(default)]\n    pub f{index}: T,\n\n"
        ));
    }
    source.push_str("        not_a_field: u8,\n}\n");
    source
}

/// A scratch workspace with `crates/app/src/tab.rs` declaring `Tab` at
/// `fields`, and a baseline recording `ceiling` for it.
fn tree(fields: usize, ceiling: usize) -> ScratchDir {
    let root = ScratchDir::new("struct-width");
    fs::create_dir_all(root.join("crates/app/src")).expect("app source is creatable");
    fs::create_dir_all(root.join("crates/guards")).expect("guards dir is creatable");
    fs::write(
        root.join("crates/app/src/tab.rs"),
        declare("pub ", "Tab", fields),
    )
    .expect("source is writable");
    fs::write(
        root.join(BASELINE_FILE),
        format!("!budget {ceiling}\napp::Tab {ceiling}\n"),
    )
    .expect("baseline is writable");
    root
}

fn lines(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| f.line.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_struct_is_as_wide_as_its_fields_at_one_indent() {
    let source = declare("pub ", "Wide", 31);
    let production: Vec<&str> = source.lines().collect();
    assert_eq!(widths(&production), vec![("Wide".to_owned(), 31)]);
}

/// Deleting `pub` is not an architectural improvement, so every visibility
/// is counted.
#[test]
fn every_visibility_is_counted() {
    for visibility in [
        "",
        "pub ",
        "pub(crate) ",
        "pub(super) ",
        "pub(in crate::a) ",
    ] {
        let source = declare(visibility, "Wide", 30);
        let production: Vec<&str> = source.lines().collect();
        assert_eq!(
            wide_structs(&production),
            vec![("Wide".to_owned(), 30)],
            "`{visibility}struct`"
        );
    }
}

#[test]
fn tuple_and_unit_structs_have_no_fields_of_this_shape() {
    let production = ["pub struct Tuple(usize, usize);", "struct Unit;"];
    assert!(widths(&production).is_empty());
}

#[test]
fn a_field_called_public_keeps_its_name() {
    assert!(is_field("    public: u8,"));
    assert!(is_field("    pub(crate) inner: u8,"));
    assert!(!is_field("    #[serde(default)]"));
}

#[test]
fn a_struct_at_its_recorded_width_is_clean() {
    let root = tree(67, 67);
    assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
    assert_eq!(measured(root.path()), Ok(67));
}

#[test]
fn one_field_past_the_ceiling_fails() {
    let root = tree(68, 67);
    let found = lines(&check(root.path()));
    assert!(
        found.contains("app::Tab: 68 fields, ceiling 67 (+1)"),
        "{found}"
    );
    assert!(
        !check_file(root.path(), "crates/app/src/tab.rs").is_empty(),
        "the edit-time check sees the widened struct in its own file"
    );
}

#[test]
fn one_field_fewer_must_be_tightened_and_tighten_writes_it() {
    let root = tree(66, 67);
    let found = lines(&check(root.path()));
    assert!(found.contains("down to 66 from 67"), "{found}");
    tighten(root.path()).expect("tighten runs");
    let text = fs::read_to_string(root.join(BASELINE_FILE)).expect("readable");
    assert!(text.contains("app::Tab 66"), "{text}");
    assert!(text.contains("!budget 66"), "{text}");
    assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
}

/// Once tightened, putting the field back is growth like any other.
#[test]
fn a_field_taken_out_cannot_quietly_come_back() {
    let root = tree(66, 67);
    tighten(root.path()).expect("tighten runs");
    fs::write(
        root.join("crates/app/src/tab.rs"),
        declare("pub ", "Tab", 67),
    )
    .expect("source is writable");
    let found = lines(&check(root.path()));
    assert!(found.contains("67 fields, ceiling 66"), "{found}");
}

/// A struct that shrinks below the width keeps its entry, so it is still
/// seen and still has to be tightened rather than reported stale.
#[test]
fn a_struct_that_narrows_below_the_width_is_tightened_not_stale() {
    let root = tree(12, 67);
    let found = lines(&check(root.path()));
    assert!(found.contains("down to 12 from 67"), "{found}");
    assert!(!found.contains("stale"), "{found}");
}

#[test]
fn a_newly_wide_struct_needs_an_entry() {
    let root = tree(67, 67);
    fs::write(
        root.join("crates/app/src/wide.rs"),
        declare("", "Other", 30),
    )
    .expect("source is writable");
    let found = lines(&check(root.path()));
    assert!(found.contains("app::Other: 30 fields"), "{found}");
    assert!(found.contains("absent from the baseline"), "{found}");
}

#[test]
fn a_struct_that_is_gone_leaves_a_stale_entry() {
    let root = tree(67, 67);
    fs::write(root.join("crates/app/src/tab.rs"), "pub struct Unit;\n")
        .expect("source is writable");
    let found = lines(&check(root.path()));
    assert!(
        found.contains("app::Tab: in the baseline but no longer"),
        "{found}"
    );
}

/// A test fixture's struct is not the trunk.
#[test]
fn a_struct_in_a_test_module_is_not_counted() {
    let root = tree(67, 67);
    fs::write(
        root.join("crates/app/src/fixture.rs"),
        format!(
            "#[cfg(test)]\nmod tests {{\n{}}}\n",
            declare("", "Fixture", 40)
        ),
    )
    .expect("source is writable");
    assert!(check(root.path()).is_empty(), "{:?}", check(root.path()));
}

#[test]
fn two_structs_sharing_a_label_are_held_to_the_wider() {
    let root = tree(67, 67);
    fs::write(root.join("crates/app/src/other.rs"), declare("", "Tab", 40))
        .expect("source is writable");
    let counts = counts(root.path()).expect("tree measures");
    assert_eq!(
        counts
            .iter()
            .filter(|(label, _)| label == "app::Tab")
            .count(),
        1
    );
    assert!(counts.contains(&("app::Tab".to_owned(), 67)));
}
