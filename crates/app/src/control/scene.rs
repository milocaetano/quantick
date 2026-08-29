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
//! A control that is not painted is not listed. A rail folded away to its
//! narrow stage contributes only the buttons that stage draws; a hidden dock
//! contributes no tabs. Availability is a separate question from presence:
//! the L2 heatmap toggle is on screen and disabled on a source that captures
//! no book, and it says so with a reason code a client can branch on.
//!
//! ## What it does not cover yet
//!
//! The scene enumerates the regions listed in [`SceneOwnerKindDto`] and no
//! others: the SOURCE, BARS, HISTORY and TRADE toolbar groups, the window
//! menus, the rail's trailing cluster and every dialog are still unnamed. A
//! capture says so in [`SceneSnapshot::coverage`], which is never `available`
//! for that reason, rather than reporting a short list as the whole screen
//! — a client that read this as complete would conclude those controls do
//! not exist, and inferred or incomplete data is labelled here as everywhere
//! else.
//!
//! ## Cost
//!
//! Nothing here runs unless a client asks, and the frame writes down nothing
//! for its benefit but the rail's stage — one enum store per frame, which is
//! what lets the rail be reported honestly instead of guessed at. The
//! projection then reads state the frame already keeps: no rectangle is
//! recorded, no hit test is run, no layout is measured. Note that a capture
//! builds the list *twice* — once for the module revision and once for the
//! scope — because module revisions are capture-derived across every module
//! (PR 2's recorded deferral); this is the largest projection in the registry
//! and the first that would benefit from the journal-driven change counters
//! that replace it.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_SCENE_MAX_CONTROLS,
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use eframe::egui;

use crate::{
    app::QuantickApp,
    chart_layers::ChartLayer,
    dock::DockTab,
    feed::stall::Recovery,
    pane::{ChartPane, PaneSide},
    tab::Tab,
    toolbar::LayerToggle,
    toolrail::{RailControl, RailControlKind},
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{
        AvailabilitySnapshot, PaneSideDto, SCREEN_DECIMAL_PLACES, available, canonical_f32,
        unavailable, visible_panes,
    },
};

pub(crate) const CONTROLS_SCOPE_ID: &str = "scene.controls";
const MODULE_ID: &str = "scene";
const SCHEMA_VERSION: u32 = 1;

/// The identifier prefix of every control the tab strip owns.
const TAB_STRIP_OWNER_ID: &str = "tab_strip";
/// The LAYERS group of the context toolbar.
const TOOLBAR_LAYERS_OWNER_ID: &str = "toolbar.layers";
/// The drawing tool rail.
const TOOL_RAIL_OWNER_ID: &str = "tool_rail";
/// The right-hand dock and its tab strip.
const DOCK_OWNER_ID: &str = "dock";
/// The feed's offline corner: the chip, and the popup it opens.
const FEED_STATUS_OWNER_ID: &str = "feed_status";
/// The chip itself.
const FEED_CHIP_CONTROL_ID: &str = "feed_status.chip";
/// The popup's two controls, named for the capability each one calls.
const FEED_RECONNECT_CONTROL_ID: &str = "feed_status.reconnect";
const FEED_RELOAD_CONTROL_ID: &str = "feed_status.reload";

/// What is on screen right now, as controls an operator can name.
///
/// The tree is expressed as a flat list plus [`SceneControlSnapshot::owner`]
/// rather than as nested arrays: a client that wants the tree walks the owner
/// links, and a client that wants one control by ID does not have to descend
/// into anything to find it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneSnapshot {
    pub active_tab_id: WireU64,
    /// The focused canvas, when this capture lists one.
    ///
    /// Absent while the active tab is between layouts and has no painted
    /// pane at all — an honest gap rather than an ID pointing at a control
    /// `controls` does not contain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_pane_id: Option<WireU64>,
    pub focused_pane_side: PaneSideDto,
    pub controls: Vec<SceneControlSnapshot>,
    /// The regions this capture enumerated, in the order it walked them.
    ///
    /// The honest bound on everything above: a control belonging to a region
    /// absent from this list was not looked for, and its absence from
    /// `controls` says nothing about whether it is on screen.
    pub covered_regions: Vec<SceneOwnerKindDto>,
    /// Whether `controls` is every control on screen. It never is yet.
    ///
    /// Two things cut it, and this says which. Each covered region is walked
    /// only as far as one group of it — the toolbar's LAYERS, the rail's
    /// tools, the dock's tab strip, a tab's canvases — so a capture that has
    /// truncated nothing is still not the screen, and reports as much rather
    /// than letting a client read a short list as a complete one. Beyond
    /// that the scene is bounded like every other projection, and a capture
    /// that met the bound says so instead of truncating in silence.
    pub coverage: AvailabilitySnapshot,
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
    /// The registered capability that *operates* this control, where one
    /// exists.
    ///
    /// Absent on every control today, and honestly so: reading the screen came
    /// before acting on it, and the cockpit tier that registers a capability
    /// per control is still ahead. An absent ID means "not reachable through
    /// the control plane yet", never "not reachable at all" — and a capability
    /// that merely *reads about* a control (a page of a canvas's bars) is not
    /// one that operates it, so it does not go here.
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
    /// A control that does one thing and returns, leaving no mode behind.
    ///
    /// The first role whose controls carry a `capability_id`: pressing one is
    /// exactly a call, so the scene can say which call it is instead of
    /// leaving an operator to guess from the label.
    Action,
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
    /// The feed's offline corner, bottom-right of the chart.
    ///
    /// Present only while the chart is not being fed — which is the whole
    /// point of it, and why a capture with no `feed_status` control in it is
    /// an operator's evidence that the feed is healthy rather than a gap in
    /// the walk.
    FeedStatus,
}

