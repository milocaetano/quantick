//! The published JSON Schema documents of the contracts this crate and those below
//! it own, committed under `schemas/control`. The host adds the few it owns itself.

use quantick_control::annotation::{AnnotationInput, AnnotationResult, RemoveInput, RemoveResult};
use quantick_control::schema::generated_schema;
use quantick_control_host::authority::{DescribeResult, EmptyInput, SnapshotReadInput};
use quantick_control_host::catalogue::SnapshotScopeDescriptor;
use quantick_control_host::events::{EventsReadInput, EventsWaitInput};
use quantick_control_host::evidence::EvidenceChunkPage;
use quantick_control_host::journal::EventPage;
use quantick_control_host::projection::SerializedSnapshotCapture;
use quantick_control_host::system::SystemSnapshot;
use serde_json::Value;

use crate::analysis::{DrawingsSnapshot, IndicatorsSnapshot};
use crate::chart::{ChartSnapshot, ChartWindowPage, ChartWindowQuery};
use crate::drawing_magnet::{
    DrawingMagnetInput, DrawingMagnetResult, ObjectMagnetInput, ObjectMagnetResult,
};
use crate::evidence::{
    EvidenceCaptureInput, EvidenceDocument, EvidenceManifest, EvidenceReadInput,
};
use crate::feed::FeedSnapshot;
use crate::health::HealthSnapshot;
use crate::indicator_guide::{IndicatorGuideInput, IndicatorGuideResult};
use crate::interaction::{CursorSnapshot, SelectionSnapshot};
use crate::layers::{LayersSnapshot, SetVisibilityInput, VisibilityResult};
use crate::layout_v2::{LayoutResultV2, ResizeInputV2};
use crate::notify::{NotifyInput, NotifyResult};
use crate::orderflow::{
    BubblesSnapshot, FootprintSnapshot, HeatmapSnapshot, L2Snapshot, TapeSnapshot,
};
use crate::scene::SceneSnapshot;
use crate::script::{AttachInput, AttachResult, DetachInput, DetachResult, ScriptDiagnostic};
use crate::session::{PaperSnapshot, ReplaySnapshot};
use crate::tape_view::{TapeViewInput, TapeViewResult};
use crate::workspace::WorkspaceSnapshot;

/// Each document's file name and its schema.
#[must_use]
pub fn documents() -> Vec<(&'static str, Value)> {
    let mut documents = vec![
        document::<crate::indicator_picker::PickerInput>("indicator-picker-input-v1.schema.json"),
        document::<crate::indicator_picker::PickerResult>("indicator-picker-result-v1.schema.json"),
        document::<EmptyInput>("observer-empty-input-v1.schema.json"),
        document::<SnapshotReadInput>("observer-snapshot-read-input-v1.schema.json"),
        document::<crate::chart::ChartWindowInput>("observer-chart-window-input-v1.schema.json"),
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
        document::<crate::price_axis::PriceAxisInput>("chart-price-axis-input-v1.schema.json"),
        document::<crate::price_axis::PriceAxisResult>("chart-price-axis-result-v1.schema.json"),
        document::<crate::chart_settings::ChartSettingsInput>(
            "chart-settings-input-v1.schema.json",
        ),
        document::<crate::chart_settings::ChartSettingsResult>(
            "chart-settings-result-v1.schema.json",
        ),
        document::<crate::journal_sessions::JournalSessionsInput>(
            "session-journal-input-v1.schema.json",
        ),
        document::<crate::journal_sessions::JournalSessionsResult>(
            "session-journal-result-v1.schema.json",
        ),
        document::<crate::journal_sessions::JournalDeleteInput>(
            "session-journal-delete-input-v1.schema.json",
        ),
        document::<TapeViewInput>("chart-tape-view-input-v1.schema.json"),
        document::<TapeViewResult>("chart-tape-view-result-v1.schema.json"),
        document::<crate::time_axis::TimeAxisResetInput>(
            "chart-time-axis-reset-input-v1.schema.json",
        ),
        document::<crate::time_axis::TimeAxisResetResult>(
            "chart-time-axis-reset-result-v1.schema.json",
        ),
        document::<crate::opening_scale::OpeningScaleInput>(
            "orderflow-opening-scale-input-v1.schema.json",
        ),
        document::<crate::opening_scale::OpeningScaleResult>(
            "orderflow-opening-scale-result-v1.schema.json",
        ),
        document::<crate::bubble_scale::BubbleScaleInput>(
            "orderflow-bubble-scale-input-v1.schema.json",
        ),
        document::<crate::bubble_scale::BubbleScaleResult>(
            "orderflow-bubble-scale-result-v1.schema.json",
        ),
        document::<crate::history::HistoryLoadInput>("feed-history-load-input-v1.schema.json"),
        document::<crate::feed_source::FeedSourceInput>("feed-source-input-v1.schema.json"),
        document::<crate::feed_source::FeedSourceResult>("feed-source-result-v1.schema.json"),
        document::<crate::history::HistoryLoadResult>("feed-history-load-result-v1.schema.json"),
        document::<crate::history::HistoryCancelInput>("feed-history-cancel-input-v1.schema.json"),
        document::<crate::history::HistoryCancelResult>(
            "feed-history-cancel-result-v1.schema.json",
        ),
        document::<HeatmapSnapshot>("observer-orderflow-heatmap-v1.schema.json"),
        document::<L2Snapshot>("observer-orderflow-l2-v1.schema.json"),
        document::<crate::orderflow_settings::SettingsSnapshot>(
            "observer-orderflow-settings-v1.schema.json",
        ),
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
        document::<IndicatorGuideInput>("indicator-mouse-vertical-line-input-v1.schema.json"),
        document::<IndicatorGuideResult>("indicator-mouse-vertical-line-result-v1.schema.json"),
        document::<DrawingMagnetInput>("annotate-magnet-input-v1.schema.json"),
        document::<DrawingMagnetResult>("annotate-magnet-result-v1.schema.json"),
        document::<ObjectMagnetInput>("annotate-drawing-magnet-input-v1.schema.json"),
        document::<ObjectMagnetResult>("annotate-drawing-magnet-result-v1.schema.json"),
        // The shape a failed compile puts in `error.context.details`, so a
        // client can generate a reader for its own diagnostics.
        document::<ScriptDiagnostic>("indicator-script-diagnostic-v1.schema.json"),
        // Version 2 of the layout calls that answer with the arrangement: the
        // share as an exact decimal, which version 1's number could not carry
        // across the wire. Version 1's schemas were never published as files.
        document::<LayoutResultV2>("layout-result-v2.schema.json"),
        document::<ResizeInputV2>("layout-resize-input-v2.schema.json"),
        document::<LayersSnapshot>("observer-layers-visibility-v1.schema.json"),
        document::<SetVisibilityInput>("layers-visibility-set-input-v1.schema.json"),
        document::<VisibilityResult>("layers-visibility-set-result-v1.schema.json"),
    ];
    documents.extend(crate::script_editor::documents());
    documents
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> (&'static str, Value) {
    (file_name, generated_schema::<T>())
}
