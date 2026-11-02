//! Generated observer contracts committed under `schemas/control`: the list of
//! documents whose types live below the app.

use quantick_control::annotation::{AnnotationInput, AnnotationResult, RemoveInput, RemoveResult};
use quantick_control::schema::generated_schema;
use quantick_control_host::authority::{DescribeResult, EmptyInput, SnapshotReadInput};
use quantick_control_host::catalogue::SnapshotScopeDescriptor;
use quantick_control_host::events::{EventsReadInput, EventsWaitInput};
use quantick_control_host::journal::EventPage;
use quantick_control_host::projection::SerializedSnapshotCapture;
use quantick_control_host::system::SystemSnapshot;
use quantick_control_schema::analysis::{DrawingsSnapshot, IndicatorsSnapshot};
use quantick_control_schema::chart::{ChartSnapshot, ChartWindowPage, ChartWindowQuery};
use quantick_control_schema::desk::{
    DeskInput, DeskPickResult, DeskReopenInput, DeskReopenResult, DeskWriteResult,
    SettingsOpenInput, SettingsOpenResult,
};
use quantick_control_schema::drawing_magnet::{
    DrawingMagnetInput, DrawingMagnetResult, ObjectMagnetInput, ObjectMagnetResult,
};
use quantick_control_schema::evidence::{
    EvidenceCaptureInput, EvidenceChunkPage, EvidenceDocument, EvidenceManifest, EvidenceReadInput,
};
use quantick_control_schema::feed::FeedSnapshot;
use quantick_control_schema::health::HealthSnapshot;
use quantick_control_schema::indicator_guide::{IndicatorGuideInput, IndicatorGuideResult};
use quantick_control_schema::interaction::{CursorSnapshot, SelectionSnapshot};
use quantick_control_schema::layers::{LayersSnapshot, SetVisibilityInput, VisibilityResult};
use quantick_control_schema::layout_v2::{LayoutResultV2, ResizeInputV2};
use quantick_control_schema::notify::{NotifyInput, NotifyResult};
use quantick_control_schema::orderflow::{
    BubblesSnapshot, FootprintSnapshot, HeatmapSnapshot, L2Snapshot, TapeSnapshot,
};
use quantick_control_schema::scene::SceneSnapshot;
use quantick_control_schema::script::{
    AttachInput, AttachResult, DetachInput, DetachResult, ScriptDiagnostic,
};
use quantick_control_schema::session::{PaperSnapshot, ReplaySnapshot};
use quantick_control_schema::tape_view::{TapeViewInput, TapeViewResult};
use quantick_control_schema::workspace::WorkspaceSnapshot;
use serde_json::Value;

use crate::attention::{MarkInput, MarkResult};

/// One file under `schemas/control`.
pub struct SchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

