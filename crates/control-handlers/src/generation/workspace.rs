//! Shared metadata for the live application projection and offline catalog.
use quantick_control::{id::ModuleId, registry::ModuleDescriptor};
use quantick_control_schema::workspace::*;

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(MODULE_ID).expect("static module ID is valid"),
        title: "Workspace".to_owned(),
        description: "Open tabs, pane layout, and current focus.".to_owned(),
    }
}

pub fn scope_spec() -> crate::dock::ScopeSpec<WorkspaceSnapshot> {
    crate::dock::ScopeSpec::new(
        SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "Workspace summary",
        "Reports every tab and pane with stable IDs, visibility, and focus.",
        &["observe", "observe.workspace", "observe.market"],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![scope_spec().descriptor()]
}
