//! Immutable observer authority, capability, and request-dispatch contract.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use quantick_control::{
    cursor::EventCursor,
    error::{ControlError, codes},
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, InstanceId, ModuleId,
        PermissionId, ProfileId, RiskFlagId, SnapshotScopeId,
    },
    limits::{
        CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS, CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
        CONTROL_MAX_SNAPSHOT_SCOPES,
    },
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
    actions::{
        ANNOTATE_ATTENTION_PERMISSION_ID, ANNOTATE_EFFECT_ID, ANNOTATE_PERMISSION_ID,
        ANNOTATOR_PROFILE_ID, ATTENTION_MODULE_ID, ActionRegistry,
    },
    annotate::{ANNOTATE_CHART_PERMISSION_ID, ANNOTATE_MODULE_ID},
    chart::{ChartWindowPage, ChartWindowQuery, chart_window_prevalidated},
    events::{
        EVENTS_MODULE_ID, EVENTS_PERMISSION_ID, EventsReadInput, EventsWaitInput,
        READ_CAPABILITY_ID as EVENTS_READ_CAPABILITY_ID,
        WAIT_CAPABILITY_ID as EVENTS_WAIT_CAPABILITY_ID, complete_wait_page, read_page,
    },
    evidence::{
        CAPTURE_CAPABILITY_ID as EVIDENCE_CAPTURE_CAPABILITY_ID, EVIDENCE_MODULE_ID,
        EVIDENCE_PERMISSION_ID, EvidenceCapture, EvidenceCaptureInput, EvidenceChunkPage,
        EvidenceManifest, EvidenceReadInput, EvidenceStore,
        READ_CAPABILITY_ID as EVIDENCE_READ_CAPABILITY_ID, RawScreenshot, SessionIdentity,
        capture_prevalidated, source_scopes,
    },
    journal::{EventJournal, EventPage},
    notify::{NOTIFY_MODULE_ID, NOTIFY_PERMISSION_ID, NOTIFY_SOUND_PERMISSION_ID},
    registry::{ProjectionRegistry, SerializedSnapshotCapture, SnapshotCapture},
    scene::CONTROLS_SCOPE_ID as SCENE_CONTROLS_SCOPE_ID,
    script::{SCRIPT_MODULE_ID, SCRIPT_PERMISSION_ID},
    types::known_error,
};

pub(crate) const OBSERVER_PROFILE_ID: &str = "observer";
/// The tier that may rearrange the trader's window.
///
/// Its own profile rather than a permission inside `annotator`, because the
/// annotate tier's consent text makes a promise it would otherwise break: it
/// tells the trader that nothing they grant there can change their layout.
/// A capability that arrives under a grant whose own words deny it is a trust
/// bug, and the trader has no way to find it.
pub(crate) const COCKPIT_PROFILE_ID: &str = "cockpit";
/// Rearranging the window: which charts are shown, where, and how wide.
pub(crate) const COCKPIT_PERMISSION_ID: &str = "cockpit";
/// The permission for the canvas layout specifically.
pub(crate) const COCKPIT_LAYOUT_PERMISSION_ID: &str = "cockpit.layout";
/// The effect every cockpit capability declares.
pub(crate) const COCKPIT_EFFECT_ID: &str = "cockpit";
/// Permission for the one cockpit act that can remove the trader's work.
///
/// Separate from `cockpit.layout` on purpose, and marked sensitive: a grant
/// that lets an assistant rearrange panes must not silently also let it close
/// an open position. The layout tier's own doc comment names that class of
/// trust bug; this is the same rule applied to the tier that destroys.
pub(crate) const COCKPIT_RECOVER_PERMISSION_ID: &str = "cockpit.recover";
/// The effect for recovering a feed by rebuilding what it fed.
pub(crate) const RECOVER_EFFECT_ID: &str = "cockpit.recover";
/// What a capability under [`RECOVER_EFFECT_ID`] declares it may cost: the
/// chart's timeline, and with it the paper position and every armed strategy.
pub(crate) const TIMELINE_REBUILT_RISK_FLAG: &str = "timeline_rebuilt";
pub(crate) const DESCRIBE_CAPABILITY_ID: &str = "control.describe";
pub(crate) const SNAPSHOT_CAPABILITY_ID: &str = "snapshot.read";
pub(crate) const CHART_WINDOW_CAPABILITY_ID: &str = "chart.window.read";
pub(crate) const DIAGNOSTICS_CAPABILITY_ID: &str = "health.diagnostics.read";
pub(crate) const SCENE_CAPABILITY_ID: &str = "scene.read";

