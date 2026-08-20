//! In-memory reference host and client using the public envelopes unchanged.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::{
    canonical::{Sha256Digest, canonical_sha256},
    error::{ControlError, codes},
    id::{
        CapabilityId, ConfirmationClassId, ConnectionId, CostClassId, EffectId, IdValidationError,
        IdempotencyKey, InstanceId, ModuleId, PermissionId, PreconditionId, PrincipalId, ProfileId,
        RequestId, RiskFlagId,
    },
    limits::{CONTROL_IDEMPOTENCY_MAX_ENTRIES, CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES},
    registry::{
        Availability, AvailabilityStatus, CapabilityDescriptor, CapabilityExample, ControlModule,
        ControlRegistry, DefaultGrant, EffectConstraints, EffectPersistence, EffectPolicy,
        ExpectedCost, IdempotencyPolicy, McpHintFloor, ModuleDescriptor, PermissionDescriptor,
        PreconditionDescriptor, ProfileDescriptor, RegistryError, RevisionPolicy,
    },
    schema::validate_instance,
    wire::{
        ActorContext, ActorKind, AuthorizedRequest, ModuleRevision, RequestEnvelope,
        ResponseEnvelope, ResponseOutcome, WireU64,
    },
};

pub const PROTOCOL_VERSION: u32 = 1;
pub const COUNTER_READ: &str = "fake.counter.read";
pub const COUNTER_SET: &str = "fake.counter.set";
pub const ECHO: &str = "extension.echo";
const FAKE_MAX_RESULT_BYTES: usize = 1024;
const FAKE_ECHO_MAX_LENGTH: usize = 128;
const FAKE_CLOCK_START_UNIX_MS: i64 = 1_700_000_000_000;
const FAKE_COUNTER_MIN: i64 = -9_007_199_254_740_991;
const FAKE_COUNTER_MAX: i64 = 9_007_199_254_740_991;

pub fn reference_registry() -> Result<ControlRegistry, RegistryError> {
    reference_registry_with(&[])
}

/// Build the reference registry with external modules through both phases of
/// the public docking port.
pub fn reference_registry_with(
    extra_modules: &[&dyn ControlModule],
) -> Result<ControlRegistry, RegistryError> {
    let mut registry = ControlRegistry::new();
    let observer = ProfileId::new("observer").expect("static ID is valid");
    let developer = ProfileId::new("developer").expect("static ID is valid");
    let observe = PermissionId::new("observe").expect("static ID is valid");

    registry.register_permission(PermissionDescriptor {
        id: observe.clone(),
        label: "Observe".to_owned(),
        description: "Read bounded fake state.".to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Granted,
        profile_ceilings: BTreeSet::from([observer.clone()]),
    })?;
    registry.register_profile(ProfileDescriptor {
        id: observer.clone(),
        label: "Observer".to_owned(),
        inherits: BTreeSet::new(),
        permissions: BTreeSet::new(),
    })?;
    registry.register_profile(ProfileDescriptor {
        id: developer.clone(),
        label: "Developer".to_owned(),
        inherits: BTreeSet::from([observer.clone()]),
        permissions: BTreeSet::new(),
    })?;

    let counter = FakeCounterModule;
    let echo = FakeEchoModule;
    counter.register_permissions(&mut registry)?;
    echo.register_permissions(&mut registry)?;
    for module in extra_modules {
        module.register_permissions(&mut registry)?;
    }
    registry.finalize_authority()?;

    registry.register_effect(EffectPolicy {
        id: EffectId::new("observe").expect("static ID is valid"),
        permission_floor: observe,
        profile_ceilings: BTreeSet::from([observer, developer]),
        confirmation_class: ConfirmationClassId::new("none").expect("static ID is valid"),
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
    })?;

    counter.register(&mut registry)?;
    echo.register(&mut registry)?;
    for module in extra_modules {
        module.register(&mut registry)?;
    }
    Ok(registry)
}

pub struct FakeCounterModule;

