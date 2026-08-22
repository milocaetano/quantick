//! Generated observer contracts committed under `schemas/control`.

use quantick_control::schema::generated_schema;
use serde_json::{Value, json};

use super::{
    chart::{ChartSnapshot, ChartWindowPage, ChartWindowQuery},
    contract::{
        ChartWindowInput, DescribeResult, EmptyInput, ObserverContract, SnapshotReadInput,
        SnapshotScopeDescriptor,
    },
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
        document::<ChartWindowQuery>("observer-chart-window-query-v1.schema.json"),
        document::<ChartWindowPage>("observer-chart-window-page-v1.schema.json"),
    ]
}

pub(crate) fn capability_catalog() -> Value {
    let projections = super::standard_registry().expect("built-in projection registry is valid");
    let contract = ObserverContract::new(&projections).expect("observer contract is valid");
    let default_scopes = contract.default_grant();
    let description = contract.describe(
        quantick_control::id::InstanceId::from_bytes([0; 16]),
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
