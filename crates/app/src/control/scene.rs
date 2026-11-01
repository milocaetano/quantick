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
//! what each pane's header painted, [`DRAWING_TOOLS`] for the
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
//! others: the SOURCE, HISTORY and TRADE toolbar groups, the window
//! menus, the rail's trailing cluster and every dialog are still unnamed. A
//! capture says so in [`SceneSnapshot::coverage`], which is never `available`
//! for that reason, rather than reporting a short list as the whole screen
//! — a client that read this as complete would conclude those controls do
//! not exist, and inferred or incomplete data is labelled here as everywhere
//! else.
//!
//! ## Cost
//!
//! Projection runs only when requested. The title strip and pane headers publish
//! the painted control rectangles their drawing passes already compute; other
//! regions read existing frame state. A capture builds the list twice, once for
//! module revision and once for scope, as other capture-derived projections do.

pub(crate) use quantick_control_schema::scene::*;

use crate::app::{ChromePort, TabsPort};
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_SCENE_MAX_CONTROLS,
    registry::ModuleDescriptor,
    wire::WireU64,
};

use eframe::egui;
use quantick_chart::pane_header::{HeaderControl, view_name};

use crate::{
    dock::DockTab,
    feed::stall::Recovery,
    pane::{ChartPane, PaneSide},
    tab::Tab,
    toolrail::{RailControl, RailControlKind},
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{
        AvailabilitySnapshot, SCREEN_DECIMAL_PLACES, available, canonical_f32, unavailable,
        visible_panes,
    },
};

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

fn revision<P: TabsPort + ChromePort + ?Sized>(app: &P) -> SceneSnapshot {
    scene_snapshot(app)
}

fn project_controls<P: TabsPort + ChromePort + ?Sized>(
    app: &P,
    _context: CaptureContext,
) -> SceneSnapshot {
    scene_snapshot(app)
}

