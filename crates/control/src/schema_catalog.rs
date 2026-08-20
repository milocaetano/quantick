//! Generated public wire contracts committed under `schemas/control`.

use serde_json::Value;

use crate::{
    cursor::{EventCursor, PageCursor},
    error::ControlError,
    fake::reference_registry,
    handshake::{HandshakeRequest, HandshakeResponse},
    registry::CapabilityDescriptor,
    schema::generated_schema,
    wire::{RequestEnvelope, ResponseEnvelope},
};

#[derive(Clone, Debug)]
pub struct PublicSchemaDocument {
    pub file_name: &'static str,
    pub schema: Value,
}

#[derive(Clone, Debug)]
pub struct PublicContractDocument {
    pub file_name: &'static str,
    pub document: Value,
    pub is_json_schema: bool,
}

pub fn public_schema_documents() -> Vec<PublicSchemaDocument> {
    vec![
        document::<CapabilityDescriptor>("capability-descriptor-v1.schema.json"),
        document::<ControlError>("control-error-v1.schema.json"),
        document::<EventCursor>("event-cursor-v1.schema.json"),
        document::<HandshakeRequest>("handshake-request-v1.schema.json"),
        document::<HandshakeResponse>("handshake-response-v1.schema.json"),
        document::<PageCursor>("page-cursor-v1.schema.json"),
        document::<RequestEnvelope>("request-envelope-v1.schema.json"),
        document::<ResponseEnvelope>("response-envelope-v1.schema.json"),
    ]
}

pub fn public_contract_documents() -> Vec<PublicContractDocument> {
    let mut documents = public_schema_documents()
        .into_iter()
        .map(|schema| PublicContractDocument {
            file_name: schema.file_name,
            document: schema.schema,
            is_json_schema: true,
        })
        .collect::<Vec<_>>();
    let registry = reference_registry().expect("reference capability registry must be valid");
    documents.push(PublicContractDocument {
        file_name: "reference-capabilities-v1.json",
        document: serde_json::json!({
            "catalog_version": 1,
            "capabilities": registry.capabilities().collect::<Vec<_>>(),
        }),
        is_json_schema: false,
    });
    documents
}

fn document<T: schemars::JsonSchema>(file_name: &'static str) -> PublicSchemaDocument {
    PublicSchemaDocument {
        file_name,
        schema: generated_schema::<T>(),
    }
}
