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
use quantick_control_schema::drawing_magnet::{
    DrawingMagnetInput, DrawingMagnetResult, ObjectMagnetInput, ObjectMagnetResult,
};
use quantick_control_schema::drawing_scope::{DrawingScopeInput, DrawingScopeResult};
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
        document::<EmptyInput>("observer-empty-input-v1.schema.json"),
        document::<SnapshotReadInput>("observer-snapshot-read-input-v1.schema.json"),
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
        document::<quantick_control_schema::orderflow_settings::SettingsSnapshot>(
            "observer-orderflow-settings-v1.schema.json",
        ),
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
        document::<DrawingScopeInput>("annotate-drawing-scope-input-v1.schema.json"),
        document::<DrawingScopeResult>("annotate-drawing-scope-result-v1.schema.json"),
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
        quantick_control_schema::catalog::documents()
            .into_iter()
            .map(|(file_name, schema)| SchemaDocument { file_name, schema }),
    );
    documents
}

/// Each referenced capability field, and the word its own file is named with.
const REFERENCED_SCHEMA_FIELDS: [(&str, &str); 2] =
    [("input_schema", "input"), ("output_schema", "result")];

/// A schema no published document carries, committed in a file named after
/// the call that declares it.
#[derive(Debug, PartialEq)]
pub struct CallSchema {
    pub file_name: String,
    pub schema: Value,
}

/// Why a catalog's schemas could not all be committed as files.
#[derive(Debug, PartialEq, Eq)]
pub enum FoldError {
    /// The capability at this position has no string `id` or integer
    /// `version` to name its file after.
    UnnamedCapability { index: usize },
    /// Two different schemas would be committed under this one file name.
    DuplicateFileName(String),
}

/// Replaces every capability schema in `catalog` with
/// `{"$ref": "<file_name>"}`, so the catalog inlines none. The reference is a
/// bare file name: callers write the files beside the catalog, as
/// `schemas/control` holds both.
///
/// A schema equal to a published document references it. When several
/// documents carry the same schema, the lowest file name is referenced, so the
/// result does not depend on registration order. Any other schema gets a file
/// of its own, `<id>-input-v<version>.schema.json` or
/// `<id>-result-v<version>.schema.json` with the id's `.` and `_` as `-`, and
/// is returned so the caller commits it.
pub fn fold_schema_refs<'a>(
    catalog: &mut Value,
    documents: impl IntoIterator<Item = (&'a str, &'a Value)>,
) -> Result<Vec<CallSchema>, FoldError> {
    let mut documents: Vec<_> = documents.into_iter().collect();
    documents.sort_by_key(|(file_name, _)| *file_name);
    let mut taken: std::collections::BTreeSet<String> = documents
        .iter()
        .map(|(file_name, _)| (*file_name).to_owned())
        .collect();
    let mut calls = Vec::new();
    let Some(capabilities) = catalog
        .get_mut("capabilities")
        .and_then(Value::as_array_mut)
    else {
        return Ok(calls);
    };
    for (index, capability) in capabilities.iter_mut().enumerate() {
        let stem = call_file_stem(capability);
        for (field, kind) in REFERENCED_SCHEMA_FIELDS {
            let Some(schema) = capability.get_mut(field) else {
                continue;
            };
            let file_name = match documents.iter().find(|(_, document)| *document == schema) {
                Some((file_name, _)) => (*file_name).to_owned(),
                None => {
                    let (id, version) = stem
                        .as_ref()
                        .ok_or(FoldError::UnnamedCapability { index })?;
                    let file_name = format!("{id}-{kind}-{version}.schema.json");
                    if !taken.insert(file_name.clone()) {
                        return Err(FoldError::DuplicateFileName(file_name));
                    }
                    calls.push(CallSchema {
                        file_name: file_name.clone(),
                        schema: schema.take(),
                    });
                    file_name
                }
            };
            *schema = serde_json::json!({ "$ref": file_name });
        }
    }
    Ok(calls)
}

/// The id with `.` and `_` as `-`, and `v<version>`, for a call's own files.
fn call_file_stem(capability: &Value) -> Option<(String, String)> {
    let id = capability.get("id")?.as_str()?;
    let version = capability.get("version")?.as_u64()?;
    Some((id.replace(['.', '_'], "-"), format!("v{version}")))
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> SchemaDocument {
    SchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}

#[cfg(test)]
#[path = "schema_documents_tests.rs"]
mod tests;