/// A control's rectangle in window coordinates, in **logical points**.
///
/// Not device pixels: the window lays out in points, and the two differ by the
/// display's scale factor — a canvas on a 200% display occupies twice these
/// numbers in the framebuffer. Reported as points anyway, because that is the
/// unit the pointer is reported in too (`interaction.cursor`), so the two
/// scopes can be compared without a conversion neither of them knows the
/// factor for. A client composing these with a screenshot must scale them
/// itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SceneBoundsSnapshot {
    #[schemars(extend("x-unit" = "logical_points"))]
    pub x_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub y_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub width_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub height_pt: CanonicalDecimal,
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

/// The identifier of one layer toggle in the toolbar's LAYERS group.
fn layer_control_id(layer: ChartLayer) -> String {
    format!("{TOOLBAR_LAYERS_OWNER_ID}.{}", layer.id())
}

/// The identifier of one button on the drawing rail.
///
/// The rail draws from three registries and they share no namespace: a tool
/// and a family may carry the same registered name (`brush` and `measure` do
/// today), and a starred tool is painted a second time in the pinned section
/// beside its slot in the run. The kind is part of the identifier so one name
/// cannot mean the family flyout on a wide window and the tool itself on a
/// narrow one.
fn rail_control_id(control: &RailControl) -> String {
    let registry = match control.kind {
        RailControlKind::Tool => "tool",
        RailControlKind::Family => "family",
        RailControlKind::Favorite => "favorite",
    };
    format!("{TOOL_RAIL_OWNER_ID}.{registry}.{}", control.id)
}

/// The identifier of one tab on the dock's strip.
fn dock_tab_control_id(tab: DockTab) -> String {
    format!("{DOCK_OWNER_ID}.tab.{}", tab.id())
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

    // The chart canvases go first, and deliberately: they are what the cursor
    // resolves to, and the cap below cuts from the end. A workspace large
    // enough to truncate must not be one where `interaction.cursor` answers
    // with a control ID this scope no longer contains.
    let focused_pane_id = push_panes(&mut controls, active, focused_side);
    push_layer_toggles(&mut controls, app, active);
    push_tool_rail(&mut controls, app);
    push_dock(&mut controls, app);
    push_feed_status(&mut controls, app);
    // Bounded like every other projection, and honest about it. Every registry
    // behind the scene is fixed-size except the trader's own tab strip, which
    // is why the strip is walked last and why it is the one walk told when to
    // stop. Cutting a finished list would still have built it: an implausible
    // number of open charts must cost a truncated answer, not an unbounded
    // allocation on the application thread.
    let strip_complete = push_tab_strip(&mut controls, tabs, active.id);

    let coverage = if !strip_complete || controls.len() > CONTROL_SCENE_MAX_CONTROLS {
        controls.truncate(CONTROL_SCENE_MAX_CONTROLS);
        unavailable("control_count_exceeded_the_scene_limit")
    } else {
        // Never `available()`. A region in `covered_regions` was *walked*,
        // not exhausted: the toolbar contributes its LAYERS group and not the
        // SOURCE, BARS, HISTORY or TRADE groups beside it, the rail its tools
        // and not its trailing cluster, the strip its chips and not the `+`.
        // Reporting completeness here would tell a client the controls this
        // walk does not reach are not on screen, which is the one lie the
        // module cannot tolerate.
        unavailable("only_the_named_group_of_each_covered_region_is_enumerated")
    };

    SceneSnapshot {
        active_tab_id: WireU64::new(active.id),
        // From the walk, not from `Tab::pane`. The two disagree for the frame
        // between a tab asking for the time layout and its time pane being
        // built: `focused_side` falls back to the flow pane while the layout
        // shows no flow pane at all, so naming a focused pane there would
        // name a canvas this capture does not list.
        focused_pane_id: focused_pane_id.map(WireU64::new),
        focused_pane_side: focused_side.into(),
        controls,
        covered_regions: COVERED_REGIONS.to_vec(),
        coverage,
    }
}

