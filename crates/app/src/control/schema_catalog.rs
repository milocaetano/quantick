//! Generated observer contracts committed under `schemas/control`.

use quantick_control::schema::generated_schema;
use quantick_control_handlers::schema_documents::SchemaDocument;
use serde_json::{Value, json};

use super::contract::{ChartWindowInput, ObserverContract};

/// The documents below the app, and the one whose type only the app owns.
pub(crate) fn documents() -> Vec<SchemaDocument> {
    let mut documents = quantick_control_handlers::schema_documents::documents();
    documents.push(SchemaDocument {
        file_name: "observer-chart-window-input-v1.schema.json",
        schema: generated_schema::<ChartWindowInput>(),
    });
    documents
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
