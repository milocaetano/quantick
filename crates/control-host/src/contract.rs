//! Owned capability registration and the complete admission transaction.
//!
//! Hosts supply authority, opaque read tokens and external schemas. Only a
//! successful final dynamic-permission check returns a prepared read to them.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, PermissionId, SnapshotScopeId},
    registry::{
        CapabilityDescriptor, ControlRegistry, EffectPolicy, ModuleDescriptor,
        PermissionDescriptor, ProfileDescriptor, RegistryError,
    },
    schema::CompiledSchema,
    wire::RequestEnvelope,
};
use serde_json::Value;

use crate::{
    admission::{self, CompiledCapabilitySchemas, TierPolicy, known_error},
    catalogue::{self, SnapshotScopeDescriptor},
    projection::ProjectionRegistry,
};

#[cfg(test)]
#[path = "contract/tests/fail_closed.rs"]
mod fail_closed;

/// Authority composition before any capability can be registered.
pub struct ContractBuilder {
    registry: ControlRegistry,
    profiles: Vec<ProfileDescriptor>,
    permissions: Vec<PermissionDescriptor>,
}

impl ContractBuilder {
    /// Register the exact ordered authority values retained for discovery.
    pub fn new(
        profiles: Vec<ProfileDescriptor>,
        permissions: Vec<PermissionDescriptor>,
    ) -> Result<Self, RegistryError> {
        let mut registry = ControlRegistry::new();
        for descriptor in &profiles {
            registry.register_profile(descriptor.clone())?;
        }
        for descriptor in &permissions {
            registry.register_permission(descriptor.clone())?;
        }
        registry.finalize_authority()?;
        Ok(Self {
            registry,
            profiles,
            permissions,
        })
    }

    pub fn register_module(&mut self, descriptor: ModuleDescriptor) -> Result<(), RegistryError> {
        self.registry.register_module(descriptor)
    }

    pub fn register_effect(&mut self, policy: EffectPolicy) -> Result<(), RegistryError> {
        self.registry.register_effect(policy)
    }

    /// Validate the scope catalogue once, against this same authority.
    pub fn build<P, H: ?Sized + 'static>(
        self,
        projections: &ProjectionRegistry<H>,
    ) -> Result<CapabilityContract<P>, RegistryError> {
        let (scope_permissions, snapshot_scopes) =
            catalogue::snapshot_scope_catalogue(projections, &self.permissions)?;
        Ok(CapabilityContract {
            registry: self.registry,
            profiles: self.profiles,
            permissions: self.permissions,
            bindings: BTreeMap::new(),
            input_validators: BTreeMap::new(),
            output_validators: BTreeMap::new(),
            snapshot_scopes,
            scope_permissions,
        })
    }
}

enum Binding<P> {
    Read(P),
    External,
}

/// A provider's actual identity and already compiled validators. The provider
/// is trusted host composition, not an untrusted client or schema attestation.
#[derive(Clone, Copy)]
pub struct ExternalSchemas<'a> {
    pub capability_id: &'a CapabilityId,
    pub version: u32,
    pub input: &'a CompiledSchema,
    pub output: &'a CompiledSchema,
}

impl ExternalSchemas<'_> {
    fn matches(&self, id: &CapabilityId, version: u32) -> bool {
        self.capability_id == id && self.version == version
    }
}

/// The only registered-state view available to a read preparation callback.
#[derive(Clone, Copy)]
pub struct ScopeCatalogue<'a> {
    permissions: &'a BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
}

impl<'a> ScopeCatalogue<'a> {
    pub fn permissions(&self, scope: &SnapshotScopeId) -> Option<&'a BTreeSet<PermissionId>> {
        self.permissions.get(scope)
    }
}

/// Callback output, consumed internally before the owner finalizes admission.
/// Preparing a value must not execute its effects.
pub struct ReadPreparation<R> {
    pub prepared: R,
    pub dynamic_permissions: BTreeSet<PermissionId>,
}

#[derive(Debug)]
pub enum AdmittedRoute<R> {
    Read(R),
    External,
}

/// A result returned only after all applicable checks succeeded.
#[derive(Debug)]
pub struct AdmittedRequest<R> {
    pub envelope: RequestEnvelope,
    pub required_permissions: BTreeSet<PermissionId>,
    pub route: AdmittedRoute<R>,
}

/// Private registered state, with atomic association and fail-closed routing.
pub struct CapabilityContract<P> {
    registry: ControlRegistry,
    bindings: BTreeMap<(CapabilityId, u32), Binding<P>>,
    input_validators: CompiledCapabilitySchemas,
    output_validators: CompiledCapabilitySchemas,
    profiles: Vec<ProfileDescriptor>,
    permissions: Vec<PermissionDescriptor>,
    snapshot_scopes: Vec<SnapshotScopeDescriptor>,
    scope_permissions: BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
}

