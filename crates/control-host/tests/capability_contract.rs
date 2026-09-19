//! A plain second host using the public owned contract, with no desktop types.
/// The three envelope fields this tier used to refuse together, now that
/// only two of them are still refused together.
///
/// `layout.*`, `feed.*` and the `trade.*` shaping family publish
/// `IdempotencyPolicy::Optional`; a read publishes `Forbidden`. Before
/// this trio the gateway refused every key regardless, so the descriptors
/// and the door disagreed. These pin both halves: the refusal a
/// descriptor asks for still happens, and the two refusals that were
/// never in dispute are untouched.
#[test]
fn a_read_still_refuses_the_idempotency_key_its_descriptor_forbids() {
    let contract = contract();
    let grant = contract.default_grant();
    let mut envelope = request(SNAPSHOT_CAPABILITY_ID, json!({ "scopes": ["system.info"] }));
    envelope.idempotency_key = Some(
        quantick_control::id::IdempotencyKey::new("key-1".to_owned())
            .expect("a printable ASCII key is valid"),
    );
    let error = contract.prepare(envelope, &grant).unwrap_err();
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert!(
        error.message.contains("forbids idempotency keys"),
        "the refusal names the policy that caused it: {}",
        error.message
    );
}

#[test]
fn a_dry_run_is_still_refused_by_this_tier() {
    let contract = contract();
    let grant = contract.default_grant();
    let mut envelope = request(SNAPSHOT_CAPABILITY_ID, json!({ "scopes": ["system.info"] }));
    envelope.dry_run = true;
    let error = contract.prepare(envelope, &grant).unwrap_err();
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert!(error.message.contains("dry runs"), "{}", error.message);
}

#[test]
fn an_expected_revision_is_still_refused_by_this_tier() {
    let contract = contract();
    let grant = contract.default_grant();
    let mut envelope = request(SNAPSHOT_CAPABILITY_ID, json!({ "scopes": ["system.info"] }));
    envelope.expected_revisions = vec![quantick_control::wire::ModuleRevision {
        module_id: ModuleId::new("chart").expect("static module ID is valid"),
        revision: quantick_control::wire::WireU64::new(1),
    }];
    let error = contract.prepare(envelope, &grant).unwrap_err();
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert!(
        error.message.contains("expected revisions"),
        "{}",
        error.message
    );
}
use quantick_control::{
    error::{ControlError, codes},
    fake::{COUNTER_READ, reference_registry},
    handshake::CURRENT_PROTOCOL_VERSION,
    id::{
        CapabilityId, ConfirmationClassId, EffectId, InstanceId, ModuleId, PermissionId, ProfileId,
        RequestId, SnapshotScopeId,
    },
    registry::{
        CapabilityDescriptor, DefaultGrant, EffectConstraints, EffectPolicy, McpHintFloor,
        ModuleDescriptor, PermissionDescriptor, ProfileDescriptor, RegistryError, RevisionPolicy,
    },
    schema::CompiledSchema,
    wire::RequestEnvelope,
};
use quantick_control_host::{
    admission::TierPolicy,
    clock::HostClock,
    contract::{
        AdmittedRequest, AdmittedRoute, CapabilityContract, ContractBuilder, ExternalSchemas,
        ReadPreparation,
    },
    projection::ProjectionRegistry,
};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeSet, sync::Arc, time::Duration};

const FIRST: &str = "fake.first";
const SNAPSHOT_CAPABILITY_ID: &str = "fake.snapshot";
const EXTERNAL: &str = "fake.external";

