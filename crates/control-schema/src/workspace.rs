//! Workspace, tab, layout, and focus snapshot.

use quantick_control_host::wire::{CanvasLayoutDto, PaneSideDto};

use quantick_control::wire::{CanonicalDecimal, WireU64};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "workspace.summary";

pub const MODULE_ID: &str = "workspace";

pub const SCHEMA_VERSION: u32 = 1;

pub const SPLIT_FRACTION_DECIMAL_PLACES: u32 = 6;

/// What `history_reach_span_minutes` reads as when a snapshot predates it.
///
/// Zero, which no running build ever reports — the setter clamps to at least
/// one minute — so a consumer can tell "this build did not say" from any real
/// span rather than being handed a plausible-looking two hours.
pub fn no_span_reported() -> WireU64 {
    WireU64::new(0)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceSnapshot {
    /// Native presentation order, independent of the retained runtime storage indices below.
    #[serde(default)]
    pub windows: Vec<crate::market_windows::MarketWindowSnapshot>,
    #[serde(default)]
    pub focused_window_id: Option<WireU64>,
    pub active_tab_index: WireU64,
    pub active_tab_id: WireU64,
    pub timezone_offset_minutes: i32,
    pub timezone_label: String,
    pub save_on_exit: bool,
    /// Whether layout edits are written into the tab's workspace as they are
    /// made (`workspace.autosave.set`). Absent from an older answer: on.
    #[serde(default = "auto_save_default")]
    pub auto_save: bool,
    pub performance_readings_visible: bool,
    /// Whether a feed that folds minutes builds its candle history a week
    /// at a time. A feed serving every interval asks one page at the pane's
    /// own interval either way, so the setting does not reach it.
    pub progressive_venue_history: bool,
    /// How far one press of *load older* reaches, in the v1 vocabulary this
    /// field has always used: `span` (an hours target; its length is
    /// `history_reach_span_minutes`) or `previous-session` (yesterday). A
    /// target that vocabulary has no word for — three or five days back —
    /// reads as the empty string; `history_target` names every target.
    ///
    /// Additive within v1 (contract §4): defaulted rather than required, so a
    /// client holding this schema still validates a summary from an instance
    /// built before the field existed.
    #[serde(default)]
    pub history_reach: String,
    /// What the History button's main click loads, as the token
    /// `feed.history.load` takes: `hours:N` or `sessions:N`. Additive; empty
    /// from a build that predates it.
    #[serde(default)]
    pub history_target: String,
    /// Minutes of *traded* time a press of the `span` reach pulls: the hours
    /// target's own length while `history_reach` reads `span`, otherwise what
    /// a legacy `span` token in a saved workspace or hook would load.
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
    pub layouts: Vec<crate::layout::LayoutTabSnapshot>,
    /// Every workspace and its colour, `Default` first, then by name.
    /// Additive; empty from a build that predates workspace colours.
    #[serde(default)]
    pub workspaces: Vec<WorkspaceColourRow>,
}

/// A workspace's colour: one slot of a fixed palette of eight, never held by
/// two workspaces (`workspace.colour.set`).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceColourDto {
    Steel,
    Violet,
    Cyan,
    Magenta,
    Azure,
    Indigo,
    Chalk,
    Cobalt,
}

impl WorkspaceColourDto {
    /// The palette, in the order a new workspace takes it.
    pub const ALL: [Self; 8] = [
        Self::Steel,
        Self::Violet,
        Self::Cyan,
        Self::Magenta,
        Self::Azure,
        Self::Indigo,
        Self::Chalk,
        Self::Cobalt,
    ];

    /// The colour of the palette slot at `index`.
    pub fn at(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }

    /// Its place in [`Self::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// One workspace's colour; `None` for a ninth while all eight are held.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceColourRow {
    pub name: String,
    pub colour: Option<WorkspaceColourDto>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceTab {
    #[serde(default)]
    pub window_id: Option<WireU64>,
    #[serde(default)]
    pub window_index: Option<WireU64>,
    #[serde(default)]
    pub selected_in_window: bool,
    pub index: WireU64,
    pub tab_id: WireU64,
    pub label: String,
    pub feed_id: String,
    pub symbol: String,
    pub active: bool,
    pub layout: CanvasLayoutDto,
    pub focused_pane: PaneSideDto,
    pub split_fraction: CanonicalDecimal,
    /// Whether the context column is collapsed to its rail. A collapsed chart
    /// is still counted `visible` in `panes`, because it comes back with its
    /// bars and drawings; this is the one field that says it is put away.
    /// The readback for `layout.pane.collapse` and `layout.pane.expand`, and
    /// `#[serde(default)]` so it is an optional, additive field of the v1
    /// payload.
    #[serde(default)]
    pub context_collapsed: bool,
    /// The right-hand chart has a draggable rail instead of its body.
    #[serde(default)]
    pub flow_collapsed: bool,
    pub panes: Vec<WorkspacePane>,
    /// The tab's venue. Additive; empty from a build that predates it.
    #[serde(default)]
    pub venue: String,
    /// The workspace this tab uses. Additive; empty from a build that predates
    /// it.
    #[serde(default)]
    pub workspace: String,
    /// Whether the tab holds edits it has not saved to that workspace.
    #[serde(default)]
    pub modified: bool,
    /// That workspace's colour, the bar on the tab's left edge. Additive.
    #[serde(default)]
    pub workspace_colour: Option<WorkspaceColourDto>,
    /// The canvas model: the preset the docked panes match and every pane's
    /// mode. Additive; absent from a build that predates it.
    #[serde(default)]
    pub canvas: Option<crate::canvas::CanvasView>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkspacePane {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    /// The pane's address within its tab — the number `layout.focus` takes.
    pub pane_index: WireU64,
    /// The layout this pane shows, by id in the strip.
    pub layout_id: WireU64,
    pub visible: bool,
    /// A folded pane retains its identity and data behind its rail.
    #[serde(default)]
    pub collapsed: bool,
    pub focused: bool,
}

/// An answer from before `auto_save` reads as the opening default.
const fn auto_save_default() -> bool {
    true
}
