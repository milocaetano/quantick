//! Build and runtime identity snapshot: the host shapes it, this build
//! supplies what only it knows.

use super::registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError};

use quantick_control_host::system::BuildIdentity;
pub(crate) use quantick_control_host::system::SystemSnapshot;

/// This build, read at compile time: the one place the crate version and the
/// stamped commit are turned into data.
pub(crate) const BUILD: BuildIdentity = BuildIdentity {
    application_version: env!("CARGO_PKG_VERSION"),
    git_commit: crate::launch::GIT_COMMIT,
};

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    registry.register_module(
        quantick_control_handlers::generation::system::module_descriptor(),
        revision,
    )?;
    quantick_control_handlers::generation::system::scope_spec().register(registry, project)
}

fn revision<P: ?Sized>(_app: &P) -> SystemSnapshot {
    snapshot()
}

fn project<P: ?Sized>(_app: &P, _context: CaptureContext) -> SystemSnapshot {
    snapshot()
}

pub(crate) fn snapshot() -> SystemSnapshot {
    quantick_control_host::system::snapshot(BUILD)
}
