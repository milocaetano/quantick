//! Shared metadata for the live application projection and offline catalog.
use quantick_control::{id::ModuleId, registry::ModuleDescriptor};
use quantick_control_host::system::{MODULE_ID, SCHEMA_VERSION, SCOPE_ID, SystemSnapshot};

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(MODULE_ID).expect("static module ID is valid"),
        title: "System".to_owned(),
        description: "Application build and runtime identity.".to_owned(),
    }
}

pub fn scope_spec() -> crate::dock::ScopeSpec<SystemSnapshot> {
    crate::dock::ScopeSpec::new(
        SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "System information",
        "Reports the Quantick build, target, protocol, and capture budget.",
        &["observe", "observe.system"],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![scope_spec().descriptor()]
}