impl ControlModule for FakeCounterModule {
    fn register_permissions(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        registry.register_permission(PermissionDescriptor {
            id: PermissionId::new("fake.write").expect("static ID is valid"),
            label: "Change fake state".to_owned(),
            description: "Replace the in-memory reference counter.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([
                ProfileId::new("developer").expect("static ID is valid")
            ]),
        })
    }

    fn register(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        let module = ModuleId::new("fake").expect("static ID is valid");
        let fake_write = PermissionId::new("fake.write").expect("static ID is valid");
        let developer = ProfileId::new("developer").expect("static ID is valid");
        let state_loss = RiskFlagId::new("state_loss").expect("static ID is valid");
        registry.register_module(ModuleDescriptor {
            id: module.clone(),
            title: "Reference counter".to_owned(),
            description: "Deterministic state used to prove the control contract.".to_owned(),
        })?;
        registry.register_effect(EffectPolicy {
            id: EffectId::new("fake.mutate").expect("static ID is valid"),
            permission_floor: fake_write.clone(),
            profile_ceilings: BTreeSet::from([developer]),
            confirmation_class: ConfirmationClassId::new("session").expect("static ID is valid"),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: true,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::from([state_loss.clone()]),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                allows_destructive: true,
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })?;

        registry.register_capability(CapabilityDescriptor {
            id: CapabilityId::new(COUNTER_READ).expect("static ID is valid"),
            version: 1,
            title: "Read reference counter".to_owned(),
            description: "Return one coherent value and revision.".to_owned(),
            module: module.clone(),
            input_schema: object_schema(BTreeMap::new(), &[]),
            output_schema: object_schema(
                BTreeMap::from([
                    ("value", fake_counter_schema()),
                    ("revision", wire_u64_schema()),
                ]),
                &["value", "revision"],
            ),
            examples: vec![CapabilityExample {
                title: "Initial value".to_owned(),
                input: json!({}),
                output: json!({"value": 0, "revision": "0"}),
            }],
            effect: EffectId::new("observe").expect("static ID is valid"),
            risk_flags: BTreeSet::new(),
            read_only: true,
            idempotency: IdempotencyPolicy::Forbidden,
            revision_policy: RevisionPolicy::Forbidden,
            stale_input_safety: None,
            dry_run_supported: false,
            persistence: EffectPersistence::None,
            reversible: false,
            destructive: false,
            risk_reducing: false,
            required_permissions: BTreeSet::from([
                PermissionId::new("observe").expect("static ID is valid")
            ]),
            preconditions: Vec::new(),
            confirmation_class: ConfirmationClassId::new("none").expect("static ID is valid"),
            availability: Availability::available(),
            expected_cost: ExpectedCost {
                class: CostClassId::new("immediate").expect("static ID is valid"),
                max_items: Some(1),
                max_response_bytes: Some(FAKE_MAX_RESULT_BYTES),
            },
            pagination: None,
        })?;

        registry.register_capability(CapabilityDescriptor {
            id: CapabilityId::new(COUNTER_SET).expect("static ID is valid"),
            version: 1,
            title: "Set reference counter".to_owned(),
            description: "Replace the counter after an optimistic revision check.".to_owned(),
            module,
            input_schema: object_schema(
                BTreeMap::from([("value", fake_counter_schema())]),
                &["value"],
            ),
            output_schema: object_schema(
                BTreeMap::from([
                    ("previous", fake_counter_schema()),
                    ("current", fake_counter_schema()),
                    ("actor_kind", json!({"const": "agent"})),
                ]),
                &["previous", "current", "actor_kind"],
            ),
            examples: vec![CapabilityExample {
                title: "Replace the initial value".to_owned(),
                input: json!({"value": 5}),
                output: json!({"previous": 0, "current": 5, "actor_kind": "agent"}),
            }],
            effect: EffectId::new("fake.mutate").expect("static ID is valid"),
            risk_flags: BTreeSet::from([state_loss]),
            read_only: false,
            idempotency: IdempotencyPolicy::Required,
            revision_policy: RevisionPolicy::Required,
            stale_input_safety: None,
            dry_run_supported: true,
            persistence: EffectPersistence::Durable,
            reversible: true,
            destructive: true,
            risk_reducing: false,
            required_permissions: BTreeSet::from([fake_write]),
            preconditions: vec![PreconditionDescriptor {
                id: PreconditionId::new("fake.revision_matches").expect("static ID is valid"),
                description: "The expected fake-module revision is current.".to_owned(),
            }],
            confirmation_class: ConfirmationClassId::new("session").expect("static ID is valid"),
            availability: Availability::available(),
            expected_cost: ExpectedCost {
                class: CostClassId::new("immediate").expect("static ID is valid"),
                max_items: Some(1),
                max_response_bytes: Some(FAKE_MAX_RESULT_BYTES),
            },
            pagination: None,
        })
    }
}

pub struct FakeEchoModule;

