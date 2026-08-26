//! The semantic scene: what is on screen, named rather than rasterised.
//!
//! An assistant that can only see pixels has to guess at everything a screen
//! reader would be told outright — what this button is, whether pressing it
//! would do anything, which chart it belongs to. This module answers those
//! questions directly: one entry per control the trader can see, each with a
//! label, an identifier that survives the next frame, whether it is selected,
//! and, when it cannot be operated, *why* — as a code, never as the sentence
//! the button shows a human.
//!
//! ## One list, never two
//!
//! Nothing here declares a control of its own. Every entry resolves to the
//! registry that already drives the interface: [`ChartLayer`] through
//! [`LayerToggle`] for the toolbar's LAYERS group, [`DRAWING_TOOLS`] for the
//! tool rail, [`DockTab::ALL`] for the dock, and the application's own tabs
//! and panes for the rest. A hand-kept list beside those would drift the day
//! someone adds a tool and forgets this file, and an operator would be told
//! about a button that is not there — or, worse, never told about one that is.
//!
//! ## What is on screen, and only that
//!
//! A control that is not painted is not listed. The tool rail folded away
//! contributes no tools; a hidden dock contributes no tabs. Availability is a
//! separate question from presence: the L2 heatmap toggle is on screen and
//! disabled on a source that captures no book, and it says so with a reason
//! code a client can branch on.
//!
//! ## Cost
//!
//! Nothing here runs unless a client asks. The projection reads state the
//! frame already keeps — no rectangle is recorded, no hit test is run and no
//! layout is measured for the scene's benefit — so an instance nobody is
//! observing pays exactly nothing for this module existing.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_SCENE_MAX_CONTROLS,
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::QuantickApp,
    dock::DockTab,
    drawings::DRAWING_TOOLS,
    pane::{ChartPane, PaneSide},
    tab::Tab,
    toolbar::LayerToggle,
    toolrail::Tool,
};

use super::{
    contract::CHART_WINDOW_CAPABILITY_ID,
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{AvailabilitySnapshot, PaneSideDto, available, canonical_f32, unavailable},
};

pub(crate) const CONTROLS_SCOPE_ID: &str = "scene.controls";
const MODULE_ID: &str = "scene";
const SCHEMA_VERSION: u32 = 1;
/// Screen coordinates are reported to the same precision as the cursor's, so
/// a bound and a pointer position can be compared without either being
/// rounded first.
const SCREEN_PIXEL_DECIMAL_PLACES: u32 = 3;

/// The identifier prefix of every control the tab strip owns.
const TAB_STRIP_OWNER_ID: &str = "tab_strip";
/// The LAYERS group of the context toolbar.
const TOOLBAR_LAYERS_OWNER_ID: &str = "toolbar.layers";
/// The drawing tool rail.
const TOOL_RAIL_OWNER_ID: &str = "tool_rail";
/// The right-hand dock and its tab strip.
const DOCK_OWNER_ID: &str = "dock";

/// What is on screen right now, as controls an operator can name.
///
/// The tree is expressed as a flat list plus [`SceneControlSnapshot::owner`]
/// rather than as nested arrays: a client that wants the tree walks the owner
/// links, and a client that wants one control by ID does not have to descend
/// into anything to find it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneSnapshot {
    pub active_tab_id: WireU64,
    pub focused_pane_id: WireU64,
    pub focused_pane_side: PaneSideDto,
    pub controls: Vec<SceneControlSnapshot>,
    /// Whether every control on screen fits in `controls`.
    ///
    /// The scene is bounded like every other projection. It says so rather
    /// than truncating in silence, because a client that read a short list as
    /// the whole screen would conclude a control does not exist.
    pub complete: AvailabilitySnapshot,
}

/// One control the trader can see.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneControlSnapshot {
    /// Stable for as long as the control is what it is.
    ///
    /// Derived from identity — a tab's ID, a pane's ID, a layer's or a tool's
    /// declared name — never from a position on screen or an index into this
    /// list. Two captures a hundred frames apart name the same button the same
    /// way, which is what makes it possible to point at one, look away, and
    /// point at it again.
    pub control_id: String,
    /// What the control is called, for a human reading the answer.
    pub label: String,
    pub role: SceneRoleDto,
    pub owner: SceneOwnerSnapshot,
    /// Whether the control is currently the chosen one of its group: the
    /// active tab, the armed tool, the open dock tab, a layer that is drawn.
    pub selected: bool,
    /// Whether operating it now would do anything, and the reason when not.
    ///
    /// The reason is a stable code, never the sentence the interface shows.
    /// A client made to parse that sentence would break the day it is
    /// reworded, and translating it would break every such client at once.
    pub availability: AvailabilitySnapshot,
    /// Where the control is, when the frame already knows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<SceneBoundsSnapshot>,
    /// Why there are no bounds, when there are none.
    ///
    /// Only the chart canvases record their rectangle during the frame they
    /// are drawn. Measuring the chrome would mean writing a rectangle per
    /// control per frame whether or not anyone is watching, which this module
    /// refuses to do — so the answer is an honest "not recorded" rather than
    /// a guess assembled from layout constants.
    pub bounds_availability: AvailabilitySnapshot,
    /// The registered capability that operates this control, where one exists.
    ///
    /// Absent for most controls today: reading the screen came before acting
    /// on it, and the cockpit tier that registers a capability per control is
    /// still ahead. An absent ID means "not reachable through the control
    /// plane yet", never "not reachable at all".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capability_id: Option<String>,
}

