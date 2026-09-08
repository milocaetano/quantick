use quantick_control::{
    schema::{Compatibility, SchemaError, require_compatible_version},
    schema_catalog::public_schema_documents,
};
use serde_json::{Value, json};

#[path = "support/published_schema.rs"]
mod published_schema;
use published_schema::{ReleasedSchemas, assert_each_removal_fails};

fn generated() -> Vec<(String, Value)> {
    public_schema_documents()
        .into_iter()
        .map(|document| (document.file_name.to_owned(), document.schema))
        .collect()
}

#[test]
fn published_core_schemas_remain_compatible() {
    ReleasedSchemas::load()
        .check_generated("core", generated())
        .unwrap();
}

#[test]
fn every_published_core_schema_is_required_in_generated_coverage() {
    assert_each_removal_fails(&ReleasedSchemas::load(), "core", &generated());
}

#[test]
fn published_core_required_field_narrowing_fails_even_after_snapshot_regeneration() {
    let released = ReleasedSchemas::load();
    let name = "request-envelope-v1.schema.json";
    let mut generated = generated();
    let (_, next) = generated.iter_mut().find(|(file, _)| file == name).unwrap();
    assert!(
        !next["required"]
            .as_array()
            .unwrap()
            .contains(&json!("reason"))
    );
    next["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("reason"));
    // A regenerated top-level snapshot could now equal `next`. The separate
    // released gate still compares it to the actual published request schema.
    let error = released.check_generated("core", generated).unwrap_err();
    assert!(
        error.contains(name) && error.contains("`reason` became required"),
        "{error}"
    );
}

#[test]
fn published_core_variants_follow_existing_additive_and_version_policy() {
    let released = ReleasedSchemas::load();
    let base = released.schema("request-envelope-v1.schema.json");
    let mut additive = base.clone();
    additive["properties"]["client_note"] = json!({"type": "string"});
    assert_eq!(
        require_compatible_version(1, base, 1, &additive).unwrap(),
        Compatibility::Additive
    );
    let mut breaking = base.clone();
    breaking["required"]
        .as_array_mut()
        .unwrap()
        .push(json!("reason"));
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

#[test]
fn published_inventory_rejects_duplicate_generation_and_unknown_owners() {
    let released = ReleasedSchemas::load();
    let mut duplicates = generated();
    duplicates.push(duplicates[0].clone());
    assert!(
        released
            .check_generated("core", duplicates)
            .unwrap_err()
            .starts_with("duplicate generated schema:")
    );
    assert_eq!(
        released.check_generated("other", []),
        Err("unknown schema owner: other".to_owned())
    );
}