impl ControlModule for FakeEchoModule {
    fn register(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        let module = ModuleId::new("extension").expect("static ID is valid");
        registry.register_module(ModuleDescriptor {
            id: module.clone(),
            title: "Second fake module".to_owned(),
            description: "Proves a second module can dock through the public port.".to_owned(),
        })?;
        registry.register_capability(CapabilityDescriptor {
            id: CapabilityId::new(ECHO).expect("static ID is valid"),
            version: 1,
            title: "Echo text".to_owned(),
            description: "Return the bounded text supplied by the caller.".to_owned(),
            module,
            input_schema: object_schema(
                BTreeMap::from([(
                    "text",
                    json!({"type": "string", "maxLength": FAKE_ECHO_MAX_LENGTH}),
                )]),
                &["text"],
            ),
            output_schema: object_schema(
                BTreeMap::from([(
                    "echo",
                    json!({"type": "string", "maxLength": FAKE_ECHO_MAX_LENGTH}),
                )]),
                &["echo"],
            ),
            examples: vec![CapabilityExample {
                title: "Echo a message".to_owned(),
                input: json!({"text": "hello"}),
                output: json!({"echo": "hello"}),
            }],
            effect: EffectId::new("observe").expect("static ID is valid"),
            risk_flags: BTreeSet::new(),
            read_only: true,
            idempotency: IdempotencyPolicy::Forbidden,
            revision_policy: RevisionPolicy::Forbidden,
            stale_input_safety: None,
            dry_run_supported: false,
            persistence: EffectPersistence::None,
            reversible: false,
            destructive: false,
            risk_reducing: false,
            required_permissions: BTreeSet::from([
                PermissionId::new("observe").expect("static ID is valid")
            ]),
            preconditions: Vec::new(),
            confirmation_class: ConfirmationClassId::new("none").expect("static ID is valid"),
            availability: Availability::available(),
            expected_cost: ExpectedCost {
                class: CostClassId::new("immediate").expect("static ID is valid"),
                max_items: Some(1),
                max_response_bytes: Some(FAKE_MAX_RESULT_BYTES),
            },
            pagination: None,
        })
    }
}

fn object_schema(properties: BTreeMap<&str, Value>, required: &[&str]) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn wire_u64_schema() -> Value {
    json!({"type": "string", "pattern": "^(0|[1-9][0-9]*)$"})
}

fn fake_counter_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": FAKE_COUNTER_MIN,
        "maximum": FAKE_COUNTER_MAX
    })
}

#[derive(Clone, Debug)]
pub struct TrustedSession {
    pub principal_id: PrincipalId,
    pub connection_id: ConnectionId,
    pub client_name: String,
    pub granted_permissions: BTreeSet<PermissionId>,
}

pub trait EnvelopeTransport {
    fn exchange(&mut self, request: RequestEnvelope) -> ResponseEnvelope;
}

pub struct FakeConnection<'host> {
    host: &'host mut FakeHost,
    session: TrustedSession,
}

impl EnvelopeTransport for FakeConnection<'_> {
    fn exchange(&mut self, request: RequestEnvelope) -> ResponseEnvelope {
        self.host.handle(request, &self.session)
    }
}

pub struct FakeClient<T> {
    transport: T,
    instance_id: InstanceId,
    next_request: u64,
}

impl<T: EnvelopeTransport> FakeClient<T> {
    pub fn new(instance_id: InstanceId, transport: T) -> Self {
        Self {
            transport,
            instance_id,
            next_request: 1,
        }
    }

    pub fn invoke(&mut self, invocation: FakeInvocation) -> ResponseEnvelope {
        let request_id = RequestId::new(format!("fake-request-{}", self.next_request))
            .expect("generated request ID is valid");
        self.next_request += 1;
        self.transport.exchange(RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id,
            instance_id: self.instance_id.clone(),
            capability_id: invocation.capability_id,
            capability_version: invocation.capability_version,
            expected_revisions: invocation.expected_revisions,
            idempotency_key: invocation.idempotency_key,
            dry_run: invocation.dry_run,
            reason: invocation.reason,
            payload: invocation.payload,
        })
    }
}

#[derive(Clone, Debug)]
pub struct FakeInvocation {
    pub capability_id: CapabilityId,
    pub capability_version: u32,
    pub expected_revisions: Vec<ModuleRevision>,
    pub idempotency_key: Option<IdempotencyKey>,
    pub dry_run: bool,
    pub reason: Option<String>,
    pub payload: Value,
}

