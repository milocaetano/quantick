//! The admission order, proved from outside the crate over the reference
//! registry `quantick_control::fake` publishes — so a change to the checks is
//! tested here without building the desktop app.
//!
//! The capability under test is docked through [`register_capability`], the
//! same call a host makes, so the fixture exercises the registration it relies
//! on instead of re-implementing it.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    error::{ControlError, codes},
    fake::{COUNTER_READ, reference_registry},
    handshake::CURRENT_PROTOCOL_VERSION,
    id::{CapabilityId, IdempotencyKey, InstanceId, ModuleId, PermissionId, RequestId},
    registry::{CapabilityDescriptor, ControlRegistry, RegistryError},
    schema::CompiledSchema,
    wire::{ModuleRevision, RequestEnvelope, WireU64},
};
use quantick_control_host::admission::{
    CompiledCapabilitySchemas, TierPolicy, admit_capability, register_capability,
};
use serde_json::{Value, json};

/// The capability the tests dock: the reference counter read, under a new ID.
const PEEK: &str = "fake.counter.peek";
/// What the host dispatches to. A tag is enough to prove which one it found.
const PEEK_HANDLER: &str = "peek-handler";

struct Host {
    registry: ControlRegistry,
    handlers: BTreeMap<(CapabilityId, u32), &'static str>,
    input_validators: CompiledCapabilitySchemas,
    output_validators: CompiledCapabilitySchemas,
}

fn peek_descriptor() -> CapabilityDescriptor {
    let registry = reference_registry().unwrap();
    let mut descriptor = registry
        .capability(&CapabilityId::new(COUNTER_READ).unwrap(), 1)
        .unwrap()
        .clone();
    descriptor.id = CapabilityId::new(PEEK).unwrap();
    descriptor
}

fn host() -> Host {
    let mut host = Host {
        registry: reference_registry().unwrap(),
        handlers: BTreeMap::new(),
        input_validators: CompiledCapabilitySchemas::new(),
        output_validators: CompiledCapabilitySchemas::new(),
    };
    register_capability(
        &mut host.registry,
        &mut host.handlers,
        &mut host.input_validators,
        &mut host.output_validators,
        peek_descriptor(),
        PEEK_HANDLER,
    )
    .unwrap();
    host
}

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

fn code(error: ControlError) -> String {
    error.code.to_string()
}

/// Admit `envelope` through both halves, the way a host's `prepare` does, and
/// return the handler the host would dispatch to.
fn admit(
    host: &Host,
    envelope: &RequestEnvelope,
    scopes: &BTreeSet<PermissionId>,
    policy: TierPolicy,
) -> Result<&'static str, String> {
    let admitted = admit_capability(&host.registry, envelope, scopes).map_err(code)?;
    let admitted = admitted
        .admit_payload(envelope, None, &host.input_validators, policy)
        .map_err(code)?;
    let descriptor = admitted.descriptor();
    Ok(host.handlers[&(descriptor.id.clone(), descriptor.version)])
}

#[test]
fn a_registered_capability_is_admitted_and_reaches_its_handler() {
    let envelope = request(PEEK, json!({}));
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Ok(PEEK_HANDLER)
    );
}

#[test]
fn registration_refuses_a_schema_that_does_not_compile() {
    let mut host = host();
    let mut descriptor = peek_descriptor();
    descriptor.id = CapabilityId::new("fake.counter.broken").unwrap();
    descriptor.input_schema = json!({ "type": 12 });
    let error = register_capability(
        &mut host.registry,
        &mut host.handlers,
        &mut host.input_validators,
        &mut host.output_validators,
        descriptor,
        "never",
    )
    .unwrap_err();
    assert!(matches!(error, RegistryError::InvalidDescriptor(_)));
    assert_eq!(host.handlers.len(), 1, "a refused capability docks nothing");
}

#[test]
fn registration_refuses_a_second_capability_with_the_same_id() {
    let mut host = host();
    let error = register_capability(
        &mut host.registry,
        &mut host.handlers,
        &mut host.input_validators,
        &mut host.output_validators,
        peek_descriptor(),
        "second",
    );
    assert!(error.is_err());
    assert_eq!(host.handlers.len(), 1);
}

