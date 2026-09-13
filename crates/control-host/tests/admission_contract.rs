//! The admission order, proved from outside the crate over the reference
//! registry `quantick_control::fake` publishes — so a change to the checks is
//! tested here without building the desktop app.

use std::collections::BTreeSet;

use quantick_control::{
    error::codes,
    fake::{COUNTER_READ, reference_registry},
    handshake::CURRENT_PROTOCOL_VERSION,
    id::{CapabilityId, IdempotencyKey, InstanceId, PermissionId, RequestId},
    registry::ControlRegistry,
    schema::CompiledSchema,
    wire::RequestEnvelope,
};
use quantick_control_host::admission::{
    CompiledCapabilitySchemas, admit_capability, admit_payload, admit_scopes,
};
use serde_json::{Value, json};

fn request(capability: &str, payload: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("admission-test").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: CapabilityId::new(capability).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload,
    }
}

fn observe() -> BTreeSet<PermissionId> {
    BTreeSet::from([PermissionId::new("observe").unwrap()])
}

/// The compiled input schema of every capability in `registry`, as
/// `register_capability` would have built it.
fn input_validators(registry: &ControlRegistry) -> CompiledCapabilitySchemas {
    let mut validators = CompiledCapabilitySchemas::new();
    for descriptor in registry.capabilities() {
        validators.entry(descriptor.id.clone()).or_default().insert(
            descriptor.version,
            CompiledSchema::new(&descriptor.input_schema).unwrap(),
        );
    }
    validators
}

/// Admit `envelope` through both halves, the way a host's `prepare` does.
fn admit(envelope: &RequestEnvelope, scopes: &BTreeSet<PermissionId>) -> Result<(), String> {
    let registry = reference_registry().unwrap();
    let validators = input_validators(&registry);
    let descriptor =
        admit_capability(&registry, envelope, scopes).map_err(|error| error.code.to_string())?;
    admit_payload(descriptor, envelope, None, &validators).map_err(|error| error.code.to_string())
}

#[test]
fn a_well_formed_granted_request_is_admitted() {
    assert_eq!(admit(&request(COUNTER_READ, json!({})), &observe()), Ok(()));
}

#[test]
fn an_unregistered_capability_is_unknown() {
    assert_eq!(
        admit(&request("fake.nothing", json!({})), &observe()),
        Err(codes::CAPABILITY_UNKNOWN.to_owned())
    );
}

#[test]
fn a_missing_permission_is_denied_before_the_payload_is_read() {
    // The payload is also invalid: permission is checked first.
    assert_eq!(
        admit(&request(COUNTER_READ, json!([])), &BTreeSet::new()),
        Err(codes::PERMISSION_DENIED.to_owned())
    );
}

#[test]
fn a_payload_outside_the_schema_is_an_invalid_request() {
    assert_eq!(
        admit(&request(COUNTER_READ, json!([])), &observe()),
        Err(codes::INVALID_REQUEST.to_owned())
    );
}

#[test]
fn a_key_the_descriptor_forbids_is_refused() {
    let mut envelope = request(COUNTER_READ, json!({}));
    envelope.idempotency_key = Some(IdempotencyKey::new("retry-1").unwrap());
    assert_eq!(
        admit(&envelope, &observe()),
        Err(codes::INVALID_REQUEST.to_owned())
    );
}

#[test]
fn a_dry_run_is_refused_by_this_tier() {
    let mut envelope = request(COUNTER_READ, json!({}));
    envelope.dry_run = true;
    assert_eq!(
        admit(&envelope, &observe()),
        Err(codes::INVALID_REQUEST.to_owned())
    );
}

#[test]
fn a_registered_capability_without_a_validator_is_unavailable() {
    let registry = reference_registry().unwrap();
    let envelope = request(COUNTER_READ, json!({}));
    let descriptor = admit_capability(&registry, &envelope, &observe()).unwrap();
    let error = admit_payload(
        descriptor,
        &envelope,
        None,
        &CompiledCapabilitySchemas::new(),
    )
    .unwrap_err();
    assert_eq!(error.code.to_string(), codes::CAPABILITY_UNAVAILABLE);
}

#[test]
fn the_hosts_own_validator_replaces_the_compiled_one() {
    let registry = reference_registry().unwrap();
    let envelope = request(COUNTER_READ, json!({}));
    let descriptor = admit_capability(&registry, &envelope, &observe()).unwrap();
    let refuses_objects = CompiledSchema::new(&json!({ "type": "array" })).unwrap();
    let error = admit_payload(
        descriptor,
        &envelope,
        Some(&refuses_objects),
        &input_validators(&registry),
    )
    .unwrap_err();
    assert_eq!(error.code.to_string(), codes::INVALID_REQUEST);
}

#[test]
fn a_scope_outside_the_grant_names_the_missing_permission() {
    let wanted = BTreeSet::from([
        PermissionId::new("observe").unwrap(),
        PermissionId::new("observe.paper").unwrap(),
    ]);
    assert!(admit_scopes(&observe(), &observe()).is_ok());
    let error = admit_scopes(&wanted, &observe()).unwrap_err();
    assert_eq!(error.code.to_string(), codes::SCOPE_DENIED);
    assert_eq!(
        error.context.details,
        Some(json!({ "missing_permissions": ["observe.paper"] }))
    );
}
