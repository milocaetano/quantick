//! Generated observer contracts committed under `schemas/control`.

use quantick_control::schema::generated_schema;
use serde_json::{Value, json};

use super::{
    actions::{MarkInput, MarkResult},
    analysis::{DrawingsSnapshot, IndicatorsSnapshot},
    annotate::{AnnotationInput, AnnotationResult, RemoveInput, RemoveResult},
    chart::{ChartSnapshot, ChartWindowPage, ChartWindowQuery},
    contract::{
        ChartWindowInput, DescribeResult, EmptyInput, ObserverContract, SnapshotReadInput,
        SnapshotScopeDescriptor,
    },
    events::{EventsReadInput, EventsWaitInput},
    evidence::{
        EvidenceCaptureInput, EvidenceChunkPage, EvidenceDocument, EvidenceManifest,
        EvidenceReadInput,
    },
    feed::FeedSnapshot,
    health::HealthSnapshot,
    interaction::{CursorSnapshot, SelectionSnapshot},
    journal::EventPage,
    notify::{NotifyInput, NotifyResult},
    orderflow::{BubblesSnapshot, FootprintSnapshot, HeatmapSnapshot, L2Snapshot, TapeSnapshot},
    registry::SerializedSnapshotCapture,
    scene::SceneSnapshot,
    script::{AttachInput, AttachResult, DetachInput, DetachResult, ScriptDiagnostic},
    session::{PaperSnapshot, ReplaySnapshot},
    system::SystemSnapshot,
    workspace::WorkspaceSnapshot,
};

pub(crate) struct SchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

pub(crate) fn documents() -> Vec<SchemaDocument> {
    vec![
        document::<EmptyInput>("observer-empty-input-v1.schema.json"),
        document::<SnapshotReadInput>("observer-snapshot-read-input-v1.schema.json"),
        document::<ChartWindowInput>("observer-chart-window-input-v1.schema.json"),
        document::<DescribeResult>("observer-describe-result-v1.schema.json"),
        document::<SnapshotScopeDescriptor>("observer-snapshot-scope-descriptor-v1.schema.json"),
        document::<SerializedSnapshotCapture>("observer-snapshot-capture-v1.schema.json"),
        document::<SystemSnapshot>("observer-system-info-v1.schema.json"),
        document::<WorkspaceSnapshot>("observer-workspace-summary-v1.schema.json"),
        document::<FeedSnapshot>("observer-feed-status-v1.schema.json"),
        document::<ChartSnapshot>("observer-chart-summary-v1.schema.json"),
        document::<HealthSnapshot>("observer-health-summary-v1.schema.json"),
        document::<CursorSnapshot>("observer-cursor-v1.schema.json"),
        document::<SelectionSnapshot>("observer-selection-v1.schema.json"),
        document::<IndicatorsSnapshot>("observer-analysis-indicators-v1.schema.json"),
        document::<DrawingsSnapshot>("observer-analysis-drawings-v1.schema.json"),
        document::<TapeSnapshot>("observer-orderflow-tape-v1.schema.json"),
        document::<FootprintSnapshot>("observer-orderflow-footprint-v1.schema.json"),
        document::<BubblesSnapshot>("observer-orderflow-bubbles-v1.schema.json"),
        document::<HeatmapSnapshot>("observer-orderflow-heatmap-v1.schema.json"),
        document::<L2Snapshot>("observer-orderflow-l2-v1.schema.json"),
        document::<ReplaySnapshot>("observer-session-replay-v1.schema.json"),
        document::<PaperSnapshot>("observer-session-paper-v1.schema.json"),
        document::<SceneSnapshot>("observer-scene-controls-v1.schema.json"),
        document::<ChartWindowQuery>("observer-chart-window-query-v1.schema.json"),
        document::<ChartWindowPage>("observer-chart-window-page-v1.schema.json"),
        document::<EventsReadInput>("observer-events-read-input-v1.schema.json"),
        document::<EventsWaitInput>("observer-events-wait-input-v1.schema.json"),
        document::<EventPage>("observer-event-page-v1.schema.json"),
        document::<EvidenceCaptureInput>("evidence-capture-input-v1.schema.json"),
        document::<EvidenceManifest>("evidence-manifest-v1.schema.json"),
        document::<EvidenceReadInput>("evidence-read-input-v1.schema.json"),
        document::<EvidenceChunkPage>("evidence-chunk-page-v1.schema.json"),
        // The shape the chunks reassemble into, so a client can generate a
        // reader for a bundle rather than reverse-engineering one.
        document::<EvidenceDocument>("evidence-bundle-v1.schema.json"),
        document::<MarkInput>("attention-mark-input-v1.schema.json"),
        document::<MarkResult>("attention-mark-result-v1.schema.json"),
        document::<AnnotationInput>("annotate-object-input-v1.schema.json"),
        document::<AnnotationResult>("annotate-object-result-v1.schema.json"),
        document::<RemoveInput>("annotate-remove-input-v1.schema.json"),
        document::<RemoveResult>("annotate-remove-result-v1.schema.json"),
        document::<NotifyInput>("notify-input-v1.schema.json"),
        document::<NotifyResult>("notify-result-v1.schema.json"),
        document::<AttachInput>("indicator-script-attach-input-v1.schema.json"),
        document::<AttachResult>("indicator-script-attach-result-v1.schema.json"),
        document::<DetachInput>("indicator-script-detach-input-v1.schema.json"),
        document::<DetachResult>("indicator-script-detach-result-v1.schema.json"),
        // The shape a failed compile puts in `error.context.details`, so a
        // client can generate a reader for its own diagnostics.
        document::<ScriptDiagnostic>("indicator-script-diagnostic-v1.schema.json"),
    ]
}

pub(crate) fn capability_catalog() -> Value {
    let projections = super::standard_registry().expect("built-in projection registry is valid");
    let actions = super::actions::standard_actions().expect("action registry is valid");
    let contract = ObserverContract::new(
        &projections,
        std::sync::Arc::new(actions),
        super::evidence::EvidenceStore::new(),
    )
    .expect("observer contract is valid");
    let default_scopes = contract.default_grant();
    let description = contract.describe(
        quantick_control::id::InstanceId::from_bytes([0; 16]),
        quantick_control::id::ProfileId::new(super::contract::OBSERVER_PROFILE_ID)
            .expect("static observer profile is valid"),
        default_scopes.clone(),
        quantick_control::handshake::ProtocolLimits::default(),
    );
    json!({
        "catalog_version": 1,
        "profile_id": super::contract::OBSERVER_PROFILE_ID,
        "default_scopes": default_scopes,
        "modules": description.modules,
        "profiles": description.profiles,
        "permissions": description.permissions,
        "capabilities": description.capabilities,
        "snapshot_scopes": description.snapshot_scopes,
    })
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> SchemaDocument {
    SchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}
