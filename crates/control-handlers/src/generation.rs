//! Deterministic control artifacts assembled from the same metadata as the live host.
use quantick_control::{
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::{InstanceId, ProfileId},
};
use quantick_control_host::authority::{self, DescribeResult};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub use crate::schema_documents::SchemaDocument;
pub use quantick_control_host::system::BuildIdentity;
pub(crate) mod composition;
pub mod feed;
pub mod interaction;
pub mod scene;
pub mod system;
pub mod workspace;

#[must_use]
pub fn documents() -> Vec<SchemaDocument> {
    crate::schema_documents::documents()
}

/// A complete discovery response, including the explicit build identity supplied by the caller.
pub fn describe(build: BuildIdentity) -> Result<DescribeResult, String> {
    let contract = composition::contract().map_err(|error| error.to_string())?;
    Ok(DescribeResult {
        instance_id: InstanceId::from_bytes([0; 16]),
        application_version: build.application_version.into(),
        application_commit: build.git_commit.unwrap_or("unknown").into(),
        protocol_version: CURRENT_PROTOCOL_VERSION,
        effective_profile: ProfileId::new(authority::OBSERVER_PROFILE_ID)
            .expect("static profile ID is valid"),
        effective_scopes: authority::default_grant(),
        effective_limits: ProtocolLimits::default(),
        modules: contract.registry().modules().cloned().collect(),
        profiles: contract.profiles().to_vec(),
        permissions: contract.permissions().to_vec(),
        capabilities: contract.registry().capabilities().cloned().collect(),
        snapshot_scopes: contract.snapshot_scopes().to_vec(),
    })
}

pub fn capability_catalog(build: BuildIdentity) -> Result<Value, String> {
    let description = describe(build)?;
    Ok(json!({
        "catalog_version": 1, "profile_id": authority::OBSERVER_PROFILE_ID,
        "default_scopes": description.effective_scopes,
        "modules": description.modules, "profiles": description.profiles,
        "permissions": description.permissions, "capabilities": description.capabilities,
        "snapshot_scopes": description.snapshot_scopes,
    }))
}

pub fn capability_inventory_markdown() -> Result<String, String> {
    let contract = composition::contract().map_err(|error| error.to_string())?;
    Ok(quantick_control_host::inventory::render(
        &contract.registry().capabilities().collect::<Vec<_>>(),
    ))
}

pub fn retry_matrix_markdown() -> Result<String, String> {
    let contract = composition::contract().map_err(|error| error.to_string())?;
    let findings =
        crate::retry_matrix::drift(quantick_control_schema::retry_matrix::READBACKS, &contract);
    if !findings.is_empty() {
        return Err(format!(
            "the retry matrix disagrees with the registry:\n  {}",
            findings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  ")
        ));
    }
    Ok(crate::retry_matrix::render(&contract))
}

#[derive(Debug, Eq, PartialEq)]
pub struct Artifact {
    pub path: String,
    pub contents: String,
}

/// Every owned control artifact, sorted by repository-relative path, with no duplicates.
pub fn artifacts(build: BuildIdentity) -> Result<Vec<Artifact>, String> {
    let mut artifacts = Vec::new();
    for document in quantick_control::schema_catalog::public_contract_documents() {
        artifacts.push(json_artifact(document.file_name, &document.document)?);
    }
    for document in documents() {
        artifacts.push(json_artifact(document.file_name, &document.schema)?);
    }
    artifacts.push(json_artifact(
        "observer-capability-catalog-v1.json",
        &capability_catalog(build)?,
    )?);
    artifacts.push(Artifact {
        path: "docs/control-plane/capability-inventory.md".into(),
        contents: capability_inventory_markdown()?,
    });
    artifacts.push(Artifact {
        path: "docs/control-plane/retry-matrix.md".into(),
        contents: retry_matrix_markdown()?,
    });
    validate_artifacts(&mut artifacts)?;
    Ok(artifacts)
}

fn json_artifact(name: &str, value: &Value) -> Result<Artifact, String> {
    Ok(Artifact {
        path: format!("schemas/control/{name}"),
        contents: format!(
            "{}\n",
            serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
        ),
    })
}

fn validate_artifacts(artifacts: &mut [Artifact]) -> Result<(), String> {
    let mut paths = BTreeSet::new();
    for artifact in artifacts.iter() {
        if !paths.insert(&artifact.path) {
            return Err(format!("duplicate generated path: {}", artifact.path));
        }
    }
    artifacts.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