pub(crate) const OBSERVE_PERMISSION_ID: &str = "observe";
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
    "observe.events",
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
    /// `events.wait`: park on the gateway side until the journal moves past
    /// the resolved position or the timeout elapses, then run the bounded
    /// read through the UI queue.
    Parked(ParkedWait),
    /// A registered action: it runs on the application thread with mutable
    /// application state, through the very handler the trader's own gesture
    /// calls. The gateway attaches the connection's trusted actor; the
    /// payload never carries one.
    Action(PreparedAction),
}

/// An action that passed its permission check and its input schema, waiting
/// for the application thread.
#[derive(Clone, Debug)]
pub(crate) struct PreparedAction {
    pub capability_id: CapabilityId,
    pub capability_version: u32,
    pub input: Value,
}

/// A `wait_for_change` that has been validated and is about to park.
#[derive(Clone, Debug)]
pub(crate) struct ParkedWait {
    pub input: EventsWaitInput,
}

impl fmt::Debug for PreparedDispatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Worker(_) => "PreparedDispatch::Worker(<registered>)",
            Self::Ui(_) => "PreparedDispatch::Ui(<registered>)",
            Self::Parked(_) => "PreparedDispatch::Parked(<events.wait>)",
            Self::Action(_) => "PreparedDispatch::Action(<registered>)",
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

/// Work a capture still owes after it has left the application thread.
///
/// The failure is a full `ControlError` and not a marker: everything after the
/// application thread — encoding, hashing, retention — can refuse for a reason
/// the client can act on, and an evidence bundle that does not fit its store
/// has to be able to say `control.backpressure` rather than "serialization
/// failed".
pub(crate) trait DeferredUiRead: Send {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError>;
}

pub(crate) type UiReadExecution = Box<dyn DeferredUiRead>;

fn serialization_failed(what: &str) -> ControlError {
    known_error(
        codes::CAPABILITY_UNAVAILABLE,
        format!("{what} could not be serialized"),
        false,
    )
}

/// An action's result, on its way off the application thread. It is already
/// a value; the wrapper only lets it travel the same channel a capture does.
pub(crate) struct DeferredActionResult(pub Value);

impl DeferredUiRead for DeferredActionResult {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        Ok(SerializedUiRead {
            capture_revision: None,
            module_revisions: Vec::new(),
            result: self.0,
        })
    }
}

/// Everything one application-thread read may touch.
///
/// A struct rather than a parameter list because the set grows with the tier:
/// evidence needs the retained store, the connection's grant and the frame's
/// pixels on top of what a snapshot needs, and threading four more arguments
/// through every implementation would make each of them harder to read for the
/// benefit of one.
pub(crate) struct UiReadContext<'a> {
    pub projections: &'a mut ProjectionRegistry,
    pub journal: &'a EventJournal,
    pub app: &'a QuantickApp,
    pub instance_id: &'a InstanceId,
    pub session: &'a SessionIdentity,
    pub evidence: &'a EvidenceStore,
    /// The pixels of the frame just painted, when one was asked for and has
    /// arrived. Taken by the read that uses it, so a stale image can never be
    /// served to the next capture.
    pub screenshot: &'a mut Option<RawScreenshot>,
}

pub(crate) trait PreparedUiRead: Send {
    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError>;