impl FakeInvocation {
    pub fn try_new(capability_id: &str, payload: Value) -> Result<Self, IdValidationError> {
        Ok(Self {
            capability_id: CapabilityId::new(capability_id)?,
            capability_version: 1,
            expected_revisions: Vec::new(),
            idempotency_key: None,
            dry_run: false,
            reason: None,
            payload,
        })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct IdempotencyScope {
    instance_id: InstanceId,
    principal_id: PrincipalId,
    capability_id: CapabilityId,
    capability_version: u32,
    key: IdempotencyKey,
}

#[derive(Clone, Debug)]
struct IdempotencyRecord {
    input_digest: Sha256Digest,
    outcome: ResponseOutcome,
    module_revisions: Vec<ModuleRevision>,
}

pub struct FakeHost {
    instance_id: InstanceId,
    registry: ControlRegistry,
    counter: i64,
    fake_revision: u64,
    capture_revision: u64,
    requested_at_unix_ms: i64,
    idempotency: BTreeMap<IdempotencyScope, IdempotencyRecord>,
}

impl FakeHost {
    pub fn new(instance_id: InstanceId) -> Result<Self, RegistryError> {
        Ok(Self {
            instance_id,
            registry: reference_registry()?,
            counter: 0,
            fake_revision: 0,
            capture_revision: 0,
            requested_at_unix_ms: FAKE_CLOCK_START_UNIX_MS,
            idempotency: BTreeMap::new(),
        })
    }

    pub fn registry(&self) -> &ControlRegistry {
        &self.registry
    }

    pub fn counter(&self) -> i64 {
        self.counter
    }

    pub fn fake_revision(&self) -> u64 {
        self.fake_revision
    }

    pub fn connect(&mut self, session: TrustedSession) -> FakeConnection<'_> {
        FakeConnection {
            host: self,
            session,
        }
    }

    fn handle(&mut self, request: RequestEnvelope, session: &TrustedSession) -> ResponseEnvelope {
        let revisions_before = self.module_revisions();
        let failure = |error| response(&request, revisions_before.clone(), None, Err(error));
        if request.instance_id != self.instance_id {
            return failure(ControlError::known(
                codes::INSTANCE_GONE,
                "request names a different running instance",
                false,
            ));
        }
        if request.protocol_version != PROTOCOL_VERSION {
            return failure(ControlError::known(
                codes::VERSION_UNSUPPORTED,
                "fake host supports protocol version 1",
                false,
            ));
        }
        if let Err(error) = request.validate() {
            return failure(error);
        }
        let Some(descriptor) = self
            .registry
            .capability(&request.capability_id, request.capability_version)
            .cloned()
        else {
            return failure(ControlError::known(
                codes::CAPABILITY_UNKNOWN,
                "capability ID or version is not registered",
                false,
            ));
        };
        if !descriptor
            .required_permissions
            .is_subset(&session.granted_permissions)
        {
            return failure(ControlError::known(
                codes::PERMISSION_DENIED,
                "connection lacks a required permission",
                false,
            ));
        }
        match descriptor.availability.status {
            AvailabilityStatus::Available => {}
            AvailabilityStatus::PermissionDenied => {
                return failure(ControlError::known(
                    codes::PERMISSION_DENIED,
                    "capability is unavailable under the current permission state",
                    false,
                ));
            }
            AvailabilityStatus::Unavailable | AvailabilityStatus::ConfirmationRequired => {
                let mut error = ControlError::known(
                    codes::CAPABILITY_UNAVAILABLE,
                    "capability is not currently available",
                    false,
                );
                if let Some(next_step) = &descriptor.availability.next_step {
                    error.context.next_steps.push(next_step.clone());
                }
                return failure(error);
            }
        }
        if let Err(error) = validate_instance(&descriptor.input_schema, &request.payload) {
            return failure(ControlError::invalid_request(error.to_string()));
        }
        if request.dry_run && !descriptor.dry_run_supported {
            return failure(ControlError::invalid_request(
                "capability does not support dry runs",
            ));
        }
        match (descriptor.idempotency, request.idempotency_key.as_ref()) {
            (IdempotencyPolicy::Forbidden, Some(_)) => {
                return failure(ControlError::invalid_request(
                    "capability forbids idempotency keys",
                ));
            }
            (IdempotencyPolicy::Required, None) if !request.dry_run => {
                return failure(ControlError::invalid_request(
                    "capability requires an idempotency key",
                ));
            }
            _ => {}
        }
        match descriptor.revision_policy {
            RevisionPolicy::Forbidden if !request.expected_revisions.is_empty() => {
                return failure(ControlError::invalid_request(
                    "capability forbids expected revisions",
                ));
            }
            RevisionPolicy::Required if request.expected_revisions.is_empty() => {
                return failure(ControlError::invalid_request(
                    "capability requires at least one expected revision",
                ));
            }
            RevisionPolicy::Forbidden
            | RevisionPolicy::OptionalForAdditive
            | RevisionPolicy::Required => {}
        }

        let actor = ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: session.principal_id.clone(),
            client_name: session.client_name.clone(),
            connection_id: session.connection_id.clone(),
            request_id: request.request_id.clone(),
            reason: request.reason.clone(),
            requested_at_unix_ms: self.requested_at_unix_ms,
        };
        self.requested_at_unix_ms = self.requested_at_unix_ms.saturating_add(1);
        let authorized = AuthorizedRequest {
            envelope: request.clone(),
            actor,
        };
        if let Err(error) = authorized.validate() {
            return failure(error);
        }

        let idempotency = match self.idempotency_context(&authorized) {
            Ok(context) => context,
            Err(error) => return failure(error),
        };
        if let Some((scope, input_digest)) = &idempotency
            && let Some(record) = self.idempotency.get(scope)
        {
            if &record.input_digest != input_digest {
                return failure(ControlError::idempotency_conflict());
            }
            return response(
                &request,
                record.module_revisions.clone(),
                None,
                match &record.outcome {
                    ResponseOutcome::Success { result } => Ok(result.clone()),
                    ResponseOutcome::Failure { error } => Err(error.clone()),
                },
            );
        }
        if idempotency.is_some() && self.idempotency.len() >= CONTROL_IDEMPOTENCY_MAX_ENTRIES {
            return failure(ControlError::known(
                codes::BACKPRESSURE,
                "idempotency store is at capacity",
                true,
            ));
        }

        let state_before = (self.counter, self.fake_revision);
        let result = self.dispatch(&descriptor, &authorized);
        if result.is_err() {
            (self.counter, self.fake_revision) = state_before;
        }
        let revisions_after = self.module_revisions();
        let capture_revision = if descriptor.read_only {
            let Some(next_capture_revision) = self.capture_revision.checked_add(1) else {
                (self.counter, self.fake_revision) = state_before;
                return failure(ControlError::known(
                    codes::CAPABILITY_UNAVAILABLE,
                    "capture revision space is exhausted for this instance",
                    false,
                ));
            };
            self.capture_revision = next_capture_revision;
            Some(WireU64::new(self.capture_revision))
        } else {
            None
        };
        let reply = response(&request, revisions_after.clone(), capture_revision, result);

        if !request.dry_run
            && let Some((scope, input_digest)) = idempotency
        {
            let record_bytes = serde_json::to_vec(&reply)
                .map(|bytes| bytes.len())
                .unwrap_or(usize::MAX);
            if record_bytes > CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES {
                (self.counter, self.fake_revision) = state_before;
                return failure(ControlError::known(
                    codes::BACKPRESSURE,
                    "terminal result is too large for safe idempotency retention",
                    false,
                ));
            }
            self.idempotency.insert(
                scope,
                IdempotencyRecord {
                    input_digest,
                    outcome: reply.outcome.clone(),
                    module_revisions: revisions_after,
                },
            );
        }
        reply
    }

