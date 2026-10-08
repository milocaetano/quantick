//! The one route a family docks by: a registry that accepts an action, or a
//! module and its scopes. The application's registries implement these
//! through their own methods, and so do the generic registries in
//! `quantick-control-host` a test docks a family into.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::{CapabilityDescriptor, ModuleDescriptor, RegistryError},
};
use quantick_control_host::{
    actions::{ActionHandler, ActionRegistry},
    projection::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
};
use schemars::JsonSchema;
use serde::Serialize;

/// Somewhere an action family docks its capabilities, over host `H` and
/// access `A`.
pub trait ActionDock<H: ?Sized, A> {
    /// Dock one action whose input is already what it will do.
    fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
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