/// The regions [`scene_snapshot`] walks, in the order it walks them.
///
/// The list a capture publishes as its own bound. Adding a region here without
/// adding the walk that fills it would be the one lie this module cannot
/// tolerate, so the two live one above the other.
const COVERED_REGIONS: [SceneOwnerKindDto; 6] = [
    SceneOwnerKindDto::Tab,
    SceneOwnerKindDto::Toolbar,
    SceneOwnerKindDto::ToolRail,
    SceneOwnerKindDto::Dock,
    SceneOwnerKindDto::FeedStatus,
    SceneOwnerKindDto::TabStrip,
];

/// The open charts, in strip order, up to the scene's ceiling.
///
/// Answers whether every open chart fitted. The last walk, and the only one
/// the trader can grow, so this is where the ceiling is enforced rather than
/// applied to a list that has already been built.
fn push_tab_strip(controls: &mut Vec<SceneControlSnapshot>, tabs: &[Tab], active_id: u64) -> bool {
    for tab in tabs {
        if controls.len() >= CONTROL_SCENE_MAX_CONTROLS {
            return false;
        }
        controls.push(SceneControlSnapshot {
            control_id: tab_control_id(tab.id),
            // The string the chip paints, not one assembled here: an
            // assistant that named a tab something the trader cannot see
            // would be describing a different screen.
            label: tab.chip_label().to_owned(),
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
    true
}

/// The toolbar's LAYERS group: one toggle per visual layer, each answering
/// for the same field the pane's own layer menu writes.
fn push_layer_toggles(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp, tab: &Tab) {
    let capabilities = tab.capabilities(app.control_config());
    // `LayerToggle::ALL` is call order, which the group's right-to-left layout
    // turns into right-to-left screen order. Reversed here so the scene lists
    // them the way the trader reads them: an assistant asked about "the third
    // button from the left" must count the same direction the eye does.
    for toggle in LayerToggle::ALL.into_iter().rev() {
        let layer = toggle.layer();
        let (on, blocked) = tab.layer_toggle_state(layer, app.control_style(), capabilities);
        controls.push(SceneControlSnapshot {
            control_id: layer_control_id(layer),
            label: layer.label().to_owned(),
            role: SceneRoleDto::Toggle,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Toolbar,
                id: TOOLBAR_LAYERS_OWNER_ID.to_owned(),
            },
            selected: on,
            // The very gate the disabled button reads, in its coded rendering:
            // one condition, two audiences, no chance of drift.
            availability: match blocked {
                None => available(),
                Some(block) => unavailable(block.code),
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
/// [`crate::toolrail::ToolRail::painted_controls`] answers with nothing in
/// that case, so the rule lives with the rail rather than here.
fn push_tool_rail(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp) {
    let rail = app.control_tool_rail();
    // What the rail *painted*, folded through the same slots the draw folds
    // through and cut by the stage and the band window the draw recorded.
    // Listing the registry instead would name thirteen tools that live behind
    // a family flyout, the ones the band has scrolled out of sight, and two
    // more the narrow stages drop entirely.
    for control in rail.painted_controls() {
        controls.push(SceneControlSnapshot {
            control_id: rail_control_id(&control),
            label: control.label.to_owned(),
            role: SceneRoleDto::Tool,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::ToolRail,
                id: TOOL_RAIL_OWNER_ID.to_owned(),
            },
            selected: control.armed,
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
            control_id: dock_tab_control_id(tab),
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

/// The feed's offline corner, while there is one.
///
/// Unlike every other region here, this one is usually empty: the chip is
/// drawn only when the chart is not being fed, so an operator reading a
/// capture with no `feed_status` control has been told the feed is healthy.
/// The two recovery controls join it only while the popup is open, because a
/// control behind a click is not on screen — and they are the first entries in
/// this module to name the capability that operates them, which is the whole
/// reason `capability_id` exists.
fn push_feed_status(controls: &mut Vec<SceneControlSnapshot>, app: &QuantickApp) {
    let Some(chip) = app.control_feed_chip_rect() else {
        return;
    };
    let owner = || SceneOwnerSnapshot {
        kind: SceneOwnerKindDto::FeedStatus,
        id: FEED_STATUS_OWNER_ID.to_owned(),
    };
    let popup_open = app.control_feed_popup_open();
    let chip_bounds = rect_bounds(chip);
    controls.push(SceneControlSnapshot {
        control_id: FEED_CHIP_CONTROL_ID.to_owned(),
        // The word the chip paints, not one assembled here.
        label: crate::feed_notice::OFFLINE_LABEL.to_owned(),
        role: SceneRoleDto::Toggle,
        owner: owner(),
        selected: popup_open,
        availability: available(),
        bounds_availability: match &chip_bounds {
            Bounds::Rect(_) => available(),
            Bounds::NotDrawn => unavailable("the_chip_has_not_been_drawn_yet"),
            Bounds::NotReportable => unavailable("the_chips_rectangle_is_not_a_reportable_number"),
        },
        bounds: chip_bounds.into_snapshot(),
        // Opening the popup is a gesture, not a call: everything behind it is
        // reachable directly, so a capability whose only effect is to show a
        // human something would be a second way to say the same thing.
        capability_id: None,
    });
    if !popup_open {
        return;
    }
    for (control_id, recovery) in [
        (FEED_RECONNECT_CONTROL_ID, Recovery::Reconnect),
        (FEED_RELOAD_CONTROL_ID, Recovery::Reload),
    ] {
        controls.push(SceneControlSnapshot {
            control_id: control_id.to_owned(),
            label: recovery.label().to_owned(),
            role: SceneRoleDto::Action,
            owner: owner(),
            // Which of the two leads is the application's judgement about
            // *this* stall, and it is a matter of emphasis rather than of
            // state: neither control is chosen until it is pressed.
            selected: false,
            availability: available(),
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: Some(super::recovery::capability_id(recovery).to_owned()),
        });
    }
}

/// The chart canvases of the active tab.
///
/// These are the one place the scene has real bounds: a pane records the
/// rectangle it drew into as part of drawing it, so reporting it costs a read.
fn push_panes(
    controls: &mut Vec<SceneControlSnapshot>,
    tab: &Tab,
    focused: PaneSide,
) -> Option<u64> {
    let mut focused_pane_id = None;
    for (pane, side) in visible_panes(tab) {
        if side == focused {
            focused_pane_id = Some(pane.id);
        }
        let bounds = pane_bounds(pane);
        controls.push(SceneControlSnapshot {
            control_id: pane_canvas_control_id(pane.id),
            label: format!("{} chart", side.title()),
            role: SceneRoleDto::Canvas,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Tab,
                id: tab_control_id(tab.id),
            },
            selected: side == focused,
            availability: available(),
            bounds_availability: match &bounds {
                Bounds::Rect(_) => available(),
                Bounds::NotDrawn => unavailable("the_pane_has_not_been_drawn_yet"),
                Bounds::NotReportable => {
                    unavailable("the_panes_rectangle_is_not_a_reportable_number")
                }
            },
            bounds: bounds.into_snapshot(),
            capability_id: None,
        });
    }
    focused_pane_id
}

/// Why a pane has no rectangle, kept apart from *having* one so the two
/// reasons never borrow each other's words: a pane drawn into a degenerate
/// layout was drawn, and telling a polling client it was not would leave it
/// waiting for a state that has already arrived.
///
/// The second reason is "not reportable", not "not finite": a coordinate can
/// also be a perfectly finite number that no decimal on this wire can carry,
/// and a client branching on a code that named non-finiteness would be told
/// something false about it.
enum Bounds {
    Rect(SceneBoundsSnapshot),
    NotDrawn,
    NotReportable,
}

impl Bounds {
    fn into_snapshot(self) -> Option<SceneBoundsSnapshot> {
        match self {
            Self::Rect(bounds) => Some(bounds),
            Self::NotDrawn | Self::NotReportable => None,
        }
    }
}

fn pane_bounds(pane: &ChartPane) -> Bounds {
    let Some(rect) = pane.last_chart_area else {
        return Bounds::NotDrawn;
    };
    rect_bounds(rect)
}

/// One recorded rectangle, in the units and with the refusals this scope
/// promises.
fn rect_bounds(rect: egui::Rect) -> Bounds {
    // `egui::Rect` does not normalise, so a degenerate layout can hand back a
    // rectangle whose corners are the wrong way round. A negative width is a
    // number a client would happily halve to find a centre, landing outside
    // the canvas; refused with the reason instead. `NaN` fails both tests.
    if !(rect.width() >= 0.0 && rect.height() >= 0.0) {
        return Bounds::NotReportable;
    }
    let point = |value: f32| canonical_f32(value, SCREEN_DECIMAL_PLACES);
    match (
        point(rect.min.x),
        point(rect.min.y),
        point(rect.width()),
        point(rect.height()),
    ) {
        (Some(x_pt), Some(y_pt), Some(width_pt), Some(height_pt)) => {
            Bounds::Rect(SceneBoundsSnapshot {
                x_pt,
                y_pt,
                width_pt,
                height_pt,
            })
        }
        _ => Bounds::NotReportable,
    }
}

fn bounds_not_recorded() -> AvailabilitySnapshot {
    unavailable("bounds_are_not_recorded_for_this_control")
}