/// Every published document whose type lives below the app.
#[must_use]
pub fn documents() -> Vec<SchemaDocument> {
    let mut documents = vec![
        document::<SettingsOpenInput>("settings-open-input-v1.schema.json"),
        document::<SettingsOpenResult>("settings-open-result-v1.schema.json"),
        document::<DeskInput>("desk-input-v1.schema.json"),
        document::<DeskWriteResult>("desk-write-result-v1.schema.json"),
        document::<DeskPickResult>("desk-pick-result-v1.schema.json"),
        document::<DeskReopenInput>("desk-reopen-input-v1.schema.json"),
        document::<DeskReopenResult>("desk-reopen-result-v1.schema.json"),
        document::<quantick_control_schema::native_window::WindowInput>(
            "native-window-input-v1.schema.json",
        ),
        document::<quantick_control_schema::native_window::WindowResult>(
            "native-window-result-v1.schema.json",
        ),
        document::<quantick_control_schema::home_tab::HomeShowInput>(
            "home-show-input-v1.schema.json",
        ),
        document::<quantick_control_schema::home_tab::HomeShowResult>(
            "home-show-result-v1.schema.json",
        ),
        document::<quantick_control_schema::home_tab::HomeLeaveInput>(
            "home-leave-input-v1.schema.json",
        ),
        document::<quantick_control_schema::home_tab::HomeLeaveResult>(
            "home-leave-result-v1.schema.json",
        ),
        document::<quantick_control_schema::indicator_picker::PickerInput>(
            "indicator-picker-input-v1.schema.json",
        ),
        document::<quantick_control_schema::indicator_picker::PickerResult>(
            "indicator-picker-result-v1.schema.json",
        ),
        document::<EmptyInput>("observer-empty-input-v1.schema.json"),
        document::<SnapshotReadInput>("observer-snapshot-read-input-v1.schema.json"),
        document::<quantick_control_schema::scene::SceneReadInput>(
            "observer-scene-read-input-v1.schema.json",
        ),
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
        document::<quantick_control_schema::price_axis::PriceAxisInput>(
            "chart-price-axis-input-v1.schema.json",
        ),
        document::<quantick_control_schema::price_axis::PriceAxisResult>(
            "chart-price-axis-result-v1.schema.json",
        ),
        document::<quantick_control_schema::chart_settings::ChartSettingsInput>(
            "chart-settings-input-v1.schema.json",
        ),
        document::<quantick_control_schema::chart_settings::ChartSettingsResult>(
            "chart-settings-result-v1.schema.json",
        ),
        document::<quantick_control_schema::journal_sessions::JournalSessionsInput>(
            "session-journal-input-v1.schema.json",
        ),
        document::<quantick_control_schema::journal_sessions::JournalSessionsResult>(
            "session-journal-result-v1.schema.json",
        ),
        document::<quantick_control_schema::journal_sessions::JournalDeleteInput>(
            "session-journal-delete-input-v1.schema.json",
        ),
        document::<TapeViewInput>("chart-tape-view-input-v1.schema.json"),
        document::<TapeViewResult>("chart-tape-view-result-v1.schema.json"),
        document::<quantick_control_schema::time_axis::TimeAxisResetInput>(
            "chart-time-axis-reset-input-v1.schema.json",
        ),
        document::<quantick_control_schema::time_axis::TimeAxisResetResult>(
            "chart-time-axis-reset-result-v1.schema.json",
        ),
        document::<quantick_control_schema::opening_scale::OpeningScaleInput>(
            "orderflow-opening-scale-input-v1.schema.json",
        ),
        document::<quantick_control_schema::opening_scale::OpeningScaleResult>(
            "orderflow-opening-scale-result-v1.schema.json",
        ),
        document::<quantick_control_schema::bubble_scale::BubbleScaleInput>(
            "orderflow-bubble-scale-input-v1.schema.json",
        ),
        document::<quantick_control_schema::bubble_scale::BubbleScaleResult>(
            "orderflow-bubble-scale-result-v1.schema.json",
        ),
        document::<quantick_control_schema::history::HistoryLoadInput>(
            "feed-history-load-input-v1.schema.json",
        ),
        document::<quantick_control_schema::feed_source::FeedSourceInput>(
            "feed-source-input-v1.schema.json",
        ),
        document::<quantick_control_schema::feed_source::FeedSourceResult>(
            "feed-source-result-v1.schema.json",
        ),
        document::<quantick_control_schema::history::HistoryLoadResult>(
            "feed-history-load-result-v1.schema.json",
        ),
        document::<quantick_control_schema::history::HistoryCancelInput>(
            "feed-history-cancel-input-v1.schema.json",
        ),
        document::<quantick_control_schema::history::HistoryCancelResult>(
            "feed-history-cancel-result-v1.schema.json",
        ),
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
    documents.extend(
        quantick_control_schema::market_windows::documents()
            .into_iter()
            .map(|(file_name, schema)| SchemaDocument { file_name, schema }),
    );
    documents.extend(
        quantick_control_schema::bubble_preset::documents()
            .into_iter()
            .map(|(file_name, schema)| SchemaDocument { file_name, schema }),
    );
    documents
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> SchemaDocument {
    SchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}
