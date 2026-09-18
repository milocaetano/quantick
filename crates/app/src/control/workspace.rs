//! Workspace, tab, layout, and focus snapshot.

pub(crate) use quantick_control_schema::workspace::*;

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{app::QuantickApp, pane::PaneSide};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{canonical_f32, wire_usize},
};

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
    let (history_reach, venue_lead_in) = app.control_history_settings();
    let history_reach_running = tabs
        .get(active_index)
        .is_some_and(|tab| tab.history_reach_running());
    WorkspaceSnapshot {
        active_tab_index: wire_usize(active_index),
        active_tab_id: tabs.get(active_index).map_or_else(
            || WireU64::new(0),
            |_| WireU64::new(tabs.id_at(active_index)),
        ),
        timezone_offset_minutes: timezone.minutes(),
        timezone_label: timezone.label(),
        layouts: super::layout::layout_tabs(app),
        save_on_exit,
        performance_readings_visible,
        progressive_venue_history,
        history_reach: history_reach.token().to_owned(),
        history_reach_span_minutes: WireU64::new(app.control_history_reach_span_minutes().into()),
        history_reach_running,
        venue_lead_in,
        replay_day_before: app.control_replay_day_before(),
        tabs: tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let active = index == active_index;
                let focused = tab.focused_side();
                let shown = usize::from(!tab.context_collapsed) * tab.context_panes_shown();
                let panes: Vec<WorkspacePane> = tab
                    .panes()
                    .map(|(pane, side)| {
                        let visible = match side {
                            PaneSide::Flow => tab.layout.shows_flow(),
                            PaneSide::Time(slot) => tab.layout.shows_time() && slot < shown,
                        };
                        WorkspacePane {
                            pane_id: WireU64::new(pane.id),
                            side: side.into(),
                            pane_index: wire_usize(side.index()),
                            layout_id: WireU64::new(
                                app.layout_state().pane_layout(tabs.id_at(index), side).0,
                            ),
                            visible: active && visible,
                            focused: active && focused == side,
                        }
                    })
                    .collect();
                WorkspaceTab {
                    index: wire_usize(index),
                    tab_id: WireU64::new(tabs.id_at(index)),
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
                    context_collapsed: tab.context_collapsed,
                    panes,
                }
            })
            .collect(),
    }
}