/// The identifier of one chart tab's entry in the tab strip.
fn tab_control_id(tab_id: u64) -> String {
    format!("{TAB_STRIP_OWNER_ID}.tab.{tab_id}")
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

pub(crate) fn scene_snapshot<P: TabsPort + ChromePort + ?Sized>(app: &P) -> SceneSnapshot {
    let tabs = app.tab_reads().tabs();
    let active = &tabs[app.tab_reads().active_tab_index()];
    let focused_side = active.focused_side();
    let mut controls = Vec::new();

    // The chart canvases go first, and deliberately: they are what the cursor
    // resolves to, and the cap below cuts from the end. A workspace large
    // enough to truncate must not be one where `interaction.cursor` answers
    // with a control ID this scope no longer contains.
    //
    // While Home is up none of them is on screen, so none is listed.
    let active_id = tabs.active_id();
    let active_chip = app
        .chrome_reads()
        .title_bar()
        .map_or(Some(active_id), |title| {
            title.surface.active_chip(active_id)
        });
    let home = active_chip.is_none();
    let mut focused_pane_id = None;
    if !home {
        focused_pane_id = push_panes(&mut controls, tabs.active_id(), active, focused_side);
        push_context_dividers(&mut controls, tabs.active_id(), active);
        push_quick_range(&mut controls, app);
        push_pane_headers(&mut controls, app, tabs.active_id(), active);
    }
    push_tool_rail(&mut controls, app);
    push_dock(&mut controls, app);
    // With the other fixed-size regions, ahead of the strip. At most three
    // entries, and they are the ones worth keeping if a workspace ever reaches
    // the ceiling: the chip is the only control on screen with no capability
    // behind it, so an operator that loses it loses its only way to the corner
    // at all.
    push_feed_status(&mut controls, app);
    // Bounded like every other projection, and honest about it. Every registry
    // behind the scene is fixed-size except the trader's own tab strip, which
    // is why the strip is walked last and why it is the one walk told when to
    // stop. Cutting a finished list would still have built it: an implausible
    // number of open charts must cost a truncated answer, not an unbounded
    // allocation on the application thread.
    let strip_complete = push_tab_strip(
        &mut controls,
        tabs,
        active_chip,
        app.chrome_reads().title_bar(),
    );
    push_title_bar(&mut controls, app);

    let coverage = if !strip_complete || controls.len() > CONTROL_SCENE_MAX_CONTROLS {
        controls.truncate(CONTROL_SCENE_MAX_CONTROLS);
        unavailable("control_count_exceeded_the_scene_limit")
    } else {
        // Never `available()`. A region in `covered_regions` was *walked*,
        // not exhausted: a pane header contributes its view, layers and sizes with
        // bounds and its bars and indicators calls without, the toolbar its quick
        // range and not its SOURCE, HISTORY or TRADE groups, the rail its tools
        // and not its trailing cluster. The title strip contributes visible chips
        // and window controls, but not an open overflow popup's entries.
        // Reporting completeness here would tell a client the controls this
        // walk does not reach are not on screen, which is the one lie the
        // module cannot tolerate.
        unavailable("only_the_named_group_of_each_covered_region_is_enumerated")
    };

    SceneSnapshot {
        active_tab_id: WireU64::new(tabs.active_id()),
        active_surface: if home {
            ActiveSurfaceDto::Home
        } else {
            ActiveSurfaceDto::Market
        },
        // From the walk, not from `Tab::pane`. The two disagree for the frame
        // between a tab asking for the time layout and its time pane being
        // built: `focused_side` falls back to the flow pane while the layout
        // shows no flow pane at all, so naming a focused pane there would
        // name a canvas this capture does not list.
        focused_pane_id: focused_pane_id.map(WireU64::new),
        focused_pane_side: crate::control::pane_side_dto(focused_side),
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

/// The contextual drawing actions visible after a temporary range settles.
fn push_quick_range<P: ChromePort + ?Sized>(controls: &mut Vec<SceneControlSnapshot>, app: &P) {
    use crate::surfaces::drawing_chrome::QuickRangeActionUi as _;
    let Some(actions) = app.chrome_reads().quick_range_actions() else {
        return;
    };
    for control in actions {
        let bounds = rect_bounds(control.rect);
        // A button that only arms a placement is a tool, not a call: the
        // trader's next click finishes it, so no capability is what pressing
        // it does.
        let arms = control.action.arms_placement();
        controls.push(SceneControlSnapshot {
            control_id: control.action.control_id().to_owned(),
            label: control.action.label().to_owned(),
            role: if arms {
                SceneRoleDto::Tool
            } else {
                SceneRoleDto::Action
            },
            owner: SceneOwnerSnapshot {
                // A contextual toolbar over the chart. Reusing the region kind is
                // also schema-compatible for existing scene clients; the owner ID
                // distinguishes it from the fixed toolbar above the canvas.
                kind: SceneOwnerKindDto::Toolbar,
                id: QUICK_RANGE_OWNER_ID.to_owned(),
            },
            selected: false,
            availability: control
                .unavailable_reason
                .map_or_else(available, unavailable),
            bounds_availability: match &bounds {
                Bounds::Rect(_) => available(),
                Bounds::NotDrawn => unavailable("the_action_has_not_been_drawn_yet"),
                Bounds::NotReportable => {
                    unavailable("the_actions_rectangle_is_not_a_reportable_number")
                }
            },
            bounds: bounds.into_snapshot(),
            capability_id: (!arms).then(|| control.action.capability_id().to_owned()),
        });
    }
}

/// Stable pair IDs are shared with the pointer handle and the resize request.
fn push_context_dividers(controls: &mut Vec<SceneControlSnapshot>, tab_id: u64, tab: &Tab) {
    let Some((_, bands)) = tab.context_stack_geometry() else {
        return;
    };
    for (index, divider) in bands.dividers.iter().enumerate() {
        let upper = tab
            .pane_at(index + 1)
            .expect("a drawn upper context pane")
            .id;
        let lower = tab
            .pane_at(index + 2)
            .expect("a drawn lower context pane")
            .id;
        let bounds = rect_bounds(*divider);
        controls.push(SceneControlSnapshot {
            control_id: format!("tab.{}.context_divider.{upper}.{lower}", tab_id),
            label: format!("Resize context panes {upper} and {lower}"),
            role: SceneRoleDto::Action,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::Tab,
                id: tab_control_id(tab_id),
            },
            selected: false,
            availability: available(),
            bounds_availability: match &bounds {
                Bounds::Rect(_) => available(),
                _ => unavailable("the_divider_rectangle_is_not_reportable"),
            },
            bounds: bounds.into_snapshot(),
            capability_id: Some(super::layout::stack::RESIZE_PAIR_CAPABILITY_ID.to_owned()),
        });
    }
}

/// The open charts, in strip order, up to the scene's ceiling.
///
/// Answers whether every open chart fitted. The last walk, and the only one
/// the trader can grow, so this is where the ceiling is enforced rather than
/// applied to a list that has already been built.
fn push_tab_strip(
    controls: &mut Vec<SceneControlSnapshot>,
    tabs: &crate::app::arrangement_host::ArrangementHost,
    selected_id: Option<u64>,
    title: Option<&crate::app::title_bar::TitleBarState>,
) -> bool {
    for (index, (tab_id, tab)) in tabs.iter_with_ids().enumerate() {
        let rect = title.and_then(|title| {
            title
                .strip
                .tabs
                .iter()
                .find(|(visible, _)| *visible == index)
                .map(|(_, rect)| *rect)
        });
        if title.is_some() && rect.is_none() {
            continue;
        }
        if controls.len() >= CONTROL_SCENE_MAX_CONTROLS {
            return false;
        }
        controls.push(SceneControlSnapshot {
            control_id: tab_control_id(tab_id),
            // The string the chip paints, not one assembled here: an
            // assistant that named a tab something the trader cannot see
            // would be describing a different screen.
            label: tab.chip_label().to_owned(),
            role: SceneRoleDto::Tab,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::TabStrip,
                id: TAB_STRIP_OWNER_ID.to_owned(),
            },
            selected: selected_id == Some(tab_id),
            availability: available(),
            bounds: rect.and_then(|rect| rect_bounds(rect).into_snapshot()),
            bounds_availability: rect.map_or_else(bounds_not_recorded, |_| available()),
            capability_id: None,
        });
    }
    true
}

fn push_title_bar<P: ChromePort + ?Sized>(controls: &mut Vec<SceneControlSnapshot>, app: &P) {
    let Some(title) = app.chrome_reads().title_bar() else {
        return;
    };
    for (id, label, rect, capability) in title
        .controls
        .iter()
        .map(|(id, rect)| {
            (
                *id,
                match *id {
                    "drag" => "Move window (native OS gesture)",
                    "minimize" => "Minimize window",
                    "maximize" if title.maximized => "Restore window",
                    "maximize" => "Maximize window",
                    "close" => "Close window",
                    _ => unreachable!("the title strip publishes registered controls"),
                },
                Some(*rect),
                (!matches!(*id, "drag" | "close"))
                    .then_some(quantick_control_schema::native_window::WINDOW_STATE_CAPABILITY_ID),
            )
        })
        .chain([
            (
                "home",
                "Home",
                title.strip.home,
                Some(quantick_control_schema::home_tab::HOME_SHOW_CAPABILITY_ID),
            ),
            ("new", "Open another market", title.strip.add, None),
            ("overflow", "All markets", title.strip.overflow, None),
        ])
    {
        let Some(rect) = rect else {
            continue;
        };
        controls.push(SceneControlSnapshot {
            control_id: format!("tab_strip.{id}"),
            label: label.to_owned(),
            role: SceneRoleDto::Action,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::TabStrip,
                id: TAB_STRIP_OWNER_ID.to_owned(),
            },
            selected: (id == "maximize" && title.maximized)
                || (id == "minimize" && title.minimized)
                || (id == "home" && title.surface.is_home()),
            availability: available(),
            bounds: rect_bounds(rect).into_snapshot(),
            bounds_availability: available(),
            capability_id: capability.map(str::to_owned),
        });
    }
}