#[test]
fn an_unregistered_capability_is_unknown() {
    let envelope = request("fake.nothing", json!({}));
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Err(codes::CAPABILITY_UNKNOWN.to_owned())
    );
}

#[test]
fn a_missing_permission_is_denied_before_the_payload_is_read() {
    // The payload is invalid too: the permission is checked first.
    let envelope = request(PEEK, json!([]));
    assert_eq!(
        admit(&host(), &envelope, &BTreeSet::new(), TierPolicy::STRICT),
        Err(codes::PERMISSION_DENIED.to_owned())
    );
}

#[test]
fn a_payload_outside_the_schema_is_an_invalid_request() {
    let envelope = request(PEEK, json!([]));
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Err(codes::INVALID_REQUEST.to_owned())
    );
}

#[test]
fn a_key_the_descriptor_forbids_is_refused() {
    let mut envelope = request(PEEK, json!({}));
    envelope.idempotency_key = Some(IdempotencyKey::new("retry-1").unwrap());
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Err(codes::INVALID_REQUEST.to_owned())
    );
}

#[test]
fn a_strict_tier_refuses_a_dry_run_and_a_tier_that_accepts_one_does_not() {
    let mut envelope = request(PEEK, json!({}));
    envelope.dry_run = true;
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Err(codes::INVALID_REQUEST.to_owned())
    );
    let previews = TierPolicy {
        accepts_dry_runs: true,
        ..TierPolicy::STRICT
    };
    assert_eq!(
        admit(&host(), &envelope, &observe(), previews),
        Ok(PEEK_HANDLER)
    );
}

#[test]
fn a_strict_tier_refuses_an_expected_revision_and_a_tier_that_accepts_one_does_not() {
    let mut envelope = request(PEEK, json!({}));
    envelope.expected_revisions = vec![ModuleRevision {
        module_id: ModuleId::new("fake").unwrap(),
        revision: WireU64::new(1),
    }];
    assert_eq!(
        admit(&host(), &envelope, &observe(), TierPolicy::STRICT),
        Err(codes::INVALID_REQUEST.to_owned())
    );
    let checks_revisions = TierPolicy {
        accepts_expected_revisions: true,
        ..TierPolicy::STRICT
    };
    assert_eq!(
        admit(&host(), &envelope, &observe(), checks_revisions),
        Ok(PEEK_HANDLER)
    );
}

#[test]
fn a_registered_capability_without_a_validator_is_unavailable() {
    // `COUNTER_READ` is in the registry but was never docked through
    // `register_capability`, so no validator was compiled for it.
    let host = host();
    let envelope = request(COUNTER_READ, json!({}));
    let error = admit_capability(&host.registry, &envelope, &observe())
        .unwrap()
        .admit_payload(&envelope, None, &host.input_validators, TierPolicy::STRICT)
        .err()
        .unwrap();
    assert_eq!(code(error), codes::CAPABILITY_UNAVAILABLE);
}

#[test]
fn the_hosts_own_validator_replaces_the_compiled_one() {
    let host = host();
    let envelope = request(PEEK, json!({}));
    let refuses_objects = CompiledSchema::new(&json!({ "type": "array" })).unwrap();
    let error = admit_capability(&host.registry, &envelope, &observe())
        .unwrap()
        .admit_payload(
            &envelope,
            Some(&refuses_objects),
            &host.input_validators,
            TierPolicy::STRICT,
        )
        .err()
        .unwrap();
    assert_eq!(code(error), codes::INVALID_REQUEST);
}

#[test]
fn a_scope_outside_the_grant_names_the_missing_permission() {
    let host = host();
    let envelope = request(PEEK, json!({}));
    let admitted = admit_capability(&host.registry, &envelope, &observe())
        .unwrap()
        .admit_payload(&envelope, None, &host.input_validators, TierPolicy::STRICT)
        .unwrap();
    assert!(admitted.admit_scopes(&observe(), &observe()).is_ok());

    let wanted = BTreeSet::from([
        PermissionId::new("observe").unwrap(),
        PermissionId::new("observe.paper").unwrap(),
    ]);
    let error = admitted.admit_scopes(&wanted, &observe()).unwrap_err();
    assert_eq!(error.code.to_string(), codes::SCOPE_DENIED);
    assert_eq!(
        error.context.details,
        Some(json!({ "missing_permissions": ["observe.paper"] }))
    );
}
