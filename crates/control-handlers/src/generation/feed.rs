//! Shared metadata for the live application projection and offline catalog.
use quantick_control::{id::ModuleId, registry::ModuleDescriptor};
use quantick_control_schema::feed::*;

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(MODULE_ID).expect("static module ID is valid"),
        title: "Feed".to_owned(),
        description: "Feed connection, capabilities, and market-data provenance.".to_owned(),
    }
}

pub fn scope_spec() -> crate::dock::ScopeSpec<FeedSnapshot> {
    crate::dock::ScopeSpec::new(
        SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "Feed status",
        "Reports the selected and active markets, source health, capabilities, and provenance.",
        &["observe", "observe.market"],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![scope_spec().descriptor()]
}
