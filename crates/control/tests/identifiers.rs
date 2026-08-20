use std::{fmt::Display, str::FromStr};

use quantick_control::id::{
    CapabilityId, EffectId, EventKind, IdempotencyKey, ModuleId, PermissionId, ProfileId,
    SnapshotScopeId,
};
use quantick_control::{
    error::codes,
    schema::{generated_schema, validate_instance},
    wire::RequestEnvelope,
};
use serde_json::json;

fn assert_extensible_string_newtype<T>(valid: &str, invalid: &str)
where
    T: FromStr + Display,
    T::Err: std::fmt::Debug,
{
    let value = valid.parse::<T>().unwrap();
    assert_eq!(value.to_string(), valid);
    assert!(invalid.parse::<T>().is_err());
}

#[test]
fn stable_cross_module_error_codes_are_unique_valid_namespaced_ids() {
    let codes = codes::ALL
        .iter()
        .map(|code| quantick_control::id::ErrorCode::new(*code).unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(codes.len(), quantick_control::error::codes::ALL.len());
}

#[test]
fn retry_keys_are_redacted_from_debug_output() {
    let key = IdempotencyKey::new("do-not-log-this").unwrap();
    assert_eq!(format!("{key:?}"), "IdempotencyKey(<redacted>)");
}

#[test]
fn public_schema_enforces_the_same_id_boundaries_as_deserialization() {
    let schema = generated_schema::<RequestEnvelope>();
    let valid_instance = quantick_control::id::InstanceId::from_bytes([1; 16]);
    let base = json!({
        "protocol_version": 1,
        "request_id": "request-1",
        "instance_id": valid_instance,
        "capability_id": "plugin.operation",
        "capability_version": 1,
        "payload": {}
    });
    validate_instance(&schema, &base).unwrap();

    let mut unnamespaced = base.clone();
    unnamespaced["capability_id"] = json!("operation");
    assert!(validate_instance(&schema, &unnamespaced).is_err());

    let mut noncanonical_runtime_id = base.clone();
    noncanonical_runtime_id["instance_id"] = json!("AAAAAAAAAAAAAAAAAAAAAB");
    assert!(validate_instance(&schema, &noncanonical_runtime_id).is_err());

    let mut control_character = base;
    control_character["request_id"] = json!("line\nbreak");
    assert!(validate_instance(&schema, &control_character).is_err());

    let mut too_many_segments = control_character;
    too_many_segments["request_id"] = json!("request-1");
    too_many_segments["capability_id"] = json!("a.b.c.d.e.f.g.h.i");
    assert!(validate_instance(&schema, &too_many_segments).is_err());
}

#[test]
fn extensible_ids_are_validated_string_newtypes_not_closed_enums() {
    assert_extensible_string_newtype::<CapabilityId>("plugin.operation", "plugin-operation");
    assert_extensible_string_newtype::<SnapshotScopeId>("plugin.snapshot", "snapshot");
    assert_extensible_string_newtype::<EventKind>("plugin.changed", "Changed");
    assert_extensible_string_newtype::<ModuleId>("future_plugin", "future plugin");
    assert_extensible_string_newtype::<EffectId>("future.effect", "future/effect");
    assert_extensible_string_newtype::<PermissionId>("future.scope", "future:scope");
    assert_extensible_string_newtype::<ProfileId>("future_profile", "FutureProfile");
}

#[test]
fn ids_enforce_byte_and_segment_bounds_before_lookup() {
    assert!(ModuleId::new("a.b.c.d.e.f.g.h").is_ok());
    assert!(ModuleId::new("a.b.c.d.e.f.g.h.i").is_err());
    assert!(ModuleId::new(format!("a{}", "x".repeat(128))).is_err());
}
