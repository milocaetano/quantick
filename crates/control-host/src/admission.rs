//! Capability admission: whether one request may reach a handler at all.
//!
//! The host's contract registers its capabilities here — each with the input
//! and output schemas compiled once — and asks, for every request, the same
//! questions in the same order: is the envelope well formed, is the
//! capability registered, does the connection hold its permissions
//! ([`admit_capability`]); does the payload match the schema, is the
//! idempotency key allowed, does this tier accept a dry run or an expected
//! revision ([`CapabilityAdmitted::admit_payload`]); and, once a handler has
//! named the snapshot scopes the request reaches, are those granted too
//! ([`PayloadAdmitted::admit_scopes`]). Each step returns the only value the
//! next one accepts, so a host cannot skip one or run them out of order.
//!
//! What is *not* here is the authority table itself — which profiles,
//! permissions, effects and capabilities exist — and the handlers. Those are
//! the application's: they name its modules. This module is the machinery the
//! table feeds, so the order of the checks is written once and cannot drift
//! between hosts.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, ErrorCode, PermissionId},
    registry::{CapabilityDescriptor, ControlRegistry, RegistryError, check_idempotency_key},
    schema::CompiledSchema,
    wire::RequestEnvelope,
};
use serde_json::{Value, json};

/// Compiled schemas by capability ID and version.
pub type CompiledCapabilitySchemas = BTreeMap<CapabilityId, BTreeMap<u32, CompiledSchema>>;

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
/// a capability can never be registered without the validators
/// [`CapabilityAdmitted::admit_payload`] and [`output_is_valid`] need. `P` is whatever the host dispatches to.
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

/// What a host's tier accepts beyond a payload that matches its schema.
///
/// A policy rather than a hard-coded refusal, so a later tier that can
/// preview an action or check a revision says so here instead of forking the
/// admission order. Every tier the application hosts today is
/// [`TierPolicy::STRICT`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TierPolicy {
    pub accepts_dry_runs: bool,
    pub accepts_expected_revisions: bool,
}

impl TierPolicy {
    /// Refuse every dry run and every expected revision.
    pub const STRICT: Self = Self {
        accepts_dry_runs: false,
        accepts_expected_revisions: false,
    };
}

/// A request that passed the first three checks — the envelope, the
/// registration, the permissions the descriptor requires — and may now have
/// its payload checked. The only way to [`PayloadAdmitted`], so the order
/// cannot be skipped.
#[must_use = "a capability admission is only half of the checks"]
pub struct CapabilityAdmitted<'a> {
    descriptor: &'a CapabilityDescriptor,
}

/// A request that passed every check that does not depend on its handler.
/// The host dispatches on [`Self::descriptor`]; a handler that names snapshot
/// scopes then passes them through [`Self::admit_scopes`].
pub struct PayloadAdmitted<'a> {
    descriptor: &'a CapabilityDescriptor,
}

/// The first three checks: the envelope, the registration, the permissions
/// the descriptor requires.
pub fn admit_capability<'a>(
    registry: &'a ControlRegistry,
    envelope: &RequestEnvelope,
    effective_scopes: &BTreeSet<PermissionId>,
) -> Result<CapabilityAdmitted<'a>, ControlError> {
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
    Ok(CapabilityAdmitted { descriptor })
}

impl<'a> CapabilityAdmitted<'a> {
    /// The admitted capability's ID, for the host to find what it holds for it.
    pub fn id(&self) -> &'a CapabilityId {
        &self.descriptor.id
    }

    /// The admitted capability's version.
    pub fn version(&self) -> u32 {
        self.descriptor.version
    }

    /// The next three checks: the payload against its schema, the
    /// idempotency key against the descriptor's policy, and the tier's
    /// refusal of what `policy` does not accept.
    ///
    /// `own_validator` is a schema the host holds for this capability outside
    /// the compiled table — an action validates against the very schema its
    /// hotkey passes, so the two paths cannot drift. Without one, the
    /// capability must have been registered through [`register_capability`].
    pub fn admit_payload(
        self,
        envelope: &RequestEnvelope,
        own_validator: Option<&CompiledSchema>,
        input_validators: &CompiledCapabilitySchemas,
        policy: TierPolicy,
    ) -> Result<PayloadAdmitted<'a>, ControlError> {
        let descriptor = self.descriptor;
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
        let refused_dry_run = envelope.dry_run && !policy.accepts_dry_runs;
        let refused_revisions =
            !envelope.expected_revisions.is_empty() && !policy.accepts_expected_revisions;
        if refused_dry_run || refused_revisions {
            return Err(ControlError::invalid_request(
                "this tier's capabilities forbid dry runs and expected revisions",
            ));
        }
        Ok(PayloadAdmitted { descriptor })
    }
}

impl<'a> PayloadAdmitted<'a> {
    /// The capability the request may now be dispatched to.
    pub fn descriptor(&self) -> &'a CapabilityDescriptor {
        self.descriptor
    }

    /// The last check, after a handler has named the snapshot scopes the
    /// request reaches: every permission they need must be in the
    /// connection's grant.
    pub fn admit_scopes(
        &self,
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
