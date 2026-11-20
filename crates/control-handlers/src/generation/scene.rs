//! Shared metadata for the live application projection and offline catalog.
use quantick_control::{id::ModuleId, registry::ModuleDescriptor};
use quantick_control_schema::scene::*;

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(MODULE_ID).expect("static module ID is valid"),
        title: "Scene".to_owned(),
        description: "What is on screen, as named controls rather than pixels.".to_owned(),
    }
}

pub fn controls_spec() -> crate::dock::ScopeSpec<SceneSnapshot> {
    crate::dock::ScopeSpec::new(
        CONTROLS_SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "Visible controls",
        "Every control on screen with a frame-stable ID, its owner, whether it is selected, and the coded reason when it cannot be operated.",
        // A chip's label is the market it is open on and a layer's reason is
        // what the live source publishes, so this scope carries the same
        // `observe.market` payload `workspace.summary` and `interaction.cursor`
        // do, and the same `observe.workspace` list of open charts. Named here
        // too: a trader who withholds one of those scopes withholds it
        // everywhere, or the narrower grant is worth nothing.
        &[
            "observe",
            "observe.attention",
            "observe.workspace",
            "observe.market",
        ],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![controls_spec().descriptor()]
}
