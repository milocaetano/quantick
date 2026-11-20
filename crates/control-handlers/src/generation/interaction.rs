//! Shared metadata for the live application projection and offline catalog.
use quantick_control::{id::ModuleId, registry::ModuleDescriptor};
use quantick_control_schema::interaction::*;
const MODULE_ID: &str = "interaction";
const SCHEMA_VERSION: u32 = 1;
const CURSOR_SCOPE_ID: &str = "interaction.cursor";
const SELECTION_SCOPE_ID: &str = "interaction.selection";

pub fn module_descriptor() -> ModuleDescriptor {
    ModuleDescriptor {
        id: ModuleId::new(MODULE_ID).expect("static module ID is valid"),
        title: "Interaction".to_owned(),
        description: "Semantic pointer resolution and current UI selections.".to_owned(),
    }
}

pub fn cursor_spec() -> crate::dock::ScopeSpec<CursorSnapshot> {
    crate::dock::ScopeSpec::new(
        CURSOR_SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "Semantic cursor",
        "Resolves the last painted pointer position to pane, axis, bar, flow cell, and drawing meaning.",
        &[
            "observe",
            "observe.attention",
            "observe.market",
            "observe.chart",
            "observe.drawings",
            "observe.orderflow",
        ],
    )
}

pub fn selection_spec() -> crate::dock::ScopeSpec<SelectionSnapshot> {
    crate::dock::ScopeSpec::new(
        SELECTION_SCOPE_ID,
        MODULE_ID,
        SCHEMA_VERSION,
        "Current selection",
        "Reports the selected drawing and paper-trade row, with unsupported selections stated explicitly.",
        &[
            "observe",
            "observe.attention",
            "observe.drawings",
            "observe.paper",
        ],
    )
}

pub(crate) fn scope_descriptors() -> Vec<quantick_control_host::catalogue::SnapshotScopeDescriptor>
{
    vec![cursor_spec().descriptor(), selection_spec().descriptor()]
}
