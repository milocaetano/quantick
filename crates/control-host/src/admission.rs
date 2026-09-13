//! Capability admission: whether one request may reach a handler at all.
//!
//! The host's contract registers its capabilities here — each with the input
//! and output schemas compiled once — and asks, for every request, the same
//! questions in the same order: is the envelope well formed, is the
//! capability registered, does the connection hold its permissions
//! ([`admit_capability`]); does the payload match the schema, is the
//! idempotency key allowed, does this tier accept a dry run or an expected
//! revision ([`admit_payload`]); and, once a handler has named the snapshot
//! scopes the request reaches, are those granted too ([`admit_scopes`]).
//!
//! What is *not* here is the authority table itself — which profiles,
//! permissions, effects and capabilities exist — and the handlers. Those are
//! the application's: they name its modules. This module is the machinery the
//! table feeds, so the order of the checks is written once and cannot drift
//! between hosts.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, ErrorCode, ModuleId, PermissionId, SnapshotScopeId},
    limits::CONTROL_MAX_SNAPSHOT_SCOPES,
    registry::{
        CapabilityDescriptor, ControlRegistry, PermissionDescriptor, RegistryError,
        check_idempotency_key,
    },
    schema::CompiledSchema,
    wire::RequestEnvelope,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::projection::ProjectionRegistry;

/// Compiled schemas by capability ID and version.
pub type CompiledCapabilitySchemas = BTreeMap<CapabilityId, BTreeMap<u32, CompiledSchema>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotScopeDescriptor {
    pub id: SnapshotScopeId,
    pub module_id: ModuleId,
    pub schema_version: u32,
    pub title: String,
    pub description: String,
    pub required_permissions: BTreeSet<PermissionId>,
    pub schema: Value,
}

/// One `control.*` failure, built from a code the contract declares. Every
/// host surface answers with the same shape, so a client never has to guess
/// which one refused it.
pub fn known_error(code: &str, message: impl AsRef<str>, retryable: bool) -> ControlError {
    ControlError::new(
        ErrorCode::new(code).expect("static error code is valid"),
        message.as_ref(),
        retryable,
    )
}

/// Register one capability, its handler and its compiled schemas together, so
/// a capability can never be registered without the validators `admit_payload`
/// and [`output_is_valid`] need. `P` is whatever the host dispatches to.
pub fn register_capability<P>(
    registry: &mut ControlRegistry,
    handlers: &mut BTreeMap<(CapabilityId, u32), P>,
    input_validators: &mut CompiledCapabilitySchemas,
    output_validators: &mut CompiledCapabilitySchemas,
    descriptor: CapabilityDescriptor,
    handler: P,
) -> Result<(), RegistryError> {
    let key = (descriptor.id.clone(), descriptor.version);
    let input_validator = CompiledSchema::new(&descriptor.input_schema).map_err(|error| {
        RegistryError::InvalidDescriptor(format!(
            "capability `{}` input schema is invalid: {error}",
            descriptor.id
        ))
    })?;
    let output_validator = CompiledSchema::new(&descriptor.output_schema).map_err(|error| {
        RegistryError::InvalidDescriptor(format!(
            "capability `{}` output schema is invalid: {error}",
            descriptor.id
        ))
    })?;
    registry.register_capability(descriptor)?;
    let previous = handlers.insert(key.clone(), handler);
    debug_assert!(
        previous.is_none(),
        "registry rejected duplicate capability IDs"
    );
    input_validators
        .entry(key.0.clone())
        .or_default()
        .insert(key.1, input_validator);
    output_validators
        .entry(key.0)
        .or_default()
        .insert(key.1, output_validator);
    Ok(())
}

/// The first three checks: the envelope, the registration, the permissions
/// the descriptor requires.
pub fn admit_capability<'a>(
    registry: &'a ControlRegistry,
    envelope: &RequestEnvelope,
    effective_scopes: &BTreeSet<PermissionId>,
) -> Result<&'a CapabilityDescriptor, ControlError> {
    envelope.validate()?;
    let descriptor = registry
        .capability(&envelope.capability_id, envelope.capability_version)
        .ok_or_else(|| {
            ControlError::new(
                ErrorCode::new(codes::CAPABILITY_UNKNOWN).expect("static error code is valid"),
                "capability ID or version is not registered",
                false,
            )
        })?;
    if !descriptor.required_permissions.is_subset(effective_scopes) {
        return Err(known_error(
            codes::PERMISSION_DENIED,
            "connection lacks a required capability permission",
            false,
        ));
    }
    Ok(descriptor)
}

