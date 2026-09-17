//! Stable saved arrangement vocabulary; runtime conversion stays in the app.
use serde::{Deserialize, Serialize};

/// The canvas layout a feed declares its tabs open on (`default_layout` in
/// the TOML), named for what each layout shows.
///
/// A config-side twin of `crate::tab::CanvasLayout` rather than that enum
/// itself, so the TOML vocabulary — part of the user-facing config contract —
/// cannot drift when the canvas grows a layout a config should not name.
/// Serialized as well as deserialized: the saved workspace
/// (`crate::ui_state`) writes a canvas layout back out, and it must speak
/// the vocabulary the config reads — one name for one layout, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum DeclaredLayout {
    /// The flow pane alone — the factory default.
    #[serde(rename = "flow")]
    Flow,
    /// A full-window timeframe chart.
    #[serde(rename = "time")]
    Time,
    /// Timeframe left, flow right, on the draggable divider.
    #[serde(rename = "time+flow")]
    TimeAndFlow,
    /// Two timeframe charts stacked left, flow right.
    #[serde(rename = "time+time+flow")]
    TimeTimeAndFlow,
}

impl DeclaredLayout {
    /// Parse the same names the serde renames above accept, for callers
    /// outside serde (the `QUANTICK_LAYOUT` env hook). One vocabulary, one
    /// place: a name added here must be added to the renames, and the
    /// `layout_names_agree_between_serde_and_parse` test holds the two
    /// together.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "flow" => Some(DeclaredLayout::Flow),
            "time" => Some(DeclaredLayout::Time),
            "time+flow" => Some(DeclaredLayout::TimeAndFlow),
            "time+time+flow" => Some(DeclaredLayout::TimeTimeAndFlow),
            _ => None,
        }
    }
}

/// Which pane the chrome spoke for, in the file's vocabulary.
///
/// A twin of `crate::pane::PaneSide` rather than that enum itself, for the
/// same reason `DeclaredLayout` is a twin of `CanvasLayout`: the file is a
/// user-facing contract and must not drift when the canvas grows a side a file
/// should not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedFocus {
    /// quantick's own chart.
    Flow,
    /// The timeframe chart beside it.
    Time,
}

/// Where the drawing rail was docked, in the file's vocabulary.
///
/// `Right` survives here as a *reading* vocabulary only: the rail no longer
/// offers the right edge (see `crate::toolrail::ToolboxDock`), but a
/// `ui-state.toml` written before that still says `right`, and refusing to
/// parse it would throw away the whole file — every other remembered panel
/// with it. It loads as `Left` and is written back as `left`, so the
/// migration happens once and is visible in the file afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedRailDock {
    Left,
    Right,
    Top,
    Bottom,
}

/// Which dock tab was open, in the file's vocabulary. Absent means the dock
/// was collapsed to its strip, which is a state in its own right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedDockTab {
    L2,
    Bubbles,
    Session,
    Trading,
    Trades,
}

