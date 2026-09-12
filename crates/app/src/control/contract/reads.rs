//! The observer tier's read capabilities: each one's payload validation and
//! the invocation it becomes.
//!
//! One capability, one pair: a `prepare_*` handler that decodes the payload,
//! checks what the schema cannot (a duplicated scope, a cursor beside a start,
//! a timeout outside its bound) and names the scopes the request reaches; and
//! the invocation that runs once the contract has said yes. The saying yes is
//! not here. `ObserverContract::prepare` in the parent owns the order of the
//! checks -- envelope, registration, permission, schema, idempotency, tier --
//! and calls a handler only after all of them; the permission ceilings and the
//! effect policies stay in the parent's constructor. What this file may decide
//! is what one read needs, never whether the caller may have it.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    cursor::EventCursor,
    error::{ControlError, codes},
    handshake::ProtocolLimits,
    id::{CapabilityId, CostClassId, InstanceId, PermissionId, ProfileId, SnapshotScopeId},
    registry::{
        Availability, CapabilityDescriptor, ControlRegistry, EffectPersistence, ExpectedCost,
        IdempotencyPolicy, RegistryError, RevisionPolicy,
    },
    schema::{CompiledSchema, generated_schema},
    wire::ModuleRevision,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use super::super::{
    chart::{ChartWindowPage, chart_window_prevalidated},
    events::{EventsReadInput, EventsWaitInput, complete_wait_page, read_page},
    evidence::{
        EvidenceCapture, EvidenceCaptureInput, EvidenceReadInput, capture_prevalidated,
        source_scopes,
    },
    journal::EventPage,
    registry::SnapshotCapture,
    scene::CONTROLS_SCOPE_ID as SCENE_CONTROLS_SCOPE_ID,
    types::known_error,
};
use super::{
    ChartWindowInput, CompiledCapabilitySchemas, DeferredUiRead, EmptyInput, NO_CONFIRMATION_ID,
    OBSERVE_EFFECT_ID, ObserverContract, ParkedWait, PrepareHandler, PreparedCapability,
    PreparedDispatch, PreparedUiRead, PreparedWorkerRead, SerializedUiRead, SnapshotReadInput,
    UiReadContext, UiReadExecution, confirmation, effect, module, permission,
};

const UI_BOUNDED_COST_ID: &str = "ui_bounded";
const CAPABILITY_VERSION: u32 = 1;

fn serialization_failed(what: &str) -> ControlError {
    known_error(
        codes::CAPABILITY_UNAVAILABLE,
        format!("{what} could not be serialized"),
        false,
    )
}

struct DescribeInvocation;

impl PreparedWorkerRead for DescribeInvocation {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError> {
        serde_json::to_value(contract.describe(
            instance_id.clone(),
            effective_profile.clone(),
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
    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        context
            .projections
            .capture(context.app, context.instance_id, &self.scopes)
            .map(|capture| Box::new(capture) as UiReadExecution)
    }
}

impl DeferredUiRead for SnapshotCapture {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        let snapshot = SnapshotCapture::into_serialized(*self)
            .map_err(|_| serialization_failed("a snapshot capture"))?;
        let capture_revision = Some(snapshot.capture_revision);
        let module_revisions = snapshot.module_revisions.clone();
        let result = serde_json::to_value(snapshot)
            .map_err(|_| serialization_failed("a snapshot capture"))?;
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
    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        chart_window_prevalidated(
            context.app,
            context.instance_id,
            &self.input.query,
            &self.canonical_query,
            self.input.cursor.as_ref(),
        )
        .map(|page| Box::new(page) as UiReadExecution)
    }
}

impl DeferredUiRead for ChartWindowPage {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        let revision = self.consistency_revision;
        let result =
            serde_json::to_value(*self).map_err(|_| serialization_failed("a chart window page"))?;
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

/// `events.read`, and the read that completes `events.wait`: a bounded page
/// of the journal, taken on the application thread like every capture.
pub(crate) struct EventsReadInvocation {
    pub input: EventsReadInput,
    pub timed_out: bool,
    /// What the gateway-side resolve learned before a wait parked: the
    /// requested position had already been evicted. The read starts at the
    /// clamped cursor and would not know on its own.
    pub dropped_before: Option<EventCursor>,
}

impl PreparedUiRead for EventsReadInvocation {
    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        let page = read_page(
            context.journal,
            context.instance_id,
            self.input.cursor.as_ref(),
            self.input.start,
            self.input.limit,
            self.timed_out,
        )?;
        Ok(Box::new(complete_wait_page(
            page,
            self.dropped_before.clone(),
        )))
    }
}

impl DeferredUiRead for EventPage {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        let result =
            serde_json::to_value(&*self).map_err(|_| serialization_failed("an event page"))?;
        Ok(SerializedUiRead {
            capture_revision: None,
            module_revisions: Vec::new(),
            result,
        })
    }
}

/// `evidence.capture`: one coherent bundle over the named scopes, plus the
/// events around it and, when asked for and available, the frame just painted.
///
/// The application thread does only the collecting. Encoding, hashing,
/// chunking and retention happen in [`EvidenceCapture::into_manifest`], on the
/// same worker that serializes every other read.
struct EvidenceCaptureInvocation {
    input: EvidenceCaptureInput,
    source_scopes: BTreeSet<PermissionId>,
}

impl PreparedUiRead for EvidenceCaptureInvocation {
    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        capture_prevalidated(context, &self.input, self.source_scopes.clone())
            .map(|capture| Box::new(capture) as UiReadExecution)
    }

    fn needs_screenshot(&self) -> bool {
        self.input.screenshot
    }
}

