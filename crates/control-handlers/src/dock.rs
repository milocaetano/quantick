//! The one route a family docks by: a registry that accepts an action, or a
//! module and its scopes. The application's registries are type aliases of the
//! generic registries in `quantick-control-host`, which a test also docks a
//! family into.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::{CapabilityDescriptor, ModuleDescriptor, RegistryError},
};
use quantick_control_host::{
    actions::{ActionHandler, ActionRegistry, ActionResolver},
    projection::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// Somewhere an action family docks its capabilities, over host `H` and
/// access `A`.
pub trait ActionDock<H: ?Sized, A> {
    /// Dock one action whose input is already what it will do.
    fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
    ) -> Result<(), RegistryError>;

    /// Dock one action that acts outside every replay: a replay's control
    /// trace never records it and never runs it.
    fn register_outside_replay(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
    ) -> Result<(), RegistryError>;

    /// Dock one action whose input is settled against live state first:
    /// `resolve` turns what the caller wrote into what will happen, and the
    /// control trace records that, validated by `canonical_schema`.
    fn register_resolved(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
        resolve: ActionResolver<H>,
        canonical_schema: Value,
    ) -> Result<(), RegistryError>;
}

impl<H: ?Sized, A> ActionDock<H, A> for ActionRegistry<H, A> {
    fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
    ) -> Result<(), RegistryError> {
        ActionRegistry::register(self, descriptor, handler)
    }

    fn register_outside_replay(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
    ) -> Result<(), RegistryError> {
        ActionRegistry::register_outside_replay(self, descriptor, handler)
    }

    fn register_resolved(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
        resolve: ActionResolver<H>,
        canonical_schema: Value,
    ) -> Result<(), RegistryError> {
        ActionRegistry::register_resolved(self, descriptor, handler, resolve, canonical_schema)
    }
}

/// Somewhere a projection family docks its module and scopes, over host `H`.
pub trait ProjectionDock<H: ?Sized> {
    /// Dock one owner module and its semantic revision projection.
    fn register_module<K>(
        &mut self,
        descriptor: ModuleDescriptor,
        revision: fn(&H) -> K,
    ) -> Result<(), ProjectionRegistryError>
    where
        K: Eq + Send + 'static;

    /// Dock one typed scope of `module_id`.
    #[allow(clippy::too_many_arguments)]
    fn register_scope<T>(
        &mut self,
        scope_id: SnapshotScopeId,
        module_id: ModuleId,
        schema_version: u32,
        title: &str,
        description: &str,
        required_permission_ids: &[&str],
        project: fn(&H, CaptureContext) -> T,
    ) -> Result<(), ProjectionRegistryError>
    where
        T: JsonSchema + Serialize + Send + 'static;
}

impl<H: ?Sized + 'static> ProjectionDock<H> for ProjectionRegistry<H> {
    fn register_module<K>(
        &mut self,
        descriptor: ModuleDescriptor,
        revision: fn(&H) -> K,
    ) -> Result<(), ProjectionRegistryError>
    where
        K: Eq + Send + 'static,
    {
        ProjectionRegistry::register_module(self, descriptor, revision)
    }

    fn register_scope<T>(
        &mut self,
        scope_id: SnapshotScopeId,
        module_id: ModuleId,
        schema_version: u32,
        title: &str,
        description: &str,
        required_permission_ids: &[&str],
        project: fn(&H, CaptureContext) -> T,
    ) -> Result<(), ProjectionRegistryError>
    where
        T: JsonSchema + Serialize + Send + 'static,
    {
        ProjectionRegistry::register_scope(
            self,
            scope_id,
            module_id,
            schema_version,
            title,
            description,
            required_permission_ids,
            project,
        )
    }
}
