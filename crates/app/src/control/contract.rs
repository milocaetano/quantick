//! Immutable observer authority, capability, and request-dispatch contract.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use quantick_control::{
    error::{ControlError, codes},
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, InstanceId, ModuleId,
        PermissionId, ProfileId, SnapshotScopeId,
    },
    limits::{CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS, CONTROL_MAX_SNAPSHOT_SCOPES},
    registry::{
        Availability, CapabilityDescriptor, ControlRegistry, DefaultGrant, EffectConstraints,
        EffectPersistence, EffectPolicy, ExpectedCost, IdempotencyPolicy, McpHintFloor,
        ModuleDescriptor, PermissionDescriptor, ProfileDescriptor, RegistryError, RevisionPolicy,
    },
    schema::{CompiledSchema, generated_schema},
    wire::{ModuleRevision, RequestEnvelope, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::QuantickApp;

use super::{
    chart::{ChartWindowPage, ChartWindowQuery, chart_window_prevalidated},
    registry::{ProjectionRegistry, SerializedSnapshotCapture, SnapshotCapture},
};

pub(crate) const OBSERVER_PROFILE_ID: &str = "observer";
pub(crate) const DESCRIBE_CAPABILITY_ID: &str = "control.describe";
pub(crate) const SNAPSHOT_CAPABILITY_ID: &str = "snapshot.read";
pub(crate) const CHART_WINDOW_CAPABILITY_ID: &str = "chart.window.read";
pub(crate) const DIAGNOSTICS_CAPABILITY_ID: &str = "health.diagnostics.read";

const OBSERVE_PERMISSION_ID: &str = "observe";
const OBSERVE_EFFECT_ID: &str = "observe";
const NO_CONFIRMATION_ID: &str = "none";
const UI_BOUNDED_COST_ID: &str = "ui_bounded";
const CAPABILITY_VERSION: u32 = 1;

pub(crate) const SAFE_DEFAULT_SCOPE_IDS: &[&str] = &[
    "observe.system",
    "observe.workspace",
    "observe.market",
    "observe.chart",
    "observe.indicators",
    "observe.drawings",
    "observe.orderflow",
    "observe.replay",
    "observe.health",
    "observe.attention",
];

const OBSERVER_SCOPE_IDS: &[(&str, &str, bool)] = &[
    (
        "observe.system",
        "Application build and runtime identity",
        false,
    ),
    ("observe.workspace", "Open tabs, layout, and focus", false),
    (
        "observe.market",
        "Feed, symbol, and visible market data",
        false,
    ),
    ("observe.chart", "Chart framing, viewport, and bars", false),
    (
        "observe.indicators",
        "Indicator state and diagnostics",
        false,
    ),
    ("observe.drawings", "Drawing state and references", false),
    (
        "observe.orderflow",
        "Order-flow and local depth state",
        false,
    ),
    ("observe.replay", "Replay state", false),
    (
        "observe.paper",
        "Paper positions, orders, and performance",
        true,
    ),
    (
        "observe.health",
        "Bounded structured health diagnostics",
        false,
    ),
    (
        "observe.attention",
        "Semantic cursor and current selection",
        false,
    ),
    ("observe.events", "Bounded semantic event stream", false),
    (
        "observe.evidence",
        "Correlated in-memory evidence bundles",
        true,
    ),
    (
        "observe.user_text",
        "User-authored labels, notes, and scripts",
        true,
    ),
    (
        "observe.diagnostic_logs",
        "Redacted diagnostic log records",
        true,
    ),
    (
        "observe.screenshot",
        "Explicit raster evidence capture",
        true,
    ),
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EmptyInput {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotReadInput {
    #[schemars(length(min = 1, max = CONTROL_MAX_SNAPSHOT_SCOPES))]
    pub scopes: Vec<SnapshotScopeId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChartWindowInput {
    pub query: ChartWindowQuery,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<quantick_control::cursor::PageCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DescribeResult {
    pub instance_id: InstanceId,
    pub application_version: String,
    pub application_commit: String,
    pub protocol_version: u32,
    pub effective_profile: ProfileId,
    pub effective_scopes: BTreeSet<PermissionId>,
    pub effective_limits: ProtocolLimits,
    pub modules: Vec<ModuleDescriptor>,
    pub profiles: Vec<ProfileDescriptor>,
    pub permissions: Vec<PermissionDescriptor>,
    pub capabilities: Vec<CapabilityDescriptor>,
    pub snapshot_scopes: Vec<SnapshotScopeDescriptor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SnapshotScopeDescriptor {
    pub id: SnapshotScopeId,
    pub module_id: ModuleId,
    pub schema_version: u32,
    pub title: String,
    pub description: String,
    pub required_permissions: BTreeSet<PermissionId>,
    pub schema: Value,
}

pub(crate) enum PreparedDispatch {
    Worker(Box<dyn PreparedWorkerRead>),
    Ui(Box<dyn PreparedUiRead>),
}

impl fmt::Debug for PreparedDispatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Worker(_) => "PreparedDispatch::Worker(<registered>)",
            Self::Ui(_) => "PreparedDispatch::Ui(<registered>)",
        })
    }
}

#[derive(Debug)]
pub(crate) struct PreparedRequest {
    pub envelope: RequestEnvelope,
    pub required_permissions: BTreeSet<PermissionId>,
    pub dispatch: PreparedDispatch,
}

pub(crate) struct SerializedUiRead {
    pub capture_revision: Option<WireU64>,
    pub module_revisions: Vec<ModuleRevision>,
    pub result: Value,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DeferredSerializationError;

pub(crate) trait DeferredUiRead: Send {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, DeferredSerializationError>;
}

pub(crate) type UiReadExecution = Box<dyn DeferredUiRead>;

pub(crate) trait PreparedUiRead: Send {
    fn execute(
        &self,
        projections: &mut ProjectionRegistry,
        app: &QuantickApp,
        instance_id: &InstanceId,
    ) -> Result<UiReadExecution, ControlError>;
}

pub(crate) trait PreparedWorkerRead: Send {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError>;
}

impl PreparedDispatch {
    pub fn execute_worker(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Option<Result<Value, ControlError>> {
        match self {
            Self::Worker(invocation) => {
                Some(invocation.execute(contract, instance_id, effective_scopes, effective_limits))
            }
            Self::Ui(_) => None,
        }
    }

    pub fn execute_ui(
        &self,
        projections: &mut ProjectionRegistry,
        app: &QuantickApp,
        instance_id: &InstanceId,
    ) -> Result<UiReadExecution, ControlError> {
        match self {
            Self::Worker(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "worker-only request entered the UI queue",
                false,
            )),
            Self::Ui(invocation) => invocation.execute(projections, app, instance_id),
        }
    }
}

struct DescribeInvocation;

impl PreparedWorkerRead for DescribeInvocation {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError> {
        serde_json::to_value(contract.describe(
            instance_id.clone(),
            effective_scopes.clone(),
            effective_limits.clone(),
        ))
        .map_err(|_| {
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "observer worker result serialization failed",
                false,
            )
        })
    }
}

struct SnapshotInvocation {
    scopes: Vec<SnapshotScopeId>,
}

impl PreparedUiRead for SnapshotInvocation {
    fn execute(
        &self,
        projections: &mut ProjectionRegistry,
        app: &QuantickApp,
        instance_id: &InstanceId,
    ) -> Result<UiReadExecution, ControlError> {
        projections
            .capture(app, instance_id, &self.scopes)
            .map(|capture| Box::new(capture) as UiReadExecution)
    }
}

impl DeferredUiRead for SnapshotCapture {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, DeferredSerializationError> {
        let snapshot =
            SnapshotCapture::into_serialized(*self).map_err(|_| DeferredSerializationError)?;
        let capture_revision = Some(snapshot.capture_revision);
        let module_revisions = snapshot.module_revisions.clone();
        let result = serde_json::to_value(snapshot).map_err(|_| DeferredSerializationError)?;
        Ok(SerializedUiRead {
            capture_revision,
            module_revisions,
            result,
        })
    }
}

struct ChartWindowInvocation {
    input: ChartWindowInput,
    canonical_query: Value,
}

impl PreparedUiRead for ChartWindowInvocation {
    fn execute(
        &self,
        _projections: &mut ProjectionRegistry,
        app: &QuantickApp,
        instance_id: &InstanceId,
    ) -> Result<UiReadExecution, ControlError> {
        chart_window_prevalidated(
            app,
            instance_id,
            &self.input.query,
            &self.canonical_query,
            self.input.cursor.as_ref(),
        )
        .map(|page| Box::new(page) as UiReadExecution)
    }
}

impl DeferredUiRead for ChartWindowPage {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, DeferredSerializationError> {
        let revision = self.consistency_revision;
        let result = serde_json::to_value(*self).map_err(|_| DeferredSerializationError)?;
        Ok(SerializedUiRead {
            capture_revision: None,
            module_revisions: vec![ModuleRevision {
                module_id: module("chart"),
                revision,
            }],
            result,
        })
    }
}

struct PreparedCapability {
    dispatch: PreparedDispatch,
    dynamic_permissions: BTreeSet<PermissionId>,
}

type PrepareHandler = fn(&ObserverContract, &Value) -> Result<PreparedCapability, ControlError>;
type CompiledCapabilitySchemas = BTreeMap<CapabilityId, BTreeMap<u32, CompiledSchema>>;

pub(crate) struct ObserverContract {
    registry: ControlRegistry,
    handlers: BTreeMap<(CapabilityId, u32), PrepareHandler>,
    input_validators: CompiledCapabilitySchemas,
    output_validators: CompiledCapabilitySchemas,
    profiles: Vec<ProfileDescriptor>,
    permissions: Vec<PermissionDescriptor>,
    snapshot_scopes: Vec<SnapshotScopeDescriptor>,
    scope_permissions: BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
}

impl ObserverContract {
    pub fn new(projections: &ProjectionRegistry) -> Result<Self, RegistryError> {
        let observer = profile(OBSERVER_PROFILE_ID);
        let mut permissions = vec![PermissionDescriptor {
            id: permission(OBSERVE_PERMISSION_ID),
            label: "Observe".to_owned(),
            description: "Invoke read-only observer capabilities.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Granted,
            profile_ceilings: BTreeSet::from([observer.clone()]),
        }];
        permissions.extend(
            OBSERVER_SCOPE_IDS
                .iter()
                .map(|(id, description, sensitive)| PermissionDescriptor {
                    id: permission(id),
                    label: id.replace('.', " "),
                    description: (*description).to_owned(),
                    sensitive: *sensitive,
                    default_grant: if *sensitive {
                        DefaultGrant::Prompt
                    } else {
                        DefaultGrant::Granted
                    },
                    profile_ceilings: BTreeSet::from([observer.clone()]),
                }),
        );
        permissions.sort_by(|left, right| left.id.cmp(&right.id));

        let profiles = vec![ProfileDescriptor {
            id: observer.clone(),
            label: "Observer".to_owned(),
            inherits: BTreeSet::new(),
            permissions: BTreeSet::new(),
        }];

        let mut registry = ControlRegistry::new();
        registry.register_profile(profiles[0].clone())?;
        for descriptor in &permissions {
            registry.register_permission(descriptor.clone())?;
        }
        registry.finalize_authority()?;

        registry.register_module(ModuleDescriptor {
            id: module("control"),
            title: "Control".to_owned(),
            description: "Running-instance contract and authority metadata.".to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module("snapshot"),
            title: "Snapshot".to_owned(),
            description: "Coherent multi-module semantic captures.".to_owned(),
        })?;
        for descriptor in projections.module_descriptors() {
            registry.register_module(descriptor.clone())?;
        }

        registry.register_effect(EffectPolicy {
            id: effect(OBSERVE_EFFECT_ID),
            permission_floor: permission(OBSERVE_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([observer]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
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

        let mut scope_permissions = BTreeMap::new();
        let mut snapshot_scopes = Vec::new();
        for descriptor in projections.descriptors() {
            let required_permissions = descriptor.required_permissions.clone();
            for permission in &required_permissions {
                if !permissions.iter().any(|known| known.id == *permission) {
                    return Err(RegistryError::Unknown {
                        kind: "permission",
                        id: permission.to_string(),
                    });
                }
            }
            scope_permissions.insert(descriptor.scope_id.clone(), required_permissions.clone());
            snapshot_scopes.push(SnapshotScopeDescriptor {
                id: descriptor.scope_id.clone(),
                module_id: descriptor.module_id.clone(),
                schema_version: descriptor.schema_version,
                title: descriptor.title.clone(),
                description: descriptor.description.clone(),
                required_permissions,
                schema: descriptor.schema.clone(),
            });
        }
        snapshot_scopes.sort_by(|left, right| left.id.cmp(&right.id));

        let mut handlers = BTreeMap::new();
        let mut input_validators = CompiledCapabilitySchemas::new();
        let mut output_validators = CompiledCapabilitySchemas::new();
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EmptyInput, DescribeResult, _>(
                DESCRIBE_CAPABILITY_ID,
                "control",
                "Describe observer access",
                "Reports this instance, protocol, modules, scopes, profiles, permissions, and registered read capabilities.",
                [OBSERVE_PERMISSION_ID],
                None,
            ),
            prepare_describe,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<SnapshotReadInput, SerializedSnapshotCapture, _>(
                SNAPSHOT_CAPABILITY_ID,
                "snapshot",
                "Read semantic snapshot",
                "Captures the requested registered scopes coherently on the application thread.",
                [OBSERVE_PERMISSION_ID],
                None,
            ),
            prepare_snapshot,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<ChartWindowInput, ChartWindowPage, _>(
                CHART_WINDOW_CAPABILITY_ID,
                "chart",
                "Read chart window",
                "Reads a bounded append-only page of chart bars with an optional continuation cursor.",
                [OBSERVE_PERMISSION_ID, "observe.market", "observe.chart"],
                Some(quantick_control::cursor::PaginationConsistency::AppendOnly),
            ),
            prepare_chart_window,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EmptyInput, SerializedSnapshotCapture, _>(
                DIAGNOSTICS_CAPABILITY_ID,
                "health",
                "Read diagnostics",
                "Captures bounded structured application, indicator, and order-flow health.",
                [
                    OBSERVE_PERMISSION_ID,
                    "observe.health",
                    "observe.indicators",
                    "observe.orderflow",
                ],
                None,
            ),
            prepare_diagnostics,
        )?;

        Ok(Self {
            registry,
            handlers,
            input_validators,
            output_validators,
            profiles,
            permissions,
            snapshot_scopes,
            scope_permissions,
        })
    }

    pub fn registry(&self) -> &ControlRegistry {
        &self.registry
    }

    pub fn validate_output(
        &self,
        capability_id: &CapabilityId,
        capability_version: u32,
        result: &Value,
    ) -> bool {
        self.output_validators
            .get(capability_id)
            .and_then(|versions| versions.get(&capability_version))
            .is_some_and(|validator| validator.validate(result).is_ok())
    }

    pub fn default_grant(&self) -> BTreeSet<PermissionId> {
        std::iter::once(permission(OBSERVE_PERMISSION_ID))
            .chain(SAFE_DEFAULT_SCOPE_IDS.iter().map(|id| permission(id)))
            .collect()
    }

    pub fn selectable_permissions(&self) -> impl Iterator<Item = &PermissionDescriptor> {
        self.permissions
            .iter()
            .filter(|descriptor| descriptor.id.as_str() != OBSERVE_PERMISSION_ID)
    }

    pub fn describe(
        &self,
        instance_id: InstanceId,
        effective_scopes: BTreeSet<PermissionId>,
        effective_limits: ProtocolLimits,
    ) -> DescribeResult {
        DescribeResult {
            instance_id,
            application_version: env!("CARGO_PKG_VERSION").to_owned(),
            application_commit: option_env!("QUANTICK_GIT_COMMIT")
                .unwrap_or("unknown")
                .to_owned(),
            protocol_version: CURRENT_PROTOCOL_VERSION,
            effective_profile: profile(OBSERVER_PROFILE_ID),
            effective_scopes,
            effective_limits,
            modules: self.registry.modules().cloned().collect(),
            profiles: self.profiles.clone(),
            permissions: self.permissions.clone(),
            capabilities: self.registry.capabilities().cloned().collect(),
            snapshot_scopes: self.snapshot_scopes.clone(),
        }
    }

    pub fn prepare(
        &self,
        envelope: RequestEnvelope,
        effective_scopes: &BTreeSet<PermissionId>,
    ) -> Result<PreparedRequest, ControlError> {
        envelope.validate()?;
        let descriptor = self
            .registry
            .capability(&envelope.capability_id, envelope.capability_version)
            .ok_or_else(|| {
                ControlError::new(
                    quantick_control::id::ErrorCode::new(codes::CAPABILITY_UNKNOWN)
                        .expect("static error code is valid"),
                    "capability ID or version is not registered",
                    false,
                )
            })?;
        if !descriptor.required_permissions.is_subset(effective_scopes) {
            return Err(known_error(
                codes::PERMISSION_DENIED,
                "connection lacks a required capability permission",
                false,
            ));
        }
        self.input_validators
            .get(&descriptor.id)
            .and_then(|versions| versions.get(&descriptor.version))
            .ok_or_else(|| {
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "registered observer capability has no input validator",
                    false,
                )
            })?
            .validate(&envelope.payload)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        if envelope.dry_run
            || envelope.idempotency_key.is_some()
            || !envelope.expected_revisions.is_empty()
        {
            return Err(ControlError::invalid_request(
                "observer reads forbid dry runs, idempotency keys, and expected revisions",
            ));
        }

        let handler = self
            .handlers
            .get(&(descriptor.id.clone(), descriptor.version))
            .ok_or_else(|| {
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "registered observer capability has no handler",
                    false,
                )
            })?;
        let PreparedCapability {
            dispatch,
            dynamic_permissions,
        } = handler(self, &envelope.payload)?;

        if !dynamic_permissions.is_subset(effective_scopes) {
            let missing = dynamic_permissions
                .difference(effective_scopes)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let mut error = known_error(
                codes::SCOPE_DENIED,
                "one or more requested snapshot scopes are outside the connection grant",
                false,
            );
            error.context.details = Some(json!({ "missing_permissions": missing }));
            error.context.next_steps =
                vec!["Enable the required read scopes in Quantick, then reconnect.".to_owned()];
            return Err(error);
        }

        let mut required_permissions = descriptor.required_permissions.clone();
        required_permissions.extend(dynamic_permissions);
        Ok(PreparedRequest {
            envelope,
            required_permissions,
            dispatch,
        })
    }
}

fn register_capability(
    registry: &mut ControlRegistry,
    handlers: &mut BTreeMap<(CapabilityId, u32), PrepareHandler>,
    input_validators: &mut CompiledCapabilitySchemas,
    output_validators: &mut CompiledCapabilitySchemas,
    descriptor: CapabilityDescriptor,
    handler: PrepareHandler,
) -> Result<(), RegistryError> {
    let key = (descriptor.id.clone(), descriptor.version);
    let input_validator = CompiledSchema::new(&descriptor.input_schema).map_err(|error| {
        RegistryError::InvalidDescriptor(format!(
            "capability `{}` input schema is invalid: {error}",
            descriptor.id
        ))
    })?;
    let output_validator = CompiledSchema::new(&descriptor.output_schema).map_err(|error| {
        RegistryError::InvalidDescriptor(format!(
            "capability `{}` output schema is invalid: {error}",
            descriptor.id
        ))
    })?;
    registry.register_capability(descriptor)?;
    let previous = handlers.insert(key.clone(), handler);
    debug_assert!(
        previous.is_none(),
        "registry rejected duplicate capability IDs"
    );
    input_validators
        .entry(key.0.clone())
        .or_default()
        .insert(key.1, input_validator);
    output_validators
        .entry(key.0)
        .or_default()
        .insert(key.1, output_validator);
    Ok(())
}

fn prepare_describe(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let _: EmptyInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Worker(Box::new(DescribeInvocation)),
        dynamic_permissions: BTreeSet::new(),
    })
}

fn prepare_snapshot(
    contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: SnapshotReadInput = decode_payload(payload)?;
    let mut unique = BTreeSet::new();
    let mut required = BTreeSet::new();
    for scope in &input.scopes {
        if !unique.insert(scope.clone()) {
            return Err(ControlError::invalid_request(format!(
                "snapshot scope `{scope}` was requested more than once"
            )));
        }
        let permissions = contract.scope_permissions.get(scope).ok_or_else(|| {
            ControlError::invalid_request(format!("snapshot scope `{scope}` is not registered"))
        })?;
        required.extend(permissions.iter().cloned());
    }
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(SnapshotInvocation {
            scopes: input.scopes,
        })),
        dynamic_permissions: required,
    })
}

