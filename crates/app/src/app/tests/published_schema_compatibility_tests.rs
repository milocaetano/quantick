use quantick_control::schema::{Compatibility, SchemaError, require_compatible_version};
use serde_json::{Value, json};

// Shared test-only inventory checks; the comparator remains in quantick-control.
#[path = "../../../../control/tests/support/published_schema.rs"]
mod published_schema;
use published_schema::{ReleasedSchemas, assert_each_removal_fails};

fn generated() -> Vec<(String, Value)> {
    crate::control::schema_catalog::documents()
        .into_iter()
        .map(|document| (document.file_name.to_owned(), document.schema))
        .collect()
}

#[test]
fn published_observer_schemas_remain_compatible() {
    ReleasedSchemas::load()
        .check_generated("app", generated())
        .unwrap();
}

#[test]
fn every_published_observer_schema_is_required_in_generated_coverage() {
    assert_each_removal_fails(&ReleasedSchemas::load(), "app", &generated());
}

#[test]
fn published_observer_numeric_narrowing_fails_even_after_snapshot_regeneration() {
    let released = ReleasedSchemas::load();
    let name = "observer-events-read-input-v1.schema.json";
    let mut generated = generated();
    let (_, next) = generated.iter_mut().find(|(file, _)| file == name).unwrap();
    assert_eq!(next["properties"]["limit"]["maximum"], 256);
    next["properties"]["limit"]["maximum"] = json!(128);
    let error = released.check_generated("app", generated).unwrap_err();
    assert!(error.contains(name) && error.contains("maximum"), "{error}");
}

#[test]
fn published_observer_variants_follow_existing_additive_and_version_policy() {
    let released = ReleasedSchemas::load();
    let base = released.schema("observer-events-read-input-v1.schema.json");
    let mut additive = base.clone();
    additive["properties"]["client_note"] = json!({"type": "string"});
    assert_eq!(
        require_compatible_version(1, base, 1, &additive).unwrap(),
        Compatibility::Additive
    );
    let mut breaking = base.clone();
    breaking["properties"]["limit"]["maximum"] = json!(128);
    assert!(matches!(
        require_compatible_version(1, base, 1, &breaking),
        Err(SchemaError::BreakingChangeWithoutVersionBump(_))
    ));
    assert!(matches!(
        require_compatible_version(1, base, 2, &breaking).unwrap(),
        Compatibility::Breaking(_)
    ));
    for (previous, next) in [(0, 1), (1, 0), (2, 1)] {
        assert!(matches!(
            require_compatible_version(previous, base, next, &breaking),
            Err(SchemaError::InvalidVersion)
        ));
    }
}
