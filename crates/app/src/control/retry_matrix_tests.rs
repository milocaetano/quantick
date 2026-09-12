//! The retry matrix against the registry it describes.
//!
//! The first two tests are the guard: the committed document is what the
//! generator emits, and the rows agree with the registry. The rest are the
//! guard's own fixtures — each hands [`drift`] a table with one thing wrong
//! and checks the finding names it — because a drift check that has never
//! been seen to fail is a drift check nobody knows works.

use serde_json::json;

use super::*;

/// Where the transport tests that prove the rows live, relative to
/// `crates/app`. A row's proving test must be a function in one of these.
const PROOF_SOURCES: [&str; 1] = ["src/app/tests/retry_readback_tests.rs"];

fn contract() -> ObserverContract {
    standard_contract().expect("the registry builds")
}

/// `READBACKS` with `edit` applied to the row for `capability`.
fn with_row(capability: &str, edit: impl Fn(&mut Readback)) -> Vec<Readback> {
    let mut rows = READBACKS.to_vec();
    let row = rows
        .iter_mut()
        .find(|row| row.capability == capability)
        .expect("the row exists");
    edit(row);
    rows
}

fn assert_finds(rows: &[Readback], expected: &Drift) {
    let findings = drift(rows, &contract());
    assert!(
        findings.contains(expected),
        "expected {expected:?} among {findings:?}"
    );
}

/// The authoritative check, as the capability inventory has: the committed
/// file is what the generator emits today, byte for byte.
#[test]
fn the_committed_retry_matrix_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/app sits two levels below the workspace root")
        .join("docs/control-plane/retry-matrix.md");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    assert_eq!(
        committed,
        retry_matrix_markdown().expect("the rows agree with the registry"),
        "the committed retry matrix is stale. Regenerate it:\n  \
         cargo run -p quantick-app -- --dump-retry-matrix > docs/control-plane/retry-matrix.md"
    );
}

/// Every mutable capability has exactly one row, and no row disagrees with
/// the registry — the check the generator itself makes before it renders.
#[test]
fn every_mutable_capability_has_one_row_and_no_row_has_drifted() {
    let contract = contract();
    let findings = drift(READBACKS, &contract);
    assert!(
        findings.is_empty(),
        "the retry matrix disagrees with the registry:\n{}",
        findings
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    );
    let mutable = contract
        .registry()
        .capabilities()
        .filter(|descriptor| !descriptor.read_only)
        .count();
    assert_eq!(READBACKS.len(), mutable, "one row per mutable capability");
}

/// The case the guard exists for: a capability registered tomorrow with no
/// word on how a client reconciles it.
#[test]
fn a_mutable_capability_without_a_row_is_drift() {
    let rows: Vec<Readback> = READBACKS
        .iter()
        .copied()
        .filter(|row| row.capability != "notify.sound")
        .collect();
    assert_finds(
        &rows,
        &Drift::Unmapped {
            capability: "notify.sound".to_owned(),
        },
    );
}

/// A row for something that is gone, or for a read that needs no
/// reconciling, is a row describing a registry that does not exist.
#[test]
fn a_row_for_no_mutable_capability_is_drift() {
    let template = *readback("layout.tab.rename").expect("the row exists");
    let mut rows = READBACKS.to_vec();
    rows.push(Readback {
        capability: "layout.tab.delete",
        ..template
    });
    rows.push(Readback {
        capability: "scene.read",
        ..template
    });
    assert_finds(
        &rows,
        &Drift::Orphan {
            capability: "layout.tab.delete".to_owned(),
        },
    );
    assert_finds(
        &rows,
        &Drift::Orphan {
            capability: "scene.read".to_owned(),
        },
    );
}

#[test]
fn a_second_row_for_one_capability_is_drift() {
    let mut rows = READBACKS.to_vec();
    rows.push(*readback("notify.toast").expect("the row exists"));
    assert_finds(
        &rows,
        &Drift::Duplicate {
            capability: "notify.toast".to_owned(),
        },
    );
}

/// The drift fixture the assessor asked for: the policy each row expects is
/// the policy its descriptor publishes, and a move on either side fails.
#[test]
fn a_policy_the_descriptor_no_longer_publishes_is_drift() {
    let rows = with_row("layout.tab.create", |row| {
        row.policy = IdempotencyPolicy::Forbidden;
    });
    assert_finds(
        &rows,
        &Drift::PolicyMoved {
            capability: "layout.tab.create".to_owned(),
            expected: IdempotencyPolicy::Forbidden,
            published: IdempotencyPolicy::Optional,
        },
    );
}

/// A readback the inventory lacks is no readback, and a "readback" that can
/// itself act would change the state it claims to report.
#[test]
fn a_readback_the_registry_lacks_or_that_acts_is_drift() {
    let unknown = with_row("notify.popup", |row| row.read = "events.replay");
    assert_finds(
        &unknown,
        &Drift::UnknownRead {
            capability: "notify.popup".to_owned(),
            read: "events.replay".to_owned(),
        },
    );
    let acting = with_row("notify.popup", |row| row.read = "notify.toast");
    assert_finds(
        &acting,
        &Drift::ReadNotReadOnly {
            capability: "notify.popup".to_owned(),
            read: "notify.toast".to_owned(),
        },
    );
}