/// One remembered market and how its canvas was arranged.
///
/// Bar specs are stored as the `kind:parameter` text `default_bars` already
/// uses (`tick:50`, `time:1m`) rather than as a tagged struct: the file stays
/// hand-editable in the vocabulary the config documents, and
/// `quantick_engine::bar_registry::BarRegistry::parse` is the one gate both go through — so a hand-edited
/// workspace can never open a chart no control could have produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// Feed id, as the config names it.
    pub feed: String,
    /// Symbol, as the feed offers it.
    pub symbol: String,
    /// Which charts the canvas showed.
    pub layout: DeclaredLayout,
    /// The context column's share of the canvas width.
    #[serde(default)]
    pub split_fraction: Option<f32>,
    /// Whether the context column was collapsed to its rail.
    ///
    /// Additive with a default, per this module's own migration policy: a
    /// workspace written before the rail existed is a workspace whose column
    /// was open, not an unreadable one. The width it springs back to is
    /// `split_fraction`, which such a file already carries.
    #[serde(default)]
    pub context_collapsed: bool,
    /// The pane the chrome spoke for.
    #[serde(default)]
    pub focus: Option<SavedFocus>,
    /// Which context chart `focus = "time"` names, top to bottom from `0`.
    ///
    /// Absent — every file written while the split had one context chart —
    /// means the top one, which is the chart those files meant.
    #[serde(default)]
    pub focus_slot: usize,
    /// The flow pane's bar rule.
    pub flow_bars: String,
    /// The time pane's interval, when the tab had one.
    ///
    /// The *top* context chart's, kept under its old name so every workspace
    /// written so far still restores; `context_bars` carries the whole stack.
    #[serde(default)]
    pub time_bars: Option<String>,
    /// Every context chart's bar rule, top to bottom, when the tab had a
    /// stack. Empty in files written before the stack existed, which then
    /// fall back to `time_bars` for the top chart and the default below it.
    #[serde(default)]
    pub context_bars: Vec<String>,
    /// The layout the flow pane showed, by id in the layouts file. Absent in
    /// files written before a pane had a layout of its own: such a pane
    /// opens on the book's active layout, which is what every pane showed.
    #[serde(default)]
    pub flow_layout: Option<u64>,
    /// Each context chart's layout, top to bottom, by id in the layouts file.
    ///
    /// `LAYOUT_UNRECORDED` where the file does not say — a pane the session
    /// had not seeded yet when the workspace was written. A sentinel rather
    /// than a `None` because TOML has no null *inside an array*: `toml` skips
    /// a `None` struct field (which is why `Self::flow_layout` may be one),
    /// but answers a `None` element with `UnsupportedNone`, and `save`
    /// answers a failed serialize by writing nothing at all — so a single
    /// unseeded pane would have cost the trader every tab in the file.
    #[serde(default)]
    pub context_layouts: Vec<u64>,
    /// Whether the flow pane's on-chart indicator legend was folded to its
    /// count puck.
    ///
    /// Per pane, like the bar rules above and for the same reason: the corner
    /// pressure that makes a trader fold one chart's legend is not on the
    /// other. Absent means expanded — a workspace written before the fold
    /// existed opens exactly as it closed.
    #[serde(default)]
    pub flow_legend_collapsed: bool,
    /// Whether the time pane's legend was folded. See
    /// `Self::flow_legend_collapsed`; a tab that never showed the split
    /// simply never had one to fold.
    #[serde(default)]
    pub time_legend_collapsed: bool,
}