/// What kind of thing a control is, so a client can decide how to talk about
/// it without matching on its identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SceneRoleDto {
    /// One of a strip of tabs; selecting it replaces what is below.
    Tab,
    /// An on/off switch that stays where it is.
    Toggle,
    /// A mode the pointer enters until another is chosen.
    Tool,
    /// A chart surface: the thing the pointer resolves against.
    Canvas,
}

/// Which region of the window a control belongs to.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneOwnerSnapshot {
    pub kind: SceneOwnerKindDto,
    /// The owner's own identifier. Where the owner is itself a control in this
    /// list — a chart tab owning its panes — this is that control's ID, so the
    /// tree can be walked without a second vocabulary.
    pub id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SceneOwnerKindDto {
    /// The strip of open chart tabs.
    TabStrip,
    /// The context toolbar above the canvas.
    Toolbar,
    /// The drawing tool rail.
    ToolRail,
    /// The right-hand dock.
    Dock,
    /// One open chart tab, owning its panes.
    Tab,
}

/// A control's rectangle in window coordinates, in pixels.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneBoundsSnapshot {
    #[schemars(extend("x-unit" = "pixels"))]
    pub x_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub y_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub width_px: CanonicalDecimal,
    #[schemars(extend("x-unit" = "pixels"))]
    pub height_px: CanonicalDecimal,
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Scene".to_owned(),
            description: "What is on screen, as named controls rather than pixels.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(CONTROLS_SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Visible controls",
        "Every control on screen with a frame-stable ID, its owner, whether it is selected, and the coded reason when it cannot be operated.",
        &["observe", "observe.attention"],
        project_controls,
    )
}

fn revision(app: &QuantickApp) -> SceneSnapshot {
    scene_snapshot(app)
}

fn project_controls(app: &QuantickApp, _context: CaptureContext) -> SceneSnapshot {
    scene_snapshot(app)
}

/// The identifier of one chart tab's entry in the tab strip.
fn tab_control_id(tab_id: u64) -> String {
    format!("{TAB_STRIP_OWNER_ID}.tab.{tab_id}")
}

/// The identifier of one pane's chart canvas.
///
/// Public to the control module because the cursor answers with it: the
/// control the pointer resolves to and the control the scene lists are the
/// same string, produced here once, so the two can never disagree.
pub(crate) fn pane_canvas_control_id(pane_id: u64) -> String {
    format!("pane.{pane_id}.canvas")
}

pub(crate) fn scene_snapshot(app: &QuantickApp) -> SceneSnapshot {
    let tabs = app.control_tabs();
    let active = &tabs[app.control_active_tab_index()];
    let focused_side = active.focused_side();
    let mut controls = Vec::new();

    push_tab_strip(&mut controls, tabs, active.id);
    push_layer_toggles(&mut controls, app, active);
    push_tool_rail(&mut controls, app);
    push_dock(&mut controls, app);
    push_panes(&mut controls, active, focused_side);

    // Bounded like every other projection, and honest about it. The registries
    // behind the scene are all fixed-size but the tab strip is not, so a
    // trader with an implausible number of charts open truncates rather than
    // building an unbounded payload on the application thread.
    let complete = if controls.len() > CONTROL_SCENE_MAX_CONTROLS {
        controls.truncate(CONTROL_SCENE_MAX_CONTROLS);
        unavailable("control_count_exceeded_the_scene_limit")
    } else {
        available()
    };

    SceneSnapshot {
        active_tab_id: WireU64::new(active.id),
        focused_pane_id: WireU64::new(active.pane(focused_side).id),
        focused_pane_side: focused_side.into(),
        controls,
        complete,
    }
}

/// The open charts, in strip order.
fn push_tab_strip(controls: &mut Vec<SceneControlSnapshot>, tabs: &[Tab], active_id: u64) {
    for tab in tabs {
        controls.push(SceneControlSnapshot {
            control_id: tab_control_id(tab.id),
            label: format!("{} {}", tab.feed_id, tab.symbol),
            role: SceneRoleDto::Tab,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::TabStrip,
                id: TAB_STRIP_OWNER_ID.to_owned(),
            },
            selected: tab.id == active_id,
            availability: available(),
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: None,
        });
    }
}