    fn idempotency_context(
        &self,
        request: &AuthorizedRequest,
    ) -> Result<Option<(IdempotencyScope, Sha256Digest)>, ControlError> {
        let Some(key) = request.envelope.idempotency_key.clone() else {
            return Ok(None);
        };
        if request.envelope.dry_run {
            return Ok(None);
        }
        let mut expected_revisions = request.envelope.expected_revisions.clone();
        expected_revisions.sort_by(|left, right| left.module_id.cmp(&right.module_id));
        let digest_input = json!({
            "expected_revisions": expected_revisions,
            "payload": request.envelope.payload,
        });
        let digest = canonical_sha256(&digest_input)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        Ok(Some((
            IdempotencyScope {
                instance_id: self.instance_id.clone(),
                principal_id: request.actor.principal_id.clone(),
                capability_id: request.envelope.capability_id.clone(),
                capability_version: request.envelope.capability_version,
                key,
            },
            digest,
        )))
    }

    fn dispatch(
        &mut self,
        descriptor: &CapabilityDescriptor,
        request: &AuthorizedRequest,
    ) -> Result<Value, ControlError> {
        match descriptor.id.as_str() {
            COUNTER_READ => Ok(json!({
                "value": self.counter,
                "revision": self.fake_revision.to_string(),
            })),
            COUNTER_SET => self.set_counter(request),
            ECHO => Ok(json!({
                "echo": request.envelope.payload["text"].clone(),
            })),
            _ => Err(ControlError::known(
                codes::CAPABILITY_UNAVAILABLE,
                "reference host has no handler for this registered capability",
                false,
            )),
        }
        .and_then(|result| {
            validate_instance(&descriptor.output_schema, &result).map_err(|_| {
                ControlError::known(
                    codes::INVALID_REQUEST,
                    "handler returned data outside its declared schema",
                    false,
                )
            })?;
            Ok(result)
        })
    }