/// The single-instance chrome around the tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedChrome {
    /// Display timezone, in whole minutes east of UTC.
    pub timezone_minutes: i32,
    /// Whether the dock (strip included) was on screen.
    pub dock_visible: bool,
    /// The open dock tab; absent means collapsed to the strip.
    #[serde(default)]
    pub dock_tab: Option<SavedDockTab>,
    /// Whether the drawing rail was on screen.
    pub rail_visible: bool,
    /// Which edge the rail was docked to.
    pub rail_dock: SavedRailDock,
    /// Whether the status bar showed fps/frame time.
    pub perf_readings: bool,
    /// Where starred tools used to live, kept only to read files that still
    /// hold them there.
    ///
    /// Favorites were part of the arrangement once, which meant a bookmark
    /// saved before the trader starred anything wiped the rail on open and a
    /// dirty exit lost the stars outright. They are a standing choice, so they
    /// moved up to `Workspace::favorite_tools`; `load` lifts what an older
    /// file kept here and empties this, and an empty list writes no key — so a
    /// file migrates once and never carries two answers.
    #[serde(
        default,
        rename = "favorite_tools",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub legacy_favorite_tools: Vec<String>,
    /// Whether venue candle history was fetched in slices, newest first.
    ///
    /// Defaults to on rather than to `false`, which is what a missing field
    /// would otherwise mean: a workspace written before this switch existed
    /// describes a cockpit whose owner never chose the slower path, and
    /// reading their silence as "off" would hand them the old wait back with
    /// no way to know why.
    #[serde(default = "yes")]
    pub progressive_history: bool,
    /// How far one press of *load older* reaches, as
    /// `quantick_feed::history_reach::HistoryReach::token` writes it.
    ///
    /// A token rather than a variant name, so a release may reword the menu
    /// label without orphaning every saved workspace. Absent — or a token a
    /// later release wrote and this one does not know — restores the default
    /// reach, which is the single page the button has always fetched: the one
    /// answer that is never a surprise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_reach: Option<String>,
    /// How far one press of the *by time* reach pulls, in minutes of traded
    /// time.
    ///
    /// Saved beside the reach because the two are one choice: a workspace that
    /// restored `by time` without its span put the menu and the press out of
    /// step — the chip read what the trader picked while the span had silently
    /// gone back to the config seed, and nothing on screen said so. Absent
    /// means a file written before this existed, and restores the configured
    /// default rather than a number nobody chose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_reach_span_minutes: Option<u32>,
    /// Whether a chart cut by trades carried the venue's candles in front of
    /// its bars.
    ///
    /// Defaults to off, which is also what a file written before the switch
    /// existed means: that cockpit's owner never asked for a prefix on a tick
    /// chart, and reading their silence as "on" would put candles in front of
    /// bars they never chose to see.
    #[serde(default)]
    pub venue_lead_in: bool,
    /// Whether a MetaTrader tab records the venue's deal counter on its own.
    /// Absent follows the feed config's `record_deals`; a file written before
    /// the recorder existed therefore changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_deals: Option<bool>,
    /// Where the trader parked the drawing-properties popup, in screen points,
    /// or absent while it still places itself beside the object it configures.
    ///
    /// One position for every tool on the rail, because there is one window:
    /// the popup is rebuilt for whatever is selected, so a trader who drags it
    /// out of the way once has moved it for the next drawing too — which is
    /// the whole reason to move it. A position per tool would put the window
    /// somewhere new on every selection, which is the behaviour being fixed.
    ///
    /// Absent in files written before this field, and absent again after the
    /// double-click that restores automatic placement; either way the app
    /// places the popup itself, exactly as it did before this field existed.
    ///
    /// Screen points, and deliberately *not* repaired here: a position that no
    /// longer fits — a smaller window, the rail on another edge — is clamped
    /// into the chart when the popup draws, by the same code that repairs one
    /// dragged half off screen. The file records what the trader did; the
    /// screen decides what is still possible.
    #[serde(default)]
    pub inspector_position: Option<[f32; 2]>,
}

pub const LAYOUT_UNRECORDED: u64 = 0;
const fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_tab_names_and_absent_fields_keep_their_original_defaults() {
        let literal = "feed = \"binance\"\nsymbol = \"TESTUSDT\"\nlayout = \"time+flow\"\nflow_bars = \"tick:50\"\n";
        let tab: SavedTab = toml::from_str(literal).unwrap();
        assert_eq!(tab.layout, DeclaredLayout::TimeAndFlow);
        assert!(!tab.context_collapsed);
        assert_eq!(tab.focus_slot, 0);
        assert_eq!(tab.flow_layout, None);
        assert!(tab.context_layouts.is_empty());
        assert!(tab.context_bars.is_empty());
        assert_eq!(
            toml::to_string(&tab).unwrap(),
            concat!(
                "feed = \"binance\"\n",
                "symbol = \"TESTUSDT\"\n",
                "layout = \"time+flow\"\n",
                "context_collapsed = false\n",
                "focus_slot = 0\n",
                "flow_bars = \"tick:50\"\n",
                "context_bars = []\n",
                "context_layouts = []\n",
                "flow_legend_collapsed = false\n",
                "time_legend_collapsed = false\n"
            )
        );
    }
    #[test]
    fn old_chrome_keeps_legacy_right_dock_and_progressive_default() {
        let chrome: SavedChrome = toml::from_str(
            r#"
timezone_minutes = -180
dock_visible = true
rail_visible = true
rail_dock = "right"
perf_readings = false
"#,
        )
        .unwrap();
        assert_eq!(chrome.rail_dock, SavedRailDock::Right);
        assert!(chrome.progressive_history);
        assert!(!chrome.venue_lead_in);
        assert!(chrome.legacy_favorite_tools.is_empty());
        assert_eq!(chrome.inspector_position, None);
        assert_eq!(chrome.record_deals, None);
    }
}