/// The toolbar's LAYERS group: one toggle per visual layer, each answering
/// for the same field the pane's own layer menu writes.
fn push_layer_toggles(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp, tab: &Tab) {
    let capabilities = tab.capabilities(app.control_config());
    for toggle in LayerToggle::ALL {
        let layer = toggle.layer();
        let gate = toggle.gate();
        controls.push(SceneControlSnapshot {
            control_id: format!("{TOOLBAR_LAYERS_OWNER_ID}.{}", layer.id()),
            label: layer.label().to_owned(),
            role: SceneRoleDto::Toggle,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Toolbar,
                id: TOOLBAR_LAYERS_OWNER_ID.to_owned(),
            },
            selected: tab.layer_toggle_on(toggle, app.control_style()),
            availability: if gate.allows(capabilities) {
                available()
            } else {
                unavailable(
                    gate.reason()
                        .unwrap_or("source_does_not_support_this_layer"),
                )
            },
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: None,
        });
    }
}

/// The drawing tool rail, when it is on screen.
///
/// A folded rail contributes nothing: the scene reports what the trader can
/// see, and the keyboard shortcuts that still arm a tool are not controls.
fn push_tool_rail(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp) {
    let rail = app.control_tool_rail();
    if !rail.visible() {
        return;
    }
    let armed = rail.tool();
    let tools = [Tool::Pointer, Tool::Crosshair]
        .into_iter()
        .chain(DRAWING_TOOLS.into_iter().map(Tool::Drawing));
    for tool in tools {
        controls.push(SceneControlSnapshot {
            control_id: format!("{TOOL_RAIL_OWNER_ID}.tool.{}", tool.id()),
            label: tool.name().to_owned(),
            role: SceneRoleDto::Tool,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::ToolRail,
                id: TOOL_RAIL_OWNER_ID.to_owned(),
            },
            selected: tool == armed,
            availability: available(),
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: None,
        });
    }
}

/// The dock's tab strip, when the dock is on screen.
fn push_dock(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp) {
    let dock = app.control_dock();
    if !dock.visible() {
        return;
    }
    let open = dock.tab();
    for tab in DockTab::ALL {
        controls.push(SceneControlSnapshot {
            control_id: format!("{DOCK_OWNER_ID}.tab.{}", tab.id()),
            label: tab.title().to_owned(),
            role: SceneRoleDto::Tab,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Dock,
                id: DOCK_OWNER_ID.to_owned(),
            },
            selected: open == Some(tab),
            availability: available(),
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: None,
        });
    }
}

/// The chart canvases of the active tab.
///
/// These are the one place the scene has real bounds: a pane records the
/// rectangle it drew into as part of drawing it, so reporting it costs a read.
fn push_panes(controls: &mut Vec<SceneControlSnapshot>, tab: &Tab, focused: PaneSide) {
    for (pane, side) in visible_panes(tab) {
        let bounds = pane_bounds(pane);
        controls.push(SceneControlSnapshot {
            control_id: pane_canvas_control_id(pane.id),
            label: format!("{} chart", side_label(side)),
            role: SceneRoleDto::Canvas,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Tab,
                id: tab_control_id(tab.id),
            },
            selected: side == focused,
            availability: available(),
            bounds_availability: if bounds.is_some() {
                available()
            } else {
                unavailable("the_pane_has_not_been_drawn_yet")
            },
            bounds,
            capability_id: Some(CHART_WINDOW_CAPABILITY_ID.to_owned()),
        });
    }
}

fn pane_bounds(pane: &ChartPane) -> Option<SceneBoundsSnapshot> {
    let rect = pane.last_chart_area?;
    Some(SceneBoundsSnapshot {
        x_px: canonical_f32(rect.min.x, SCREEN_PIXEL_DECIMAL_PLACES)?,
        y_px: canonical_f32(rect.min.y, SCREEN_PIXEL_DECIMAL_PLACES)?,
        width_px: canonical_f32(rect.width(), SCREEN_PIXEL_DECIMAL_PLACES)?,
        height_px: canonical_f32(rect.height(), SCREEN_PIXEL_DECIMAL_PLACES)?,
    })
}

fn side_label(side: PaneSide) -> &'static str {
    match side {
        PaneSide::Flow => "Flow",
        PaneSide::Time => "Time",
    }
}

/// The panes the active layout actually shows, in the order they are drawn.
///
/// The same rule the cursor resolves against, so a pane the pointer can hit
/// is a pane the scene lists.
fn visible_panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let mut panes = Vec::with_capacity(2);
    if tab.layout.shows_time()
        && let Some(time) = &tab.time_pane
    {
        panes.push((time, PaneSide::Time));
    }
    if tab.layout.shows_flow() {
        panes.push((&tab.flow_pane, PaneSide::Flow));
    }
    panes
}

fn bounds_not_recorded() -> AvailabilitySnapshot {
    unavailable("bounds_are_not_recorded_for_this_control")
}
