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

/// What `history_reach_span_minutes` reads as when a snapshot predates it.
///
/// Zero, which no running build ever reports — the setter clamps to at least
/// one minute — so a consumer can tell "this build did not say" from any real
/// span rather than being handed a plausible-looking two hours.
fn no_span_reported() -> WireU64 {
    WireU64::new(0)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WorkspaceSnapshot {
    pub active_tab_index: WireU64,
    pub active_tab_id: WireU64,
    pub timezone_offset_minutes: i32,
    pub timezone_label: String,
    pub save_on_exit: bool,
    pub performance_readings_visible: bool,
    pub progressive_venue_history: bool,
    /// How far one press of *load older* reaches, as the reach registry's own
    /// token (`page`, `previous-session`) — the same string the harness hook
    /// takes, so what an operator sets is what it reads back.
    ///
    /// Additive within v1 (contract §4): defaulted rather than required, so a
    /// client holding this schema still validates a summary from an instance
    /// built before the field existed.
    #[serde(default)]
    pub history_reach: String,
    /// Minutes of *traded* time one press of the `span` reach pulls.
    ///
    /// Beside the reach because the two are one choice: an operator that
    /// can read back `by time` but not how much time cannot tell what the
    /// next press will do.
    ///
    /// `serde(default)` like every optional neighbour: v1 is frozen, and a
    /// snapshot from a build that predates this field must still validate
    /// against the shipped schema. A new *required* key is a breaking change
    /// wearing an additive diff.
    #[serde(default = "no_span_reported")]
    pub history_reach_span_minutes: WireU64,
    /// Whether a run of *load older* requests is in flight on the active tab.
    ///
    /// The setting above says what a press will do; this says whether one is
    /// still doing it. Without it an operator that started a reach has no way
    /// to tell a finished run from a running one except by polling bar counts
    /// and guessing.
    #[serde(default)]
    pub history_reach_running: bool,
    /// Whether a chart cut by trades carries the venue's candles in front of
    /// its bars. Read with each pane's `venue_prefix_present`: this is what
    /// the trader asked for, that is what the pane actually holds.
    #[serde(default)]
    pub venue_lead_in: bool,
    /// Whether opening a recording joins the session day before it, and a
    /// download fetches that day's tape as well.
    ///
    /// Additive within v1 (contract §4). It decides what a replay an operator
    /// is about to open will actually hold, so it has to be readable before
    /// they open one — the bar counts afterwards are too late to plan with.
    #[serde(default)]
    pub replay_day_before: bool,
    pub tabs: Vec<WorkspaceTab>,
    /// The layout strip: every layout, and which is active. Additive.
    #[serde(default)]
    pub layouts: Vec<super::layout::LayoutTabSnapshot>,
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
    /// The pane's address within its tab — the number `layout.focus` takes.
    pub pane_index: WireU64,
    /// The layout this pane shows, by id in the strip.
    pub layout_id: WireU64,
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
    let (history_reach, venue_lead_in) = app.control_history_settings();
    let history_reach_running = tabs
        .get(active_index)
        .is_some_and(|tab| tab.history_reach_running());
    WorkspaceSnapshot {
        active_tab_index: wire_usize(active_index),
        active_tab_id: tabs
            .get(active_index)
            .map_or_else(|| WireU64::new(0), |tab| WireU64::new(tab.id)),
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
                let shown = tab.context_panes_shown();
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
                            layout_id: WireU64::new(app.pane_layout(tab.id, side).0),
                            visible: active && visible,
                            focused: active && focused == side,
                        }
                    })
                    .collect();
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