struct FixedClock;
impl HostClock for FixedClock {
    fn unix_ms(&self) -> i64 {
        0
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn permissions(ids: &[&str]) -> BTreeSet<PermissionId> {
    ids.iter()
        .map(|id| PermissionId::new(*id).unwrap())
        .collect()
}

fn builder() -> ContractBuilder {
    // Deliberately not lexical: discovery must retain supplied authority order.
    let profiles = ["zobserver", "aobserver"]
        .map(|id| ProfileDescriptor {
            id: ProfileId::new(id).unwrap(),
            label: id.into(),
            inherits: BTreeSet::new(),
            permissions: BTreeSet::new(),
        })
        .to_vec();
    let permission_descriptors = ["observe", "observe.z", "observe.a"]
        .map(|id| PermissionDescriptor {
            id: PermissionId::new(id).unwrap(),
            label: id.into(),
            description: "Second host permission".into(),
            sensitive: false,
            default_grant: DefaultGrant::Granted,
            profile_ceilings: BTreeSet::from([ProfileId::new("zobserver").unwrap()]),
        })
        .to_vec();
    let mut builder = ContractBuilder::new(profiles, permission_descriptors).unwrap();
    builder
        .register_effect(EffectPolicy {
            id: EffectId::new("revise").unwrap(),
            permission_floor: PermissionId::new("observe").unwrap(),
            profile_ceilings: BTreeSet::from([ProfileId::new("zobserver").unwrap()]),
            confirmation_class: ConfirmationClassId::new("none").unwrap(),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: false,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                allows_destructive: false,
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })
        .unwrap();
    builder
        .register_module(ModuleDescriptor {
            id: ModuleId::new("fake").unwrap(),
            title: "Second host".into(),
            description: "No desktop state".into(),
        })
        .unwrap();
    builder
        .register_effect(EffectPolicy {
            id: EffectId::new("observe").unwrap(),
            permission_floor: PermissionId::new("observe").unwrap(),
            profile_ceilings: BTreeSet::from([ProfileId::new("zobserver").unwrap()]),
            confirmation_class: ConfirmationClassId::new("none").unwrap(),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: true,
                destructive: false,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(true),
                allows_destructive: false,
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })
        .unwrap();
    builder
}

fn projections() -> ProjectionRegistry<()> {
    let mut projections = ProjectionRegistry::new(Arc::new(FixedClock));
    projections
        .register_module(
            ModuleDescriptor {
                id: ModuleId::new("fake").unwrap(),
                title: "Fake".into(),
                description: "Scope owner".into(),
            },
            |_: &()| 0u8,
        )
        .unwrap();
    for id in ["fake.z", "fake.a"] {
        projections
            .register_scope(
                SnapshotScopeId::new(id).unwrap(),
                ModuleId::new("fake").unwrap(),
                1,
                "Scope",
                "A second-host scope",
                &["observe.a", "observe.z"],
                |_: &(), _| 1u8,
            )
            .unwrap();
    }
    projections
}

fn descriptor(id: &str) -> CapabilityDescriptor {
    let mut descriptor = reference_registry()
        .unwrap()
        .capability(&CapabilityId::new(COUNTER_READ).unwrap(), 1)
        .unwrap()
        .clone();
    descriptor.id = CapabilityId::new(id).unwrap();
    descriptor.input_schema = json!({"type":"object","properties":{"scopes":{"type":"array","items":{"type":"string"}}},"additionalProperties":false});
    descriptor.output_schema = json!({"type":"integer"});
    for example in &mut descriptor.examples {
        example.input = json!({});
        example.output = json!(1);
    }
    descriptor
}

#[derive(Debug, PartialEq)]
enum Token {
    First,
    Snapshot,
}

struct Host {
    contract: CapabilityContract<Token>,
    external: CapabilityDescriptor,
    input: CompiledSchema,
    output: CompiledSchema,
}

fn contract() -> Host {
    let mut contract = builder().build(&projections()).unwrap();
    contract
        .register_read(descriptor(FIRST), Token::First)
        .unwrap();
    contract
        .register_read(descriptor(SNAPSHOT_CAPABILITY_ID), Token::Snapshot)
        .unwrap();
    let mut external = descriptor(EXTERNAL);
    external.read_only = false;
    external.effect = EffectId::new("revise").unwrap();
    external.revision_policy = RevisionPolicy::Required;
    contract.register_external(external.clone()).unwrap();
    let input = CompiledSchema::new(&external.input_schema).unwrap();
    let output = CompiledSchema::new(&external.output_schema).unwrap();
    Host {
        contract,
        external,
        input,
        output,
    }
}

impl Host {
    fn schemas(&self) -> ExternalSchemas<'_> {
        ExternalSchemas {
            capability_id: &self.external.id,
            version: self.external.version,
            input: &self.input,
            output: &self.output,
        }
    }
    fn default_grant(&self) -> BTreeSet<PermissionId> {
        permissions(&["observe"])
    }
    fn prepare(
        &self,
        envelope: RequestEnvelope,
        grant: &BTreeSet<PermissionId>,
    ) -> Result<AdmittedRequest<&'static str>, ControlError> {
        self.contract.admit(
            envelope,
            grant,
            TierPolicy::STRICT,
            |_, _| Some(self.schemas()),
            |token, _, _| {
                Ok(ReadPreparation {
                    prepared: match token {
                        Token::First => "first",
                        Token::Snapshot => "snapshot",
                    },
                    dynamic_permissions: BTreeSet::new(),
                })
            },
        )
    }
}