/// A read with the wrong kind of selector is a row no client can follow: a
/// journal read with no event kind, a snapshot read handed an event kind, and
/// a read that has no readback grammar at all.
#[test]
fn a_read_with_a_selector_it_does_not_take_is_drift() {
    let expected = |read: &str| Drift::ReadShape {
        capability: "notify.popup".to_owned(),
        read: read.to_owned(),
    };
    let no_kind = with_row("notify.popup", |row| row.event = None);
    assert_finds(&no_kind, &expected("events.read"));
    let snapshot_with_kind = with_row("notify.popup", |row| {
        row.read = SNAPSHOT_CAPABILITY_ID;
        row.scope = Some(workspace::SCOPE_ID);
    });
    assert_finds(&snapshot_with_kind, &expected(SNAPSHOT_CAPABILITY_ID));
    let other_read = with_row("notify.popup", |row| row.read = "scene.read");
    assert_finds(&other_read, &expected("scene.read"));
}

/// The named readback scope, the other half of the assessor's fixture.
#[test]
fn a_scope_the_registry_lacks_is_drift() {
    let rows = with_row("layout.tab.switch", |row| {
        row.scope = Some("workspace.layouts");
    });
    assert_finds(
        &rows,
        &Drift::UnknownScope {
            capability: "layout.tab.switch".to_owned(),
            scope: "workspace.layouts".to_owned(),
        },
    );
}

/// A field the scope's published schema does not carry is a field no client
/// can read — renamed, or never there — and the walk has to see through an
/// array without its `[]` as well as through a name that is simply wrong.
#[test]
fn a_field_the_scope_schema_lacks_is_drift() {
    for field in ["layouts[].title", "tabs.focused_pane"] {
        let rows = with_row("layout.tab.rename", |row| row.field = field);
        assert_finds(
            &rows,
            &Drift::FieldMissing {
                capability: "layout.tab.rename".to_owned(),
                scope: workspace::SCOPE_ID.to_owned(),
                field: field.to_owned(),
            },
        );
    }
}

/// A caller granted the call must be able to read it back without asking the
/// trader for more: `analysis.indicators` carries the trader's own input
/// values behind `observe.user_text`, which the script tier does not hold, so
/// a script row reading it back there reconciles nothing for its caller.
#[test]
fn a_readback_beyond_the_callers_grant_is_drift() {
    let rows = with_row("indicator.script.attach", |row| {
        row.read = SNAPSHOT_CAPABILITY_ID;
        row.scope = Some(super::super::analysis::INDICATORS_SCOPE_ID);
        row.event = None;
        row.field = "tabs[].panes[].indicators[].slot_id";
    });
    assert_finds(
        &rows,
        &Drift::ReadbackOutOfReach {
            capability: "indicator.script.attach".to_owned(),
            profile: "annotator".to_owned(),
        },
    );
}

/// Every row names the tests that prove it, and every name is a function in
/// the transport suite — a proof column that could name a deleted test would
/// be a column of claims.
#[test]
fn every_proving_test_is_a_function_in_the_transport_suite() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources: String = PROOF_SOURCES
        .iter()
        .map(|relative| {
            std::fs::read_to_string(manifest.join(relative))
                .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"))
        })
        .collect();
    for row in READBACKS {
        assert!(
            !row.proven_by.is_empty(),
            "{} names no proof",
            row.capability
        );
        for test in row.proven_by {
            assert!(
                sources.contains(&format!("fn {test}(")),
                "the row for {} names `{test}`, which is not a test in {PROOF_SOURCES:?}",
                row.capability
            );
        }
    }
}

/// The document carries one row per mutable capability the inventory lists,
/// each with all eight columns filled. The byte comparison above cannot see a
/// renderer that dropped a column, because it compares against itself.
#[test]
fn every_row_carries_policy_enforcement_reach_readback_and_proof() {
    let markdown = retry_matrix_markdown().expect("the rows agree with the registry");
    let rows: Vec<&str> = markdown
        .lines()
        .filter(|line| line.starts_with("| `"))
        .collect();
    let inventory =
        super::super::inventory::capability_inventory_markdown().expect("the registry builds");
    let mutable_in_inventory = inventory
        .lines()
        .filter(|line| line.starts_with("| `") && line.contains(" | no | "))
        .count();
    assert_eq!(rows.len(), mutable_in_inventory);
    for row in rows {
        let columns: Vec<&str> = row.trim_matches('|').split('|').collect();
        assert_eq!(columns.len(), 8, "row has the wrong column count: {row}");
        assert!(
            matches!(columns[1].trim(), "optional" | "forbidden" | "required"),
            "policy is not a policy: {row}"
        );
        for (index, column) in columns.iter().enumerate() {
            assert!(!column.trim().is_empty(), "column {index} is empty: {row}");
        }
        assert!(
            columns[4].trim().starts_with('`'),
            "no readback capability: {row}"
        );
    }
}

/// The schema walk, on a schema written here rather than generated: `$ref`
/// into `$defs`, an optional wrapped in `anyOf`, and arrays by `items`.
#[test]
fn the_schema_walk_follows_refs_optionals_and_arrays() {
    let schema = json!({
        "type": "object",
        "properties": {
            "tabs": { "type": "array", "items": { "$ref": "#/$defs/Tab" } },
            "note": { "anyOf": [{ "$ref": "#/$defs/Note" }, { "type": "null" }] }
        },
        "$defs": {
            "Tab": {
                "type": "object",
                "properties": { "label": { "type": "string" } }
            },
            "Note": {
                "type": "object",
                "properties": { "text": { "type": "string" } }
            }
        }
    });
    assert!(schema_has_path(&schema, "tabs[].label"));
    assert!(schema_has_path(&schema, "note.text"));
    assert!(
        !schema_has_path(&schema, "tabs.label"),
        "an array is not an object"
    );
    assert!(!schema_has_path(&schema, "tabs[].name"));
    assert!(!schema_has_path(&schema, "missing"));
}
