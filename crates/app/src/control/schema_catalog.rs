//! Generated observer contracts committed under `schemas/control`.

use quantick_control_handlers::schema_documents::SchemaDocument;
use serde_json::{Value, json};

use super::contract::ObserverContract;

/// The shared documents, compared to this application's live registry below.
pub(crate) fn documents() -> Vec<SchemaDocument> {
    quantick_control_handlers::generation::documents()
}

pub(crate) fn capability_catalog() -> Value {
    let projections = super::standard_registry().expect("built-in projection registry is valid");
    let actions = super::actions::standard_actions().expect("action registry is valid");
    let contract = ObserverContract::new(
        &projections,
        std::sync::Arc::new(actions),
        super::evidence::EvidenceStore::new(),
        crate::control::system::BUILD,
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