fn request(capability: &str, payload: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("contract-test").unwrap(),
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

#[test]
fn two_read_tokens_and_an_explicit_external_route_are_selected() {
    let host = contract();
    assert_eq!(host.contract.read_count(), 2);
    for (id, expected) in [(FIRST, "first"), (SNAPSHOT_CAPABILITY_ID, "snapshot")] {
        let envelope = request(id, json!({}));
        let result = host
            .prepare(envelope.clone(), &host.default_grant())
            .unwrap();
        assert_eq!(result.envelope, envelope);
        assert!(matches!(result.route, AdmittedRoute::Read(value) if value == expected));
        assert_eq!(result.required_permissions, host.default_grant());
    }
    let result = host
        .contract
        .admit(
            request(EXTERNAL, json!({})),
            &host.default_grant(),
            TierPolicy::STRICT,
            |_, _| Some(host.schemas()),
            |_, _, _| -> Result<ReadPreparation<()>, ControlError> {
                panic!("external must not call a read")
            },
        )
        .unwrap();
    assert!(matches!(result.route, AdmittedRoute::External));
}

#[test]
fn envelope_static_payload_and_version_failures_never_call_preparation() {
    let host = contract();
    let callbacks = Cell::new(0);
    let mut malformed = request(FIRST, json!({}));
    malformed.protocol_version = 0;
    let mut wrong_version = request(FIRST, json!({}));
    wrong_version.capability_version = 2;
    for (envelope, grant, expected) in [
        (malformed, BTreeSet::new(), codes::INVALID_REQUEST),
        (
            request(FIRST, json!(42)),
            BTreeSet::new(),
            codes::PERMISSION_DENIED,
        ),
        (
            request(FIRST, json!(42)),
            host.default_grant(),
            codes::INVALID_REQUEST,
        ),
        (
            wrong_version,
            host.default_grant(),
            codes::CAPABILITY_UNKNOWN,
        ),
        (
            request("fake.unknown", json!({})),
            host.default_grant(),
            codes::CAPABILITY_UNKNOWN,
        ),
    ] {
        let error = host
            .contract
            .admit(
                envelope,
                &grant,
                TierPolicy::STRICT,
                |_, _| panic!("read does not ask an external provider"),
                |_, _, _| {
                    callbacks.set(callbacks.get() + 1);
                    Ok(ReadPreparation {
                        prepared: (),
                        dynamic_permissions: BTreeSet::new(),
                    })
                },
            )
            .unwrap_err();
        assert_eq!(error.code.as_str(), expected);
    }
    assert_eq!(callbacks.get(), 0);
}

#[derive(Debug)]
struct Invocation<'a> {
    drops: &'a Cell<usize>,
    effects: &'a Cell<usize>,
}
impl Invocation<'_> {
    fn execute(&self) {
        self.effects.set(self.effects.get() + 1);
    }
}
impl Drop for Invocation<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn dynamic_authorization_finalizes_the_union_and_drops_a_denied_preparation() {
    let host = contract();
    let callbacks = Cell::new(0);
    let drops = Cell::new(0);
    let effects = Cell::new(0);
    let prepare =
        |token: &Token, _: &Value, scopes: quantick_control_host::contract::ScopeCatalogue<'_>| {
            assert_eq!(*token, Token::Snapshot);
            callbacks.set(callbacks.get() + 1);
            let required = scopes
                .permissions(&SnapshotScopeId::new("fake.a").unwrap())
                .unwrap()
                .clone();
            Ok(ReadPreparation {
                prepared: Invocation {
                    drops: &drops,
                    effects: &effects,
                },
                dynamic_permissions: required,
            })
        };
    let error = host
        .contract
        .admit(
            request(SNAPSHOT_CAPABILITY_ID, json!({})),
            &host.default_grant(),
            TierPolicy::STRICT,
            |_, _| None,
            prepare,
        )
        .unwrap_err();
    assert_eq!(callbacks.get(), 1);
    assert_eq!(drops.get(), 1);
    assert_eq!(effects.get(), 0);
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({"code":"control.scope_denied","message":"one or more requested snapshot scopes are outside the connection grant","retryable":false,"details":{"missing_permissions":["observe.a","observe.z"]},"next_steps":["Enable the required read scopes in Quantick, then reconnect."]})
    );
    let grant = permissions(&["observe", "observe.a", "observe.z"]);
    let result = host
        .contract
        .admit(
            request(SNAPSHOT_CAPABILITY_ID, json!({})),
            &grant,
            TierPolicy::STRICT,
            |_, _| None,
            prepare,
        )
        .unwrap();
    assert_eq!(result.required_permissions, grant);
    assert_eq!(callbacks.get(), 2);
    assert_eq!(effects.get(), 0);
    let AdmittedRoute::Read(invocation) = result.route else {
        panic!("read route required")
    };
    invocation.execute();
    assert_eq!(effects.get(), 1);
    drop(invocation);
    assert_eq!(drops.get(), 2);
}

