//! Workspace, tab, layout, and focus snapshot.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{app::QuantickApp, pane::PaneSide};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{CanvasLayoutDto, PaneSideDto, canonical_f32, wire_usize},
};

pub(crate) const SCOPE_ID: &str = "workspace.summary";
const MODULE_ID: &str = "workspace";
const SCHEMA_VERSION: u32 = 1;
const SPLIT_FRACTION_DECIMAL_PLACES: u32 = 6;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WorkspaceSnapshot {
    pub active_tab_index: WireU64,
    pub active_tab_id: WireU64,
    pub timezone_offset_minutes: i32,
    pub timezone_label: String,
    pub save_on_exit: bool,
    pub performance_readings_visible: bool,
    pub progressive_venue_history: bool,
    pub tabs: Vec<WorkspaceTab>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WorkspaceTab {
    pub index: WireU64,
    pub tab_id: WireU64,
    pub label: String,
    pub feed_id: String,
    pub symbol: String,
    pub active: bool,
    pub layout: CanvasLayoutDto,
    pub focused_pane: PaneSideDto,
    pub split_fraction: CanonicalDecimal,
    pub panes: Vec<WorkspacePane>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WorkspacePane {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub visible: bool,
    pub focused: bool,
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Workspace".to_owned(),
            description: "Open tabs, pane layout, and current focus.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Workspace summary",
        "Reports every tab and pane with stable IDs, visibility, and focus.",
        &["observe", "observe.workspace", "observe.market"],
        project,
    )
}

fn revision(app: &QuantickApp) -> WorkspaceSnapshot {
    snapshot(app)
}

fn project(app: &QuantickApp, _context: CaptureContext) -> WorkspaceSnapshot {
    snapshot(app)
}

fn snapshot(app: &QuantickApp) -> WorkspaceSnapshot {
    let active_index = app.control_active_tab_index();
    let tabs = app.control_tabs();
    let timezone = app.control_timezone();
    let (save_on_exit, performance_readings_visible, progressive_venue_history) =
        app.control_workspace_flags();
    WorkspaceSnapshot {
        active_tab_index: wire_usize(active_index),
        active_tab_id: tabs
            .get(active_index)
            .map_or_else(|| WireU64::new(0), |tab| WireU64::new(tab.id)),
        timezone_offset_minutes: timezone.minutes(),
        timezone_label: timezone.label(),
        save_on_exit,
        performance_readings_visible,
        progressive_venue_history,
        tabs: tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let active = index == active_index;
                let focused = tab.focused_side();
                let mut panes = vec![WorkspacePane {
                    pane_id: WireU64::new(tab.flow_pane.id),
                    side: PaneSideDto::Flow,
                    visible: active && tab.layout.shows_flow(),
                    focused: active && focused == PaneSide::Flow,
                }];
                if let Some(time) = tab.time_pane() {
                    panes.push(WorkspacePane {
                        pane_id: WireU64::new(time.id),
                        side: PaneSideDto::Time,
                        visible: active && tab.layout.shows_time(),
                        focused: active && focused == PaneSide::Time,
                    });
                }
                WorkspaceTab {
                    index: wire_usize(index),
                    tab_id: WireU64::new(tab.id),
                    label: tab.chip_label().to_owned(),
                    feed_id: tab.feed_id.clone(),
                    symbol: tab.symbol.clone(),
                    active,
                    layout: tab.layout.into(),
                    focused_pane: focused.into(),
                    split_fraction: canonical_f32(
                        tab.split_fraction,
                        SPLIT_FRACTION_DECIMAL_PLACES,
                    )
                    .expect("the pane split fraction is finite"),
                    panes,
                }
            })
            .collect(),
    }
}
