//! Generated observer contracts committed under `schemas/control`.

use quantick_control::schema::generated_schema;
use serde_json::{Value, json};

use quantick_control_handlers::attention::{MarkInput, MarkResult};

use super::contract::ObserverContract;

pub(crate) struct SchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

pub(crate) fn documents() -> Vec<SchemaDocument> {
    let mut documents = vec![
        document::<MarkInput>("attention-mark-input-v1.schema.json"),
        document::<MarkResult>("attention-mark-result-v1.schema.json"),
    ];
    documents.extend(
        quantick_control_schema::catalog::documents()
            .into_iter()
            .chain(quantick_control_schema::bubble_preset::documents())
            .chain(quantick_control_schema::appearance::documents())
            .map(|(file_name, schema)| SchemaDocument { file_name, schema }),
    );
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

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> SchemaDocument {
    SchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}
