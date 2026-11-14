use super::*;

/// The authoritative check: the committed file is what this generator
/// emits today, byte for byte, against the real registry.
///
/// `crates/guards` cannot make this comparison — it has no dependencies
/// and therefore no registry — so it compares identifier sets textually
/// instead. That catches the common mistake in one second; this catches
/// every mistake, including a permission added to a capability that
/// already had a row, which moves no identifier and so is invisible to a
/// set comparison.
#[test]
fn the_committed_inventory_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .join("docs/control-plane/capability-inventory.md");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert_eq!(
        committed,
        capability_inventory_markdown().expect("the registry builds"),
        "the committed capability inventory is stale. Regenerate it:\n  \
             cargo run -p quantick-app -- --dump-capability-inventory > \
             docs/control-plane/capability-inventory.md"
    );
}

/// Every row carries the four facts a caller needs before it can invoke a
/// capability. A generator that silently dropped a column would still pass
/// the byte comparison above, because that test regenerates the file it is
/// comparing against.
#[test]
fn every_row_carries_identifier_version_module_and_permissions() {
    let markdown = capability_inventory_markdown().expect("the registry builds");
    let rows: Vec<&str> = markdown
        .lines()
        .filter(|line| line.starts_with("| `"))
        .collect();
    assert!(!rows.is_empty(), "the inventory rendered no capabilities");
    for row in rows {
        let columns: Vec<&str> = row.trim_matches('|').split('|').collect();
        assert_eq!(columns.len(), 5, "row has the wrong column count: {row}");
        assert!(columns[0].trim().starts_with('`'), "no identifier: {row}");
        assert!(
            columns[1].trim().parse::<u32>().is_ok(),
            "version is not a number: {row}"
        );
        assert!(columns[2].trim().starts_with('`'), "no module: {row}");
        assert!(
            matches!(columns[3].trim(), "yes" | "no"),
            "read-only is not a yes/no: {row}"
        );
    }
}