impl DeferredUiRead for EvidenceCapture {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        let (manifest, capture_revision) = (*self).into_manifest()?;
        let result = serde_json::to_value(manifest)
            .map_err(|_| serialization_failed("an evidence manifest"))?;
        Ok(SerializedUiRead {
            capture_revision: Some(capture_revision),
            module_revisions: Vec::new(),
            result,
        })
    }
}

/// `evidence.read`: one page of a retained bundle.
///
/// A worker read, and deliberately: paging a retained resource needs no
/// application state at all, so it costs the frame nothing even while a client
/// pulls a bundle down chunk by chunk.
struct EvidenceReadInvocation {
    input: EvidenceReadInput,
}

impl PreparedWorkerRead for EvidenceReadInvocation {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        _effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        _effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError> {
        let page = contract.evidence.read(
            &self.input.evidence_id,
            self.input.cursor.as_ref(),
            instance_id,
            effective_scopes,
            crate::metrics::wall_clock_ms(),
        )?;
        serde_json::to_value(page).map_err(|_| serialization_failed("an evidence page"))
    }
}

pub(super) fn register_capability(
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

pub(super) fn prepare_describe(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let _: EmptyInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Worker(Box::new(DescribeInvocation)),
        dynamic_permissions: BTreeSet::new(),
    })
}

pub(super) fn prepare_snapshot(
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

/// A bundle asks for exactly the permissions a snapshot of the same scopes
/// would, plus the evidence scope the capability already requires and, when an
/// image is asked for, the screenshot scope. Aggregation is not a way in.
pub(super) fn prepare_evidence_capture(
    contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: EvidenceCaptureInput = decode_payload(payload)?;
    let mut unique = BTreeSet::new();
    let mut scope_permissions = Vec::with_capacity(input.scopes.len());
    for scope in &input.scopes {
        if !unique.insert(scope.clone()) {
            return Err(ControlError::invalid_request(format!(
                "snapshot scope `{scope}` was requested more than once"
            )));
        }
        scope_permissions.push(contract.scope_permissions.get(scope).ok_or_else(|| {
            ControlError::invalid_request(format!("snapshot scope `{scope}` is not registered"))
        })?);
    }
    let source_scopes = source_scopes(scope_permissions.into_iter(), input.screenshot);
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(EvidenceCaptureInvocation {
            input,
            source_scopes: source_scopes.clone(),
        })),
        dynamic_permissions: source_scopes,
    })
}

pub(super) fn prepare_evidence_read(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: EvidenceReadInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Worker(Box::new(EvidenceReadInvocation { input })),
        // The bundle's own source scopes are rechecked inside the store, from
        // the manifest: what a bundle aggregated is known there and nowhere
        // else, and a resource identifier is never an authorization.
        dynamic_permissions: BTreeSet::new(),
    })
}

pub(super) fn prepare_chart_window(
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

pub(super) fn prepare_events_read(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: EventsReadInput = decode_payload(payload)?;
    if input.cursor.is_some() == input.start.is_some() {
        return Err(known_error(
            codes::CURSOR_INVALID,
            "event read must supply either one cursor or one explicit start",
            false,
        ));
    }
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(EventsReadInvocation {
            input,
            timed_out: false,
            dropped_before: None,
        })),
        dynamic_permissions: BTreeSet::new(),
    })
}

pub(super) fn prepare_events_wait(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let input: EventsWaitInput = decode_payload(payload)?;
    if input.cursor.is_some() == input.start.is_some() {
        return Err(known_error(
            codes::CURSOR_INVALID,
            "event wait must supply either one cursor or one explicit start",
            false,
        ));
    }
    if input.timeout_ms == 0
        || input.timeout_ms > quantick_control::limits::CONTROL_WAIT_TIMEOUT_MAX_MS
    {
        return Err(ControlError::invalid_request(format!(
            "wait timeout must be in 1..={} ms",
            quantick_control::limits::CONTROL_WAIT_TIMEOUT_MAX_MS
        )));
    }
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Parked(ParkedWait { input }),
        dynamic_permissions: BTreeSet::new(),
    })
}

pub(super) fn prepare_diagnostics(
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

/// The scene is one scope, so the named tool takes no input beyond the
/// instance it routes to — exactly like the diagnostics read above.
pub(super) fn prepare_scene(
    _contract: &ObserverContract,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let _: EmptyInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Ui(Box::new(SnapshotInvocation {
            scopes: vec![
                SnapshotScopeId::new(SCENE_CONTROLS_SCOPE_ID).expect("static scope ID is valid"),
            ],
        })),
        dynamic_permissions: BTreeSet::new(),
    })
}

/// One read capability, with its pagination mode and that mode's own page
/// ceiling.
///
/// The ceiling travels with the mode because it is per capability, not per
/// protocol: a chart page is bounded by the bars an owned DTO may copy, an
/// evidence page by the chunks that fit one response, and the descriptor is
/// where a client learns which.
pub(super) fn read_capability<I, O, const N: usize>(
    id: &str,
    module_id: &str,
    title: &str,
    description: &str,
    permissions: [&str; N],
    pagination: Option<(quantick_control::cursor::PaginationConsistency, usize)>,
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
            max_items: pagination.map(|(_, max_items)| max_items),
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: pagination.map(|(mode, _)| mode),
    }
}

fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &Value) -> Result<T, ControlError> {
    serde_json::from_value(payload.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
