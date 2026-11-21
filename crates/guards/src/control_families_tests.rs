use super::*;
use crate::scratch_dir::ScratchDir;

fn fixture(rows: &str) -> String {
    format!(
        "macro_rules! control_families {{ ($consumer:ident) => {{ $consumer! {{ {rows} }} }}; }}"
    )
}

fn row(id: &str, binding: &str) -> String {
    format!("\"{id}\" {{ readbacks: [{binding}], }}")
}

#[test]
fn manifest_rejects_duplicate_and_unsorted_rows_and_bindings() {
    for rows in [
        row("z", "z") + &row("a", "a"),
        row("a", "a") + &row("a", "b"),
        row("a", "same") + &row("b", "same"),
    ] {
        assert!(manifest(&fixture(&rows)).is_err());
    }
    assert!(manifest(&fixture("")).is_err());
    assert_eq!(manifest(&fixture(&row("a", "a"))).unwrap().len(), 1);
}

#[test]
fn readback_scan_distinguishes_declarations_from_comments_and_literals() {
    assert!(
        readback_declarations("// const READBACKS: &[T] = &[];\n")
            .unwrap()
            .is_empty()
    );
    assert!(
        readback_declarations("const TEXT: &str = \"const READBACKS: &[T]\";")
            .unwrap()
            .is_empty()
    );
    assert!(
        readback_declarations("pub use other::READBACKS;")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        readback_declarations("mod nested { pub static READBACKS: &[T] = &[]; }").unwrap(),
        [("::nested".into(), false)]
    );
    assert_eq!(
        readback_declarations("pub const READBACKS: &[T] = &[];").unwrap(),
        [(String::new(), false)]
    );
}

#[test]
fn control_families_cover_readbacks() {
    let scratch = ScratchDir::new("control-families");
    let root = scratch.path();
    fs::create_dir_all(root.join("crates/control/src")).unwrap();
    fs::create_dir_all(root.join("crates/control-schema/src")).unwrap();
    fs::write(root.join(MANIFEST), fixture(&row("a", "a"))).unwrap();
    fs::write(
        root.join("crates/control-schema/src/a.rs"),
        "pub const READBACKS: &[T] = &[];",
    )
    .unwrap();
    assert!(check(root).is_empty());
    fs::create_dir_all(root.join("crates/other/src")).unwrap();
    fs::write(
        root.join("crates/other/src/new.rs"),
        "pub const READBACKS: &[T] = &[];",
    )
    .unwrap();
    let findings = check(root);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].line.contains("crates/other/src/new.rs"));
    fs::remove_file(root.join("crates/control-schema/src/a.rs")).unwrap();
    assert!(
        check(root)
            .iter()
            .any(|finding| finding.line.contains("has no READBACKS"))
    );
}

#[test]
fn a_fixture_without_the_control_crate_is_outside_this_guard() {
    let scratch = ScratchDir::new("control-families-absent");
    fs::create_dir_all(scratch.path().join("crates/other/src")).unwrap();
    assert!(check(scratch.path()).is_empty());
}

#[test]
fn nested_readbacks_cannot_hide_behind_a_docked_file() {
    let scratch = ScratchDir::new("control-nested-readbacks");
    let root = scratch.path();
    fs::create_dir_all(root.join("crates/control/src")).unwrap();
    fs::create_dir_all(root.join("crates/control-schema/src")).unwrap();
    fs::write(root.join(MANIFEST), fixture(&row("a", "a"))).unwrap();
    fs::write(
        root.join("crates/control-schema/src/a.rs"),
        "const READBACKS: &[T] = &[]; mod hidden { const READBACKS: &[T] = &[]; }",
    )
    .unwrap();
    let findings = check(root);
    assert_eq!(findings.len(), 1);
    assert!(findings[0].line.contains("a.rs::hidden"));
}

#[test]
fn aggregate_requires_exact_live_declaration() {
    let scratch = ScratchDir::new("control-readback-aggregate");
    let root = scratch.path();
    fs::create_dir_all(root.join("crates/control/src")).unwrap();
    fs::create_dir_all(root.join("crates/control-schema/src/retry_matrix")).unwrap();
    fs::write(root.join(MANIFEST), fixture(&row("a", "a"))).unwrap();
    fs::write(
        root.join("crates/control-schema/src/a.rs"),
        "const READBACKS: &[T] = &[];",
    )
    .unwrap();
    for declaration in [
        "pub const READBACKS: &[Readback] = &OTHER; // pub const READBACKS: &[Readback] = &JOINED;",
        "pub const READBACKS: &[Readback] = &JOINED; mod hidden { const READBACKS: &[T] = &[]; }",
    ] {
        fs::write(root.join(AGGREGATE), declaration).unwrap();
        assert!(
            check(root)
                .iter()
                .any(|finding| finding.line.contains("aggregate must export"))
        );
    }
    fs::write(
        root.join(AGGREGATE),
        "pub const READBACKS: &[Readback] = &JOINED;",
    )
    .unwrap();
    assert!(check(root).is_empty());
}