    fn set_counter(&mut self, request: &AuthorizedRequest) -> Result<Value, ControlError> {
        let expected = request
            .envelope
            .expected_revisions
            .iter()
            .find(|revision| revision.module_id.as_str() == "fake")
            .map(|revision| revision.revision.get());
        if expected != Some(self.fake_revision) {
            let mut error = ControlError::revision_conflict(self.module_revisions());
            error.context.violated_precondition =
                Some(PreconditionId::new("fake.revision_matches").expect("static ID is valid"));
            return Err(error);
        }
        let value = request.envelope.payload["value"]
            .as_i64()
            .ok_or_else(|| ControlError::invalid_request("counter value must fit i64"))?;
        if !(FAKE_COUNTER_MIN..=FAKE_COUNTER_MAX).contains(&value) {
            return Err(ControlError::invalid_request(
                "counter value must be a JSON safe integer",
            ));
        }
        let previous = self.counter;
        let next_revision = self.fake_revision.checked_add(1).ok_or_else(|| {
            ControlError::known(
                codes::CAPABILITY_UNAVAILABLE,
                "fake-module revision space is exhausted for this instance",
                false,
            )
        })?;
        if !request.envelope.dry_run {
            self.counter = value;
            self.fake_revision = next_revision;
        }
        Ok(json!({
            "previous": previous,
            "current": value,
            "actor_kind": "agent",
        }))
    }

    fn module_revisions(&self) -> Vec<ModuleRevision> {
        vec![ModuleRevision {
            module_id: ModuleId::new("fake").expect("static ID is valid"),
            revision: WireU64::new(self.fake_revision),
        }]
    }
}

fn response(
    request: &RequestEnvelope,
    module_revisions: Vec<ModuleRevision>,
    capture_revision: Option<WireU64>,
    result: Result<Value, ControlError>,
) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: request.protocol_version,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision,
        module_revisions,
        outcome: match result {
            Ok(result) => ResponseOutcome::Success { result },
            Err(error) => ResponseOutcome::Failure { error },
        },
        warnings: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_session() -> TrustedSession {
        TrustedSession {
            principal_id: PrincipalId::from_bytes([2; 16]),
            connection_id: ConnectionId::from_bytes([3; 16]),
            client_name: "reference client".to_owned(),
            granted_permissions: BTreeSet::from([
                PermissionId::new("observe").unwrap(),
                PermissionId::new("fake.write").unwrap(),
            ]),
        }
    }

    #[test]
    fn revision_conflict_does_not_change_state() {
        let instance = InstanceId::from_bytes([1; 16]);
        let mut host = FakeHost::new(instance.clone()).unwrap();
        {
            let connection = host.connect(full_session());
            let mut client = FakeClient::new(instance, connection);
            let mut invocation = FakeInvocation::try_new(COUNTER_SET, json!({"value": 8})).unwrap();
            invocation.expected_revisions = vec![ModuleRevision {
                module_id: ModuleId::new("fake").unwrap(),
                revision: WireU64::new(9),
            }];
            invocation.idempotency_key = Some(IdempotencyKey::new("set-8").unwrap());
            assert!(matches!(
                client.invoke(invocation).outcome,
                ResponseOutcome::Failure { error }
                    if error.code.as_str() == codes::REVISION_CONFLICT
            ));
        }
        assert_eq!(host.counter(), 0);
        assert_eq!(host.fake_revision(), 0);
    }
}