fn prepare_chart_window(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: ChartWindowInput = decode_payload(payload)?;
    let canonical_query = serde_json::to_value(&input.query)
        .map_err(|error| ControlError::invalid_request(format!("invalid chart query: {error}")))?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(ChartWindowInvocation {
            input,
            canonical_query,
        })),
        dynamic_permissions: BTreeSet::new(),
    })
}

fn prepare_diagnostics(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let _: EmptyInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(SnapshotInvocation {
            scopes: vec![SnapshotScopeId::new("health.summary").expect("static scope ID is valid")],
        })),
        dynamic_permissions: BTreeSet::new(),
    })
}

fn read_capability<I, O, const N: usize>(
    id: &str,
    module_id: &str,
    title: &str,
    description: &str,
    permissions: [&str; N],
    pagination: Option<quantick_control::cursor::PaginationConsistency>,
) -> CapabilityDescriptor
where
    I: JsonSchema,
    O: JsonSchema,
{
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: module(module_id),
        input_schema: generated_schema::<I>(),
        output_schema: generated_schema::<O>(),
        examples: Vec::new(),
        effect: effect(OBSERVE_EFFECT_ID),
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
        required_permissions: permissions.iter().map(|id| permission(id)).collect(),
        preconditions: Vec::new(),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: pagination.map(|_| CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS),
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination,
    }
}

fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &Value) -> Result<T, ControlError> {
    serde_json::from_value(payload.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn known_error(code: &str, message: &str, retryable: bool) -> ControlError {
    ControlError::new(
        quantick_control::id::ErrorCode::new(code).expect("static error code is valid"),
        message,
        retryable,
    )
}

fn module(id: &str) -> ModuleId {
    ModuleId::new(id).expect("static module ID is valid")
}

fn permission(id: &str) -> PermissionId {
    PermissionId::new(id).expect("static permission ID is valid")
}

fn profile(id: &str) -> ProfileId {
    ProfileId::new(id).expect("static profile ID is valid")
}

fn effect(id: &str) -> EffectId {
    EffectId::new(id).expect("static effect ID is valid")
}

fn confirmation(id: &str) -> ConfirmationClassId {
    ConfirmationClassId::new(id).expect("static confirmation ID is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control::{id::RequestId, wire::RequestEnvelope};

    fn contract() -> ObserverContract {
        ObserverContract::new(&super::super::standard_registry().unwrap()).unwrap()
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
    fn sensitive_cross_scope_reference_fails_closed() {
        let contract = contract();
        let grant = contract.default_grant();
        let error = contract
            .prepare(
                request(
                    SNAPSHOT_CAPABILITY_ID,
                    json!({ "scopes": ["interaction.selection"] }),
                ),
                &grant,
            )
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::SCOPE_DENIED);
        assert!(
            error.context.details.unwrap()["missing_permissions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|permission| permission == "observe.paper")
        );
    }

    #[test]
    fn default_grant_can_read_chart_but_not_sensitive_scopes() {
        let contract = contract();
        let grant = contract.default_grant();
        contract
            .prepare(
                request(
                    SNAPSHOT_CAPABILITY_ID,
                    json!({ "scopes": ["chart.summary", "interaction.cursor"] }),
                ),
                &grant,
            )
            .unwrap();
        assert!(!grant.contains(&permission("observe.paper")));
        assert!(!grant.contains(&permission("observe.evidence")));
        assert!(!grant.contains(&permission("observe.user_text")));
    }

    #[test]
    fn observer_registry_contains_only_read_capabilities() {
        let contract = contract();
        let capabilities = contract.registry.capabilities().collect::<Vec<_>>();
        assert_eq!(capabilities.len(), 4);
        assert_eq!(contract.handlers.len(), capabilities.len());
        assert!(capabilities.iter().all(|capability| capability.read_only));
        assert!(
            capabilities
                .iter()
                .all(|capability| !capability.destructive)
        );
    }

    #[test]
    fn a_second_registered_handler_docks_without_changing_gateway_dispatch() {
        let mut contract = contract();
        register_capability(
            &mut contract.registry,
            &mut contract.handlers,
            &mut contract.input_validators,
            &mut contract.output_validators,
            read_capability::<EmptyInput, DescribeResult, _>(
                "control.second",
                "control",
                "Second observer handler",
                "Exercises the registered capability handler port.",
                [OBSERVE_PERMISSION_ID],
                None,
            ),
            prepare_describe,
        )
        .unwrap();

        let prepared = contract
            .prepare(
                request("control.second", json!({})),
                &contract.default_grant(),
            )
            .unwrap();
        assert!(
            prepared
                .dispatch
                .execute_worker(
                    &contract,
                    &InstanceId::from_bytes([1; 16]),
                    &contract.default_grant(),
                    &ProtocolLimits::default(),
                )
                .unwrap()
                .is_ok()
        );
    }
}