/// Every pane's header line and capsules, as painted: a side too narrow for its
/// capsule, a glyph the data blocks or a clipped one is not listed. The state is read now, through the calls
/// the header drew from. The tab owns them, as it owns its panes; the control's
/// own id, `pane.<id>.header.*`, says which pane.
fn push_pane_headers<P: TabsPort + ChromePort + ?Sized>(
    controls: &mut Vec<SceneControlSnapshot>,
    app: &P,
    tab_id: u64,
    tab: &Tab,
) {
    let capabilities = tab.capabilities(app.tab_reads().config());
    let style = app.chrome_reads().style();
    for (pane, _) in tab.panes() {
        for &(control, rect) in &pane.frame.header_controls {
            let (label, role, selected, blocked, capability) = match control {
                HeaderControl::View => (
                    pane.orderflow.as_ref().map_or("", |tape| {
                        view_name(tape.cached_config().live_lane.shown_view())
                    }),
                    SceneRoleDto::Action,
                    false,
                    None,
                    quantick_control_schema::tape_view_set::TAPE_VIEW_SET_CAPABILITY_ID,
                ),
                HeaderControl::Layer(toggle) => (
                    toggle.name(),
                    SceneRoleDto::Toggle,
                    pane.layer_switched_on(toggle.layer(), style),
                    pane.layer_blocked(toggle.layer(), capabilities),
                    quantick_control_schema::layers::SET_VISIBILITY_CAPABILITY_ID,
                ),
                HeaderControl::Sample(sample) => (
                    sample.name(),
                    SceneRoleDto::Action,
                    false,
                    None,
                    quantick_control_schema::bubble_scale::BUBBLE_SCALE_CAPABILITY_ID,
                ),
            };
            let bounds = rect_bounds(rect);
            controls.push(SceneControlSnapshot {
                control_id: format!("pane.{}.header.{}", pane.id, control.id_in_header()),
                label: label.to_owned(),
                role,
                owner: SceneOwnerSnapshot {
                    kind: SceneOwnerKindDto::Tab,
                    id: tab_control_id(tab_id),
                },
                selected,
                // The very gate the glyph reads, in its coded rendering: one
                // condition, two audiences, no chance of drift.
                availability: blocked.map_or_else(available, |block| unavailable(block.code)),
                bounds_availability: match &bounds {
                    Bounds::Rect(_) => available(),
                    _ => unavailable("the_controls_rectangle_is_not_reportable"),
                },
                bounds: bounds.into_snapshot(),
                capability_id: Some(capability.to_owned()),
            });
        }
    }
}