impl<P> CapabilityContract<P> {
    /// Schema compilation and registry checks finish before any association is
    /// committed. On failure the previously reachable contract is unchanged.
    pub fn register_read(
        &mut self,
        descriptor: CapabilityDescriptor,
        token: P,
    ) -> Result<(), RegistryError> {
        admission::register_capability(
            &mut self.registry,
            &mut self.bindings,
            &mut self.input_validators,
            &mut self.output_validators,
            descriptor,
            Binding::Read(token),
        )
    }

    /// External validators remain owned by the host's action registry.
    pub fn register_external(
        &mut self,
        descriptor: CapabilityDescriptor,
    ) -> Result<(), RegistryError> {
        let key = (descriptor.id.clone(), descriptor.version);
        self.registry.register_capability(descriptor)?;
        let previous = self.bindings.insert(key, Binding::External);
        debug_assert!(
            previous.is_none(),
            "registry rejected duplicate capability IDs"
        );
        Ok(())
    }

    pub fn registry(&self) -> &ControlRegistry {
        &self.registry
    }
    pub fn profiles(&self) -> &[ProfileDescriptor] {
        &self.profiles
    }
    pub fn permissions(&self) -> &[PermissionDescriptor] {
        &self.permissions
    }
    pub fn snapshot_scopes(&self) -> &[SnapshotScopeDescriptor] {
        &self.snapshot_scopes
    }
    /// Counts registered read bindings, excluding external actions.
    ///
    /// Deliberately published introspection support for public contract tests
    /// and application registration regressions; this scans the binding map.
    pub fn read_count(&self) -> usize {
        self.bindings
            .values()
            .filter(|binding| matches!(binding, Binding::Read(_)))
            .count()
    }
    pub fn readable_scopes(&self, grant: &BTreeSet<PermissionId>) -> Vec<SnapshotScopeId> {
        catalogue::readable_scopes(&self.snapshot_scopes, grant)
    }

    /// Envelope/static -> schema/key/tier -> read preparation -> dynamic
    /// permission finalization. No prepared value escapes an error branch.
    pub fn admit<'a, R>(
        &self,
        envelope: RequestEnvelope,
        grant: &BTreeSet<PermissionId>,
        policy: TierPolicy,
        external: impl FnOnce(&CapabilityId, u32) -> Option<ExternalSchemas<'a>>,
        prepare: impl FnOnce(&P, &Value, ScopeCatalogue<'_>) -> Result<ReadPreparation<R>, ControlError>,
    ) -> Result<AdmittedRequest<R>, ControlError> {
        let admitted = admission::admit_capability(&self.registry, &envelope, grant)?;
        let binding = self
            .bindings
            .get(&(envelope.capability_id.clone(), envelope.capability_version))
            .ok_or_else(|| {
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "registered observer capability has no handler",
                    false,
                )
            })?;
        let schemas = match binding {
            Binding::Read(_) => None,
            Binding::External => Some(
                external(&envelope.capability_id, envelope.capability_version)
                    .filter(|schemas| {
                        schemas.matches(&envelope.capability_id, envelope.capability_version)
                    })
                    .ok_or_else(|| {
                        known_error(
                            codes::CAPABILITY_UNAVAILABLE,
                            "registered observer capability has no input validator",
                            false,
                        )
                    })?,
            ),
        };
        let admitted = admitted.admit_payload(
            &envelope,
            |_, _| schemas,
            |schemas| schemas.input,
            &self.input_validators,
            policy,
        )?;
        let descriptor = admitted.descriptor();
        let (route, dynamic_permissions) = match binding {
            Binding::External => (AdmittedRoute::External, BTreeSet::new()),
            Binding::Read(token) => {
                let prepared = prepare(
                    token,
                    &envelope.payload,
                    ScopeCatalogue {
                        permissions: &self.scope_permissions,
                    },
                )?;
                admitted.admit_scopes(&prepared.dynamic_permissions, grant)?;
                (
                    AdmittedRoute::Read(prepared.prepared),
                    prepared.dynamic_permissions,
                )
            }
        };
        let mut required_permissions = descriptor.required_permissions.clone();
        required_permissions.extend(dynamic_permissions);
        Ok(AdmittedRequest {
            envelope,
            required_permissions,
            route,
        })
    }

    /// Exact registration is required even when an external provider exists.
    /// A matching provider takes precedence; mismatches never fall back.
    pub fn validate_output<'a>(
        &self,
        id: &CapabilityId,
        version: u32,
        result: &Value,
        external: impl FnOnce(&CapabilityId, u32) -> Option<ExternalSchemas<'a>>,
    ) -> bool {
        // Read validators are inserted atomically with registration, so their
        // indexed presence proves the exact registered read without cloning an
        // ID for another registry lookup on the output-validation path.
        let read = self
            .output_validators
            .get(id)
            .and_then(|versions| versions.get(&version));
        if read.is_none()
            && !matches!(
                self.bindings.get(&(id.clone(), version)),
                Some(Binding::External)
            )
        {
            return false;
        }
        if let Some(schemas) = external(id, version) {
            return schemas.matches(id, version) && schemas.output.validate(result).is_ok();
        }
        read.is_some_and(|validator| validator.validate(result).is_ok())
    }
}