    /// Whether this read needs the frame rasterised before it can run.
    ///
    /// The gateway asks before dispatching: a read that says yes and finds no
    /// image waits one frame while the window is asked for one, rather than
    /// answering without it.
    fn needs_screenshot(&self) -> bool {
        false
    }
}

pub(crate) trait PreparedWorkerRead: Send {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError>;
}

impl PreparedDispatch {
    pub fn execute_worker(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Option<Result<Value, ControlError>> {
        match self {
            Self::Worker(invocation) => Some(invocation.execute(
                contract,
                instance_id,
                effective_profile,
                effective_scopes,
                effective_limits,
            )),
            Self::Ui(_) | Self::Parked(_) | Self::Action(_) => None,
        }
    }

    pub fn execute_ui(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        match self {
            Self::Worker(_) | Self::Parked(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "a request that does not execute on the application thread entered the UI queue",
                false,
            )),
            Self::Action(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "an action reached the read path instead of the action path",
                false,
            )),
            Self::Ui(invocation) => invocation.execute(context),
        }
    }

    /// Whether this request needs the window rasterised first.
    pub fn needs_screenshot(&self) -> bool {
        match self {
            Self::Ui(invocation) => invocation.needs_screenshot(),
            Self::Worker(_) | Self::Parked(_) | Self::Action(_) => false,
        }
    }
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

struct PreparedCapability {
    dispatch: PreparedDispatch,
    dynamic_permissions: BTreeSet<PermissionId>,
}

type PrepareHandler = fn(&ObserverContract, &Value) -> Result<PreparedCapability, ControlError>;
type CompiledCapabilitySchemas = BTreeMap<CapabilityId, BTreeMap<u32, CompiledSchema>>;

pub(crate) struct ObserverContract {
    registry: ControlRegistry,
    actions: Arc<ActionRegistry>,
    handlers: BTreeMap<(CapabilityId, u32), PrepareHandler>,
    input_validators: CompiledCapabilitySchemas,
    output_validators: CompiledCapabilitySchemas,
    profiles: Vec<ProfileDescriptor>,
    permissions: Vec<PermissionDescriptor>,
    snapshot_scopes: Vec<SnapshotScopeDescriptor>,
    scope_permissions: BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
    /// The retained evidence bundles of this instance.
    ///
    /// Held here because a worker read reaches the contract and nothing else:
    /// paging a bundle needs no application state, so it must not have to
    /// travel the application thread to find its own store. The handle is
    /// shared; the application thread keeps one too, and empties it when
    /// access is withdrawn.
    evidence: EvidenceStore,
}

