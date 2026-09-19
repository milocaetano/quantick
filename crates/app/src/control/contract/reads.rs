//! The observer tier's read capabilities: each one's payload validation and
//! the invocation it becomes.
//!
//! One capability, one pair: a `prepare_*` handler that decodes the payload,
//! checks what the schema cannot (a duplicated scope, a cursor beside a start,
//! a timeout outside its bound) and names the scopes the request reaches; and
//! the invocation that runs once the contract has said yes. The saying yes is
//! not here. `CapabilityContract::admit` in control-host owns the order of the
//! checks -- envelope, registration, permission, schema, idempotency, tier --
//! and calls a handler only after all of them; the permission ceilings and the
//! effect policies stay in the parent's constructor. What this file may decide
//! is what one read needs, never whether the caller may have it.

use std::collections::BTreeSet;

use quantick_control::{
    cursor::EventCursor,
    error::{ControlError, codes},
    handshake::ProtocolLimits,
    id::{InstanceId, PermissionId, ProfileId, SnapshotScopeId},
    registry::CapabilityDescriptor,
    wire::ModuleRevision,
};
use quantick_control_host::authority::module;
use quantick_control_host::contract::ScopeCatalogue;
use serde::Deserialize;
use serde_json::Value;

use super::super::{
    chart::{ChartWindowPage, chart_window_prevalidated},
    events::{EventsReadInput, EventsWaitInput, complete_wait_page, read_page},
    evidence::{
        EvidenceCapture, EvidenceCaptureInput, EvidenceChunkPage, EvidenceManifest,
        EvidenceReadInput, capture_prevalidated, source_scopes,
    },
    journal::EventPage,
    registry::{SerializedSnapshotCapture, SnapshotCapture},
    scene::CONTROLS_SCOPE_ID as SCENE_CONTROLS_SCOPE_ID,
    types::known_error,
};
use super::{
    ChartWindowInput, DeferredUiRead, DescribeResult, EmptyInput, ObserverContract, ParkedWait,
    PrepareHandler, PreparedCapability, PreparedDispatch, PreparedUiRead, PreparedWorkerRead,
    SerializedUiRead, SnapshotReadInput, UiReadContext, UiReadExecution,
};

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

pub(super) fn prepare_describe(
    _scopes: ScopeCatalogue<'_>,
    payload: &Value,
) -> Result<PreparedCapability, ControlError> {
    let _: EmptyInput = decode_payload(payload)?;
    Ok(PreparedCapability {
        dispatch: PreparedDispatch::Worker(Box::new(DescribeInvocation)),
        dynamic_permissions: BTreeSet::new(),
    })
}

pub(super) fn prepare_snapshot(
    scopes: ScopeCatalogue<'_>,
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
        let permissions = scopes.permissions(scope).ok_or_else(|| {
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
    scopes: ScopeCatalogue<'_>,
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
        scope_permissions.push(scopes.permissions(scope).ok_or_else(|| {
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
    _scopes: ScopeCatalogue<'_>,
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
    _scopes: ScopeCatalogue<'_>,
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
    _scopes: ScopeCatalogue<'_>,
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
    _scopes: ScopeCatalogue<'_>,
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
    _scopes: ScopeCatalogue<'_>,
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
    _scopes: ScopeCatalogue<'_>,
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

/// Every read this application answers, with the handler that prepares it:
/// the descriptor comes from the published authority, the schemas and the
/// handler from here. Adding a read is one row.
pub(super) fn bindings() -> [(CapabilityDescriptor, PrepareHandler); 9] {
    use quantick_control_host::authority::{
        CHART_WINDOW, DESCRIBE, DIAGNOSTICS, EVENTS_READ, EVENTS_WAIT, EVIDENCE_CAPTURE,
        EVIDENCE_READ, SCENE, SNAPSHOT, read_descriptor,
    };
    [
        (
            read_descriptor::<EmptyInput, DescribeResult>(&DESCRIBE),
            prepare_describe,
        ),
        (
            read_descriptor::<SnapshotReadInput, SerializedSnapshotCapture>(&SNAPSHOT),
            prepare_snapshot,
        ),
        (
            read_descriptor::<ChartWindowInput, ChartWindowPage>(&CHART_WINDOW),
            prepare_chart_window,
        ),
        (
            read_descriptor::<EmptyInput, SerializedSnapshotCapture>(&DIAGNOSTICS),
            prepare_diagnostics,
        ),
        (
            read_descriptor::<EmptyInput, SerializedSnapshotCapture>(&SCENE),
            prepare_scene,
        ),
        (
            read_descriptor::<EventsReadInput, EventPage>(&EVENTS_READ),
            prepare_events_read,
        ),
        (
            read_descriptor::<EventsWaitInput, EventPage>(&EVENTS_WAIT),
            prepare_events_wait,
        ),
        (
            read_descriptor::<EvidenceCaptureInput, EvidenceManifest>(&EVIDENCE_CAPTURE),
            prepare_evidence_capture,
        ),
        (
            read_descriptor::<EvidenceReadInput, EvidenceChunkPage>(&EVIDENCE_READ),
            prepare_evidence_read,
        ),
    ]
}

fn decode_payload<T: for<'de> Deserialize<'de>>(payload: &Value) -> Result<T, ControlError> {
    serde_json::from_value(payload.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
