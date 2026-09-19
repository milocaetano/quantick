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

use quantick_control_host::wire::{AvailabilitySnapshot, PaneSideDto};

use quantick_control::wire::{CanonicalDecimal, WireU64};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const CONTROLS_SCOPE_ID: &str = "scene.controls";

pub const MODULE_ID: &str = "scene";

pub const SCHEMA_VERSION: u32 = 1;

/// The identifier prefix of every control the tab strip owns.
pub const TAB_STRIP_OWNER_ID: &str = "tab_strip";

/// The LAYERS group of the context toolbar.
pub const TOOLBAR_LAYERS_OWNER_ID: &str = "toolbar.layers";

/// The drawing tool rail.
pub const TOOL_RAIL_OWNER_ID: &str = "tool_rail";

/// The right-hand dock and its tab strip.
pub const DOCK_OWNER_ID: &str = "dock";

/// The feed's offline corner: the chip, and the popup it opens.
pub const FEED_STATUS_OWNER_ID: &str = "feed_status";

/// The chip itself.
pub const FEED_CHIP_CONTROL_ID: &str = "feed_status.chip";

/// The popup's two controls, named for the capability each one calls.
pub const FEED_RECONNECT_CONTROL_ID: &str = "feed_status.reconnect";

pub const FEED_RELOAD_CONTROL_ID: &str = "feed_status.reload";

/// The compact action bar raised by a completed secondary-button range.
pub const QUICK_RANGE_OWNER_ID: &str = "quick_range";

/// What is on screen right now, as controls an operator can name.
///
/// The tree is expressed as a flat list plus [`SceneControlSnapshot::owner`]
/// rather than as nested arrays: a client that wants the tree walks the owner
/// links, and a client that wants one control by ID does not have to descend
/// into anything to find it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneSnapshot {
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
pub struct SceneControlSnapshot {
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
pub enum SceneRoleDto {
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
pub struct SceneOwnerSnapshot {
    pub kind: SceneOwnerKindDto,
    /// The owner's own identifier. Where the owner is itself a control in this
    /// list — a chart tab owning its panes — this is that control's ID, so the
    /// tree can be walked without a second vocabulary.
    pub id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneOwnerKindDto {
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
pub struct SceneBoundsSnapshot {
    #[schemars(extend("x-unit" = "logical_points"))]
    pub x_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub y_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub width_pt: CanonicalDecimal,
    #[schemars(extend("x-unit" = "logical_points"))]
    pub height_pt: CanonicalDecimal,
}