impl ObserverContract {
    pub fn new(
        projections: &ProjectionRegistry,
        actions: Arc<ActionRegistry>,
        evidence: EvidenceStore,
    ) -> Result<Self, RegistryError> {
        let observer = profile(OBSERVER_PROFILE_ID);
        let annotator = profile(ANNOTATOR_PROFILE_ID);
        let cockpit = profile(COCKPIT_PROFILE_ID);
        let mut permissions = vec![
            PermissionDescriptor {
                id: permission(OBSERVE_PERMISSION_ID),
                label: "Observe".to_owned(),
                description: "Invoke read-only observer capabilities.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Granted,
                profile_ceilings: BTreeSet::from([observer.clone()]),
            },
            // The annotate tier's floor. Every scope below it is off by
            // default and reaches a client only when the trader ticks it in
            // the access panel, which is also what raises the connection's
            // ceiling to the `annotator` profile (contract §7.1).
            PermissionDescriptor {
                id: permission(ANNOTATE_PERMISSION_ID),
                label: "Annotate".to_owned(),
                description: "Add reversible state or bounded notifications; never remove existing work or affect a position.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
            PermissionDescriptor {
                id: permission(COCKPIT_PERMISSION_ID),
                label: "Rearrange the window".to_owned(),
                description: "Change which charts are on screen, where they sit and how wide they are. Never places or removes an object, and never touches a position.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([cockpit.clone()]),
            },
            PermissionDescriptor {
                id: permission(COCKPIT_LAYOUT_PERMISSION_ID),
                label: "Change the chart layout".to_owned(),
                description: "Apply a layout preset, move a chart within the stack, resize a column, collapse it to its rail or expand it again, and move focus between charts.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([cockpit.clone()]),
            },
            PermissionDescriptor {
                id: permission(COCKPIT_RECOVER_PERMISSION_ID),
                label: "Rebuild a stalled chart".to_owned(),
                description: "Throw a stalled feed's timeline away and rebuild it, which closes any open paper position (journaled, with its reason) and disarms every strategy.".to_owned(),
                // Marked, and off until ticked: this is the one cockpit act
                // that ends something the trader started. Reconnecting — which
                // keeps the timeline, the position and the strategies — needs
                // none of this and stays under plain `cockpit`.
                sensitive: true,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([cockpit.clone()]),
            },
            PermissionDescriptor {
                id: permission(ANNOTATE_ATTENTION_PERMISSION_ID),
                label: "Create marks".to_owned(),
                description: "Append marks carrying the resolved cursor target to the event journal.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
            PermissionDescriptor {
                id: permission(ANNOTATE_CHART_PERMISSION_ID),
                label: "Answer on the chart".to_owned(),
                description: "Place labels, arrows and zones, attributed and removable in one action.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
            PermissionDescriptor {
                id: permission(NOTIFY_PERMISSION_ID),
                label: "Interrupt with a message".to_owned(),
                description: "Raise a popup or a toast the trader has to read and dismiss.".to_owned(),
                sensitive: false,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
            PermissionDescriptor {
                id: permission(NOTIFY_SOUND_PERMISSION_ID),
                label: "Make a sound".to_owned(),
                description: "Play the platform's alert sound, which reaches the trader even when they are not looking at the window.".to_owned(),
                // Off by default and marked: a sound cannot be taken back and
                // arrives whether or not anyone is at the screen.
                sensitive: true,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
            PermissionDescriptor {
                id: permission(SCRIPT_PERMISSION_ID),
                label: "Attach an indicator script".to_owned(),
                description: "Compile a Quantick Pine script and attach the indicator it produces to a pane, with a detach that restores the pane exactly.".to_owned(),
                sensitive: true,
                default_grant: DefaultGrant::Prompt,
                profile_ceilings: BTreeSet::from([annotator.clone()]),
            },
        ];
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

        let profiles = vec![
            ProfileDescriptor {
                id: observer.clone(),
                label: "Observer".to_owned(),
                inherits: BTreeSet::new(),
                permissions: BTreeSet::new(),
            },
            ProfileDescriptor {
                id: annotator.clone(),
                label: "Annotator".to_owned(),
                inherits: BTreeSet::from([observer.clone()]),
                permissions: BTreeSet::new(),
            },
            ProfileDescriptor {
                id: cockpit.clone(),
                label: "Cockpit".to_owned(),
                // Inherits the annotator, and through it the observer's reads
                // — rearranging a window you cannot see is not a coherent
                // grant. A *ceiling* is not a grant: what a connection may
                // actually call is the ceiling intersected with the scopes the
                // trader ticked, so nesting cockpit above annotator hands
                // nobody a capability they did not tick. What it does buy is
                // the property the handshake depends on: the profiles are a
                // chain, so any two of them are comparable.
                //
                // Left as a sibling of the annotator, the two ceilings
                // overlapped without nesting, and `handshake::authorize`
                // refuses an incomparable pair outright. A trader who ticked
                // both tiers got the cockpit ceiling on the panel, which drops
                // every `annotate.*` scope on the way out — and a client asking
                // for `--profile annotator` against that grant could not
                // connect at all.
                inherits: BTreeSet::from([annotator.clone()]),
                permissions: BTreeSet::new(),
            },
        ];

        let mut registry = ControlRegistry::new();
        for descriptor in &profiles {
            registry.register_profile(descriptor.clone())?;
        }
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
        registry.register_module(ModuleDescriptor {
            id: module(EVENTS_MODULE_ID),
            title: "Events".to_owned(),
            description: "The bounded semantic event journal and its cursor.".to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(EVIDENCE_MODULE_ID),
            title: "Evidence".to_owned(),
            description:
                "Coherent in-memory investigation bundles, read back as a paginated resource."
                    .to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(ANNOTATE_MODULE_ID),
            title: "Annotate".to_owned(),
            description: "Objects an operator places on the chart, attributed and removable."
                .to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(super::layout::LAYOUT_MODULE_ID),
            title: "Layout".to_owned(),
            description: "The canvas: which charts are on screen, where they sit, and how wide."
                .to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(NOTIFY_MODULE_ID),
            title: "Notify".to_owned(),
            description: "Interruptions: a popup, a toast, a sound.".to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(SCRIPT_MODULE_ID),
            title: "Indicators".to_owned(),
            description: "Indicator slots, and the Quantick Pine scripts attached to them."
                .to_owned(),
        })?;
        registry.register_module(ModuleDescriptor {
            id: module(ATTENTION_MODULE_ID),
            title: "Attention".to_owned(),
            description: "Human marks: what the user pointed at, as a durable referent.".to_owned(),
        })?;
        for descriptor in projections.module_descriptors() {
            registry.register_module(descriptor.clone())?;
        }

        registry.register_effect(EffectPolicy {
            id: effect(ANNOTATE_EFFECT_ID),
            permission_floor: permission(ANNOTATE_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([annotator.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
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
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })?;
        registry.register_effect(EffectPolicy {
            id: effect(COCKPIT_EFFECT_ID),
            permission_floor: permission(COCKPIT_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                destructive: false,
                // Applying the same layout twice leaves the same layout, which
                // is what lets a client retry a dropped call without wondering
                // what it did the first time.
                idempotent: true,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                // Nothing here removes the trader's work. A layout that hides
                // a pane keeps its drawings and its indicators, which is why
                // rearranging is not destructive even when it takes a chart
                // off the screen.
                allows_destructive: false,
                durable_requires_reversible: true,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })?;
        registry.register_effect(EffectPolicy {
            id: effect(RECOVER_EFFECT_ID),
            permission_floor: permission(COCKPIT_RECOVER_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([cockpit.clone()]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                // Follows the capabilities under it, which cannot claim
                // `destructive` while this host refuses the expected-revision
                // check the registry couples to it — see the note on the
                // descriptor in `super::recovery`. The irreversibility is
                // declared through the required risk flag below and through
                // each capability's `reversible: false`.
                destructive: false,
                // Rebuilding twice rebuilds twice. Each call really does throw
                // a timeline away, so a client must not be told a retry is
                // free.
                idempotent: false,
                open_world: false,
            },
            // Every capability here says, in its own descriptor, that it can
            // cost the trader their timeline.
            required_risk_flags: BTreeSet::from([
                RiskFlagId::new(TIMELINE_REBUILT_RISK_FLAG).expect("static risk flag is valid")
            ]),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                // The one effect in this contract that may. It exists because
                // the honest alternative was worse: a capability that destroys
                // while declaring it does not, so that it could sit under the
                // `cockpit` effect whose own words are "nothing here removes
                // the trader's work".
                allows_destructive: true,
                // A rebuilt chart is durable and cannot be put back. Saying so
                // here is what lets the descriptor say `reversible: false`
                // instead of claiming a reversal it cannot perform.
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })?;
        registry.register_effect(super::notify::effect_policy(&annotator))?;
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
                Some((
                    quantick_control::cursor::PaginationConsistency::AppendOnly,
                    CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
                )),
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
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EmptyInput, SerializedSnapshotCapture, _>(
                SCENE_CAPABILITY_ID,
                "scene",
                "Read semantic scene",
                "Names every control on screen with a frame-stable ID, its owner, whether it is selected, and the coded reason when it cannot be operated.",
                // The scope's own list, and for its reasons: the labels name
                // the markets the trader has open.
                [
                    OBSERVE_PERMISSION_ID,
                    "observe.attention",
                    "observe.workspace",
                    "observe.market",
                ],
                None,
            ),
            prepare_scene,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EventsReadInput, EventPage, _>(
                EVENTS_READ_CAPABILITY_ID,
                EVENTS_MODULE_ID,
                "Read events",
                "Reads a bounded page of the semantic event journal after a cursor or from an explicit start, and says when older events were dropped.",
                [OBSERVE_PERMISSION_ID, EVENTS_PERMISSION_ID],
                None,
            ),
            prepare_events_read,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EventsWaitInput, EventPage, _>(
                EVENTS_WAIT_CAPABILITY_ID,
                EVENTS_MODULE_ID,
                "Wait for change",
                "Parks off the application thread until the journal moves past the cursor or the timeout elapses, then reads the bounded page that completes the call.",
                [OBSERVE_PERMISSION_ID, EVENTS_PERMISSION_ID],
                None,
            ),
            prepare_events_wait,
        )?;
        // Read-only in the sense the effect policy means: a bundle changes no
        // application state, touches no position and takes nothing away from
        // the trader. What it creates is the answer itself — bounded by its
        // own named limits, expiring on its own, and gone the moment access
        // is withdrawn.
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EvidenceCaptureInput, EvidenceManifest, _>(
                EVIDENCE_CAPTURE_CAPABILITY_ID,
                EVIDENCE_MODULE_ID,
                "Capture evidence",
                "Freezes the named scopes, the events around them and the effective configuration into one hashed, redacted in-memory bundle, and answers with its manifest.",
                // The floor. Every scope the bundle actually aggregates is
                // added per request, so a capture can never reach further
                // than a snapshot of the same scopes would.
                [OBSERVE_PERMISSION_ID, EVIDENCE_PERMISSION_ID],
                None,
            ),
            prepare_evidence_capture,
        )?;
        register_capability(
            &mut registry,
            &mut handlers,
            &mut input_validators,
            &mut output_validators,
            read_capability::<EvidenceReadInput, EvidenceChunkPage, _>(
                EVIDENCE_READ_CAPABILITY_ID,
                EVIDENCE_MODULE_ID,
                "Read evidence bundle",
                "Reads a retained bundle in chunks of its canonical text, rechecking the grant the bundle aggregated on every page.",
                [OBSERVE_PERMISSION_ID, EVIDENCE_PERMISSION_ID],
                Some((
                    quantick_control::cursor::PaginationConsistency::RetainedResource,
                    CONTROL_EVIDENCE_MAX_CHUNKS_PER_PAGE,
                )),
            ),
            prepare_evidence_read,
        )?;
        // Actions are discoverable through the same registry as the reads;
        // they have no prepare handler here, so a remote request for one that
        // passes the permission check still fails closed before dispatch.
        for descriptor in actions.descriptors() {
            registry.register_capability(descriptor.clone())?;
        }

        Ok(Self {
            registry,
            actions,
            handlers,
            input_validators,
            output_validators,
            evidence,
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
        if let Some(action) = self
            .actions
            .lookup(capability_id.as_str(), capability_version)
        {
            return action.output.validate(result).is_ok();
        }
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

    /// Every registered snapshot scope this grant already reaches, in
    /// registration order and capped at what one capture may carry.
    ///
    /// Derived from the registry, never a hand-kept list: a module that
    /// registers a scope tomorrow is in a bundle tomorrow, without an edit
    /// here or in whatever asked.
    pub fn readable_scopes(&self, grant: &BTreeSet<PermissionId>) -> Vec<SnapshotScopeId> {
        self.snapshot_scopes
            .iter()
            .filter(|descriptor| descriptor.required_permissions.is_subset(grant))
            .map(|descriptor| descriptor.id.clone())
            .take(CONTROL_MAX_SNAPSHOT_SCOPES)
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
        effective_profile: ProfileId,
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
            effective_profile,
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
        let action = self
            .actions
            .lookup(descriptor.id.as_str(), descriptor.version);
        let validator = match &action {
            // An action validates against its own compiled schema — the same
            // one the hotkey's call passes — so the two paths cannot drift.
            Some(action) => &action.input,
            None => self
                .input_validators
                .get(&descriptor.id)
                .and_then(|versions| versions.get(&descriptor.version))
                .ok_or_else(|| {
                    known_error(
                        codes::CAPABILITY_UNAVAILABLE,
                        "registered observer capability has no input validator",
                        false,
                    )
                })?,
        };
        validator
            .validate(&envelope.payload)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        if envelope.dry_run
            || envelope.idempotency_key.is_some()
            || !envelope.expected_revisions.is_empty()
        {
            return Err(ControlError::invalid_request(
                "this tier's capabilities forbid dry runs, idempotency keys, and expected revisions",
            ));
        }
        if action.is_some() {
            let dispatch = PreparedDispatch::Action(PreparedAction {
                capability_id: descriptor.id.clone(),
                capability_version: descriptor.version,
                input: envelope.payload.clone(),
            });
            return Ok(PreparedRequest {
                required_permissions: descriptor.required_permissions.clone(),
                envelope,
                dispatch,
            });
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

/// A bundle asks for exactly the permissions a snapshot of the same scopes
/// would, plus the evidence scope the capability already requires and, when an
/// image is asked for, the screenshot scope. Aggregation is not a way in.
fn prepare_evidence_capture(
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

fn prepare_evidence_read(
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

fn prepare_events_read(
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

fn prepare_events_wait(
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

/// The scene is one scope, so the named tool takes no input beyond the
/// instance it routes to — exactly like the diagnostics read above.
fn prepare_scene(
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
fn read_capability<I, O, const N: usize>(
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
    use quantick_control::handshake::ProfileAuthority as _;
    use quantick_control::{id::RequestId, wire::RequestEnvelope};

    fn contract() -> ObserverContract {
        ObserverContract::new(
            &super::super::standard_registry().unwrap(),
            Arc::new(super::super::actions::standard_actions().unwrap()),
            EvidenceStore::new(),
        )
        .unwrap()
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
        // Nine reads with prepare handlers, plus the registered actions,
        // which have none here: an action is prepared from the action registry
        // and sits behind annotate permissions the observer ceiling does not
        // hold — discoverable to every client, reachable by none of them
        // until the trader grants the annotator profile.
        const READ_CAPABILITIES: usize = 9;
        let contract = contract();
        let capabilities = contract.registry.capabilities().collect::<Vec<_>>();
        let actions = contract.actions.descriptors().count();
        assert_eq!(capabilities.len(), READ_CAPABILITIES + actions);
        assert_eq!(contract.handlers.len(), READ_CAPABILITIES);
        let observer_ceiling = contract
            .registry
            .permission_ceiling(&profile(OBSERVER_PROFILE_ID))
            .expect("the observer profile has a ceiling");
        for capability in &capabilities {
            let reachable = capability.required_permissions.is_subset(&observer_ceiling);
            assert_eq!(
                reachable, capability.read_only,
                "{}: only read-only capabilities sit inside the observer ceiling",
                capability.id
            );
            // No capability in this contract is destructive, and the reason
            // is documented on `super::recovery`'s descriptor: the flag is
            // coupled to an expected-revision check this host refuses, so
            // claiming it would advertise a guarantee nothing delivers. The
            // assertion that matters either way is that nothing which could
            // end a trade is reachable from the observer ceiling.
            assert!(
                !(capability.destructive && reachable),
                "{}: a destructive capability is inside the observer ceiling",
                capability.id
            );
        }
        assert!(
            capabilities
                .iter()
                .any(|capability| capability.id.as_str()
                    == super::super::actions::MARK_CAPABILITY_ID)
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
                    &profile(OBSERVER_PROFILE_ID),
                    &contract.default_grant(),
                    &ProtocolLimits::default(),
                )
                .unwrap()
                .is_ok()
        );
    }
}