#[test]
fn failed_registration_is_atomic_and_cannot_replace_a_binding() {
    let mut host = contract();
    let before =
        serde_json::to_value(host.contract.registry().capabilities().collect::<Vec<_>>()).unwrap();
    for half in ["input", "output"] {
        let mut broken = descriptor("fake.broken");
        if half == "input" {
            broken.input_schema = json!({"type":12});
        } else {
            broken.output_schema = json!({"type":12});
        }
        assert!(matches!(
            host.contract.register_read(broken, Token::First),
            Err(RegistryError::InvalidDescriptor(_))
        ));
        assert!(
            host.contract
                .registry()
                .capability(&CapabilityId::new("fake.broken").unwrap(), 1)
                .is_none()
        );
    }
    assert!(
        host.contract
            .register_read(descriptor(FIRST), Token::Snapshot)
            .is_err()
    );
    assert!(host.contract.register_external(descriptor(FIRST)).is_err());
    assert!(
        host.contract
            .register_read(descriptor(EXTERNAL), Token::First)
            .is_err()
    );
    assert_eq!(
        before,
        serde_json::to_value(host.contract.registry().capabilities().collect::<Vec<_>>()).unwrap()
    );
    assert!(matches!(
        host.prepare(request(FIRST, json!({})), &host.default_grant())
            .unwrap()
            .route,
        AdmittedRoute::Read("first")
    ));
    assert_eq!(host.contract.read_count(), 2);
}

#[test]
fn external_input_requires_an_exact_provider_and_never_falls_back_to_a_read() {
    let host = contract();
    let wrong_id = CapabilityId::new(FIRST).unwrap();
    for provider in [
        None,
        Some(ExternalSchemas {
            capability_id: &wrong_id,
            ..host.schemas()
        }),
        Some(ExternalSchemas {
            version: 2,
            ..host.schemas()
        }),
    ] {
        let error = host
            .contract
            .admit(
                request(EXTERNAL, json!({})),
                &host.default_grant(),
                TierPolicy::STRICT,
                |_, _| provider,
                |_, _, _| -> Result<ReadPreparation<()>, ControlError> {
                    panic!("external never falls back")
                },
            )
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            json!({"code":"control.capability_unavailable","message":"registered observer capability has no input validator","retryable":false})
        );
    }
    let error = host
        .contract
        .admit(
            request(EXTERNAL, json!(42)),
            &BTreeSet::new(),
            TierPolicy::STRICT,
            |_, _| panic!("static denial precedes provider lookup"),
            |_, _, _| -> Result<ReadPreparation<()>, ControlError> { panic!("denied") },
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
}