/// The next three: the payload against its schema, the idempotency key
/// against the descriptor's policy, and this tier's refusal of dry runs and
/// expected revisions.
///
/// `own_validator` is a schema the host holds for this capability outside the
/// compiled table — an action validates against the very schema its hotkey
/// passes, so the two paths cannot drift. Without one, the capability must
/// have been registered through [`register_capability`].
pub fn admit_payload(
    descriptor: &CapabilityDescriptor,
    envelope: &RequestEnvelope,
    own_validator: Option<&CompiledSchema>,
    input_validators: &CompiledCapabilitySchemas,
) -> Result<(), ControlError> {
    let validator = match own_validator {
        Some(validator) => validator,
        None => input_validators
            .get(&descriptor.id)
            .and_then(|versions| versions.get(&descriptor.version))
            .ok_or_else(|| {
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "registered observer capability has no input validator",
                    false,
                )
            })?,
    };
    validator
        .validate(&envelope.payload)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let carries_key = envelope.idempotency_key.is_some();
    check_idempotency_key(descriptor.idempotency, carries_key, envelope.dry_run)?;
    if envelope.dry_run || !envelope.expected_revisions.is_empty() {
        return Err(ControlError::invalid_request(
            "this tier's capabilities forbid dry runs and expected revisions",
        ));
    }
    Ok(())
}

/// The last check, after a handler has named the snapshot scopes a request
/// reaches: every permission they need must be in the connection's grant.
pub fn admit_scopes(
    dynamic_permissions: &BTreeSet<PermissionId>,
    effective_scopes: &BTreeSet<PermissionId>,
) -> Result<(), ControlError> {
    if dynamic_permissions.is_subset(effective_scopes) {
        return Ok(());
    }
    let missing = dynamic_permissions
        .difference(effective_scopes)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let mut error = known_error(
        codes::SCOPE_DENIED,
        "one or more requested snapshot scopes are outside the connection grant",
        false,
    );
    error.context.details = Some(json!({ "missing_permissions": missing }));
    error.context.next_steps =
        vec!["Enable the required read scopes in Quantick, then reconnect.".to_owned()];
    Err(error)
}

/// Whether `result` matches the compiled output schema of a registered
/// capability. An unregistered capability or version is never valid.
pub fn output_is_valid(
    output_validators: &CompiledCapabilitySchemas,
    capability_id: &CapabilityId,
    capability_version: u32,
    result: &Value,
) -> bool {
    output_validators
        .get(capability_id)
        .and_then(|versions| versions.get(&capability_version))
        .is_some_and(|validator| validator.validate(result).is_ok())
}

/// Every snapshot scope a projection registry declares, as the contract
/// publishes it (sorted by ID), and the permissions each one needs. A scope
/// naming a permission the authority table does not know is refused.
pub type SnapshotScopeCatalogue = (
    BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
    Vec<SnapshotScopeDescriptor>,
);

/// Build the [`SnapshotScopeCatalogue`] of `projections` against the
/// registered `permissions`.
pub fn snapshot_scope_catalogue<H: 'static>(
    projections: &ProjectionRegistry<H>,
    permissions: &[PermissionDescriptor],
) -> Result<SnapshotScopeCatalogue, RegistryError> {
    let mut scope_permissions = BTreeMap::new();
    let mut snapshot_scopes = Vec::new();
    for descriptor in projections.descriptors() {
        let required_permissions = descriptor.required_permissions.clone();
        for permission in &required_permissions {
            if !permissions.iter().any(|known| known.id == *permission) {
                return Err(RegistryError::Unknown {
                    kind: "permission",
                    id: permission.to_string(),
                });
            }
        }
        scope_permissions.insert(descriptor.scope_id.clone(), required_permissions.clone());
        snapshot_scopes.push(SnapshotScopeDescriptor {
            id: descriptor.scope_id.clone(),
            module_id: descriptor.module_id.clone(),
            schema_version: descriptor.schema_version,
            title: descriptor.title.clone(),
            description: descriptor.description.clone(),
            required_permissions,
            schema: descriptor.schema.clone(),
        });
    }
    snapshot_scopes.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((scope_permissions, snapshot_scopes))
}

/// Every scope in `snapshot_scopes` this grant already reaches, in
/// registration order and capped at what one capture may carry.
pub fn readable_scopes(
    snapshot_scopes: &[SnapshotScopeDescriptor],
    grant: &BTreeSet<PermissionId>,
) -> Vec<SnapshotScopeId> {
    snapshot_scopes
        .iter()
        .filter(|descriptor| descriptor.required_permissions.is_subset(grant))
        .map(|descriptor| descriptor.id.clone())
        .take(CONTROL_MAX_SNAPSHOT_SCOPES)
        .collect()
}
