use quantick_control::schema::{
    Compatibility, compare_schemas, require_compatible_version, validate_schema,
};
use serde_json::Value;

fn fixture(name: &str) -> Value {
    let contents = match name {
        "base" => include_str!("fixtures/schema-v1.json"),
        "additive" => include_str!("fixtures/schema-v1-additive.json"),
        "breaking" => include_str!("fixtures/schema-v1-breaking.json"),
        _ => panic!("unknown fixture"),
    };
    serde_json::from_str(contents).unwrap()
}

#[test]
fn committed_schema_fixtures_are_valid_draft_2020_12() {
    for name in ["base", "additive", "breaking"] {
        validate_schema(&fixture(name)).unwrap();
    }
}

#[test]
fn breaking_fixture_requires_a_version_bump() {
    let base = fixture("base");
    let breaking = fixture("breaking");
    assert!(matches!(
        compare_schemas(&base, &breaking),
        Compatibility::Breaking(_)
    ));
    assert!(require_compatible_version(1, &base, 1, &breaking).is_err());
    assert!(require_compatible_version(1, &base, 2, &breaking).is_ok());
}

#[test]
fn optional_field_fixture_is_additive_without_a_bump() {
    let base = fixture("base");
    let additive = fixture("additive");
    assert_eq!(
        require_compatible_version(1, &base, 1, &additive).unwrap(),
        Compatibility::Additive
    );
}

#[test]
fn newly_added_constraints_are_detected_as_narrowing() {
    for next in [
        serde_json::json!({"type": "string"}),
        serde_json::json!({"enum": ["one", "two"]}),
        serde_json::json!({"type": "array", "items": {"type": "integer"}}),
        serde_json::json!({"type": "object", "additionalProperties": {"type": "string"}}),
    ] {
        assert!(
            compare_schemas(&serde_json::json!({}), &next).is_breaking(),
            "missed narrowing schema {next}"
        );
    }
    assert_eq!(
        compare_schemas(
            &serde_json::json!({"type": "string"}),
            &serde_json::json!({})
        ),
        Compatibility::Additive
    );
}

#[test]
fn changes_inside_referenced_definitions_are_compared() {
    let previous = serde_json::json!({
        "$defs": {"Name": {"type": "string", "maxLength": 64}},
        "$ref": "#/$defs/Name"
    });
    let narrowed = serde_json::json!({
        "$defs": {"Name": {"type": "string", "maxLength": 32}},
        "$ref": "#/$defs/Name"
    });
    assert!(compare_schemas(&previous, &narrowed).is_breaking());
}

#[test]
fn integer_bounds_are_compared_without_floating_point_rounding() {
    let previous = serde_json::json!({
        "type": "integer",
        "maximum": 9_007_199_254_740_993_u64
    });
    let narrowed = serde_json::json!({
        "type": "integer",
        "maximum": 9_007_199_254_740_992_u64
    });
    assert!(compare_schemas(&previous, &narrowed).is_breaking());
}

#[test]
fn enum_growth_is_conservatively_breaking_for_typed_readers() {
    let previous = serde_json::json!({"enum": ["ready"]});
    let expanded = serde_json::json!({"enum": ["ready", "paused"]});
    assert!(compare_schemas(&previous, &expanded).is_breaking());
}