#[test]
fn output_requires_registration_and_matching_external_schema_takes_precedence() {
    let host = contract();
    let first = CapabilityId::new(FIRST).unwrap();
    let external = &host.external.id;
    let string_output = CompiledSchema::new(&json!({"type":"string"})).unwrap();
    let override_schema = ExternalSchemas {
        capability_id: &first,
        output: &string_output,
        ..host.schemas()
    };
    assert!(
        host.contract
            .validate_output(&first, 1, &json!(1), |_, _| None)
    );
    assert!(
        !host
            .contract
            .validate_output(&first, 1, &json!("value"), |_, _| None)
    );
    assert!(
        host.contract
            .validate_output(&first, 1, &json!("value"), |_, _| Some(override_schema))
    );
    assert!(
        !host
            .contract
            .validate_output(&first, 1, &json!(1), |_, _| Some(override_schema))
    );
    assert!(
        !host
            .contract
            .validate_output(&first, 1, &json!(1), |_, _| Some(host.schemas()))
    );
    assert!(
        !host
            .contract
            .validate_output(external, 1, &json!(1), |_, _| None)
    );
    assert!(
        host.contract
            .validate_output(external, 1, &json!(1), |_, _| Some(host.schemas()))
    );
    assert!(
        !host
            .contract
            .validate_output(external, 1, &json!({}), |_, _| Some(host.schemas()))
    );
    assert!(
        !host
            .contract
            .validate_output(external, 2, &json!(1), |_, _| panic!(
                "unregistered version before provider"
            ))
    );
    assert!(
        !host
            .contract
            .validate_output(&first, 1, &json!(1), |_, _| Some(ExternalSchemas {
                version: 2,
                ..override_schema
            }))
    );
}

#[test]
fn discovery_retains_authority_order_and_sorted_scope_catalogue() {
    let host = contract();
    assert_eq!(
        host.contract
            .profiles()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["zobserver", "aobserver"]
    );
    assert_eq!(
        host.contract
            .permissions()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["observe", "observe.z", "observe.a"]
    );
    assert_eq!(
        host.contract
            .snapshot_scopes()
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["fake.a", "fake.z"]
    );
    assert!(
        host.contract
            .readable_scopes(&host.default_grant())
            .is_empty()
    );
    assert_eq!(
        host.contract
            .readable_scopes(&permissions(&["observe.a", "observe.z"]))
            .len(),
        2
    );
    let mut registry = projections();
    registry
        .register_scope(
            SnapshotScopeId::new("fake.unknown").unwrap(),
            ModuleId::new("fake").unwrap(),
            1,
            "Unknown",
            "Not in authority",
            &["unregistered"],
            |_: &(), _| 1u8,
        )
        .unwrap();
    assert!(matches!(
        builder().build::<Token, _>(&registry),
        Err(RegistryError::Unknown {
            kind: "permission",
            ..
        })
    ));
}

#[test]
fn strict_tier_preserves_omitted_required_revision_behavior() {
    let mut host = contract();
    let mut required = descriptor("fake.revision");
    required.read_only = false;
    required.effect = EffectId::new("revise").unwrap();
    required.revision_policy = RevisionPolicy::Required;
    host.contract.register_read(required, Token::First).unwrap();
    host.prepare(request("fake.revision", json!({})), &host.default_grant())
        .unwrap();
    let error = host
        .contract
        .admit(
            request("fake.revision", json!({})),
            &host.default_grant(),
            TierPolicy {
                accepts_dry_runs: false,
                accepts_expected_revisions: true,
            },
            |_, _| None,
            |_, _, _| -> Result<ReadPreparation<()>, ControlError> {
                panic!("missing revision before preparation")
            },
        )
        .unwrap_err();
    assert_eq!(
        error.message,
        "capability requires at least one expected revision"
    );
}
