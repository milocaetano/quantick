//! Build and runtime identity snapshot: the host shapes it, this build
//! supplies what only it knows.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
};

use crate::app::QuantickApp;

use super::registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError};

use quantick_control_host::system::{BuildIdentity, MODULE_ID, SCHEMA_VERSION};
pub(crate) use quantick_control_host::system::{SCOPE_ID, SystemSnapshot};

/// This build, read at compile time: the one place the crate version and the
/// stamped commit are turned into data.
pub(crate) const BUILD: BuildIdentity = BuildIdentity {
    application_version: env!("CARGO_PKG_VERSION"),
    git_commit: option_env!("QUANTICK_GIT_COMMIT"),
};

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "System".to_owned(),
            description: "Application build and runtime identity.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "System information",
        "Reports the Quantick build, target, protocol, and capture budget.",
        &["observe", "observe.system"],
        project,
    )
}

fn revision(_app: &QuantickApp) -> SystemSnapshot {
    snapshot()
}

fn project(_app: &QuantickApp, _context: CaptureContext) -> SystemSnapshot {
    snapshot()
}

pub(crate) fn snapshot() -> SystemSnapshot {
    quantick_control_host::system::snapshot(BUILD)
}