/// The drawing tool rail, when it is on screen.
///
/// A folded rail contributes nothing: the scene reports what the trader can
/// see, and the keyboard shortcuts that still arm a tool are not controls.
/// [`crate::toolrail::ToolRail::painted_controls`] answers with nothing in
/// that case, so the rule lives with the rail rather than here.
fn push_tool_rail<P: ChromePort + ?Sized>(controls: &mut Vec<SceneControlSnapshot>, app: &P) {
    let rail = app.chrome_reads().tool_rail();
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
    // The magnet's own button, where the rail draws one; the snapshot's
    // `analysis.drawings` reads its state whether or not it is on screen.
    if rail.magnet_painted() {
        controls.push(SceneControlSnapshot {
            control_id: format!("{TOOL_RAIL_OWNER_ID}.toggle.magnet"),
            label: "Snap anchors to OHLC".to_owned(),
            role: SceneRoleDto::Toggle,
            owner: SceneOwnerSnapshot {
                kind: SceneOwnerKindDto::ToolRail,
                id: TOOL_RAIL_OWNER_ID.to_owned(),
            },
            selected: rail.magnet(),
            availability: available(),
            bounds: None,
            bounds_availability: bounds_not_recorded(),
            capability_id: Some(super::drawing_magnet::DRAWING_MAGNET_CAPABILITY_ID.to_owned()),
        });
    }
}

/// The dock's tab strip, when the dock is on screen.
fn push_dock<P: ChromePort + ?Sized>(controls: &mut Vec<SceneControlSnapshot>, app: &P) {
    let dock = app.chrome_reads().dock();
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
fn push_feed_status<P: ChromePort + ?Sized>(controls: &mut Vec<SceneControlSnapshot>, app: &P) {
    let Some(chip) = app.chrome_reads().feed_chip_rect() else {
        return;
    };
    let owner = || SceneOwnerSnapshot {
        kind: SceneOwnerKindDto::FeedStatus,
        id: FEED_STATUS_OWNER_ID.to_owned(),
    };
    let popup_open = app.chrome_reads().feed_popup_open();
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
            capability_id: Some(
                quantick_control_handlers::recovery::capability_id(recovery).to_owned(),
            ),
        });
    }
}

/// The chart canvases of the active tab.
///
/// These are the one place the scene has real bounds: a pane records the
/// rectangle it drew into as part of drawing it, so reporting it costs a read.
fn push_panes(
    controls: &mut Vec<SceneControlSnapshot>,
    tab_id: u64,
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
                id: tab_control_id(tab_id),
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
        controls.extend(pane_header_controls(
            pane.id,
            &side.title(),
            tab_control_id(tab_id),
            bounds_not_recorded(),
        ));
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
    let Some(rect) = pane.frame.chart_area else {
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
