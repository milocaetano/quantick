//! Generated observer contracts committed under `schemas/control`.

use quantick_control::schema::generated_schema;
use serde_json::Value;

use super::{
    chart::{ChartSnapshot, ChartWindowPage, ChartWindowQuery},
    feed::FeedSnapshot,
    health::HealthSnapshot,
    interaction::{CursorSnapshot, SelectionSnapshot},
    registry::SerializedSnapshotCapture,
    system::SystemSnapshot,
    workspace::WorkspaceSnapshot,
};

pub(crate) struct SchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

pub(crate) fn documents() -> Vec<SchemaDocument> {
    vec![
        document::<SerializedSnapshotCapture>("observer-snapshot-capture-v1.schema.json"),
        document::<SystemSnapshot>("observer-system-info-v1.schema.json"),
        document::<WorkspaceSnapshot>("observer-workspace-summary-v1.schema.json"),
        document::<FeedSnapshot>("observer-feed-status-v1.schema.json"),
        document::<ChartSnapshot>("observer-chart-summary-v1.schema.json"),
        document::<HealthSnapshot>("observer-health-summary-v1.schema.json"),
        document::<CursorSnapshot>("observer-cursor-v1.schema.json"),
        document::<SelectionSnapshot>("observer-selection-v1.schema.json"),
        document::<ChartWindowQuery>("observer-chart-window-query-v1.schema.json"),
        document::<ChartWindowPage>("observer-chart-window-page-v1.schema.json"),
    ]
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> SchemaDocument {
    SchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}
