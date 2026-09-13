//! The application's snapshot projection registry.
//!
//! The registry itself — scope validation, revision tracking, the capture
//! budget — is headless and lives in `quantick_control_host::projection`. What
//! stays here is what only the application can supply: the host state its
//! projectors read ([`QuantickApp`]) and the clock ([`SystemClock`]).

use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use quantick_control::{
    error::ControlError,
    id::{InstanceId, ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
};
use quantick_control_host::{
    clock::HostClock,
    projection::{self, ProjectionPerformance},
};
use schemars::JsonSchema;
use serde::Serialize;

use crate::{app::QuantickApp, metrics};

#[cfg(test)]
pub(crate) use quantick_control_host::projection::SerializedScope;
pub(crate) use quantick_control_host::projection::{
    CaptureContext, ProjectionRegistryError, SerializedSnapshotCapture, SnapshotCapture,
};

/// The projection registry over the running application.
///
/// A newtype that forwards, rather than a type alias: the extension-boundary
/// guard refuses any alias that names `QuantickApp`, because an alias of the
/// root itself would let an `impl` escape its measurement. Every method is
/// the generic registry's own, specialised to this host.
pub(crate) struct ProjectionRegistry(projection::ProjectionRegistry<QuantickApp>);

impl ProjectionRegistry {
    /// An empty registry that stamps and times its captures by `clock`.
    pub fn new(clock: Arc<dyn HostClock>) -> Self {
        Self(projection::ProjectionRegistry::new(clock))
    }

    /// The generic registry this one specialises, for code that is generic
    /// over the host.
    pub fn inner(&self) -> &projection::ProjectionRegistry<QuantickApp> {
        &self.0
    }

    /// Dock one owner module and its semantic revision projection.
    pub fn register_module<K>(
        &mut self,
        descriptor: ModuleDescriptor,
        revision: fn(&QuantickApp) -> K,
    ) -> Result<(), ProjectionRegistryError>
    where
        K: Eq + Send + 'static,
    {
        self.0.register_module(descriptor, revision)
    }

    /// Dock one typed scope; see the generic registry's `register_scope`.
    #[allow(clippy::too_many_arguments)]
    pub fn register_scope<T>(
        &mut self,
        scope_id: SnapshotScopeId,
        module_id: ModuleId,
        schema_version: u32,
        title: impl Into<String>,
        description: impl Into<String>,
        required_permission_ids: &[&str],
        project: fn(&QuantickApp, CaptureContext) -> T,
    ) -> Result<(), ProjectionRegistryError>
    where
        T: JsonSchema + Serialize + Send + 'static,
    {
        self.0.register_scope(
            scope_id,
            module_id,
            schema_version,
            title,
            description,
            required_permission_ids,
            project,
        )
    }

    /// Test-only here: production reads the scopes through [`Self::inner`].
    #[cfg(test)]
    pub fn descriptors(
        &self,
    ) -> impl Iterator<Item = &projection::ProjectionDescriptor<QuantickApp>> {
        self.0.descriptors()
    }

    pub fn module_descriptors(&self) -> impl Iterator<Item = &ModuleDescriptor> {
        self.0.module_descriptors()
    }

    pub fn performance(&self) -> ProjectionPerformance {
        self.0.performance()
    }

    /// Capture exactly the requested scopes in one bounded, immutable pass.
    pub fn capture(
        &mut self,
        app: &QuantickApp,
        instance_id: &InstanceId,
        requested_scopes: &[SnapshotScopeId],
    ) -> Result<SnapshotCapture, ControlError> {
        self.0.capture(app, instance_id, requested_scopes)
    }
}

/// The clock a capture is stamped and timed by: the process wall clock, and a
/// monotonic clock measured from its first reading.
pub(crate) struct SystemClock;

impl HostClock for SystemClock {
    fn unix_ms(&self) -> i64 {
        metrics::wall_clock_ms()
    }

    fn monotonic(&self) -> Duration {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed()
    }
}
