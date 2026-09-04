//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.
//!
//! What is on the canvas lives one layer down, in [`crate::pane`]: the bar
//! series, the viewport, the drawings and the indicator slots belong to a
//! [`ChartPane`], not to the window. The market feeding that pane — channels,
//! connection state, notices, history counters — is still owned here.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::canvas_layout::{MAX_CANVAS_PANES, PaneIdAllocator};

mod chart_layers_wiring;
mod demo_hooks;
mod drawing_input;
mod health;
mod indicator_manager;
mod layout_wiring;
mod workspace_restore;
mod workspace_save;
use crate::chart_layers;
// `ChartLayer` left this file's production code with `maintain_chart_layers`
// and lives in `app::chart_layers_wiring` now. The one name still read here is
// read by `heatmap_lamp_on`, which is itself test-only, so the import is gated
// the same way rather than kept alive by a `chart_layers::` prefix that would
// appear on exactly one line of the file.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
use crate::config::AppConfig;
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{self, DeleteOutcome, DrawingAuthor};
use crate::feed_notice;
use crate::harness::{ContextMenuPane, Harness, ScriptedMenu};
use crate::indicator_panel::SettingsDialog;
use crate::indicator_worker::{IndicatorSource, SlotId};
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file::{self, SavedKind};
use crate::loading::{self, LoadingScope, LoadingTask};
use crate::metrics::{self, FrameStats};
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX, PaneSide};
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::BarSpec;
use crate::statusbar;
use crate::style::ChartStyle;
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::{CanvasChrome, CanvasLayout, Tab};
use crate::tabstrip::{self, TabAction};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail, ToolboxDock};
use crate::ui_state;
use crate::window_scale;
use crate::workspace_store::{LayoutStore, StorePaths, WorkspaceStore};
use quantick_feed::history_reach;
use quantick_feed::{self as feed, FeedCommand, FeedHandle, ReplayControl};
use quantick_orderflow::LaneWindow;
use smallvec::SmallVec;

/// Id of the tab the window opens with.
const FIRST_TAB_ID: u64 = 0;

/// How much of the newest chart the `QUANTICK_DRAWINGS_DEMO` hook spreads its
/// objects across. Close to what a default viewport shows, so every object
/// lands on screen — a demo the camera cannot see proves nothing.
const DEMO_VISIBLE_SLOTS: usize = 90;
/// The natives `QUANTICK_INDICATORS_AUTOSTART` opens with: the overlay and
/// the pane, so a scripted run photographs both shapes. Named by catalog id
/// rather than "all of them", because the hook's contract is a fixed,
/// deterministic pair — a native added later must not silently change what
/// every existing capture shows.
const AUTOSTART_NATIVES: &[&str] = &["native.ema", "native.cvd"];
/// Horizontal offset of a duplicated drawing, so the copy is visibly a copy.
const DUPLICATE_OFFSET_BARS: f32 = 2.0;

/// The slice the drawing chrome reads, assembled from the pieces of the
/// application it is allowed to see.
///
/// A free function rather than a method for the reason
/// [`indicator_preview_area`] is one: every caller has already split
/// `QuantickApp` into disjoint borrows to draw a surface through `&mut`, and a
/// method would want the whole of `self` back. That the compiler insists on
/// the split is the port working.
///
/// `manager_rows` is handed in rather than gathered here. Only one of the two
/// call sites draws the list, and building a row per object for the site that
/// does not would be a per-frame allocation for a window nobody is looking at.
fn drawing_env<'a>(
    tab: &'a Tab,
    toolrail: &ToolRail,
    presets: &'a drawings::presets::PresetStore,
    read: DrawingRead<'a>,
) -> crate::surfaces::DrawingEnv<'a> {
    let side = tab.drawing_side();
    let pane = tab.pane(side);
    let selected = pane.drawings.selected().and_then(|index| {
        pane.drawings
            .items()
            .get(index)
            .map(|drawing| crate::surfaces::drawing_chrome::SelectedDrawing { index, drawing })
    });
    crate::surfaces::DrawingEnv {
        selected,
        chart_area: pane.last_chart_area,
        focused_chart_area: tab.focused_pane().last_chart_area,
        lane_divider_x: pane.last_lane_divider_x,
        auto_range: pane.last_auto_range,
        selected_bbox: read.selected_bbox,
        selected_band: read.selected_band,
        tab: tab.id,
        side,
        drawing_tool_armed: matches!(toolrail.tool(), Tool::Drawing(_)),
        toolbox_dock: toolrail.dock(),
        authored_objects: read.authored_objects,
        manager_rows: read.manager_rows,
        presets,
    }
}

/// The parts of [`drawing_env`] that cost something to work out, gathered by
/// the caller so each pass pays only for what it draws.
///
/// Three fields and three prices. Projecting the selection's painted bounds
/// walks its anchors through the price scale; naming its band formats a
/// string; counting an assistant's objects walks every pane of every tab. All
/// three are per-frame while a selection is on screen, which is why the pass
/// that only runs the capture hooks gathers none of them and says so.
#[derive(Default)]
struct DrawingRead<'a> {
    selected_bbox: Option<egui::Rect>,
    selected_band: Option<String>,
    authored_objects: usize,
    manager_rows: &'a [crate::surfaces::drawing_chrome::ManagerRow],
}

/// The chart rectangle a settings dialog is previewing an unapplied draft on,
/// if one is.
///
/// The pane the *dialog* was opened over, not the focused one: a trader can
/// preview a curve on the left pane and then click the right, and the banner
/// belongs over the numbers that are actually provisional.
///
/// A free function rather than a method because its only caller has already
/// split `QuantickApp` into disjoint borrows to build the surface
/// environment, and a method would want the whole of `self` back. Per-frame,
/// and shaped to leave immediately: no dialog open — the ordinary case — is
/// one `Option` test before the tab scan is reached.
fn indicator_preview_area(
    tabs: &[Tab],
    dialog: Option<&SettingsDialog>,
    target: TabSlot,
) -> Option<egui::Rect> {
    if !dialog.is_some_and(|dialog| dialog.previewed) {
        return None;
    }
    tabs.iter()
        .find(|tab| tab.id == target.tab)
        .map(|tab| tab.pane(target.side))
        .and_then(|pane| pane.last_chart_area)
}

/// Format the forming bar's countdown, e.g. `37/50 ticks`.
///
/// Trailing zeros are trimmed on both figures: a volume bar's accumulator
/// carries the feed's own scale, and `1.20000000/5 vol` reads as noise.
fn fmt_progress(progress: &quantick_engine::BarProgress, unit: &str) -> String {
    format!(
        "{}/{} {unit}",
        progress.done.normalize(),
        progress.target.normalize()
    )
}

/// Read a tape window off `QUANTICK_TAPE_WINDOW`.
///
/// `auto` follows the bars; a duration pins it, in the units a human would
/// type (`90s`, `2min`, `120000ms`, or bare milliseconds). `None` for anything
/// else, so a typo leaves the tape at its default rather than photographing an
/// invented window. The value is clamped by the setter, not here — one owner
/// for the drawable range.
fn parse_tape_window(value: &str) -> Option<LaneWindow> {
    let value = value.trim().to_ascii_lowercase();
    if value == "auto" {
        return Some(LaneWindow::default());
    }
    let (number, scale) = if let Some(rest) = value.strip_suffix("ms") {
        (rest, 1)
    } else if let Some(rest) = value.strip_suffix("min") {
        (rest, 60_000)
    } else if let Some(rest) = value.strip_suffix('m') {
        (rest, 60_000)
    } else if let Some(rest) = value.strip_suffix('s') {
        (rest, 1_000)
    } else {
        (value.as_str(), 1)
    };
    let parsed = number.trim().parse::<f64>().ok()?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return None;
    }
    Some(LaneWindow::Fixed {
        ms: (parsed * f64::from(scale)).round() as i64,
    })
}

/// Read-only frame observations exposed to on-demand semantic projections.
pub(crate) struct ControlFrameMetrics {
    pub wall_average_ms: Option<f32>,
    pub wall_worst_ms: Option<f32>,
    pub frames_per_second: Option<f32>,
    pub cpu_average_ms: Option<f32>,
    pub cpu_worst_ms: Option<f32>,
}

/// The quantick chart window.
///
/// One workspace: N open markets ([`Tab`]) and the chrome around them. The
/// chrome is what is single-instance by nature — one menu bar, one toolbox,
/// one dock, one appearance, one status line — plus the indicator persistence
/// layer, which describes a workspace rather than a market.
pub struct QuantickApp {
    /// The open markets, left to right as the strip shows them (§11).
    ///
    /// Retained trades are O(trades × panes × open tabs): every tab keeps its
    /// own history and a split tab keeps it twice. Nothing caps the count —
    /// the strip is as long as the user makes it.
    tabs: Vec<Tab>,
    /// Which of them is on screen. Every tab drains every frame; only this one
    /// renders, and the chrome speaks for it.
    active_tab: usize,
    /// Handed out to new tabs and never reused, so a closed tab's ids can
    /// never be mistaken for a living one's.
    next_tab_id: u64,
    /// Where the offline chip was drawn, or `None` when it was not.
    ///
    /// Written as part of drawing it, exactly as a pane records its own chart
    /// area, and for the same two reasons. It says what is *painted* rather
    /// than what a fresh reading of the clock would have painted a
    /// millisecond later, so the scene and the screen cannot disagree across
    /// the edge of a stall budget. And it is the one control on screen with
    /// no capability behind it — opening a popup is a gesture, not a call —
    /// so its rectangle is the only way an operator reaches it at all.
    ///
    /// A `Rect` per frame, and only while the chart is not being fed. Nothing
    /// is recorded on a healthy chart, which is every frame of a normal
    /// session.
    feed_chip_rect: Option<egui::Rect>,
    /// The tab whose chip opened the feed's recovery popup, if any.
    ///
    /// Opened by clicking the offline chip and by nothing else — the rule the
    /// trader asked for, after a card that opened itself over the chart every
    /// morning. It is the *tab's* id rather than a window-wide flag because
    /// one dead terminal stalls every MT5 tab at once: a bare flag opened on
    /// one chart and then found the next chart already offline, and drew
    /// itself there with nobody having clicked anything. The chip is window
    /// chrome speaking for the active market, and this says which market it
    /// was speaking for.
    ///
    /// Leaving that chart closes it, the way clicking elsewhere does: the
    /// frame answers for the tab it is drawing, so a switch clears the flag.
    /// A glance, not a mode — nothing waits on a chart nobody is looking at.
    feed_popup_tab: Option<u64>,
    /// Whether the toolbar's layout popover is open.
    layout_picker_open: bool,
    /// The window's one source of pane ids. Pane ids namespace egui
    /// interaction state across the whole window rather than within a tab, so
    /// this may not be per-tab state: two panes sharing an id share a drag.
    pane_ids: PaneIdAllocator,
    /// The instruments the user added from the picker, already folded into
    /// `config`'s catalog. Kept apart from it so the picker can tell an
    /// addition — which it may take back out — from a shipped entry, which is
    /// the config file's and not the app's to touch.
    added_symbols: AddedSymbols,

    config: AppConfig,

    /// Explicitly enabled local observer gateway. `None` only while this field
    /// is temporarily moved out to dispatch a frame without borrowing the app
    /// through itself.
    control_access: Option<crate::control::ControlAccess>,

    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    script_library: ScriptLibrary,
    /// The open indicator-settings dialog, if any (one at a time).
    indicator_settings: Option<SettingsDialog>,
    /// The slot the open dialog edits. Held apart from the dialog so a tab or
    /// pane changing under it cannot retarget its Apply.
    indicator_settings_target: TabSlot,
    /// File-backed script slots: (slot, library index, last seen mtime) —
    /// what the hot-reload poll walks.
    script_files: Vec<(TabSlot, usize, std::time::SystemTime)>,
    /// How each live slot restores (the persistence identity per slot).
    ///
    /// Stays beside the library and the state file rather than moving into the
    /// panes with the slots themselves: one file records what the window had
    /// open, so one list records what is in it.
    slot_kinds: Vec<(TabSlot, SavedKind)>,
    /// Slots placed hidden by a layout, applied when their Rebuilt lands —
    /// the view a hide acts on is born from the worker's first answer.
    pending_hidden: Vec<TabSlot>,
    /// Per-plot style layers placed by a layout, applied when their Rebuilt
    /// lands — the same deferral [`Self::pending_hidden`] performs, for the
    /// same reason.
    pending_styles: Vec<(TabSlot, crate::indicator_style::StyleOverride)>,
    /// The layout being renamed in the strip, with the draft name.
    layout_rename: Option<(crate::layouts::LayoutId, String)>,
    /// The layout a delete is waiting on: deleting takes its drawings with
    /// it, on disk too, so it is the one strip action behind a confirmation.
    layout_delete_confirm: Option<crate::layouts::LayoutId>,
    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    last_script_poll: Instant,

    /// The browser window and, while the active tab replays, the transport.
    replay_view: ReplayView,

    // External chart chrome: the tabbed right dock and the edge-docked
    // drawing rail. Neither is painted over the chart canvas.
    dock: Dock,
    toolrail: ToolRail,

    /// Floating chrome that owns the state it draws: the assistant's popup,
    /// the acknowledgement toast. One field for the whole set, one module
    /// per surface — see [`crate::surfaces::Surfaces`].
    surfaces: crate::surfaces::Surfaces,
    /// The `QUANTICK_CONTROL_ACCESS` hook: enable observer access on the
    /// first frame, through the panel button's own `enable`.
    pending_control_access_enable: bool,
    /// The indicator slots an operator other than the trader attached — the
    /// only ones the annotate tier may take back off the chart. Keyed by the
    /// whole [`TabSlot`]: a slot number is allocated per pane and is reused
    /// by every other pane, so the number alone would mark one tab's slot 0
    /// as an operator's because another tab's slot 0 was.
    operator_slots: std::collections::BTreeSet<TabSlot>,
    /// The `QUANTICK_CONTROL_ANNOTATE` hook: an agent-authored label on the
    /// first frame, so every attribution surface can be photographed.
    pending_control_annotation: Option<String>,
    /// The `QUANTICK_CONTROL_NOTIFY` hook: `<channel>:<message>`.
    pending_control_notification: Option<String>,
    /// The `QUANTICK_CONTROL_EVIDENCE` hook: which scopes to capture, and
    /// whether to rasterise the window with them.
    pending_control_evidence: Option<String>,
    /// The `QUANTICK_CONTROL_MARK` hook: take a mark on the first frame,
    /// through the hotkey's own action, with the note the hook carried.
    pending_control_mark: Option<String>,
    /// The popup's position changed by hand this frame and the workspace has
    /// not been told yet.
    ///
    /// The position itself is automatic until the user drags the title bar and
    /// manual from then on (only ever re-clamped), and the chart rectangle it
    /// is placed against belongs to the focused [`ChartPane`] — so a split
    /// window places against the pane the selection lives on, not the window.
    ///
    /// A flag rather than a write on the spot, for two reasons. A drag reports
    /// a new position on *every* frame the hand is moving, and writing the file
    /// sixty times a second for a window that has not landed yet is a lot of
    /// disk for one decision. And the write itself belongs beside the other
    /// workspace writes ([`Self::maintain_workspace`]), not inside the closure
    /// that is painting the window — one place that knows how a workspace
    /// reaches the disk, not two.
    ///
    /// That host runs at the top of a frame, so the file is written on the
    /// frame *after* the one the hand came off in — sixteen milliseconds, and
    /// the frame that closes the window flushes this before taking the exit
    /// save, so nothing can be dropped between the two.
    inspector_position_dirty: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    /// The window this app is drawing into, kept so the health summary can
    /// report the client area the platform believes it has — see
    /// [`crate::window_scale`] for why that number is worth logging, and for
    /// the defect it was measured chasing.
    surface: Option<window_scale::SurfaceProbe>,
    /// Where a pane's layer menu leaves the grid switch and the "an indicator
    /// was hidden" flag; drained right after the canvas is drawn.
    layer_actions: chart_layers::LayerActions,
    /// The footprint layer's signal tunables — resolved at boot (env >
    /// `config/footprint.toml` preset > saved edits > defaults), edited live
    /// by the layer menu's controls.
    footprint_config: crate::footprint_config::FootprintConfig,
    // Named input setups per indicator kind, offered by the settings
    // dialog's preset picker.
    indicator_presets: preset_file::PresetStore,
    /// Where the Workspace button was drawn, published by the menu bar so the
    /// hook can click it rather than guess at a coordinate.
    workspace_menu_rect: Option<egui::Rect>,
    /// Where the toolbar's history caret is, published by the draw. `None`
    /// while the menu is unreachable — a feed that pages nothing has no menu
    /// to open, and a hook must photograph that rather than force it.
    history_menu_rect: Option<egui::Rect>,
    /// Where signal alarms are played. The shipped sink is the platform's
    /// own sounds; a test swaps in a recorder, which is how "the alarm
    /// sounded, once, and it was the sound the preset named" is asserted
    /// without a build machine making noise.
    alerts: Box<dyn crate::audio::AlertSink>,
    /// The last reason a sound could not be played, shown once in the
    /// dialog. A build with no audio backend, or a platform that refused,
    /// is reported: an alarm the trader never heard is never assumed heard.
    alert_failure: Option<String>,

    /// The chart appearance every renderer reads. The window that edits it
    /// is `surfaces::style_panel`, which hands back a copy rather than
    /// holding a reference to this one.
    style: ChartStyle,
    style_revision: u64,
    // Whether the status bar shows the perf readings (View → perf readings).
    show_perf: bool,
    /// Whether venue candle history is asked for in slices, newest first
    /// (View → progressive venue history).
    ///
    /// On by default. A span of one-minute candles is a run of sequential
    /// venue round trips — seconds for the opening week, and another such run
    /// for every span the trader reaches back through — and fetched whole the
    /// chart shows nothing at all for the whole of it. Off restores exactly
    /// that: one
    /// request, one reply, one very late frame — kept because a trader on a
    /// metered or rate-limited connection may prefer the smaller number of
    /// requests, and because a setting whose "off" is not the old behaviour is
    /// not a setting the user can fall back to.
    progressive_history: bool,
    /// How far one press of the chart's *load older* button reaches — one
    /// page of trades, or back past the market's last close with a lead into
    /// the session before it.
    ///
    /// A standing choice of the window rather than of a market: a trader who
    /// wants to see yesterday wants it in the tab they open next too. Mirrored
    /// onto every tab each frame, which is where the press is actually served.
    history_reach: history_reach::HistoryReach,
    /// Minutes of *traded* time one press of the `by time` reach pulls.
    ///
    /// On the window beside the reach it belongs to, and mirrored onto every
    /// tab by `drain_tabs`, exactly as the reach itself is: the two are one
    /// choice, and a tab opened after the trader set it must press the way
    /// they said. Seeded from `[history] reach_span_minutes` and editable
    /// afterwards, because it is the trader's own answer to "how much more
    /// tape per press" and that differs between a contract printing a million
    /// times a day and one printing a thousand.
    history_reach_span_minutes: u32,
    /// Whether a chart *not* cut by time may carry the venue's own candles in
    /// front of its bars.
    ///
    /// Off by default: a tick chart has always opened on the prints this
    /// session saw, and nothing is put in front of them unasked. On, a chart
    /// cut by trades gets the venue's 1-minute candles as a labelled prefix —
    /// the only way such a chart can show yesterday at all, since a candle
    /// cannot be folded into a tick bar and must never pretend to be one.
    venue_lead_in: bool,

    // Fixed UTC offset the time axis is displayed in (default UTC−03:00).
    tz: TzOffset,
    /// Where the workspace lives on disk, and whether what is on screen has
    /// reached it: the six store paths, the layout book with the one rule that
    /// decides when it is written, the chart-layer baseline, and what the
    /// Workspace menu knows without asking the filesystem.
    ///
    /// One field where there were twenty-one. See [`crate::workspace_store`]
    /// for why the owner is a new type rather than a home inside `ui_state`,
    /// `layouts` or `workspace_bundle` — none of which holds session state —
    /// and for the invariant the layout trio could not carry apart.
    workspace: crate::workspace_store::WorkspaceStore,
    /// The window's inner size as of the last frame, in points — captured here
    /// because the size a workspace records is the one the user last saw, and
    /// by exit time the viewport has already been asked to close.
    window_size: Option<[f32; 2]>,
    frames: FrameStats,
    /// CPU time per frame (update + tessellation + paint, no vsync wait), from
    /// eframe. Separates "we are slow" from "we are waiting for the display".
    cpu_frames: FrameStats,
    last_frame: Option<Instant>,
    /// Live trades taken in since the last perf summary, across every tab —
    /// what the window is ingesting, not what one market prints.
    trades_since_summary: u64,
    last_summary: Instant,
    /// Every environment hook an agent drives this window by, read once at
    /// launch and named. See [`crate::harness`] for what belongs here and
    /// why the trunk asks it rather than holding its flags: twenty-three of
    /// them used to sit in this struct, beside the state the chart actually
    /// trades on.
    harness: Harness,
}

/// An indicator slot together with the tab and pane that own it.
///
/// Slot ids are allocated per pane, so the id alone identifies nothing once
/// there are two panes, let alone two tabs: without the rest, removing one
/// tab's slot 0 would drop another's bookkeeping for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TabSlot {
    tab: u64,
    side: PaneSide,
    slot: SlotId,
}

impl QuantickApp {
    /// Create the app on `config`, opening one tab on `feed_id`/`symbol`
    /// (already streaming through `feed`) and bar `spec`, with no saved
    /// workspace to restore.
    ///
    /// The window itself always has one (`main` reads the file before it
    /// spawns a feed), so this is the tests' entry point: a case about the
    /// chart is not a case about what the last session left on disk.
    #[cfg(test)]
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
    ) -> Self {
        Self::new_with_workspace(
            config,
            feed_id,
            symbol,
            spec,
            feed,
            ui_state::Workspace::default(),
        )
    }

    /// The same, restoring `workspace` over the configured defaults.
    ///
    /// The first tab is already streaming when this is called — `main` spawns
    /// it, because a window with no feed has nothing to show while it waits —
    /// so the caller is expected to have picked its market from
    /// [`Workspace::first_market`]. Everything else the workspace remembers is
    /// applied here.
    ///
    /// `workspace` must already have been through
    /// [`Workspace::restore`](ui_state::Workspace::restore): this function
    /// opens what it is given, and a market the config no longer offers is not
    /// its to discover.
    #[must_use]
    pub fn new_with_workspace(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
        workspace: ui_state::Workspace,
    ) -> Self {
        let state_path = crate::paper_state::default_path();
        // Read before `config` is moved into the struct below: this seeds the
        // window's own copy of the span, which the trader then edits.
        let reach_span_minutes = config.history.reach_span_minutes;
        let paper_state = crate::paper_state::load(&state_path);
        let cmd_trading = crate::paper_trading::CmdTradingSettings::from_state(&paper_state);
        let (trades_dir, consolidated) = crate::paper_home::startup_home(
            config.paper.trades_dir.as_deref(),
            paper_state.trades_dir.as_deref(),
            &state_path,
        );
        let mut pane_ids = PaneIdAllocator::new();
        let loaded_layouts =
            Self::load_layouts(&crate::layouts::default_path(), &state_file::default_path());
        let mut tab = Tab::new(
            FIRST_TAB_ID,
            pane_ids.alloc(),
            feed_id.into(),
            symbol.into(),
            spec,
            feed,
            trades_dir.clone(),
        );
        tab.paper.set_cmd_trading(cmd_trading);
        // The named ladders the trader built last session, and the one the
        // ticket was set to. A selection naming a strategy the file no
        // longer carries selects nothing, which `set_order_strategies`
        // enforces rather than each caller remembering to.
        tab.paper.account_mut().set_order_strategies(
            paper_state.order_strategies.clone().unwrap_or_default(),
            paper_state.selected_order_strategy.as_deref(),
        );
        // A step this build cannot parse is dropped rather than defaulted:
        // the instrument then follows its derived default, which is a real
        // answer, where a zero would be a silently broken wheel.
        tab.paper.set_ruler_steps(
            paper_state
                .ruler_steps
                .iter()
                .filter_map(|(symbol, step)| step.parse().ok().map(|value| (symbol.clone(), value)))
                .collect(),
        );
        // The risk per trade, and the money it is measured in. A trader who
        // never set any of this gets the mode off and an empty book, which
        // leaves every screen exactly as it was.
        //
        // Skipped when a launch hook set the risk for this run: an
        // environment variable is an explicit request for one run and
        // outranks the stored settings. Restoring them here left the hook's
        // whole point - the derived size, the sentence, the lock -
        // unreachable from a capture.
        if !tab.paper.account().risk_from_hook() {
            tab.paper
                .account_mut()
                .set_risk_settings(crate::risk_sizing::settings_from_sidecar(
                    paper_state.risk_per_trade_basis.as_deref(),
                    paper_state.risk_per_trade_amount.as_deref(),
                    paper_state.risk_per_trade_currency.as_deref(),
                    paper_state.risk_per_trade_percent.as_deref(),
                    paper_state.risk_per_trade_lock,
                ));
            tab.paper
                .account_mut()
                .set_capital(crate::risk_sizing::capital_from_records(
                    &paper_state.paper_capital,
                ));
        }
        tab.paper
            .account_mut()
            .set_instrument_money(crate::risk_sizing::book_from_records(
                &paper_state.instrument_money,
            ));
        // Resolved once: under test the settings path is a fresh scratch
        // file per call, and the load must read the same file the saves
        // will write.
        let symbols_path = symbols_file::default_path();
        let footprint_settings_path = crate::footprint_config::settings_path();
        let indicator_presets_path = preset_file::default_path();
        let mut app = Self {
            tabs: vec![tab],
            active_tab: 0,
            harness: Harness::from_env(),
            next_tab_id: FIRST_TAB_ID + 1,
            layout_picker_open: false,
            pane_ids,
            added_symbols: symbols_file::load(&symbols_path),
            config,
            control_access: Some(crate::control::ControlAccess::new()),
            script_library: ScriptLibrary::scan(),
            indicator_settings: None,
            indicator_settings_target: TabSlot {
                tab: FIRST_TAB_ID,
                side: PaneSide::Flow,
                slot: SlotId(0),
            },
            script_files: Vec::new(),
            slot_kinds: Vec::new(),
            pending_hidden: Vec::new(),
            pending_styles: Vec::new(),
            layout_rename: None,
            layout_delete_confirm: None,
            last_script_poll: Instant::now(),
            replay_view: ReplayView::new(
                workspace.replay_folder.as_deref(),
                workspace.replay_day_before,
            ),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            surfaces: crate::surfaces::Surfaces::default(),
            pending_control_access_enable: false,
            operator_slots: std::collections::BTreeSet::new(),
            pending_control_annotation: None,
            pending_control_notification: None,
            pending_control_evidence: None,
            pending_control_mark: None,
            inspector_position_dirty: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            surface: None,
            layer_actions: chart_layers::LayerActions::default(),
            footprint_config: crate::footprint_config::load(&footprint_settings_path),
            indicator_presets: preset_file::PresetStore::load(&indicator_presets_path),
            workspace_menu_rect: None,
            history_menu_rect: None,
            alerts: Box::new(crate::audio::Speaker::default()),
            alert_failure: None,
            style: ChartStyle::default(),
            style_revision: 0,
            show_perf: true,
            progressive_history: true,
            history_reach: history_reach::HistoryReach::default(),
            history_reach_span_minutes: reach_span_minutes,
            venue_lead_in: false,
            feed_chip_rect: None,
            // The hook stands in for a click on the opening tab's chip, which
            // is the first tab there is.
            feed_popup_tab: feed_notice::popup_open_from_env().then_some(FIRST_TAB_ID),
            tz: TzOffset::default(),
            workspace: WorkspaceStore::new(
                StorePaths {
                    symbols: symbols_path,
                    chart_layers: chart_layers::default_path(),
                    footprint_settings: footprint_settings_path,
                    indicator_presets: indicator_presets_path,
                    ui_state: ui_state::default_path(),
                },
                LayoutStore::new(
                    loaded_layouts.0,
                    crate::layouts::default_path(),
                    loaded_layouts.1,
                ),
                trades_dir,
            ),
            window_size: None,
            frames: FrameStats::new(120),
            cpu_frames: FrameStats::new(120),
            last_frame: None,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        };
        // Recording is not a display choice: it starts with the feed, so
        // hiding the map later never leaves a hole in what was captured.
        let config = app.config.clone();
        app.active_tab_mut().refresh_chip_label(&config);
        app.active_tab_mut().ensure_book_capture(&config);
        // A feed that declares its own look opens wearing it.
        app.active_tab_mut().apply_feed_bubble_preset(&config);
        // Same for a declared opening layout: a feed the user reads by
        // timeframe can open straight on the timeframe chart.
        app.active_tab_mut().apply_feed_declared_layout(&config);
        // The code's own baseline, and nothing more: what a launch actually
        // opens with is `config/chart-layers.toml`, applied by
        // `restore_chart_layers` immediately below and shipping the map on.
        // This line is what remains if that config is ever unreadable — a
        // layer nobody requested costing no projection. Capture is already
        // running either way, so it is a display choice and nothing else.
        app.active_tab_mut().tape_mut().set_depth_visible(false);
        // What the user last had on the canvas, applied over those defaults and
        // under the autostart hooks below: an env var is an explicit request
        // for this run and must still win (see `restore_chart_layers`).
        app.restore_chart_layers();
        // And the workspace itself — the tab strip, each tab's canvas, and the
        // chrome around them. After the config defaults (a saved cockpit is
        // the user's own answer to what a feed declares) and before the
        // autostart hooks, which are explicit requests for this one run.
        app.restore_workspace(workspace);
        // Dev/ops can open the map without a click.
        if std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().tape_mut().set_depth_visible(true);
        }
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().flow_pane.live_strip_visible = true;
        }
        // Local agent access, reachable without a click: the panel through the
        // Tools menu entry's own function, and the enable action through the
        // panel button's own function on the first frame — one path for the
        // human, the hook and any later operator. Enabling publishes a real
        // descriptor in the private runtime directory, removed on a clean exit.
        if std::env::var("QUANTICK_CONTROL_PANEL").is_ok_and(|value| value == "1")
            && let Some(access) = app.control_access.as_mut()
        {
            access.open_panel();
        }
        // Which scopes the next connection is granted, by ID — the panel's
        // own checkboxes without a hand on the mouse. `annotate` grants the
        // whole annotate tier (the profile follows the scopes), and any
        // comma-separated list of registered permission IDs is honoured, so a
        // scripted run can reproduce exactly the grant a trader would tick.
        if let Ok(scopes) = std::env::var("QUANTICK_CONTROL_SCOPES")
            && let Some(access) = app.control_access.as_mut()
            && let Err(error) = access.configure_scopes(&scopes)
        {
            {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_SCOPE_HOOK_REFUSED",
                    error = %error,
                    "QUANTICK_CONTROL_SCOPES named something this build does not register"
                );
            }
        }
        app.pending_control_access_enable =
            std::env::var("QUANTICK_CONTROL_ACCESS").is_ok_and(|value| value == "1");
        // A mark from a launch: `1` marks with no note, anything else is the
        // note. It goes through the same action the hotkey calls.
        app.pending_control_mark = std::env::var("QUANTICK_CONTROL_MARK")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| if value == "1" { String::new() } else { value });
        // An assistant's own object and an assistant's own interruption, from
        // a launch: the surfaces that say *who* acted cannot be photographed
        // without something an operator other than the trader put there.
        // One evidence bundle from a launch, through the same read a client
        // calls: the capture a validation run asserts against, and the
        // screenshot notice a capture run photographs.
        app.pending_control_evidence = std::env::var("QUANTICK_CONTROL_EVIDENCE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        app.pending_control_annotation = std::env::var("QUANTICK_CONTROL_ANNOTATE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        app.pending_control_notification = std::env::var("QUANTICK_CONTROL_NOTIFY")
            .ok()
            .filter(|value| !value.trim().is_empty());
        // Drawing-toolbar hooks, so a validation run reaches every new
        // surface without a click (`.claude/skills/ui-harness`).
        if let Ok(id) = std::env::var("QUANTICK_DRAWING_TOOL")
            && let Some(tool) = drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == id.trim())
        {
            app.toolrail.arm(Tool::Drawing(tool));
        }
        if std::env::var("QUANTICK_DRAWING_MAGNET").is_ok_and(|value| value == "1") {
            app.toolrail.set_magnet(true);
        }
        // Pinned favorites by tool id, comma-separated — the same restore
        // path the workspace file takes, so the hook cannot drift from it.
        if let Ok(ids) = std::env::var("QUANTICK_TOOL_FAVORITES") {
            let ids: Vec<String> = ids
                .split(',')
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect();
            app.toolrail.set_favorites(&ids);
            // A staged rail, so a star toggled during the run stays in the run.
            app.workspace.session_mut().stage_favorites();
        }
        // Dock the rail against a named edge, so a validation run can shoot
        // the horizontal band without editing the workspace file.
        if let Ok(edge) = std::env::var("QUANTICK_TOOLBOX_DOCK") {
            let dock = match edge.trim() {
                "left" => Some(ToolboxDock::Left),
                "top" => Some(ToolboxDock::Top),
                "bottom" => Some(ToolboxDock::Bottom),
                _ => None,
            };
            if let Some(dock) = dock {
                app.toolrail.set_dock(dock);
            }
        }
        // Park the scrolling tool band mid-travel. Only the middle of the
        // run shows both chevrons live at once, and a screenshot cannot
        // click an arrow to get there.
        // Nonsense is refused rather than guessed, like the dock above: a
        // typo that silently parked the band at zero would photograph the
        // wrong state and call it the right one.
        if let Ok(offset) = std::env::var("QUANTICK_TOOLBAR_SCROLL") {
            let parked = match offset.trim() {
                "end" => Some(f32::INFINITY),
                other => other.parse::<f32>().ok().filter(|at| at.is_finite()),
            };
            if let Some(parked) = parked {
                app.toolrail.set_band_offset(parked);
            }
        }
        // Open a family flyout on the first frame — the star column lives
        // there, and a screenshot cannot click a caret.
        if let Ok(family_id) = std::env::var("QUANTICK_TOOLBOX_FLYOUT") {
            app.toolrail.request_flyout(family_id.trim().to_owned());
        }
        // The switch itself, so both sides of it are reachable without a
        // click. Set explicitly, it also overrides what the workspace saved:
        // a validation run must be able to pin the state it is photographing.
        // The same registry the menu lists from, so a hook can reach every
        // reach the trader can — and an unknown token is refused out loud
        // rather than silently leaving the default in place, which would look
        // like a press that ignored the run it was told to make.
        if let Ok(token) = std::env::var("QUANTICK_HISTORY_REACH") {
            match history_reach::HistoryReach::from_token(&token) {
                Some(reach) => app.set_history_reach(reach),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_REACH_HOOK_UNKNOWN",
                    token = %token,
                    action = "keep_current_reach",
                    "QUANTICK_HISTORY_REACH names no reach this build has"
                ),
            }
        }
        if let Ok(raw) = std::env::var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
            // Beside `QUANTICK_HISTORY_REACH`, because the reach and how far it
            // goes are one choice: a hook that could pick `by time` but not say
            // how much time would leave the operator setting half of it.
            match raw.trim().parse::<u32>() {
                Ok(minutes) => app.set_history_reach_span_minutes(minutes),
                Err(_) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HISTORY_REACH_SPAN_HOOK_UNREADABLE",
                    value = %raw,
                    action = "keep_current_span",
                    "QUANTICK_HISTORY_REACH_SPAN_MINUTES is not a whole number of minutes"
                ),
            }
        }
        if let Ok(value) = std::env::var("QUANTICK_VENUE_LEAD_IN") {
            // `1` and `0`, and nothing else understood. A typo must not decide
            // a switch the trader set: read as a bare truthiness test, `true`
            // or `on` would silently turn the lead-in *off* and overwrite what
            // the workspace saved, and a capture run would photograph the off
            // state while reporting it as on.
            match value.trim() {
                "1" => app.venue_lead_in = true,
                "0" => app.venue_lead_in = false,
                other => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "VENUE_LEAD_IN_HOOK_UNKNOWN",
                    value = %other,
                    action = "keep_current_setting",
                    "QUANTICK_VENUE_LEAD_IN takes 1 or 0"
                ),
            }
        }
        if let Ok(value) = std::env::var("QUANTICK_PROGRESSIVE_HISTORY") {
            match value.trim() {
                "1" => app.progressive_history = true,
                "0" => app.progressive_history = false,
                // Nonsense is refused rather than guessed: a typo leaves the
                // trader's own setting alone instead of silently flipping it.
                _ => {}
            }
        }
        // The drawing chrome's five hooks, read here rather than on the first
        // drawn frame: the demo appliers run earlier in that frame and ask
        // whether the inspector is open, so a hook another hook depends on has
        // to be in place before any of them. They live with the fields they
        // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
        crate::surfaces::drawing_chrome::apply_launch_hooks(&mut app.surfaces.drawing_chrome);

        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
        }
        // The chart upside down, through the very setter the axis menu's
        // checkbox calls. The inverted frame is otherwise only reachable by
        // a long axis drag no scripted run can perform. Both panes of a
        // split layout: the hook exists so one capture audits every
        // price-mapped surface at once, and a half-inverted frame would
        // silently audit the time pane the right way up.
        if std::env::var("QUANTICK_INVERTED").is_ok_and(|value| value.trim() == "1") {
            let tab = app.active_tab_mut();
            for pane in tab.panes_mut() {
                pane.price_view.set_inverted(true);
            }
        }
        // The tape switch in the canvas's top-right corner — the one control
        // that decides whether there is a band at all. Same setter the chip
        // calls, so a capture shows what a click shows. Anything but `on`/`off`
        // leaves the tape alone rather than guessing.
        if let Ok(value) = std::env::var("QUANTICK_TAPE") {
            match value.trim() {
                "on" => app.active_tab_mut().tape_mut().set_lane_enabled(true),
                "off" => app.active_tab_mut().tape_mut().set_lane_enabled(false),
                _ => {}
            }
        }
        // The tape's own layer switches. The two panes are configured apart and
        // the tape's menu is a right-click a scripted run cannot perform, so the
        // state behind it needs a door of its own — the state, not a second
        // way of drawing it: each entry calls the very setter the menu's
        // checkbox calls. Unlisted layers stay as they were, which is what
        // keeps this hook from being a second opinion about the whole tape.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_LAYERS") {
            let wanted: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .collect();
            let tape = app.active_tab_mut().tape_mut();
            if wanted.contains(&"none") {
                tape.set_lane_depth_visible(false);
                tape.set_lane_bubbles_enabled(false);
            } else {
                for entry in wanted {
                    match entry {
                        "heatmap" => tape.set_lane_depth_visible(true),
                        "bubbles" => tape.set_lane_bubbles_enabled(true),
                        "no-heatmap" => tape.set_lane_depth_visible(false),
                        "no-bubbles" => tape.set_lane_bubbles_enabled(false),
                        // A typo leaves the tape alone rather than guessing at
                        // a layer: a capture of the wrong state is worse than
                        // a capture of the default one.
                        _ => {}
                    }
                }
            }
        }
        // How much market time the tape shows: `auto` follows the bars, a
        // duration pins it (`90s`, `2min`, `120000ms`, or bare milliseconds).
        // Nonsense is refused rather than guessed at, so a typo photographs
        // the default instead of an invented window.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_WINDOW")
            && let Some(window) = parse_tape_window(value.trim())
        {
            app.active_tab_mut().tape_mut().set_live_lane_window(window);
        }
        // Same convenience for the candle footprint — the same field the
        // pane's layer menu writes, so a validation run sees exactly what a
        // click would show.
        if app.harness.footprint() {
            app.active_tab_mut().flow_pane.footprint_visible = true;
        }
        // Every style by its own id, resolved through the same registry the
        // panel's selector and the TOML read. A style reachable by click but
        // not by name is a style the second operator cannot pick, and one
        // more list to keep in step by hand.
        if let Ok(value) = std::env::var("QUANTICK_FOOTPRINT_STYLE") {
            match crate::footprint_config::FootprintStyle::from_id(value.trim()) {
                Some(style) => app.footprint_config.style = style,
                // Named and unknown is a typo in a validation script, and a
                // silent fallback to the default would have it photograph the
                // wrong style and call it a pass.
                None => tracing::warn!(
                    requested = %value,
                    known = ?crate::footprint_config::FootprintStyle::ALL
                        .map(crate::footprint_config::FootprintStyle::id),
                    "QUANTICK_FOOTPRINT_STYLE names no known style; keeping the current one",
                ),
            }
        }
        // The zoom, scriptable: the footprint's detail levels are functions
        // of candle width, and a validation run cannot drag a scroll wheel.
        // Same clamp as the gesture (see Viewport::set_px_per_bar).
        if let Some(px) = app.harness.candle_width() {
            app.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // The bubble budget, scriptable. The fold is the one bubble state a
        // capture cannot otherwise reach: it needs a tape dense enough to
        // exhaust a budget of seven hundred, which is a market condition and
        // not a setting. `QUANTICK_BUBBLE_BUDGET=8` squeezes the same budget
        // the frame always spends, through the same field the projection
        // reads, so what a screenshot shows is what a busy session shows —
        // folded marks wearing their ring and their count.
        if let Ok(value) = std::env::var("QUANTICK_BUBBLE_BUDGET")
            && let Ok(budget) = value.trim().parse::<usize>()
            && budget > 0
        {
            for tab in &mut app.tabs {
                tab.tape_mut().set_primitive_budget(budget);
            }
        }
        // A starved tape, scriptable — the state this whole fix is about. The
        // bubbles trailing the lane's right edge, and past its window leaving
        // it empty, happen when the book keeps arriving and nothing prints. No
        // setting produces that and no capture can wait for the market to do
        // it, so `QUANTICK_TAPE_STARVE_AFTER_MS=8000` stops feeding the tape
        // eight seconds in and lets the book run. Nothing is forged: the
        // prints are withheld through the feed's own call, and the axis then
        // reports the age it actually observes.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_STARVE_AFTER_MS")
            && let Ok(after_ms) = value.trim().parse::<i64>()
            && after_ms >= 0
        {
            for tab in &mut app.tabs {
                tab.tape_mut().set_starve_tape_after_ms(after_ms);
            }
        }
        // Same convenience for indicators: open with the two M1 natives on
        // (EMA overlay + CVD pane), through the same code path the toolbar
        // menu takes, so a scripted validation run needs no clicks.
        if std::env::var("QUANTICK_INDICATORS_AUTOSTART").is_ok_and(|value| value == "1") {
            let pane = &mut app.active_tab_mut().flow_pane;
            for id in AUTOSTART_NATIVES {
                pane.add_indicator(IndicatorSource::Native {
                    id: (*id).to_owned(),
                    values: Vec::new(),
                });
            }
        }
        // The folded legend, reachable from a clean launch: without it the
        // collapsed state is un-photographable by an agent, and a surface no
        // harness can reach is a surface no visual QA covers. Goes through
        // `set_focused_legend_collapsed`, the same call the chevron and the
        // menu entry make — never a field poked from the side.
        if std::env::var("QUANTICK_LEGEND_COLLAPSED").is_ok_and(|value| value == "1") {
            app.set_focused_legend_collapsed(true);
        }
        // Put the active layout on the first tab's panes before any autostart
        // hook: the file is what the user actually had open.
        app.seed_new_panes();
        // The layout strip's hooks (`ui-harness`): open on a named layout,
        // creating it when the file has none by that name, and open the
        // rename box on the active one.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT_TAB")
            && let Some(name) = crate::layouts::clean_name(&name)
        {
            let wanted = app.layouts().by_name(&name).map(|layout| layout.id);
            let outcome = match wanted {
                Some(id) => app.switch_layout(id).map(|_| id),
                None => app.create_layout(Some(&name)),
            };
            if let Err(error) = outcome {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_TAB_HOOK_REFUSED",
                    layout = %name,
                    %error,
                    action = "hook_ignored",
                    "QUANTICK_LAYOUT_TAB could not open the layout"
                );
            }
        }
        // One layout per pane, by name, in pane-address order (`flow,top,bottom`):
        // a capture of two charts on two layouts side by side. Names the book
        // lacks are created empty; an empty entry leaves that pane alone.
        if let Ok(names) = std::env::var("QUANTICK_PANE_LAYOUTS") {
            app.apply_pane_layouts_hook(&names);
        }
        if std::env::var("QUANTICK_LAYOUT_RENAME").is_ok_and(|value| value == "1") {
            let active = app.focused_pane_layout();
            app.begin_layout_rename(active);
        }
        if std::env::var("QUANTICK_LAYOUT_DELETE").is_ok_and(|value| value == "1") {
            let active = app.focused_pane_layout();
            app.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
        }
        // Scripted validation runs can open with library scripts loaded:
        // a comma-separated list of script names, each through the same
        // code path the INDICATORS menu takes.
        if let Ok(names) = std::env::var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match app
                    .script_library
                    .entries()
                    .iter()
                    .position(|entry| entry.name == name)
                {
                    Some(_) => {
                        // Straight onto the focused pane, with no mirror: an
                        // env var is not a user edit. Without this, a scripted
                        // validation run appended its own scripts to the
                        // layout and they opened by themselves on the next
                        // plain launch — config presence activating
                        // something, which the rules forbid. The natives hook
                        // above never registers a kind, so it is already inert.
                        let (tab, side) = {
                            let tab = app.active_tab();
                            (tab.id, tab.focused_side())
                        };
                        app.add_indicator_at(
                            tab,
                            side,
                            &SavedKind::Script {
                                name: name.to_owned(),
                            },
                        );
                        app.forget_last_indicator_state_change();
                    }
                    None => tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "INDICATOR_SCRIPT_UNKNOWN",
                        script = %name,
                        action = "autostart_entry_skipped",
                        "autostart names a script the library does not have"
                    ),
                }
            }
        }
        // Whether a recording opens with the day before it joined in front.
        // Read before anything loads a session, because that is the frame the
        // setting is consulted on. Staged rather than chosen: a validation run
        // states the screen it wants to photograph, and must not write a QA
        // preference into the trader's workspace — the same rule the replay
        // folder follows.
        if let Ok(value) = std::env::var("QUANTICK_REPLAY_DAY_BEFORE") {
            // Refused rather than guessed, like the autostart hook below it: a
            // typo that quietly meant "off" would photograph a single-day
            // chart under a run that believed it had staged a join, which is
            // the one state this hook exists to reach.
            let staged = match value.trim() {
                "1" | "true" | "on" => Some(true),
                "0" | "false" | "off" => Some(false),
                _ => None,
            };
            match staged {
                Some(enabled) => {
                    app.replay_view.stage_day_before(enabled);
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "REPLAY_DAY_BEFORE_STAGED",
                        enabled,
                        requested = value.trim(),
                        "the day before was staged for this run"
                    );
                }
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_UNREADABLE",
                    requested = value.trim(),
                    action = "left_as_the_workspace_has_it",
                    "the day-before hook takes 0 or 1; this run keeps the trader's own setting"
                ),
            }
        }
        // Same convenience for Market Replay: scan the folder in force — the
        // hook, else the stored pick, else the documents home — and play its
        // first session. The same code path a click takes, so a scripted run
        // and a person get the same behaviour.
        // `1` loads and plays, as it always has. `paused` loads and waits,
        // which is what a person now gets when they open a recording, and a
        // state no other hook can reach.
        let autostart_play = match std::env::var("QUANTICK_REPLAY_AUTOSTART")
            .unwrap_or_default()
            .trim()
        {
            "1" => Some(true),
            "paused" => Some(false),
            _ => None,
        };
        if let Some(play) = autostart_play {
            let speed = std::env::var("QUANTICK_REPLAY_SPEED")
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|speed| *speed > 0.0)
                .unwrap_or(1.0);
            // Which recording, when the folder holds more than one. The
            // scan lists them oldest first, so without this a folder of days
            // always opens the one that can have nothing joined in front of
            // it — the single state this hook family exists to avoid.
            let day = std::env::var("QUANTICK_REPLAY_SESSION")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let started = app.replay_view.autostart(speed, day.as_deref(), play);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                // The folder actually scanned, not the environment variable:
                // once a stored pick can supply it, reading the hook back
                // would report an empty folder for a run that scanned a full
                // one — a log that lies about the input it acted on.
                folder = app.replay_view.folder_in_use(),
                speed,
                day = day.as_deref().unwrap_or("(first)"),
                day_before = app.replay_view.day_before(),
                play,
                started,
                action = if started { "load_first_session" } else { "open_browser" },
                "market replay autostart"
            );
        }
        // The session list, opened outright. The browser is one menu entry
        // deep and a validation run has no mouse, so without this the half
        // that shows what a trader already has is the one half no capture can
        // reach — and "I could not find my recordings" is a report about that
        // window, not about the list inside it.
        if std::env::var("QUANTICK_REPLAY_BROWSER").is_ok_and(|value| value == "1") {
            app.replay_view.open_browser();
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_BROWSER_AUTOSTART",
                folder = app.replay_view.folder_in_use(),
                "opened the session browser"
            );
        }
        // The download half of the same browser. Reached on its own because a
        // scripted run has to photograph the Get data tab without a click, and
        // it is a different screen from the session list beside it. Takes the
        // same path the tab click takes — including, for a bare `1`, the
        // chart's own instrument, because that is what clicking the tab now
        // fills the field with and a hook that opened it emptier than a click
        // would photograph a screen no person ever sees.
        if let Ok(value) = std::env::var(crate::replay_view::GET_DATA_ENV) {
            let symbol = match value.trim() {
                "1" | "" => Some(app.active_tab().symbol.clone()),
                symbol => Some(symbol.to_string()),
            };
            app.replay_view.open_get_data(symbol.as_deref());
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_GET_DATA_AUTOSTART",
                symbol = symbol.as_deref().unwrap_or(""),
                "opened the replay download tab"
            );
        }
        // Same convenience for the dock: open a named tab, so a scripted
        // validation run shows a panel without a click.
        if let Ok(name) = std::env::var("QUANTICK_DOCK_TAB") {
            let tab = match name.trim() {
                "l2" => Some(DockTab::L2),
                "bubbles" => Some(DockTab::Bubbles),
                "session" => Some(DockTab::Session),
                "trading" => Some(DockTab::Trading),
                "trades" => Some(DockTab::Trades),
                _ => None,
            };
            match tab {
                Some(tab) => app.dock.open_tab(tab),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "DOCK_TAB_AUTOSTART_UNKNOWN",
                    tab = %name,
                    action = "dock_left_as_is",
                    "QUANTICK_DOCK_TAB names no dock tab"
                ),
            }
        }
        // And for the performance report window — the Report… button's own
        // path, so a scripted run can show it.
        if std::env::var("QUANTICK_PAPER_REPORT_AUTOSTART").is_ok_and(|value| value == "1") {
            app.active_tab_mut().paper.account_mut().autostart_report();
        }
        // The calendar the report grew: reachable open, on a chosen day or
        // a chosen span, with no clicks at all.
        if let Ok(spec) = std::env::var("QUANTICK_PAPER_CALENDAR") {
            match crate::paper_calendar::parse_selection(&spec) {
                Some(selection) => app
                    .active_tab_mut()
                    .paper
                    .account_mut()
                    .autostart_calendar(selection),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PAPER_CALENDAR_AUTOSTART_UNKNOWN",
                    spec = %spec,
                    action = "calendar_left_closed",
                    "QUANTICK_PAPER_CALENDAR is not 1, YYYY-MM-DD or YYYY-MM-DD..YYYY-MM-DD"
                ),
            }
        }
        // Which instrument's saved history the ledger lists.
        if let Ok(spec) = std::env::var("QUANTICK_LEDGER_SCOPE") {
            let scope = match spec.trim() {
                "chart" => Some(crate::paper_trading::LedgerScope::Chart),
                "all" => Some(crate::paper_trading::LedgerScope::All),
                "" => None,
                symbol => Some(crate::paper_trading::LedgerScope::Symbol(symbol.to_owned())),
            };
            match scope {
                Some(scope) => app
                    .active_tab_mut()
                    .paper
                    .account_mut()
                    .set_ledger_scope(scope),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LEDGER_SCOPE_AUTOSTART_UNKNOWN",
                    scope = %spec,
                    action = "ledger_left_on_the_chart",
                    "QUANTICK_LEDGER_SCOPE wants `chart`, `all`, or a symbol folder name"
                ),
            }
        }
        // And the ledger past its first page of saved history — a state
        // only a click on "show older" otherwise reaches.
        if let Ok(text) = std::env::var("QUANTICK_LEDGER_PAGES") {
            match text.trim().parse::<usize>() {
                Ok(pages) if pages >= 1 => {
                    app.active_tab_mut()
                        .paper
                        .account_mut()
                        .autostart_ledger_pages(pages);
                }
                _ => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LEDGER_PAGES_AUTOSTART_UNKNOWN",
                    pages = %text,
                    action = "ledger_left_on_its_first_page",
                    "QUANTICK_LEDGER_PAGES wants a whole number of pages, one or more"
                ),
            }
        }
        // Every day folded shut — the one-line-per-day read, which is
        // otherwise a click on each header.
        if std::env::var("QUANTICK_LEDGER_FOLD").is_ok_and(|value| value == "1") {
            let tz = app.tz;
            app.active_tab_mut()
                .paper
                .account_mut()
                .autostart_folded_days(tz);
        }
        // The report's trade list is open by default, so the hook is how a
        // capture reaches it collapsed.
        if let Ok(value) = std::env::var("QUANTICK_PAPER_REPORT_LIST") {
            app.active_tab_mut()
                .paper
                .account_mut()
                .set_report_list_open(value.trim() != "0");
        }
        // Open on a named canvas layout, through the same path the View menu
        // takes. An env var is an explicit request for this run, so it wins
        // over a feed's declared `default_layout`.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT") {
            let layout = crate::config::DeclaredLayout::parse(&name).map(CanvasLayout::from);
            match layout {
                Some(layout) => app.active_tab_mut().set_layout(layout),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_AUTOSTART_UNKNOWN",
                    layout = %name,
                    action = "layout_left_as_is",
                    accepted = %crate::canvas_layout::LAYOUT_PRESETS
                        .iter()
                        .map(|preset| preset.id)
                        .collect::<Vec<_>>()
                        .join(", "),
                    // Built from the registry rather than spelled out: a
                    // hand-written list goes stale the day a preset is added,
                    // and a run that mistypes an id deserves the real one.
                    "QUANTICK_LAYOUT names no canvas layout"
                ),
            }
        }
        // The Workspace menu's own path, so a validation run can see the save
        // confirmation without a click. A menu entry cannot be reached by an
        // env var, but the state it produces has to be
        // (`.claude/skills/ui-harness`). This writes the file for real,
        // exactly as the entry does — a hook that fakes its surface proves
        // nothing — so point `QUANTICK_UI_STATE` at a scratchpad first.
        if std::env::var("QUANTICK_WORKSPACE_SAVE").is_ok_and(|value| value == "1") {
            app.save_workspace("autostart");
        }
        // The three file entries, reachable with no click for the same reason
        // (`.claude/skills/ui-harness`). Each runs the menu entry's own code
        // past the OS dialog — the dialog is the one thing a scripted run
        // cannot drive, so the path is given instead of picked. They really
        // write and really replace the cockpit, so point `QUANTICK_UI_STATE`
        // and its sibling stores at scratchpad files first.
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_EXPORT") {
            app.export_workspace_to(std::path::Path::new(&path));
        }
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_IMPORT") {
            app.import_workspace_from(std::path::Path::new(&path));
        }
        // An env var is not a user edit: what the autostart hooks switched on
        // must not be written back as though the user had asked for it every
        // launch from now on. Same rule the indicator state follows.
        let staged_layers = app.layer_mask();
        app.workspace.layers_mut().record(staged_layers);
        // The cockpit rescue ran in `main`, before any store was read. A
        // silent one would look like the app relocated the trader's settings
        // behind their back — and leave them not knowing which folder to back
        // up. Same toast channel the journal's rescue uses.
        // `QUANTICK_TOAST=paper`: a simulator acknowledgement, posted through
        // the panel's own `show_toast`.
        //
        // The surface's own hook can raise a message *in* the lane; only this
        // one proves the route to it, which is the half this change built —
        // the panel's outbox, the drain in `settle_paper_panels`, and the
        // eight-second clock the surface owns. Without it the paper path is
        // reachable from a launch only by waiting for a fill and hoping the
        // shutter lands inside the window: the demo trades within the first
        // second and the message is gone eight seconds later, so a capture
        // run photographs an empty lane and cannot tell that from a defect.
        if std::env::var("QUANTICK_TOAST").is_ok_and(|value| value == "paper") {
            app.tabs[0]
                .paper
                .show_toast("SIM: stop filled at 169 790 — flat.".to_owned());
        }
        if let Some(notice) = crate::store_home::rescue_notice() {
            app.tabs[0].paper.show_toast(notice);
        }
        if let Some(summary) = consolidated
            && summary.imported() > 0
        {
            // A silent rescue would look like the app moved files on its
            // own; the toast says what happened and that copies were made.
            app.tabs[0]
                .paper
                .show_toast(crate::paper_home::import_toast(&summary));
        }
        app
    }

    /// Ask the operating system for a trades folder, off the UI thread —
    /// the panel's "choose where trades are saved". One dialog at a time.
    fn open_trades_dir_picker(&mut self) {
        if self.workspace.trades_dir_picker_open() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        // Start where trades actually go right now — under an env override
        // that is the override's folder, not the stored base.
        let start = self.active_tab().paper.account().trades_dir().to_path_buf();
        std::thread::Builder::new()
            .name("quantick-trades-dir-picker".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title("Choose where trades are saved");
                if start.is_dir() {
                    dialog = dialog.set_directory(&start);
                }
                let _ = sender.send(dialog.pick_folder());
            })
            .expect("spawn trades-dir picker thread");
        self.workspace.open_trades_dir_picker(receiver);
    }

    /// Land the picked folder: every tab journals there from now on, and
    /// the choice is remembered across restarts (`paper-state.toml`) —
    /// files already written stay where they are.
    fn poll_trades_dir_picker(&mut self) {
        let Some(receiver) = self.workspace.trades_dir_picker() else {
            return;
        };
        let Ok(choice) = receiver.try_recv() else {
            return;
        };
        self.workspace.close_trades_dir_picker();
        let Some(dir) = choice else { return };
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.trades_dir = Some(dir.display().to_string());
        crate::paper_state::save(&path, &state);
        self.workspace.set_trades_dir(dir);
        for tab in &mut self.tabs {
            tab.paper
                .account_mut()
                .set_trades_dir(self.workspace.trades_dir().to_path_buf());
        }
    }

    /// Persist the active tab's cmd-trading settings and fan them out —
    /// one gesture, one meaning, every tab (the trades-dir rule).
    fn persist_cmd_trading(&mut self) {
        let settings = self.active_tab().paper.account().cmd_trading();
        for tab in &mut self.tabs {
            tab.paper.set_cmd_trading(settings);
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.cmd_trading_enabled = Some(settings.enabled);
        state.cmd_buy_modifier = Some(settings.buy.as_str().to_owned());
        state.cmd_entry_kind = Some(settings.kind.as_str().to_owned());
        state.cmd_sell_modifier = Some(settings.sell.as_str().to_owned());
        crate::paper_state::save(&path, &state);
    }

    /// Save and fan out the strategies after a capability changed them, so a
    /// named call leaves the same durable trace a click does.
    pub(crate) fn control_persist_order_strategies(&mut self) {
        self.persist_order_strategies();
    }

    /// Save and fan out the risk per trade after a capability changed it.
    pub(crate) fn control_persist_risk_settings(&mut self) {
        self.persist_risk_settings();
    }

    /// Persist the risk per trade, the declared capital and the instrument
    /// money, and fan all three out.
    ///
    /// App-wide, like the ticket's other settings: a ceiling a trader sets
    /// in one tab is one they mean everywhere, and what a point of WIN is
    /// worth does not change because a second tab is looking at it.
    pub(crate) fn persist_risk_settings(&mut self) {
        let risk = self.active_tab().paper.account().risk_settings().clone();
        let capital = self.active_tab().paper.account().capital().clone();
        let book = self.active_tab().paper.account().instrument_money().clone();
        for tab in &mut self.tabs {
            tab.paper.account_mut().set_risk_settings(risk.clone());
            tab.paper.account_mut().set_capital(capital.clone());
            tab.paper.account_mut().set_instrument_money(book.clone());
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.risk_per_trade_basis = Some(risk.basis.token().to_owned());
        state.risk_per_trade_amount = Some(risk.amount.normalize().to_string());
        state.risk_per_trade_percent = Some(risk.percent.normalize().to_string());
        state.risk_per_trade_lock = Some(risk.lock);
        state.paper_capital = crate::risk_sizing::records_from_capital(&capital);
        state.instrument_money = crate::risk_sizing::records_from_book(&book);
        crate::paper_state::save(&path, &state);
    }

    /// Persist the named exit strategies and the ticket's selection, and fan
    /// them out - app-wide like cmd trading, because a ladder a trader built
    /// in one tab is a ladder they mean everywhere.
    fn persist_order_strategies(&mut self) {
        // The wheel's per-instrument step rides with the strategies: both
        // are ticket settings the trader configures once, and both are
        // app-wide rather than per tab.
        let steps: std::collections::BTreeMap<String, String> = self
            .active_tab()
            .paper
            .ruler_steps()
            .iter()
            .map(|(symbol, step)| (symbol.clone(), step.normalize().to_string()))
            .collect();
        let strategies = self
            .active_tab()
            .paper
            .account()
            .order_strategies()
            .to_vec();
        let selected = self
            .active_tab()
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        for tab in &mut self.tabs {
            tab.paper
                .account_mut()
                .set_order_strategies(strategies.clone(), selected.as_deref());
            tab.paper.set_ruler_steps(
                steps
                    .iter()
                    .filter_map(|(symbol, step)| {
                        step.parse().ok().map(|value| (symbol.clone(), value))
                    })
                    .collect(),
            );
        }
        let path = crate::paper_state::default_path();
        let mut state = crate::paper_state::load(&path);
        state.order_strategies = Some(strategies);
        state.selected_order_strategy = selected;
        state.ruler_steps = steps;
        crate::paper_state::save(&path, &state);
    }

    /// The active tab beside the config it reads.
    ///
    /// Split here, once, because almost every tab operation needs both and
    /// `self.tabs[i].f(&self.config)` is a borrow error at every call site.
    fn active_with_config(&mut self) -> (&mut Tab, &AppConfig) {
        (&mut self.tabs[self.active_tab], &self.config)
    }

    /// The tab on screen.
    fn active_tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// See [`Self::active_tab`].
    fn active_tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }

    /// Read-only application roots available to the on-demand control
    /// projections. The gateway never receives `QuantickApp`; it receives the
    /// owned DTOs built from these narrow views.
    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn control_tab_at(&self, index: usize) -> Option<&Tab> {
        self.tabs.get(index)
    }

    /// The mutable twin, for the cockpit tier.
    ///
    /// Narrow on purpose: the layout capabilities need to *change* a tab, and
    /// handing them the whole application would let a later one reach past the
    /// canvas into the feed or the simulator.
    pub(crate) fn control_tab_at_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One tab beside the configuration it reads, by position.
    ///
    /// [`Self::active_with_config`] for a tab that is not necessarily the
    /// active one — a capability names the tab it acts on, and respawning a
    /// feed needs the feed table the same way a click in the corner does.
    pub(crate) fn control_tab_with_config(
        &mut self,
        index: usize,
    ) -> Option<(&mut Tab, &AppConfig)> {
        let Self { tabs, config, .. } = self;
        tabs.get_mut(index).map(|tab| (tab, &*config))
    }

    /// The trading host of the tab on screen — where the `trade.*` actions
    /// land. The active tab and not an addressed one: an order belongs to
    /// the symbol the trader is looking at, and a call that could quietly
    /// trade a chart nobody has open is a call nobody should be able to
    /// make.
    pub(crate) fn control_active_paper_mut(
        &mut self,
    ) -> Option<&mut crate::paper_trading::PaperTrading> {
        // Fallible, because the rest of the control code does not trust the
        // invariant either: `annotate::resolve_target` guards an empty tab
        // list and clamps the index, and two more sites clamp it. A
        // `trade.*` call must answer "this window has no chart open" rather
        // than panic the whole trading application, and it must resolve the
        // *same* tab its own read-back resolves.
        self.tabs.get_mut(self.active_tab).map(|tab| &mut tab.paper)
    }

    /// The read side of [`Self::control_active_paper_mut`], resolved the same
    /// way so a call and its read-back can never name different tabs.
    pub(crate) fn control_active_paper(&self) -> Option<&crate::paper_trading::PaperTrading> {
        self.tabs.get(self.active_tab).map(|tab| &tab.paper)
    }

    pub(crate) fn control_tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub(crate) fn control_active_tab_index(&self) -> usize {
        self.active_tab
    }

    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    pub(crate) fn show_agent_popup(&mut self, popup: crate::control::AgentPopup) {
        self.surfaces.agent_popup.show(popup);
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    pub(crate) fn show_agent_toast(&mut self, message: String) {
        self.surfaces.toast.note(message, Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    pub(crate) fn sound_agent_alert(&mut self) -> Option<String> {
        self.alerts
            .play(&[crate::audio::Cue::default()])
            .err()
            .map(ToOwned::to_owned)
    }

    /// One pane, by tab position and side — the mutable half of
    /// [`Self::control_tabs`], for the actions that place objects.
    pub(crate) fn control_pane_mut(
        &mut self,
        tab_index: usize,
        side: crate::pane::PaneSide,
    ) -> &mut ChartPane {
        self.tabs[tab_index].pane_mut(side)
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn control_new_drawing(&self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(&self.drawing_presets, tool)
    }

    pub(crate) fn control_config(&self) -> &AppConfig {
        &self.config
    }

    /// The window's footprint setup — the one a pane falls back to when it
    /// carries no override of its own.
    pub(crate) fn control_footprint_config(&self) -> &crate::footprint_config::FootprintConfig {
        &self.footprint_config
    }

    /// The window's shared chart style, which owns the layers no pane does.
    pub(crate) fn control_style(&self) -> &ChartStyle {
        &self.style
    }

    /// The drawing tool rail: which tool is armed, and whether it is on
    /// screen at all.
    pub(crate) fn control_tool_rail(&self) -> &ToolRail {
        &self.toolrail
    }

    /// The colour the chart's corner is wearing, or `None` while the chart
    /// is being fed.
    ///
    /// The status line's provenance dot takes this rather than deciding for
    /// itself. It used to read the connection alone, which is a socket's
    /// opinion: a terminal that froze with the socket open had the
    /// bottom-left of the window saying `live` while the bottom-right said
    /// `offline`, about the same feed, at the same moment. Two surfaces
    /// disagreeing about the one question the trader is asking is worse than
    /// either answer alone, so there is one report and both read it.
    fn feed_offline_accent(
        &self,
        stall: Option<&quantick_feed::stall::Stall>,
    ) -> Option<egui::Color32> {
        feed_notice::report(&self.active_tab().notice, stall)
            .filter(feed_notice::Report::is_offline)
            .map(|report| report.accent())
    }

    /// Where the feed's offline chip was painted, or `None` when it was not.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn control_feed_chip_rect(&self) -> Option<egui::Rect> {
        self.feed_chip_rect
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn control_feed_popup_open(&self) -> bool {
        self.feed_popup_tab == Some(self.active_tab().id)
    }

    /// The right-hand dock: whether it is shown, and which tab is open.
    pub(crate) fn control_dock(&self) -> &Dock {
        &self.dock
    }

    pub(crate) fn control_timezone(&self) -> TzOffset {
        self.tz
    }

    pub(crate) fn control_workspace_flags(&self) -> (bool, bool, bool) {
        (
            self.workspace.session().save_on_exit(),
            self.show_perf,
            self.progressive_history,
        )
    }

    /// Choose how far one press of *load older* reaches.
    ///
    /// The named call behind the history menu's reach chips and the
    /// `QUANTICK_HISTORY_REACH` hook — one path, so an operator without a
    /// mouse sets what a click sets. Mirrored onto every tab by `drain_tabs`,
    /// where a run in flight also reads it: withdrawing the longer reach is
    /// how a trader calls that run off.
    pub(crate) fn set_history_reach(&mut self, reach: history_reach::HistoryReach) {
        self.history_reach = reach;
    }

    /// How far back one press of the `by time` reach pulls, in minutes of
    /// traded time.
    ///
    /// Clamped rather than refused: a span of zero is a press that asks for
    /// nothing, and the operator that sent it meant *some* history. The
    /// ceiling is the campaign's own span cap, past which no run can reach
    /// anyway, so accepting a larger number would be promising a reach the
    /// budgets forbid.
    pub(crate) fn set_history_reach_span_minutes(&mut self, minutes: u32) {
        let ceiling = (history_reach::MAX_CAMPAIGN_SPAN_MS / 60_000) as u32;
        self.history_reach_span_minutes = minutes.clamp(1, ceiling);
    }

    /// What that span is now, for an operator reading back what it set.
    pub(crate) fn control_history_reach_span_minutes(&self) -> u32 {
        self.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn control_history_settings(&self) -> (history_reach::HistoryReach, bool) {
        (self.history_reach, self.venue_lead_in)
    }

    /// Whether a recording opens with the session day before it joined in
    /// front, and a download fetches that day's tape too.
    ///
    /// A choice an operator without a mouse has to be able to read back after
    /// setting it: it decides what a replay they are about to open will hold.
    pub(crate) fn control_replay_day_before(&self) -> bool {
        self.replay_view.day_before()
    }

    /// Invoke one registered control action from inside the application,
    /// attributed to the human at this window (or to automation when a
    /// control trace replays it). The hotkey, the `QUANTICK_CONTROL_MARK`
    /// hook and the tests all arrive here; there is no second path.
    pub(crate) fn control_action(
        &mut self,
        capability_id: &str,
        capability_version: u32,
        origin: crate::control::ActionOrigin,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        let outcome =
            access.invoke_local_action(self, capability_id, capability_version, input, origin);
        self.control_access = Some(access);
        outcome
    }

    /// How many objects an operator other than the trader placed, across
    /// every pane one can reach — an assistant may annotate any open tab, so
    /// counting the active pane alone would offer to take back a subset and
    /// call it all of them.
    fn authored_object_count(tabs: &[Tab]) -> usize {
        tabs.iter()
            .map(|tab| {
                tab.panes()
                    .map(|(pane, _side)| pane.drawings.authored_count())
                    .sum::<usize>()
            })
            .sum()
    }

    /// Take back every object an operator placed, wherever it is. One undo
    /// entry per pane, and the resting orders of any armed strategy go with
    /// the objects they were anchored to.
    fn remove_every_authored_object(&mut self) -> usize {
        let mut removed = 0;
        for tab in &mut self.tabs {
            // Every pane the tab holds, not the two it used to. "Remove
            // objects placed for you" promises to take them *all* back, and a
            // sweep that skipped the second stacked chart would leave an
            // assistant's marks behind while reporting the job done.
            for pane in tab.panes_mut() {
                let taken = pane.drawings.remove_authored();
                if taken > 0 {
                    pane.sweep_strategy_orphans();
                    removed += taken;
                }
            }
        }
        removed
    }

    /// The annotate tier's launch hooks: one agent-authored label, one
    /// notification. Both go through the registered action with an agent
    /// actor — the same path the gateway takes for a remote client — so what
    /// a screenshot shows is what a real assistant would have produced.
    fn apply_control_annotate_hooks(&mut self) {
        if let Some(text) = self.pending_control_annotation.take() {
            let anchor = {
                let pane = self.active_tab().drawing_pane();
                let slot = pane.slots().saturating_sub(1);
                match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
                    (Some(time), Some(bar)) => Some(serde_json::json!({
                        "time_unix_ms": time,
                        "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close)
                            .unwrap_or(1.0)
                            .to_string(),
                    })),
                    // No bars yet: put the hook back and take it next frame,
                    // rather than annotating a chart that has nothing on it.
                    _ => {
                        self.pending_control_annotation = Some(text.clone());
                        None
                    }
                }
            };
            if let Some(anchor) = anchor {
                self.pending_control_annotation = None;
                self.run_hook_action(
                    "annotate.label.create",
                    serde_json::json!({ "anchors": [anchor], "text": text }),
                );
            }
        }
        if let Some(request) = self.pending_control_notification.take() {
            let (channel, message) = request
                .split_once(':')
                .unwrap_or(("toast", request.as_str()));
            let capability = match channel.trim() {
                "popup" => Some("notify.popup"),
                "sound" => Some("notify.sound"),
                "toast" => Some("notify.toast"),
                other => {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
                        channel = other,
                        "QUANTICK_CONTROL_NOTIFY names no notification channel"
                    );
                    None
                }
            };
            if let Some(capability) = capability {
                self.run_hook_action(
                    capability,
                    serde_json::json!({ "message": message, "title": "From your assistant" }),
                );
            }
        }
    }

    /// Invoke one registered action as an *agent* would, from inside this
    /// window. The hooks use it so a screenshot shows a real assistant's
    /// object, attribution and all, without a client on the socket.
    fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.control_access.take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        // No identity, no actor to sign with: the same structured refusal an
        // action gets, rather than a panic on the first frame.
        let Some(actor) = access.hook_agent_actor() else {
            self.control_access = Some(access);
            return Err(quantick_control::error::ControlError::invalid_request(
                "this window has no control identity to act with",
            ));
        };
        let outcome = access.invoke_local_action(
            self,
            capability_id,
            1,
            input,
            crate::control::ActionOrigin::Remote(Box::new(actor)),
        );
        self.control_access = Some(access);
        outcome
    }

    /// A launch hook's action, with its failure reported where a scripted run
    /// will see it: the hook is fire-and-forget, so nothing else would.
    fn run_hook_action(&mut self, capability_id: &str, input: serde_json::Value) {
        if let Err(error) = self.run_agent_action(capability_id, input) {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_HOOK_ACTION_FAILED",
                capability = capability_id,
                error_code = %error.code,
                error = %error.message,
                "an annotate hook could not run its action"
            );
        }
    }

    /// The mark hotkey's body: `attention.mark.create` with the resolved
    /// cursor target, attributed to the human.
    pub(crate) fn take_mark(&mut self, note: Option<String>) {
        let mut input = serde_json::Map::new();
        if let Some(note) = note {
            input.insert("note".to_owned(), serde_json::Value::String(note));
        }
        // No target: the action port resolves the pointer at the moment of
        // the gesture and records the resolved input, so the trace line
        // determines the mark on its own and a rerun marks the same bar.
        match self.control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::Value::Object(input),
        ) {
            Ok(result) => tracing::info!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_TAKEN",
                sequence = %result["sequence"],
                "mark taken"
            ),
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_MARK_REFUSED",
                code = %error.code,
                "mark refused"
            ),
        }
    }

    pub(crate) fn control_frame_metrics(&self) -> ControlFrameMetrics {
        ControlFrameMetrics {
            wall_average_ms: self.frames.avg_ms(),
            wall_worst_ms: self.frames.worst_ms(),
            frames_per_second: self.frames.fps(),
            cpu_average_ms: self.cpu_frames.avg_ms(),
            cpu_worst_ms: self.cpu_frames.worst_ms(),
        }
    }

    /// The pane the chrome speaks for: the active tab's focused pane (§11).
    fn focused_pane(&self) -> &ChartPane {
        self.active_tab().focused_pane()
    }

    /// See [`Self::focused_pane`].
    fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().focused_pane_mut()
    }

    /// The pane every drawing surface speaks for: the one holding the
    /// selection, which is the focused pane unless a shared mark was taken
    /// from the chart it is mirrored on (see [`Tab::drawing_side`]).
    ///
    /// The inspector, the keyboard, the object manager and the toast all read
    /// through here, so an object selected on either of its two charts is
    /// edited and deleted from either of them.
    fn drawing_pane(&self) -> &ChartPane {
        self.active_tab().drawing_pane()
    }

    /// See [`Self::drawing_pane`].
    fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().drawing_pane_mut()
    }

    /// The slot a command from the chrome addresses: the active tab, its
    /// focused pane, that slot.
    fn target_slot(&self, slot: SlotId) -> TabSlot {
        TabSlot {
            tab: self.active_tab().id,
            side: self.active_tab().focused_side(),
            slot,
        }
    }

    /// Open `feed_id`/`symbol` in a new tab and make it active.
    ///
    /// Opening a market a tab already holds is allowed — two views of one
    /// book are a legitimate thing to want. For MetaTrader that means two
    /// listeners on one port, and the second one loses the bind: that tab
    /// shows the bridge's own bind-failure notice, which is the honest answer
    /// and the reason `[metatrader.ports]` maps a port per symbol.
    fn open_tab(&mut self, feed_id: String, symbol: String, spec: Option<BarSpec>) {
        let Some(provider) = self.config.provider_of(&feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "TAB_OPEN_UNKNOWN_FEED",
                feed = %feed_id,
                action = "ignore_request",
                "asked to open a feed the config does not have"
            );
            return;
        };
        // One feed per tab, resolved per symbol: a MetaTrader tab binds the
        // port `[metatrader.ports]` maps its symbol to (`endpoint_for`), so two
        // MT5 tabs on different symbols listen on different ports and each
        // finds its own bridge. Two tabs on the *same* MT5 symbol is allowed
        // and means one port for two listeners: the second loses the bind and
        // shows the feed's own MT5_BIND_FAILED notice, which is the honest
        // answer rather than a silently dead chart.
        let handle = feed::spawn_live(
            provider,
            &symbol,
            &self.config.metatrader,
            crate::paper_home::shelf_dir(),
        );
        self.adopt_tab(feed_id, symbol, handle, spec);
    }

    /// Take a market that is already streaming as a new tab, and make it the
    /// active one.
    ///
    /// The bar spec is inherited from the tab you were on: opening a second
    /// market to compare it against the first is the reason to do this, and
    /// landing on a different aggregation would defeat that. A feed that
    /// declares its own `default_bars`/`default_layout` overrides the
    /// inheritance — the declaration exists because that market reads
    /// differently, which is exactly when inheriting would mislead.
    /// `spec` overrides both, and exists for the one caller that already knows
    /// the answer: a workspace restoring the bar rule this market was last
    /// read on. Inheriting there would quietly discard what the user saved.
    fn adopt_tab(
        &mut self,
        feed_id: String,
        symbol: String,
        feed: FeedHandle,
        spec: Option<BarSpec>,
    ) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_OPENED",
            tab = id,
            feed = %feed_id,
            symbol = %symbol,
            tabs = self.tabs.len() + 1,
            action = "activate_new_tab",
            "opening a market in a new tab"
        );
        let spec = spec.unwrap_or_else(|| {
            self.config
                .startup_spec_for(&feed_id)
                .unwrap_or_else(|| self.active_tab().flow_pane.state.spec().clone())
        });
        let trades_dir = self.workspace.trades_dir().to_path_buf();
        // Cmd trading is app-wide (the trades-dir rule): a new tab starts
        // with the settings every other tab already carries.
        let cmd_trading = self.active_tab().paper.account().cmd_trading();
        let inherited_strategies = self
            .active_tab()
            .paper
            .account()
            .order_strategies()
            .to_vec();
        let inherited_selection = self
            .active_tab()
            .paper
            .account()
            .selected_order_strategy()
            .map(|strategy| strategy.name.clone());
        // Orientation travels with the working state the new tab inherits —
        // a market opened to compare against the active one is only
        // comparable the same way up. Per pane; a pane the source tab does
        // not have follows its flow chart.
        // The layers the active tab is *actually showing*, read before the new
        // tab is pushed. This used to be `self.layer_defaults` — the map read
        // off the file at startup — which was only harmless while that map was
        // whatever partial thing the trader's file happened to hold. Now that a
        // file's silence resolves to the shipped answer (`chart_layers::load`),
        // that map speaks for every layer, and applying it here would undo the
        // switches of the session mid-flight. Reading the live state is also
        // what the comment below has always promised.
        let inherited_risk = self.active_tab().paper.account().risk_settings().clone();
        let inherited_capital = self.active_tab().paper.account().capital().clone();
        let inherited_money = self.active_tab().paper.account().instrument_money().clone();
        let inherited_layers = self.active_tab().flow_pane.layer_states(&self.style);
        let flow_inverted = self.active_tab().flow_pane.price_view.is_inverted();
        let time_inverted = self
            .active_tab()
            .time_pane()
            .map_or(flow_inverted, |pane| pane.price_view.is_inverted());
        // The layout the trader is looking at is the one the new chart
        // opens on — read before the new tab takes the focus.
        let inherited_layout = (!self.tabs.is_empty()).then(|| self.focused_pane_layout());
        let flow_pane_id = self.pane_ids.alloc();
        let mut tab = Tab::new(id, flow_pane_id, feed_id, symbol, spec, feed, trades_dir);
        tab.paper.set_cmd_trading(cmd_trading);
        tab.paper
            .account_mut()
            .set_order_strategies(inherited_strategies, inherited_selection.as_deref());
        // The risk per trade travels with them. It is app-wide like the rest
        // of the ticket's settings, and a tab that opened without it would
        // hand the trader a bare quantity field on a market they meant to
        // size the same way as the one beside it.
        tab.paper.account_mut().set_risk_settings(inherited_risk);
        tab.paper.account_mut().set_capital(inherited_capital);
        tab.paper
            .account_mut()
            .set_instrument_money(inherited_money);
        tab.flow_pane.layout = inherited_layout;
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        let config = self.config.clone();
        self.active_tab_mut().refresh_chip_label(&config);
        self.active_tab_mut().ensure_book_capture(&config);
        self.active_tab_mut().apply_feed_bubble_preset(&config);
        self.active_tab_mut().apply_feed_declared_layout(&config);
        // The new tab opens on the layers the user left showing, over the
        // preset it just put on: opening a second market is not a request to
        // bring back the chrome they switched off.
        self.active_tab_mut()
            .flow_pane
            .apply_layer_states(&inherited_layers);
        // The scripted footprint/zoom hooks reach tabs opened later too: the
        // replay tab a validation run autostarts is the tab the run means,
        // and it does not exist yet when the boot hooks fire.
        if self.harness.footprint() {
            self.active_tab_mut().flow_pane.footprint_visible = true;
        }
        if let Some(px) = self.harness.candle_width() {
            self.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // After the declared layout ran: that is what decides whether the
        // new tab has a time pane to orient at all.
        let tab = self.active_tab_mut();
        tab.flow_pane.price_view.set_inverted(flow_inverted);
        for time_pane in tab.time_panes.iter_mut() {
            time_pane.price_view.set_inverted(time_inverted);
        }
    }

    /// Close the tab at `index`, activating a neighbour.
    ///
    /// The last tab stays: a window with no market has nothing to draw. What
    /// the closed tab owned goes with it — dropping its `FeedHandle` closes
    /// the receivers its feed thread sends into, and dropping its panes drops
    /// the indicator worker and book worker handles, whose run loops end when
    /// their command channels disconnect. No joins, no shutdown protocol.
    fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        let mut closed = self.tabs.remove(index);
        // The tab's session ends here. Everything else it owns can simply be
        // dropped — the feed thread and the workers stop when their channels
        // go — but a simulated position is state the user created, and the
        // paper-trading contract says it ends in a labeled, journaled flatten,
        // never by vanishing with its window.
        closed.close();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "TAB_CLOSED",
            tab = closed.id,
            feed = %closed.feed_id,
            symbol = %closed.symbol,
            tabs = self.tabs.len(),
            action = "drop_feed_and_workers",
            "closing a market tab"
        );
        // Its slots are gone with its panes; the bookkeeping must not outlive
        // them or a later tab reusing a slot number would inherit its kind.
        self.slot_kinds.retain(|(owner, _)| owner.tab != closed.id);
        self.operator_slots.retain(|owner| owner.tab != closed.id);
        self.script_files
            .retain(|(owner, ..)| owner.tab != closed.id);
        self.pending_hidden.retain(|owner| owner.tab != closed.id);
        self.pending_styles
            .retain(|(owner, _)| owner.tab != closed.id);
        self.active_tab = self.active_tab.min(self.tabs.len() - 1);
        drop(closed);
    }

    /// Move `delta` tabs along the strip, wrapping (§10: Ctrl+Tab).
    fn cycle_tab(&mut self, delta: isize) {
        if self.tabs.len() < 2 {
            return;
        }
        let count = self.tabs.len() as isize;
        let next = (self.active_tab as isize + delta).rem_euclid(count);
        self.active_tab = next as usize;
    }

    /// Whether the toolbar's heatmap lamp is lit.
    ///
    /// The *switch*, not what capture lets through it — the same reading the
    /// layer file was taught in 848cba0, and for a sibling reason. A lamp lit
    /// from `depth_visible()` (`enabled && show_depth`) reports the heatmap off
    /// for as long as book capture is starting, and forever on a source with no
    /// book: the trader sees an unlit button, presses it, and switches the
    /// layer they wanted *off*. The button already has an honest way to say a
    /// source cannot fill it — `.enabled(...)` carrying its
    /// `disabled_explanation` — so the lamp beside it answers the only other
    /// question there is.
    ///
    /// A named reading rather than an expression inside the toolbar's own
    /// frame, so the rule can be asserted without painting a toolbar. What
    /// reads it back without looking at the screen is the semantic scene,
    /// which takes the same `Tab::layer_toggle_state` this delegates to.
    #[must_use]
    #[cfg(test)]
    fn heatmap_lamp_on(&self) -> bool {
        // Through the group's one reading, so this named rule and the lamp the
        // toolbar actually paints cannot become two answers to one question.
        // `#[cfg(test)]` because the toolbar now takes the group's reading
        // directly: keeping a second production entry point to the same answer
        // is how the two drift.
        self.active_tab()
            .layer_toggle_state(
                ChartLayer::Heatmap,
                &self.style,
                self.active_tab().capabilities(&self.config),
            )
            .0
    }

    /// Build the toolbar's model from the app's state, draw it, and carry
    /// out whatever it asked (§6 — the toolbar module owns grouping and the
    /// overflow rule; this method owns the side effects).
    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        // Pre-collect owned option lists so the toolbar's combos don't borrow
        // `self.config` while they mutate `self.feed_id` / `self.active_tab().symbol`.
        // Providers that aren't streaming yet are labelled "(soon)" so the
        // menu is honest about what actually connects.
        let feeds: Vec<(String, String)> = self
            .config
            .feeds
            .iter()
            .map(|f| {
                let label = if f.provider.is_implemented() {
                    f.name.clone()
                } else {
                    format!("{} (soon)", f.name)
                };
                (f.id.clone(), label)
            })
            .collect();
        let symbols: Vec<String> = self
            .config
            .feed(&self.active_tab().feed_id)
            .map(|f| f.symbols.clone())
            .unwrap_or_default();
        // During a replay the SOURCE group gives way to what is actually
        // playing: a live venue cannot be picked without leaving the
        // recording first, and a combo that silently did so would throw away
        // the session mid-run.
        let replay = self
            .active_tab()
            .replay
            .as_ref()
            .map(|link| toolbar::ReplaySource {
                label: link.label(),
                hover: format!(
                    "Replaying {}\nSide source: {}",
                    link.session.path.display(),
                    link.session
                        .header
                        .side_source
                        .as_deref()
                        .unwrap_or("not recorded"),
                ),
            });
        let capabilities = self.active_tab().capabilities(&self.config);
        let candles_held = self.active_tab().venue_candles_held();
        let older_candles = self.active_tab().older_candles(capabilities);
        let feed_display_name = self.active_tab().feed_display_name(&self.config).to_owned();
        // One reading per lamp, taken through the call the semantic scene
        // makes too, so the button and what an operator captures cannot
        // disagree about a layer. Every lamp reports the *switch* rather than
        // what the source lets through it — the rule `heatmap_lamp_on` names,
        // now the whole group's.
        let layers = toolbar::LayerToggle::ALL.map(|toggle| {
            let (on, blocked) =
                self.active_tab()
                    .layer_toggle_state(toggle.layer(), &self.style, capabilities);
            toolbar::LayerToggleState { on, blocked }
        });
        // The focused pane's slots (§11): the menu lists what a command from
        // it would act on, and never the pane beside it.
        let indicators: Vec<toolbar::IndicatorMenuEntry> = self
            .focused_pane()
            .indicators
            .all()
            .iter()
            .map(|view| toolbar::IndicatorMenuEntry {
                slot: view.slot.0,
                label: view.label().to_owned(),
                hidden: view.hidden,
                errored: view.error.is_some(),
                stale: view.stale.is_some(),
            })
            .collect();
        let scripts: Vec<String> = self
            .script_library
            .entries()
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let dock_visible = self.dock.visible();
        let show_style = self.surfaces.style_panel.is_open();
        // Read before the tab is borrowed mutably. The reach is the window's
        // standing choice, like the progressive-history switch — a trader who
        // picked "previous session" once means it in the next tab too — so it
        // is split off and written back the way the layout picker's flags are.
        let history_reach_running = self.active_tab().history_reach_running();
        let mut history_reach = self.history_reach;
        let mut history_reach_span_minutes = self.history_reach_span_minutes;
        let mut history_menu_rect = self.history_menu_rect;
        // The SOURCE group writes straight into the active tab: a feed or
        // symbol change is that tab's market switch. The BARS group writes
        // into the *focused pane* — the pane the status bar reads and every
        // indicator command lands on (§11) — so the three chrome surfaces
        // can never disagree about which chart a command describes, and in
        // the Time layout the group governs the chart actually on screen.
        // Split off the picker's flags before the tab borrow: the model wants
        // both, and they live on the same struct.
        let mut layout_picker_open = self.layout_picker_open;
        // One shot: the hook opens the popover on the first drawn frame and
        // then gets out of the way, so a trader's click can close it.
        let layout_picker_autostart = self.harness.take_layout_picker_autostart();
        let tab = self.active_tab_mut();
        let focused = tab.focused_side();
        let pane = match focused {
            PaneSide::Time(slot) => tab.time_panes.get_mut(slot).unwrap_or(&mut tab.flow_pane),
            PaneSide::Flow => &mut tab.flow_pane,
        };
        let mut model = toolbar::ToolbarModel {
            layout_preset: Some(tab.layout.preset()),
            layout_picker_open: &mut layout_picker_open,
            layout_picker_request_open: layout_picker_autostart,
            feeds,
            feed_id: &mut tab.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut tab.symbol,
            replay,
            kind: &mut pane.kind,
            tick_n: &mut pane.tick_n,
            volume_units: &mut pane.volume_units,
            dollar_notional: &mut pane.dollar_notional,
            time_interval_ms: &mut pane.time_interval_ms,
            imbalance_target: &mut pane.imbalance_target,
            imbalance_unit: &mut pane.imbalance_unit,
            history_step: &mut tab.history_step,
            history_menu_rect: &mut history_menu_rect,
            history_reach_span_minutes: &mut history_reach_span_minutes,
            history_reach: &mut history_reach,
            history_reach_running,
            history_trades: tab.history_trades,
            history_candles: candles_held,
            older_candles,
            capabilities,
            layers,
            dock_visible,
            appearance_open: show_style,
            paper: toolbar::PaperTradeModel {
                // The lock reaches the toolbar too. Gating only the dock's
                // pair left these lit while the ticket refused, so a fast
                // click here only toasted - and the doc promises the entry
                // pair disables.
                ready: tab.paper.ready() && !tab.paper.risk_report().1,
                buy_label: tab.paper.entry_label(quantick_engine::Side::Buy),
                sell_label: tab.paper.entry_label(quantick_engine::Side::Sell),
                buy_hover: tab.paper.entry_hover(quantick_engine::Side::Buy),
                sell_hover: tab.paper.entry_hover(quantick_engine::Side::Sell),
                close_label: tab.paper.close_button_label(),
            },
            indicators,
            scripts,
        };
        let actions = toolbar::draw(ctx, &mut model);
        // The popover's own state, back where it lives. Without this the flag
        // resets every frame and the button never reads as open.
        drop(model);
        self.layout_picker_open = layout_picker_open;
        self.set_history_reach(history_reach);
        // Through the setter, so a value dragged past the campaign's own span
        // cap is clamped in the one place that knows the cap.
        self.set_history_reach_span_minutes(history_reach_span_minutes);
        self.history_menu_rect = history_menu_rect;
        // A newly picked feed may not offer the current symbol. Never during
        // a replay: the recorded instrument belongs to no live feed's menu,
        // and snapping it away would relabel the whole session — the status
        // bar and the logs must keep naming what is actually playing.
        if self.active_tab().replay.is_none() {
            let (tab, config) = self.active_with_config();
            tab.ensure_symbol_valid(config);
            tab.refresh_chip_label(config);
        }
        for action in actions {
            self.apply_toolbar_action(action);
        }
    }

    /// Switch the active tab to `preset`.
    ///
    /// **The one path.** The toolbar's picker, the `View → Layout` menu and
    /// the keyboard all arrive here, so none of them can grow its own idea of
    /// what applying a layout does. A control-plane capability joins them by
    /// calling this, never by repeating it.
    fn apply_layout_preset(&mut self, preset: &'static crate::canvas_layout::LayoutPreset) {
        let Some(layout) = CanvasLayout::from_preset(preset) else {
            // A preset the canvas cannot draw yet is refused rather than
            // approximated: switching to the nearest arrangement would be the
            // picker showing one layout and the canvas drawing another.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_PRESET_UNSUPPORTED",
                preset = %preset.id,
                action = "layout_left_as_is",
                "the layout registry names an arrangement the canvas cannot draw yet"
            );
            return;
        };
        self.active_tab_mut().set_layout(layout);
    }

    /// One toolbar side effect. Layer toggles reuse the same code paths the
    /// old checkboxes took, so provider gating and command acknowledgement
    /// rules are unchanged.
    fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadOlder => {
                let (tab, config) = self.active_with_config();
                tab.request_older_history(config);
            }
            ToolbarAction::LoadOlderCandles => {
                // Read before the tab is borrowed mutably — and the capability
                // block rather than the whole config, because that is all the
                // request needs to know.
                let capabilities = self.active_tab().capabilities(&self.config);
                self.active_tab_mut()
                    .request_older_ohlcv_history(capabilities);
            }
            ToolbarAction::SetHeatmap(shown) => {
                self.active_tab_mut().tape_mut().set_depth_visible(shown);
            }
            ToolbarAction::SetBubbles(enabled) => {
                self.active_tab_mut()
                    .tape_mut()
                    .set_bubbles_enabled(enabled);
            }
            ToolbarAction::SetLiveStrip(shown) => {
                self.active_tab_mut().flow_pane.live_strip_visible = shown;
            }
            // The focused pane's own field, through the same setter the pane's
            // layer menu calls — so the button, the menu and the lamp can
            // never disagree about which chart the command described.
            ToolbarAction::SetFootprint(shown) => {
                self.focused_pane_mut().footprint_visible = shown;
            }
            ToolbarAction::OpenFootprintSettings => self.surfaces.footprint_settings.open(),
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::SetLayout(preset) => self.apply_layout_preset(preset),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.surfaces.style_panel.toggle(),
            // Every indicator command lands on the focused pane (§11), which
            // is the flow pane whenever the canvas is not split.
            // Adding an indicator by hand is the plainest possible request to
            // see one, so it opens a folded legend rather than letting the new
            // row land inside the puck — the trader would get one more dot and
            // no way to tell the add from a no-op. Not the auto-collapse the
            // design ruled out: that rule protects against hiding what nobody
            // asked to hide, and unfolding hides nothing. It lives on this
            // path, the trader's own, and not in `ChartPane::add_indicator`,
            // which the workspace restore and the harness hooks also travel —
            // there it would erase the fold on every launch.
            ToolbarAction::AddNative(id) => {
                self.set_focused_legend_collapsed(false);
                self.add_native_indicator(id);
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.toggle_indicator_hidden_at(target);
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.remove_indicator_at(target);
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.set_focused_legend_collapsed(false);
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let target = self.target_slot(SlotId(slot));
                self.open_indicator_settings_at(target);
            }
            // The toolbar acts on the market it is showing: the active tab's
            // simulator, whose tape the buttons' price came from.
            ToolbarAction::PaperBuy => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Buy),
            ToolbarAction::PaperSell => self
                .active_tab_mut()
                .paper
                .market(quantick_engine::Side::Sell),
            ToolbarAction::PaperClose => self.active_tab_mut().paper.close_position(),
        }
    }

    /// Hand the app the window it is drawing into.
    ///
    /// Called once from `main`, which is where eframe offers the handle: the
    /// hook that needs it (`raw_input_hook`) is given only the input. One
    /// registration line rather than a constructor argument, because every
    /// other construction path — every test — wants the `None` this defaults
    /// to, which is also what every non-Windows target gets.
    pub fn attach_surface(&mut self, handle: &impl raw_window_handle::HasWindowHandle) {
        self.surface = window_scale::SurfaceProbe::new(handle);
    }
}

/// Opens the Market Replay browser (§10).
const REPLAY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
/// Shows/hides the panels dock (§10).
/// `Ctrl+1` … `Ctrl+9` apply the layout registry's presets, in table order.
///
/// The keys are listed; which preset each one reaches is not. A preset added
/// to `LAYOUT_PRESETS` gets its shortcut from its position without this array
/// or its dispatch being edited — the same rule the picker and the View menu
/// follow. Nine is what a number row has; `MAX_CANVAS_PANES` keeps the
/// registry far below that.
const LAYOUT_PRESET_KEYS: [egui::Key; 9] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

/// The shortcut that reaches the preset at `index` in the registry, if a
/// number key still reaches that far.
fn layout_preset_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::CTRL, *key))
}

/// The shortcut that reaches the layout tab at strip position `index`, if a
/// number key still reaches that far.
fn layout_tab_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::ALT, *key))
}

/// `Ctrl+0` puts the context charts away, or brings them back.
///
/// The number row's own zero, beside `Ctrl+1..9` for the presets: nine keys
/// choose an arrangement and the tenth dismisses the column that arrangement
/// put beside the heatmap. Without it the only way to collapse was a drag,
/// which a trader working by keyboard cannot make and which WCAG 2.2's
/// dragging rule wants an alternative to besides.
const COLLAPSE_CONTEXT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Num0);

const DOCK_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);
/// Folds the focused pane's on-chart indicator legend to its count puck, or
/// opens it back up (see [`crate::indicator_legend`]).
///
/// Ctrl+letter like the dock's own switch above, not the bare `L` the drawing
/// tools answer to: bare letters are the toolbox's namespace, and a chrome
/// switch borrowing one would arm a tool on every trader who learned it there.
const LEGEND_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L);
/// Saves the workspace — the arrangement the next launch opens on.
///
/// Ctrl+Shift+S rather than the Ctrl+S every editor uses, deliberately: a
/// chart has no document, and a trader who reaches for Ctrl+S out of habit
/// mid-session should hit nothing rather than silently redefine what their
/// platform opens on.
const SAVE_WORKSPACE_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::S,
);
/// Opens the source picker for a new tab (§10).
const NEW_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T);
/// Closes the active tab (§10). Free of any other binding: the chart has no
/// text inputs and no document to "write".
const CLOSE_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W);
/// Cycles forward through the strip (§10).
const NEXT_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Tab);
/// See [`NEXT_TAB_SHORTCUT`].
const PREVIOUS_TAB_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::Tab,
);
/// Simulated buy at market (`docs/ux/paper-trading.md` §9). All the
/// trading hotkeys are Shift+letter and stand down while any text field
/// owns the keyboard — a capital letter typed into a symbol box must
/// never become an order.
const PAPER_BUY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::B);
/// Simulated sell at market.
const PAPER_SELL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::S);
/// Reverse the simulated position.
const PAPER_REVERSE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::R);
/// Flatten: close the position and cancel every working order.
const PAPER_FLATTEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F);
/// Cancel every working order without trading.
const PAPER_CANCEL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::X);
/// Height of the menu bar, in pixels (§5 zone 1).
const MENU_BAR_HEIGHT: f32 = 28.0;

impl QuantickApp {
    /// The window's menu bar (§10): shallow menus for discoverability and
    /// shortcuts, never the only path to anything.
    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&REPLAY_SHORTCUT)) {
            self.replay_view.open_browser();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&DOCK_SHORTCUT)) {
            self.dock.toggle_visible();
        }
        // Gated on the same condition the View menu's entry is gated on: only
        // a layout that carves a context column *beside* the flow pane has a
        // column to put away. Ungated, `Ctrl+0` on the Flow or Timeframe
        // layout set a flag nothing drew — and swallowed the key besides, so
        // egui's own "reset zoom" never ran.
        if self.active_tab().layout.shows_time()
            && self.active_tab().layout.shows_flow()
            && ctx.input_mut(|i| i.consume_shortcut(&COLLAPSE_CONTEXT_SHORTCUT))
        {
            let collapsed = self.active_tab().context_collapsed;
            self.active_tab_mut().set_context_collapsed(!collapsed);
        }
        // Layout by number, straight off the registry. The same
        // `apply_layout_preset` the picker and the menu call — three doors,
        // one room.
        for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
            let Some(shortcut) = layout_preset_shortcut(index) else {
                break;
            };
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.apply_layout_preset(preset);
            }
        }
        // Layout tabs by number: `Alt+1..9`, beside `Ctrl+1..9` for the
        // presets — one row of keys, two things a trader switches by number.
        // Not while a text field has the keyboard — a rename box, a note,
        // the ticket — where Alt+1 is text, not a switch.
        let typing = ctx.memory(|memory| memory.focused().is_some());
        for index in 0..LAYOUT_PRESET_KEYS.len() {
            let Some(shortcut) = layout_tab_shortcut(index) else {
                break;
            };
            if !typing
                && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
                && let Err(error) = self.switch_layout_index(index)
            {
                self.note_workspace(error.to_string());
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&LEGEND_SHORTCUT)) {
            let collapsed = self.focused_legend_collapsed();
            self.set_focused_legend_collapsed(!collapsed);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&crate::control::MARK_SHORTCUT)) {
            self.take_mark(None);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE_WORKSPACE_SHORTCUT)) {
            self.save_workspace("shortcut");
        }
        // Trading hotkeys, swallowed only while no text field owns the
        // keyboard. Market entries use the ticket's quantity and offsets,
        // exactly like the toolbar buttons they twin.
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_BUY_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Buy);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_SELL_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Sell);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_REVERSE_SHORTCUT)) {
                self.active_tab_mut().paper.reverse_position();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_FLATTEN_SHORTCUT)) {
                self.active_tab_mut().paper.flatten();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_CANCEL_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .account_mut()
                    .cancel_all_orders();
            }
        }

        let mut tab_action = None;
        egui::TopBottomPanel::top("menu_bar")
            .exact_height(MENU_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add(
                                egui::Button::new("New Tab…")
                                    .shortcut_text(ui.ctx().format_shortcut(&NEW_TAB_SHORTCUT)),
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::New);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.tabs.len() > 1,
                                egui::Button::new("Close Tab").shortcut_text(
                                    ui.ctx().format_shortcut(&CLOSE_TAB_SHORTCUT),
                                ),
                            )
                            .on_disabled_hover_text(
                                "The last tab stays open — a window with no market has nothing to show",
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::Close(self.active_tab));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new("Market Replay…")
                                    .shortcut_text(ui.ctx().format_shortcut(&REPLAY_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.replay_view.open_browser();
                            ui.close_menu();
                        }
                        if self.active_tab().replay.is_some() && ui.button("Close Replay").clicked()
                        {
                            let (tab, config) = self.active_with_config();
                            tab.close_replay(config);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        // What the canvas shows is a view concern, so the
                        // switch lives here rather than under File, and each
                        // entry names the charts it shows — "Timeframe", not
                        // layout jargon (audit §3).
                        ui.menu_button("Layouts", |ui| {
                            // The strip's tabs, from the book: switch the
                            // focused pane by name, and the three edits the
                            // strip's own menu holds.
                            let active = self.focused_pane_layout();
                            let names: Vec<(crate::layouts::LayoutId, String)> = self
                                .layouts()
                                .layouts()
                                .iter()
                                .map(|layout| (layout.id, layout.name.clone()))
                                .collect();
                            for (index, (id, name)) in names.iter().enumerate() {
                                let mut button = egui::Button::new(name.as_str());
                                if let Some(shortcut) = layout_tab_shortcut(index) {
                                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                                }
                                if ui.add(button.selected(*id == active)).clicked() {
                                    self.apply_strip_action(crate::layout_strip::StripAction::Switch(*id));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            let can_add = names.len() < crate::layouts::MAX_LAYOUTS;
                            if ui
                                .add_enabled(can_add, egui::Button::new("New layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Create);
                                ui.close_menu();
                            }
                            if ui.button("Rename layout…").clicked() {
                                self.apply_strip_action(crate::layout_strip::StripAction::BeginRename(active));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(names.len() > 1, egui::Button::new("Delete layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("Layout", |ui| {
                            // Read from the registry, like the picker: a menu
                            // holding its own list of layouts is the second
                            // opinion that goes stale the day one is added.
                            let current = self.active_tab().layout.preset();
                            for (index, preset) in
                                crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate()
                            {
                                // The menu is where a shortcut is learned, so
                                // it carries the binding beside the name.
                                let label = match layout_preset_shortcut(index) {
                                    Some(shortcut) => format!(
                                        "{}	{}",
                                        preset.label,
                                        ui.ctx().format_shortcut(&shortcut)
                                    ),
                                    None => preset.label.to_owned(),
                                };
                                if ui
                                    .selectable_label(current.id == preset.id, label)
                                    .clicked()
                                {
                                    self.apply_layout_preset(preset);
                                    ui.close_menu();
                                }
                            }
                        });
                        // Collapsing was drag-only, which a trader working
                        // by keyboard could not do at all — while the
                        // assistant had `layout.pane.collapse`. Same call,
                        // three doors.
                        if self.active_tab().layout.shows_time()
                            && self.active_tab().layout.shows_flow()
                        {
                            let collapsed = self.active_tab().context_collapsed;
                            let label = if collapsed {
                                "Show context charts"
                            } else {
                                "Hide context charts"
                            };
                            if ui
                                .add(
                                    egui::Button::new(label).shortcut_text(
                                        ui.ctx().format_shortcut(&COLLAPSE_CONTEXT_SHORTCUT),
                                    ),
                                )
                                .clicked()
                            {
                                self.active_tab_mut().set_context_collapsed(!collapsed);
                                ui.close_menu();
                            }
                        }
                        // Reposition without a drag. WCAG 2.2's dragging rule
                        // wants a single-pointer alternative to every drag, and
                        // TradingView — the reference the trader named — moves
                        // charts by a menu command rather than by dragging at
                        // all. Both go through `Tab::move_context_pane`.
                        let context_panes = self.active_tab().pane_count().saturating_sub(1);
                        if context_panes > 1 {
                            ui.menu_button("Move chart", |ui| {
                                for slot in 1..=context_panes {
                                    let up = ui
                                        .add_enabled(slot > 1, egui::Button::new(format!(
                                            "Chart {slot} up"
                                        )))
                                        .on_disabled_hover_text("already the top chart");
                                    if up.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot - 1);
                                        ui.close_menu();
                                    }
                                    let down = ui
                                        .add_enabled(
                                            slot < context_panes,
                                            egui::Button::new(format!("Chart {slot} down")),
                                        )
                                        .on_disabled_hover_text("already the bottom chart");
                                    if down.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot + 1);
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                        ui.separator();
                        let panels_label = if self.dock.visible() {
                            "Hide panels"
                        } else {
                            "Show panels"
                        };
                        if ui
                            .add(
                                egui::Button::new(panels_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&DOCK_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.dock.toggle_visible();
                            ui.close_menu();
                        }
                        // The legend belongs to a pane, so this entry names
                        // the focused one's state — the same pane the chevron
                        // on screen would fold.
                        let collapsed = self.focused_legend_collapsed();
                        // Split open: say *which* chart, the way the layout
                        // entries above name the charts they show. The action
                        // follows the focus like every other chrome control,
                        // and a trader reading "Collapse indicator legend"
                        // over two charts has no way to know which corner is
                        // about to change.
                        let split = self.active_tab().shows_context_charts();
                        let pane_name = self.active_tab().focused_side().title();
                        let legend_label = match (collapsed, split) {
                            (true, false) => "Show indicator legend".to_owned(),
                            (false, false) => "Collapse indicator legend".to_owned(),
                            (true, true) => format!("Show indicator legend ({pane_name})"),
                            (false, true) => format!("Collapse indicator legend ({pane_name})"),
                        };
                        // A pane with no indicators has no legend to fold, and
                        // an entry that is enabled and does nothing reads as a
                        // broken feature rather than as an empty chart.
                        let has_legend = {
                            let tab = self.active_tab();
                            !tab.pane(tab.focused_side()).indicators.all().is_empty()
                        };
                        if ui
                            .add_enabled(
                                has_legend,
                                egui::Button::new(legend_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&LEGEND_SHORTCUT)),
                            )
                            .on_hover_text(
                                "Folds the healthy rows to a count on the focused chart. Errored and stale indicators stay on it.",
                            )
                            .on_disabled_hover_text(
                                "This chart has no indicators, so there is no legend to fold",
                            )
                            .clicked()
                        {
                            self.set_focused_legend_collapsed(!collapsed);
                            ui.close_menu();
                        }
                        ui.menu_button("Drawing toolbar", |ui| {
                            for (dock, label) in [
                                (ToolboxDock::Left, "Left"),
                                (ToolboxDock::Top, "Top"),
                                (ToolboxDock::Bottom, "Bottom"),
                            ] {
                                if ui
                                    .selectable_label(self.toolrail.dock() == dock, label)
                                    .clicked()
                                {
                                    self.toolrail.set_dock(dock);
                                    ui.close_menu();
                                }
                            }
                        });
                        let toolbox_label = if self.toolrail.visible() {
                            "Hide drawing toolbar"
                        } else {
                            "Show drawing toolbar"
                        };
                        if ui.button(toolbox_label).clicked() {
                            self.toolrail.toggle_visible();
                            ui.close_menu();
                        }
                        for (tab, label) in [
                            (DockTab::L2, "L2 settings"),
                            (DockTab::Bubbles, "Bubble settings"),
                            (DockTab::Session, "Session"),
                            (DockTab::Trading, "Paper trading"),
                            (DockTab::Trades, "Trades"),
                        ] {
                            if ui.button(label).clicked() {
                                self.dock.open_tab(tab);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_perf, "Perf readings")
                            .on_hover_text("fps, frame time and trade count on the status bar");
                        ui.checkbox(&mut self.progressive_history, "Progressive venue history")
                            .on_hover_text(
                                "Build the venue's candle history from now backwards, a week at \
                                 a time, so the chart fills in while the rest arrives. Off asks \
                                 for the whole span in one request: fewer calls, nothing on \
                                 screen until all of it lands.",
                            );
                        ui.checkbox(&mut self.venue_lead_in, "Venue candles on charts cut by trades")
                            .on_hover_text(
                                "A tick, volume, dollar or imbalance chart cannot fold venue \
                                 candles into its own bars, so it opens holding only the prints \
                                 this session saw. Switch this on to put the venue's 1-minute \
                                 candles in front of them anyway — counted apart from built bars \
                                 on the status bar — so yesterday is on screen to compare \
                                 against. They stay candles: a minute never becomes a tick bar, \
                                 and an indicator running across the seam is averaging both \
                                 kinds.",
                            );
                        ui.separator();
                        ui.menu_button("Timezone", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for tz in TzOffset::ALL {
                                        if ui.selectable_label(self.tz == tz, tz.label()).clicked()
                                        {
                                            self.tz = tz;
                                            ui.close_menu();
                                        }
                                    }
                                });
                        });
                    });
                    // The workspace is its own menu, not a File entry: "what
                    // does quantick open on" is a question a trader asks about
                    // their cockpit, not about a document, and burying it
                    // under File is how a platform ends up with traders who
                    // rebuild their screen every morning without knowing they
                    // never had to (audit §6).
                    // The button's own rect, published for the capture hook —
                    // read, never acted on, so the menu behaves identically
                    // whether or not a scripted run is watching.
                    let workspace_menu = ui.menu_button("Workspace", |ui| {
                        if ui
                            .add(
                                egui::Button::new("Save workspace").shortcut_text(
                                    ui.ctx().format_shortcut(&SAVE_WORKSPACE_SHORTCUT),
                                ),
                            )
                            .on_hover_text(
                                "Remember this arrangement — the tabs, the charts on each, the \
                                 panels, the timezone and the window — as what quantick opens on",
                            )
                            .clicked()
                        {
                            self.save_workspace("menu");
                            ui.close_menu();
                        }
                        // Enabled only when there is something on disk to go
                        // back to: an entry that would forget nothing is a
                        // question the trader should not have to answer by
                        // clicking it.
                        if ui
                            .add_enabled(
                                self.workspace.session().saved(),
                                egui::Button::new("Reset startup layout"),
                            )
                            .on_hover_text(
                                "Forget the saved workspace; the next launch opens on the \
                                 configured default. The charts on screen are left alone.",
                            )
                            .on_disabled_hover_text(
                                "Nothing saved yet — quantick already opens on the configured \
                                 default",
                            )
                            .clicked()
                        {
                            self.forget_workspace();
                            ui.close_menu();
                        }
                        ui.separator();
                        // Bookmarks. Named apart from the two entries above on
                        // purpose: those govern what the app *opens on*, these
                        // are places to come back to. The wording carries the
                        // distinction so the menu does not need a paragraph.
                        if ui
                            .button("Save as…")
                            // Says what a bookmark keeps *and what it does
                            // not*: it is the tabs and the panels, not the
                            // indicators or the colours. Two entries in one
                            // menu that both "save a workspace" but restore
                            // different amounts is exactly how a trader comes
                            // to believe the app forgets things — use Export
                            // to file for the whole cockpit.
                            .on_hover_text(
                                "Keep these tabs and panels under a name you can reopen later. \
                                 It does not change what quantick opens on, and it does not \
                                 keep indicators or colours — use Export to file for those.",
                            )
                            .clicked()
                        {
                            self.surfaces.workspace_name.open();
                            ui.close_menu();
                        }
                        let mut open: Option<String> = None;
                        let mut delete: Option<String> = None;
                        ui.add_enabled_ui(!self.workspace.session().bookmarks().is_empty(), |ui| {
                            ui.menu_button("Open", |ui| {
                                for entry in self.workspace.session().bookmarks() {
                                    let tabs = entry.tabs.len();
                                    if ui
                                        .button(&entry.name)
                                        .on_hover_text(format!(
                                            "{tabs} chart {} — replaces what is on screen",
                                            if tabs == 1 { "tab" } else { "tabs" }
                                        ))
                                        .clicked()
                                    {
                                        open = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("Nothing saved under a name yet");
                            ui.menu_button("Delete", |ui| {
                                for entry in self.workspace.session().bookmarks() {
                                    if ui.button(&entry.name).clicked() {
                                        delete = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        if let Some(name) = open {
                            self.open_named_workspace(&name);
                        }
                        if let Some(name) = delete {
                            self.delete_named_workspace(&name);
                        }
                        ui.separator();
                        // Files, named apart from the two groups above again:
                        // those live inside quantick, these are documents the
                        // trader owns, can copy, back up and carry to another
                        // machine. That is the difference the wording carries.
                        if ui
                            .button("Export to file…")
                            .on_hover_text(
                                "Save the whole cockpit — tabs, indicators, layers, drawing \
                                 colours, footprint and added symbols — as one file in your \
                                 documents",
                            )
                            .clicked()
                        {
                            self.open_workspace_export_picker();
                            ui.close_menu();
                        }
                        if ui
                            .button("Open from file…")
                            .on_hover_text(
                                "Open a workspace file. It replaces the cockpit on screen; a \
                                 file that cannot be read changes nothing.",
                            )
                            .clicked()
                        {
                            self.open_workspace_import_picker();
                            ui.close_menu();
                        }
                        // Read off the field, not the filesystem: this body
                        // runs every frame the menu is open.
                        let mut reopen: Option<std::path::PathBuf> = None;
                        ui.add_enabled_ui(!self.workspace.session().recent_on_disk().is_empty(), |ui| {
                            ui.menu_button("Open recent", |ui| {
                                for path in self.workspace.session().recent_on_disk() {
                                    if ui
                                        .button(crate::workspace_bundle::recent_label(path))
                                        // The same warning the bookmark list
                                        // carries: this replaces the cockpit,
                                        // and a trader mid-tape has to read
                                        // that before the click, not after.
                                        .on_hover_text(format!(
                                            "Replaces the cockpit on screen\n{}",
                                            path.display()
                                        ))
                                        .clicked()
                                    {
                                        reopen = Some(path.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("No workspace files opened yet");
                        });
                        if let Some(path) = reopen {
                            self.import_workspace_from(&path);
                        }
                        if ui
                            .button("Show where it's saved")
                            .on_hover_text(
                                "Open the folder quantick keeps your cockpit in, so you can see \
                                 it and back it up",
                            )
                            .clicked()
                        {
                            self.reveal_cockpit_home();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .checkbox(self.workspace.session_mut().save_on_exit_mut(), "Save on exit")
                            .on_hover_text(
                                "Keep the arrangement automatically when the window closes. Off, \
                                 only Save workspace changes what quantick opens on.",
                            )
                            .changed()
                        {
                            // The setting lives in the file it governs, so
                            // switching it has to reach the disk now — not at
                            // the next exit, which is exactly the exit it may
                            // have just switched off.
                            self.save_workspace("save_on_exit_toggled");
                        }
                    });
                    self.workspace_menu_rect = Some(workspace_menu.response.rect);
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Appearance…").clicked() {
                            self.surfaces.style_panel.open();
                            ui.close_menu();
                        }
                        let access_label = self
                            .control_access
                            .as_ref()
                            .map_or("Local agent access…", |access| access.menu_label());
                        if ui.button(access_label).clicked() {
                            if let Some(access) = self.control_access.as_mut() {
                                access.open_panel();
                            }
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Replay file format…").clicked() {
                            self.replay_view.open_format_help();
                            ui.close_menu();
                        }
                    });
                    if self
                        .control_access
                        .as_ref()
                        .is_some_and(crate::control::ControlAccess::is_enabled)
                        && ui.button("Agent access: on").clicked()
                        && let Some(access) = self.control_access.as_mut()
                    {
                        access.open_panel();
                    }
                    ui.separator();
                    // The tab strip shares the menu row: zone 1 already had
                    // the horizontal room, so tabs cost no chrome budget.
                    tab_action = self.draw_tab_strip(ui);
                });
            });
        if let Some(action) = tab_action {
            self.apply_tab_action(action);
        }
    }

    /// The chips, built from what each tab actually is right now.
    fn draw_tab_strip(&self, ui: &mut egui::Ui) -> Option<TabAction> {
        let chips: Vec<tabstrip::TabChip<'_>> = self
            .tabs
            .iter()
            .map(|tab| tabstrip::TabChip {
                label: tab.chip_label(),
                replaying: tab.replay.is_some(),
                needs_attention: tab.needs_attention(),
            })
            .collect();
        tabstrip::draw(ui, &chips, self.active_tab)
    }

    /// One delete command for every trigger (inspector button, keyboard,
    /// manager). A locked object raises the confirmation next to the trigger
    /// instead of deleting; a landed delete raises the Undo toast.
    fn request_delete_selected(&mut self, now: Instant) {
        // Read the name before the object is gone. "Drawing deleted" makes
        // the undo useless on a crowded chart: the trader has to know *what*
        // they lost to know whether they want it back — and the context bar
        // deletes on a bare glyph, so the toast is what pays for that.
        let doomed = self.drawing_pane().drawings.selected().and_then(|index| {
            let drawing = self.drawing_pane().drawings.items().get(index)?;
            // The trader's own name when one was given; the tool name
            // otherwise — a positional index would be noise on an object
            // that no longer has a position.
            let label = drawing
                .name
                .clone()
                .unwrap_or_else(|| drawing.tool.name().to_owned());
            Some((drawing.id, label))
        });
        match self.drawing_pane_mut().drawings.delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.surfaces.drawing_chrome.set_delete_confirm(false);
                // The instance dies with its drawing, immediately — not on
                // the next closed bar, which a quiet tape may never bring.
                if let Some((id, _)) = &doomed {
                    self.drawing_pane_mut().remove_strategy_for_drawing(*id);
                }
                let name = doomed.map(|(_, label)| label);
                let message = name.map_or_else(
                    || "Drawing deleted.".to_owned(),
                    |name| format!("{name} deleted."),
                );
                self.surfaces.toast.note_with_undo(message, now);
            }
            DeleteOutcome::NeedsConfirmation => {
                self.surfaces.drawing_chrome.set_delete_confirm(true);
            }
            DeleteOutcome::NothingSelected => {}
        }
    }

    /// Record one edit gesture as the single undo entry it earned, on the pane
    /// it started on.
    ///
    /// That pane's tab may have been closed under the gesture, in which case
    /// the object it described is gone with it.
    fn record_drawing_edit(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        index: usize,
        before: drawings::Drawing,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.pane_mut(side).drawings.record_edit_of(index, before);
        }
    }

    /// Put the caret in a note, on the chart — the one call that opens the
    /// editor, whether a placement, a double click or a script asked for it.
    ///
    /// The surface decides whether the caret is allowed: an object that holds
    /// no words, or a locked one, refuses it, because an editor that opened and
    /// then dropped every keystroke would be worse than none. The store command
    /// and the per-pane stand-down are the host's, so they happen here.
    pub fn begin_inline_text_edit(&mut self, index: usize) -> bool {
        let Self {
            tabs,
            active_tab,
            surfaces,
            ..
        } = self;
        let tab = &tabs[*active_tab];
        let side = tab.drawing_side();
        let Some(drawing) = tab.pane(side).drawings.items().get(index) else {
            return false;
        };
        if !surfaces
            .drawing_chrome
            .begin_inline_text_edit(tab.id, side, index, drawing)
        {
            return false;
        }
        self.drawing_pane_mut().drawings.select(Some(index));
        self.sync_content_editing();
        true
    }

    /// Close the editor, keeping whatever was typed and recording it as the one
    /// edit it was — on the pane the note actually lives on, which is not
    /// necessarily the one in front when it closes.
    fn end_inline_text_edit(&mut self) {
        if let Some(edit) = self.surfaces.drawing_chrome.end_inline_text_edit() {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        self.sync_content_editing();
    }

    /// Tell every pane whether one of its objects is having its content typed
    /// somewhere else on screen, so exactly one object anywhere stands down.
    ///
    /// Every pane, not just the one in front: the flag is what suppresses the
    /// object's own painting, and a pane left holding a stale index would keep
    /// a note invisible for the rest of the session with no way back.
    fn sync_content_editing(&mut self) {
        let editing = self.surfaces.drawing_chrome.content_editing_target();
        for tab in &mut self.tabs {
            let target = editing
                .filter(|(id, _, _)| *id == tab.id)
                .map(|(_, side, index)| (side, index));
            tab.set_content_editing(target);
        }
    }

    /// Which note is being typed on the chart right now — what a second
    /// operator reads to know the keyboard belongs to an object.
    #[must_use]
    pub fn inline_text_editing(&self) -> Option<usize> {
        self.surfaces.drawing_chrome.inline_text_editing()
    }

    /// The rows the object manager lists.
    ///
    /// A row's facts come from the drawing, the pane's band registry and the
    /// tab's layout, and assembling them here is what keeps the manager from
    /// needing all three. Built only while the window is open, like the market
    /// dialog's list of open markets: a dozen short strings once a frame, on a
    /// window that is shut the rest of the session.
    fn drawing_manager_rows(&self) -> Vec<crate::surfaces::drawing_chrome::ManagerRow> {
        let pane = self.drawing_pane();
        let selected = pane.drawings.selected();
        let focused = self.focused_pane();
        pane.drawings
            .items()
            .iter()
            .enumerate()
            .map(
                |(index, drawing)| crate::surfaces::drawing_chrome::ManagerRow {
                    name: drawing.display_label(index),
                    selected: selected == Some(index),
                    locked: drawing.locked,
                    hidden: drawing.hidden,
                    shared: drawing.scope == drawings::DrawingScope::AllCharts,
                    off_series: drawing.off_series,
                    foreign_market: drawing.foreign_market,
                    author: drawing.author.as_ref().map(DrawingAuthor::label),
                    band: focused.band_label(drawing),
                },
            )
            .collect()
    }

    /// Where the selected object is painted, in screen points. The chrome
    /// cannot work this out for itself: it needs the viewport and the price
    /// scale the host owns.
    ///
    /// Two separate answers rather than one pair, because they cost different
    /// things and not every pass wants both — this one walks the object's
    /// anchors through the price scale. Nothing is projected while nothing is
    /// selected, which is every frame of an ordinary session.
    fn selected_drawing_bbox(&self) -> Option<egui::Rect> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        let chart = pane.last_chart_area?;
        self.drawing_bbox_on_screen(chart, index)
    }

    /// What the band the selected object lives on is called, for the
    /// inspector's title. `None` on the price band, where a suffix on every
    /// object would be noise. Formats a string, so it is asked for only by a
    /// pass that shows the title.
    fn selected_drawing_band(&self) -> Option<String> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        self.focused_pane()
            .band_label(pane.drawings.items().get(index)?)
            .chip()
    }

    /// The docked inspector.
    ///
    /// Its own call site because a `SidePanel` has to be declared *before* the
    /// central canvas — the canvas pays its width, and a panel declared after
    /// it would overlay the chart instead of docking beside it.
    fn draw_pinned_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.surfaces.drawing_chrome.inspector_pinned() {
            return;
        }
        // No painted bounds: a docked panel has no placement rule to keep
        // clear of the object, so the projection the floating one needs is not
        // gathered here. The band still is — it is in the title.
        let read = DrawingRead {
            selected_band: self.selected_drawing_band(),
            ..DrawingRead::default()
        };
        let ask = self.draw_chrome_pass(ctx, read, false);
        self.apply_drawing_chrome(ask, now);
    }

    /// The four floating pieces, registered after the canvas so they stay in
    /// front of the chart they are anchored to.
    fn draw_drawing_chrome(&mut self, ctx: &egui::Context, now: Instant) {
        let manager_open = self.surfaces.drawing_chrome.manager_open();
        let rows = if manager_open {
            self.drawing_manager_rows()
        } else {
            Vec::new()
        };
        // The band name goes in the inspector's title and nowhere else, so
        // it is formatted only when one of the two inspector hosts is on
        // screen. A selection alone raises the context bar, which never shows
        // it — and `band_label` scans the pane's indicator views and `chip`
        // allocates, every frame, for a value nothing would read.
        let inspector_showing = self.surfaces.drawing_chrome.inspector_open()
            || self.surfaces.drawing_chrome.inspector_pinned();
        let read = DrawingRead {
            selected_bbox: self.selected_drawing_bbox(),
            selected_band: inspector_showing
                .then(|| self.selected_drawing_band())
                .flatten(),
            // Counted only for the window that offers to take them back, and
            // over every tab: an object an assistant placed on another chart
            // still belongs in that count.
            authored_objects: if manager_open {
                Self::authored_object_count(&self.tabs)
            } else {
                0
            },
            manager_rows: &rows,
        };
        let ask = self.draw_chrome_pass(ctx, read, true);
        self.apply_drawing_chrome(ask, now);
    }

    /// One split for both call sites. `floating` picks which of the surface's
    /// two entry points runs.
    fn draw_chrome_pass(
        &mut self,
        ctx: &egui::Context,
        read: DrawingRead<'_>,
        floating: bool,
    ) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
        // Split into disjoint borrows, like the surface registry above: the
        // chrome is drawn through `&mut` while what it reads is borrowed from
        // the rest of the application.
        let Self {
            surfaces,
            tabs,
            active_tab,
            toolrail,
            drawing_presets,
            ..
        } = self;
        let env = drawing_env(&tabs[*active_tab], toolrail, drawing_presets, read);
        if floating {
            surfaces.drawing_chrome.draw_floating(ctx, &env)
        } else {
            surfaces.drawing_chrome.draw_pinned_panel(ctx, &env)
        }
    }

    /// The `QUANTICK_TEXT_NOTE` hook's other half: place a note in the middle
    /// of the window and open its editor, through the same two calls a click
    /// makes.
    ///
    /// Here rather than in the surface because every line of it is the host's:
    /// where the visible window is, what the tape last closed at, and the saved
    /// defaults a fresh object opens with.
    fn place_text_note(&mut self) -> bool {
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
        else {
            return false;
        };
        let point = {
            let pane = self.drawing_pane();
            let slots = pane.slots();
            if pane.last_chart_area.is_none() || slots == 0 {
                // No laid-out pane yet, and nothing to place against. The ask
                // stands and the next frame tries again.
                return false;
            }
            let close = pane
                .closed_bar(slots.saturating_sub(1))
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            let centre = pane
                .last_auto_range
                .filter(|(lo, hi)| hi > lo)
                .map_or(close, |(lo, hi)| (lo + hi) / 2.0);
            let visible = DEMO_VISIBLE_SLOTS.min(slots);
            let slot = (slots - visible / 2).min(slots.saturating_sub(1));
            drawings::ChartPoint::at_time(slot as f32 + 0.5, centre, pane.slot_open_time(slot))
        };
        // Through the same door the click path uses, saved defaults and all —
        // and on the same pane every drawing surface reads, so the index the
        // editor opens on is the object this just placed.
        let fresh = drawings::new_drawing_from_defaults(&self.drawing_presets, tool);
        let placed = self.drawing_pane_mut().drawings.place_with(
            tool,
            &drawings::DrawingBand::Price,
            point,
            |_| fresh,
        );
        if placed && let Some(index) = self.drawing_pane().drawings.selected() {
            self.begin_inline_text_edit(index);
        }
        placed
    }

    /// The selected object's screen bounding box, expanded by the anchor
    /// radius — the rectangle the inspector must not cover. Projected on the
    /// focused pane, which is where the selection lives.
    fn drawing_bbox_on_screen(&self, chart: egui::Rect, index: usize) -> Option<egui::Rect> {
        let total = self.drawing_pane().slots();
        let auto = self.drawing_pane().last_auto_range?;
        let scale = self.drawing_pane().price_view.scale(
            auto,
            self.drawing_pane().last_chart_top,
            self.drawing_pane().last_chart_top + self.drawing_pane().last_chart_height,
        );
        let history_right = self
            .drawing_pane()
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let drawing = self.drawing_pane().drawings.items().get(index)?;
        let points =
            self.drawing_pane()
                .projected_drawing_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        // What the tool paints, which is not always where its anchors are: a
        // fixed-range profile anchors at one price and covers the axis. Every
        // popup that keeps clear of an object reads this rectangle, so asking
        // the anchors alone is what let a panel land in the middle of a
        // profile while believing it had walked around it.
        let bbox = drawing.tool.painted_bounds(bbox, chart);
        Some(bbox.expand(DRAWING_ANCHOR_RADIUS_PX))
    }

    /// Carry out what the replay interface asked for.
    /// Whether the action reached its destination. Only a transport control
    /// can fail to — see the drop below — and the one caller that gets a
    /// single shot at it (the scripted seek) reads this before spending it.
    fn apply_replay_action(&mut self, action: ReplayAction) -> bool {
        match action {
            ReplayAction::Open(request) => {
                let (tab, config) = self.active_with_config();
                tab.open_replay(config, *request);
                true
            }
            ReplayAction::Close => {
                let (tab, config) = self.active_with_config();
                tab.close_replay(config);
                true
            }
            ReplayAction::Control(control) => {
                // A dropped transport click is not worth a retry queue: the
                // worker drains commands every 8 ms, so a full channel means
                // the click was already superseded.
                if let Err(e) = self
                    .active_tab()
                    .commands
                    .try_send(FeedCommand::Replay(control))
                {
                    tracing::debug!(
                        target: "quantick::app",
                        event_code = "REPLAY_COMMAND_DROPPED",
                        reason = %e,
                        "transport command not queued"
                    );
                    return false;
                }
                true
            }
        }
    }
}

impl QuantickApp {
    /// Where a scripted right-click should land to reach `pane`'s menu.
    ///
    /// Mid-height, and mid-pane horizontally, off the geometry the draw
    /// published — so the click lands on the canvas rather than on the axis,
    /// the legend or the divider handle. `None` until the pane has drawn once
    /// (no divider yet), and `None` for the tape on a canvas that has none:
    /// there is no tape menu to open where there is no tape.
    fn scripted_context_menu_pos(&self, pane: ContextMenuPane) -> Option<egui::Pos2> {
        let flow = &self.active_tab().flow_pane;
        // The axis's menu lives on the gutter, off the canvas entirely — the
        // draw publishes that band the same way it publishes the divider.
        if pane == ContextMenuPane::Axis {
            return Some(flow.last_price_gutter?.center());
        }
        // The time axis, likewise off the canvas — and its own published band,
        // because the segment past the lane divider is the tape's.
        if pane == ContextMenuPane::Time {
            return Some(flow.last_time_strip?.center());
        }
        let rect = flow.last_chart_rect?;
        let divider = flow.last_lane_divider_x;
        let x = match (pane, divider) {
            (ContextMenuPane::Tape, Some(divider)) => (divider + rect.right()) / 2.0,
            (ContextMenuPane::Tape, None) => return None,
            // Axis and Time returned above; anything else is the candles'
            // canvas.
            (_, Some(divider)) => (rect.left() + divider) / 2.0,
            (_, None) => rect.center().x,
        };
        Some(egui::pos2(x, rect.center().y))
    }

    /// Where `QUANTICK_POINTER` puts the mouse this frame, in window points.
    ///
    /// Resolved against the *drawing* area rather than the whole chart, so a
    /// fraction means a place among the candles whatever share of the canvas
    /// the live lane has taken, and against the flow pane for the same reason
    /// [`Self::scripted_context_menu_pos`] does — one canvas per capture.
    /// `None` until the pane has drawn once: there is no candle area to be a
    /// fraction of before then, and guessing one would park the pointer
    /// somewhere the author did not ask for.
    fn scripted_pointer_pos(&self) -> Option<egui::Pos2> {
        let fraction = self.harness.pointer()?;
        let flow = &self.active_tab().flow_pane;
        let candles = flow.drawing_area(flow.last_chart_rect?);
        Some(egui::pos2(
            candles.left() + fraction.x * candles.width(),
            candles.top() + fraction.y * candles.height(),
        ))
    }

    /// Deliver the parked pointer, every frame it is parked.
    fn push_scripted_pointer(&self, raw_input: &mut egui::RawInput) {
        if let Some(position) = self.scripted_pointer_pos() {
            raw_input.events.push(egui::Event::PointerMoved(position));
        }
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu_frames.record(cpu * 1000.0);
        }
        self.draw_frame(ctx, Instant::now());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Whatever the debounce was still holding: a level drawn a moment
        // before closing is a level the trader expects back.
        self.flush_layouts();
        if let Some(access) = self.control_access.as_mut() {
            access.shutdown_for_exit();
        }
    }

    /// The one place a scripted run can put a pointer event into the app.
    ///
    /// A context menu opens on a real right-click and on nothing else — there
    /// is no "open the menu" call to reach for, and reaching into egui's menu
    /// state to fake one would be a second activation path that drifts from
    /// the first. So the hook supplies the click itself, on the pane it names,
    /// and every line after that is the code a trader's own click runs.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Second frame: the button comes up where it went down, and the menu
        // that opened on the press stays open.
        if let Some(position) = self.harness.take_context_menu_release() {
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        // The menu bar's own button, clicked. A menu is a popup egui owns, so
        // there is no state to set that would not be a second way of opening
        // it; the hook supplies the press, and every line after it is what a
        // trader's click runs. The rect is published by the draw, so the
        // first frame has none — wait for it rather than guess.
        if let Some(position) = self.harness.take_menu_release() {
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        if let Some(menu) = self.harness.menu()
            && let Some(position) = match menu {
                ScriptedMenu::Workspace => self.workspace_menu_rect,
                ScriptedMenu::History => self.history_menu_rect,
            }
            .map(|rect| rect.center())
        {
            self.harness.menu_pressed(position);
            raw_input.events.push(egui::Event::PointerMoved(position));
            raw_input.events.push(egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            });
            return;
        }
        // The parked pointer, re-delivered — before the menu branch and not
        // inside its `else`, because a menu whose position never resolves
        // returns early for ever. `QUANTICK_CONTEXT_MENU=tape` on a canvas
        // with no lane is exactly that, and it used to take the pointer hook
        // down with it: the capture showed no compass, no crosshair and no
        // hover readout at all, and read as "the compass does not draw"
        // rather than "the menu never opened".
        self.push_scripted_pointer(raw_input);
        let Some(pane) = self.harness.context_menu() else {
            return;
        };
        // The divider is published by the draw, so the first frame has none:
        // wait for it rather than guess where the tape is.
        let Some(position) = self.scripted_context_menu_pos(pane) else {
            return;
        };
        self.harness.context_menu_pressed(position);
        raw_input.events.push(egui::Event::PointerMoved(position));
        raw_input.events.push(egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: egui::Modifiers::default(),
        });
    }
}

impl QuantickApp {
    /// The scripted view hooks (`QUANTICK_CANDLE_WIDTH`, `QUANTICK_PAN_PX`),
    /// re-applied every frame.
    ///
    /// Every frame rather than once at boot, for two reasons. A pan needs bars
    /// to move over and at boot there are none — repeating it is what makes
    /// `QUANTICK_PAN_PX=-9000` mean "as far left as it goes" whatever the
    /// zoom: each frame pushes, the per-frame clamp holds, and the view settles
    /// on the projection margin.
    ///
    /// And the view is *rebuilt* under both hooks by anything that re-cuts the
    /// series: `ChartPane::reset_series` hands back a fresh `Viewport`, which a
    /// replay autostart does before its first frame. A zoom set at boot was
    /// therefore thrown away, and every scripted capture of a recorded session
    /// photographed the default zoom rather than the one it asked for.
    ///
    /// A run with neither variable set does nothing here.
    fn apply_scripted_view(&mut self) {
        let (width, pan) = self.harness.scripted_view();
        if width.is_none() && pan.is_none() {
            return;
        }
        let pane = &mut self.active_tab_mut().flow_pane;
        if let Some(px) = width {
            pane.viewport.set_px_per_bar(px);
        }
        let slots = pane.slots();
        if let Some(dx) = pan
            && slots > 0
        {
            pane.viewport.pan_pixels(dx, slots);
        }
    }

    /// The `QUANTICK_LOAD_OLDER` hook: press "+ older" this many times, once
    /// the chart has something to page back from.
    ///
    /// Goes through [`Tab::request_older_history`] — the very function the
    /// toolbar button calls — rather than reaching for the feed command itself,
    /// so a run under this hook exercises the trader's path including its
    /// loading indicator, and cannot drift from it.
    ///
    /// One page per frame at most: the pages are answered asynchronously and
    /// the feed serves one request at a time, so firing them together would
    /// have every page after the first refused and answered empty — a capture
    /// of the drop path rather than of the feature.
    fn apply_load_older(&mut self) {
        let Some(pages) = self.harness.load_older_pages() else {
            return;
        };
        if self.active_tab().flow_pane.slots() == 0 {
            // Nothing charted yet. Wait, but not forever.
            if self.harness.spend_load_older_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_AUTOSTART_GAVE_UP",
                    pages,
                    frames_waited = crate::harness::LOAD_OLDER_HOOK_FRAMES,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER found no bars to page back from"
                );
            }
            return;
        }
        if self.active_tab().loading.is_active(LoadingTask::History) {
            // The previous page is still coming. Asking now would be refused
            // and answered empty, which is not what the hook is for.
            return;
        }
        let (tab, config) = self.active_with_config();
        tab.request_older_history(config);
        self.harness.load_older_page_sent();
    }

    /// The `QUANTICK_HISTORY_NOTE` hook: the sentence a settled reach leaves,
    /// held up over a chart for as long as the hook's budget lasts.
    ///
    /// Re-applied every frame rather than raised once, the way
    /// `QUANTICK_PAN_PX` re-applies its drag — and for a reason a one-shot
    /// could not survive. Switching source clears the note along with the run
    /// that raised it, exactly as it should: a new market has nothing to say
    /// about the last one's press. But a launch under `QUANTICK_REPLAY_AUTOSTART`
    /// *is* a source switch, arriving a second after the first bars, so a note
    /// raised once was swept away before any shutter could open on it.
    ///
    /// Holding it re-raises only while it is absent, so the surface itself is
    /// unchanged — the same sentence, from the same call, in the same lane.
    /// When the budget runs out the note keeps its ordinary
    /// [`crate::tab::HISTORY_NOTE_LINGER`] from the last raise and then leaves
    /// on its own, so even a hooked run photographs a note that expires.
    fn apply_history_note_hook(&mut self) {
        let Some(end) = self.harness.history_note_ending() else {
            return;
        };
        if self.harness.spend_history_note_frame().gave_up {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_NOTE_HOOK_RELEASED",
                ending = end.action(),
                frames_held = crate::harness::HISTORY_NOTE_HOOK_FRAMES,
                action = "note_left_to_expire",
                "QUANTICK_HISTORY_NOTE let go of its sentence"
            );
            return;
        }
        let tab = self.active_tab();
        // Nothing charted yet, or the note is already up: nothing to raise.
        //
        // And never while a request is out. Paired with `QUANTICK_LOAD_OLDER`
        // — which the harness table pairs it with — the press clears the note
        // and sends a real `load_older`, and re-raising here would paint a
        // settled verdict over a request still in flight, with the spinner
        // turning above it. That is the dishonesty this branch removes, and a
        // hook has no business manufacturing it for a capture.
        if tab.flow_pane.slots() == 0
            || tab.history_note().is_some()
            || tab.loading.is_active(LoadingTask::History)
        {
            return;
        }
        // Always `Some`: the hook only ever holds an ending the env read above
        // kept, and it keeps only endings that have words.
        let Some(notice) = end.notice() else {
            return;
        };
        self.active_tab_mut().raise_history_note(notice);
    }

    /// The `QUANTICK_LOAD_OLDER_CANDLES` hook: the history menu's "+ older
    /// candles" entry, pressed without a hand, once per frame at most.
    ///
    /// Same shape and same reasons as [`Self::apply_load_older`], against a
    /// different record: it goes through `Tab::request_older_ohlcv_history`
    /// rather than the feed command, so a run under this hook exercises the
    /// trader's own path; it waits, because there is nothing to reach back
    /// *from* until the opening request has landed; and it gives up rather
    /// than hanging a capture on a venue that never answers.
    fn apply_load_older_candles(&mut self) {
        let Some(spans) = self.harness.load_older_candle_spans() else {
            return;
        };
        let capabilities = self.active_tab().capabilities(&self.config);
        // Waiting costs budget, but a *slower* budget. A span really being
        // fetched is the feature working, and charging it at the same rate as
        // an empty chart would give up around the fourth of the documented
        // thirteen spans. Charging it nothing, though, is how a venue that
        // simply never answers hangs a capture run for the life of the
        // process — which is the exact failure this counter exists to bound,
        // and what the doc above promises it does. So a fetching frame spends
        // one tick of a budget scaled to how long fetching legitimately takes.
        if self
            .active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory)
        {
            if self.harness.spend_load_older_candles_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    reason = "venue_never_answered",
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES gave up waiting for a span to arrive"
                );
            }
            return;
        }
        if !self.active_tab().can_load_older_candles(capabilities) {
            // Nothing to reach back *from* yet, or the venue's record starts
            // here. Both are worth waiting a bounded while for, and both end
            // the same way; the log names what the tab held so an operator can
            // tell them apart.
            if self.harness.spend_load_older_candles_frame().gave_up {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                    spans,
                    frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                    candles_held = self.active_tab().venue_candles_held(),
                    ohlcv_history = capabilities.ohlcv_history,
                    action = "chart_left_as_it_is",
                    "QUANTICK_LOAD_OLDER_CANDLES found nothing to reach back from"
                );
            }
            return;
        }
        // Only a request that actually went out costs a *span*. A full command
        // channel is a busy frame, not a span delivered, and counting it as one
        // would quietly shorten the reach the operator asked for — but it still
        // costs a frame of budget, or a permanently saturated channel leaves
        // the hook armed for the life of the process with nothing ever logged.
        if self
            .active_tab_mut()
            .request_older_ohlcv_history(capabilities)
        {
            self.harness.load_older_candles_span_sent();
        } else if self.harness.spend_load_older_candles_frame().gave_up {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LOAD_OLDER_CANDLES_AUTOSTART_GAVE_UP",
                spans,
                frames_waited = crate::harness::LOAD_OLDER_CANDLES_HOOK_FRAMES,
                reason = "request_never_queued",
                action = "chart_left_as_it_is",
                "QUANTICK_LOAD_OLDER_CANDLES could not get a request out"
            );
        }
    }

    /// Play the alarm cues every tab's armed instances asked for this
    /// frame, and empty their queues.
    ///
    /// Every tab, not only the active one: a tab the trader is not looking
    /// at keeps its feed running and its instances judging, and an alarm
    /// exists precisely to be heard when the eyes are elsewhere.
    ///
    /// One cue per *distinct* cue per frame, across every tab. The kernel's
    /// repeat rule has already thinned each instance's stream to one per
    /// bar (or one per cooldown); this is the second, blunter guard, for
    /// the frame that ingested a burst of prints and closed several bars at
    /// once — four identical beeps stacked into one instant are one noise,
    /// not four alarms.
    ///
    /// Deduplicating by *cue* rather than collapsing to one is the whole
    /// point of letting a preset choose a sound: a trader who gave two
    /// regions two sounds did it to tell them apart, and swallowing the
    /// second because it shared a frame with the first would hide a signal
    /// and leave no trace that it had. The frame's cues go to the sink as
    /// one batch, which plays them in order — so the second is heard after
    /// the first rather than instead of it. The set of sounds is small and
    /// fixed, so this is bounded by the catalogue however busy the tape.
    ///
    /// Per frame, but cheap: the walk is over a handful of tabs whose
    /// queues are empty on every frame but the one a signal happened on,
    /// and the sink is only asked when something is queued.
    fn play_pending_alarms(&mut self) {
        // Order of first request, duplicates dropped — `dedup` alone would
        // only collapse neighbours.
        let mut distinct: Vec<crate::audio::Cue> = Vec::new();
        for tab in &mut self.tabs {
            for cue in tab.pending_alarm_sounds.drain(..) {
                if !distinct.contains(&cue) {
                    distinct.push(cue);
                }
            }
        }
        if distinct.is_empty() {
            return;
        }
        let outcome = self.alerts.play(&distinct);
        self.report_alert_attempt(outcome);
    }

    /// Record whether a sound actually reached the trader.
    ///
    /// A notification that never arrived is reported, never assumed — so a
    /// first failure raises a toast rather than waiting for the trader to
    /// reopen the arming dialog, which they may never do. Only the *first*
    /// of a run: a build with no audio backend fails on every alarm, and a
    /// toast per bar would be its own noise. A success clears the reason, so
    /// one transient refusal does not leave a permanent red line behind it.
    fn report_alert_attempt(&mut self, outcome: Result<(), &'static str>) {
        match outcome {
            Ok(()) => self.alert_failure = None,
            Err(reason) => {
                let first = self.alert_failure.as_deref() != Some(reason);
                self.alert_failure = Some(reason.to_owned());
                if first {
                    self.show_agent_toast(format!("no alarm sound was played: {reason}"));
                }
            }
        }
    }

    /// Arm one instance on a drawing: compile the form, warm the trigger on
    /// the bars already closed (gates shut, so nothing fires from history),
    /// attach it, and start the paper host listening. `Err` carries the
    /// human-readable refusal for the dialog to show.
    /// Duplicate the selected drawing with everything riding it.
    ///
    /// The one door, because a duplication is not only a copied mark: an
    /// armed strategy rides the drawing today and whatever docks next will
    /// ride it too. Two call sites — the hotkey and the context bar — each
    /// spelling out "copy, then carry" is a third one that copies and
    /// forgets, and a band that silently loses its bot is exactly the class
    /// of silence this change exists to end.
    ///
    /// Rate: rare — one keystroke or one click.
    fn duplicate_selected_drawing(&mut self) {
        let side = self.active_tab().drawing_side();
        let Some(duplicated) = self
            .drawing_pane_mut()
            .drawings
            .duplicate_selected(DUPLICATE_OFFSET_BARS)
        else {
            return;
        };
        self.carry_strategy_to_duplicate(side, duplicated);
    }

    /// Carry an armed strategy across a duplication.
    ///
    /// A copied region is a region the trader wants watched the same way —
    /// duplicating the band and then re-typing the form is a step that only
    /// exists because the copy forgot. The copy is armed through
    /// [`Self::arm_strategy_instance`], the same door the dialog uses, from
    /// the stored form the source kept: one construction path, so a copy
    /// cannot quietly differ from what the dialog would have built.
    ///
    /// **Only a watching instance travels.** A source that is `Done`, or
    /// disarmed for any reason, was stopped — by the trader's own hand, by
    /// a rejected entry, by a spent one shot — and a copy that springs back
    /// to life places orders the trader last said no to. The copy lands
    /// offset to the *right*, the direction that makes a dead span live
    /// again, so Ctrl+D would otherwise be the one gesture that silently
    /// revives what was deliberately stopped.
    ///
    /// **State does not travel either.** The copy starts `Armed`, with a
    /// fresh ruler warmed on this pane's own bars, a fresh alarm (no
    /// inherited cooldown, no inherited preview mark) and no order id.
    /// Cloning a `Fired` instance would hang a second badge on one order.
    ///
    /// A refusal is reported rather than swallowed: `duplicate_selected`
    /// clones `hidden`, `off_series` and `foreign_market` verbatim — only
    /// `locked` is reset — and arming refuses all three. A trader who
    /// pressed hide-all and then Ctrl+D would otherwise unhide to two
    /// identical bands wearing one badge and believe both were watching.
    ///
    /// Rate: rare — one keystroke.
    fn carry_strategy_to_duplicate(
        &mut self,
        side: pane::PaneSide,
        duplicated: drawings::Duplicated,
    ) {
        use quantick_strategy::ArmedState;
        let Some((spec, label)) = self
            .active_tab()
            .pane(side)
            .strategies
            .for_drawing(duplicated.source)
            .filter(|instance| {
                matches!(
                    instance.armed.state(),
                    ArmedState::Armed | ArmedState::Fired { .. } | ArmedState::InPosition
                )
            })
            .map(|instance| (instance.spec.clone(), instance.preset.clone()))
        else {
            return;
        };
        if let Err(reason) = self.arm_strategy_instance(side, duplicated.copy, &spec, label) {
            self.note_workspace(format!("the copy carries no strategy: {reason}"));
        }
    }

    fn arm_strategy_instance(
        &mut self,
        side: pane::PaneSide,
        drawing: drawings::DrawingId,
        form: &crate::strategy_presets::StoredPreset,
        preset_label: String,
    ) -> Result<(), String> {
        let Some(compiled) = form.to_kernel() else {
            return Err(
                "a field does not parse: quantity, factors and multipliers must be numbers, \
                 and an instance that neither trades nor alarms cannot be armed"
                    .to_owned(),
            );
        };
        let crate::strategy_presets::CompiledPreset {
            params,
            force,
            alarm,
        } = compiled;
        let tab = self.active_tab_mut();
        let replaced_cleanup = {
            let pane = tab.pane_mut(side);
            // Re-validate everything the menu's gate promised: this is also
            // the seam a future programmatic caller (the NL layer) comes
            // through, and it must not be able to arm what the menu would
            // refuse — the wrong shape, another band, a drawing with no
            // footing here, or one nobody can see.
            let Some(index) = pane.drawings.index_of(drawing) else {
                return Err("the drawing is gone".to_owned());
            };
            let target = &pane.drawings.items()[index];
            if target.tool.id() != drawings::RECTANGLE_TOOL_ID
                || target.band != drawings::DrawingBand::Price
                || target.points.len() != 2
            {
                return Err("only price-band rectangles carry strategies".to_owned());
            }
            if target.foreign_market || target.off_series {
                return Err(
                    "this drawing belongs to another market or lost its series — redraw the \
                     region here first"
                        .to_owned(),
                );
            }
            if target.hidden || pane.drawings.all_hidden() {
                return Err("unhide the drawing first — an armed region stays visible".to_owned());
            }
            // A region whose drawn span can no longer cover a future bar
            // can never fire: the badge would show "armed" over a bot that
            // is structurally done — the silent halt the named disarms
            // exist to prevent. One predicate, shared with re-arm and the
            // evaluation sweep (`Pane::strategy_region_can_fire`), refuses
            // it with the fix in hand.
            if !pane.strategy_region_can_fire(drawing) {
                return Err(
                    "the region ends before the next bar, so nothing can ever fire — \
                     stretch it past the right edge, or turn on \"extend right\" in its \
                     Region settings"
                        .to_owned(),
                );
            }
            let mut armed = quantick_strategy::ArmedStrategy::new(
                params,
                Box::new(quantick_strategy::ForceTrigger::new(force.clone())),
            );
            // Warm the ruler on the bars the chart is already showing —
            // armed means armed now, not after another twenty bars of
            // warmup the trader cannot see the reason for. The trigger
            // declares its own depth (`warmup_bars`), and the pane keeps
            // venue-prefix candles out: they measure another ruler
            // entirely (a 1-minute body dwarfs a tick-bar body).
            armed.warm(&pane.strategy_warmup_bars(armed.trigger().warmup_bars()));
            pane.strategies
                .arm(crate::strategy_anchors::AnchoredInstance {
                    drawing,
                    preset: preset_label,
                    spec: form.clone(),
                    armed,
                    alarm: alarm.map(|setup| quantick_strategy::SignalAlarm::new(setup.params)),
                    cue: alarm.map(|setup| setup.cue).unwrap_or_default(),
                    mark: crate::strategy_anchors::AlarmMark::Quiet,
                })
        };
        for command in replaced_cleanup {
            // Arming over an instance with a pending entry sweeps that
            // entry — a resting order must never outlive its bot.
            let _ = tab.paper.account_mut().apply_strategy_command(command);
        }
        tab.paper.account_mut().set_bot_listening(true);
        // Only now, past every gate: the sink opens its device at arm time
        // so the first signal does not pay for it on the tape's path, but a
        // *refused* arm must open nothing. Ctrl+D over a band the copy
        // cannot be armed on discards the `Err` by design — the absent
        // badge is the message — and warming above the gates turned that
        // silence into an audio stack enumerated once per keypress, against
        // the sink's own promise that a chart which never arms an alarm
        // never touches a device.
        if let Some(setup) = alarm {
            self.alerts.warm_up(setup.cue);
        }
        Ok(())
    }

    /// The `QUANTICK_REPLAY_RESTART_AFTER` hook: press the transport's own
    /// Restart once the session has closed that many round trips.
    ///
    /// The seek is the only way to put a closed trade ahead of the tape the
    /// chart holds — the recording starts over, the round trips stay in the
    /// ledger because they happened, and their fills are now at instants no
    /// bar on screen covers. That is the state the marks used to stack on
    /// the edge bar in, and it takes a click on a transport button a
    /// scripted capture cannot make. Nothing happens without a recording
    /// playing: there is no timeline to seek on a live feed.
    ///
    /// Consumed once, whether or not the trades ever arrived — an env var
    /// is a request for this run, not a standing rule.
    fn apply_replay_restart(&mut self) {
        let Some(after) = self.harness.replay_restart_after() else {
            return;
        };
        let tab = self.active_tab();
        if tab.replay.is_none() || tab.paper.session_trades().len() < after {
            return;
        }
        // Spent only once the transport took it. A hook that cleared itself
        // on a dropped command would leave the capture photographing an
        // un-seeked timeline while the harness believed otherwise; the next
        // frame simply tries again.
        if self.apply_replay_action(ReplayAction::Control(ReplayControl::Restart)) {
            self.harness.replay_restart_taken();
        }
    }
}

impl QuantickApp {
    /// One frame of the application: drain, lay out the chrome, draw the
    /// chart.
    ///
    /// Everything `update` does that is not eframe's own bookkeeping, so a
    /// test can run a real frame against a headless [`egui::Context`] and read
    /// what was painted — the only honest way to assert that a chart is on
    /// screen rather than a blank rectangle.
    fn draw_frame(&mut self, ctx: &egui::Context, now: Instant) {
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_tabs();
        // A "load older" outcome is a passing remark: it leaves after
        // `tab::HISTORY_NOTE_LINGER` whether or not anyone read it. Every tab,
        // not only the one on screen — a background tab keeps draining, so it
        // can settle a run while hidden, and bringing it forward minutes later
        // must not surface a sentence about a press that is long over.
        for tab in &mut self.tabs {
            tab.expire_history_note(now);
        }
        // After the expiry, never before it: the hook re-raises a note it
        // finds absent, and running it first would let a note expire *after*
        // it looked, drawing one frame with an empty lane before the next
        // raise. A shutter timed on the linger catches exactly that frame.
        self.apply_history_note_hook();
        if self.pending_control_access_enable {
            self.pending_control_access_enable = false;
            if let Some(access) = self.control_access.as_mut() {
                access.enable(ctx);
            }
        }
        // Replay determinism: a session with a control trace beside it
        // re-injects its actions at their logical time, connected or not.
        // Before the hook's mark, so a loaded sidecar has seeded the trace
        // sequence the mark will take.
        if let Some(mut access) = self.control_access.take() {
            access.service_replay_trace(self);
            self.control_access = Some(access);
        }
        if let Some(note) = self.pending_control_mark.take() {
            let note = (!note.is_empty()).then_some(note);
            self.take_mark(note);
        }
        self.apply_control_annotate_hooks();
        // After the annotate hooks and before the gateway's own drain: a
        // bundle captured from a launch then describes the window an
        // assistant has already written on, which is the state a validation
        // run is actually asking about.
        self.apply_control_evidence_hook(ctx);
        if self
            .control_access
            .as_ref()
            .is_some_and(crate::control::ControlAccess::needs_frame_service)
            && let Some(mut access) = self.control_access.take()
        {
            access.begin_frame(self, ctx);
            self.control_access = Some(access);
        }
        self.apply_scripted_view();
        self.apply_drawing_demo();
        self.apply_load_older();
        self.apply_load_older_candles();
        self.apply_drawing_draft();
        self.apply_venue_history_demo();
        self.apply_frvp_demo();
        self.apply_avwap_demo();
        self.apply_strategy_demo();
        self.apply_replay_restart();
        self.apply_maximize_hook(ctx);
        self.maybe_emit_summary(now, ctx);
        self.maintain_workspace(ctx);

        let bg = pane::background_color(&self.style);
        // Rail shortcuts first: Esc/1/2 must be read before any widget can
        // claim the keyboard this frame.
        self.toolrail.handle_keys(ctx);
        self.handle_tab_keys(ctx);
        self.handle_drawing_keys(ctx, now);
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the edge-docked drawing rail and
        // the right dock. The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        if let Some(access) = self.control_access.as_mut() {
            access.draw_panel(ctx);
        }
        self.draw_toolbar(ctx);
        // Before the dialog is drawn, so a double click on a pane or a curve
        // opens it on the same frame the gesture happened rather than the next.
        self.open_requested_indicator_settings();
        self.draw_indicator_settings(ctx);
        self.draw_indicator_legends(ctx);
        // **After** the dialogs above, and that placement is load-bearing.
        // The preview watermark reads whether a settings dialog is previewing
        // an unapplied draft, and `draw_indicator_settings` is what sets that
        // — so an environment built before it would put the banner on screen
        // a frame after the legend chip that says the same thing, and take it
        // off a frame later too. Two surfaces the trader reads as one is this
        // repo's own bug class; sixteen milliseconds of it is still one
        // frame a capture can photograph.
        //
        // It is also where the windows this pass drew used to sit: the
        // appearance and footprint panels ran after the toolbar that toggles
        // them, so a click on LOOK opens the panel on the same frame rather
        // than the next.
        // A pane's right-click asked to arm one of its drawings. Drained
        // here, into the surface that owns the dialog: the click happens
        // while the canvas draws, which is later in this frame than the
        // surfaces are, so the dialog opens on the next one — a frame the
        // trader cannot see, and the price of the dialog no longer living in
        // the trunk.
        let sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            self.active_tab().sides().collect();
        for side in sides {
            let request = self
                .active_tab_mut()
                .pane_mut(side)
                .strategy_popup_request
                .take();
            if let Some(drawing) = request {
                let form = crate::strategy_presets::StoredPreset::starting_point(
                    quantick_engine::Side::Buy,
                );
                let tab = self.active_tab().id;
                self.surfaces.strategy_popup.open(tab, side, drawing, form);
            }
        }
        // The bar rules the arming dialog's alarm section reads: a share of
        // the bar only means something where the rule closes on a count.
        // Built only while that dialog is open, like the open markets below.
        // `hooks_pending` is the frame a capture hook opens a surface from
        // inside `draw_all`: it is not open yet when this runs, but it is
        // about to be, and it must not draw its first frame against an empty
        // environment.
        let staging = self.surfaces.hooks_pending();
        let counted_bar_sides: SmallVec<[pane::PaneSide; MAX_CANVAS_PANES]> =
            if staging || self.surfaces.strategy_popup.is_open() {
                let tab = self.active_tab();
                tab.sides()
                    .filter(|side| tab.pane(*side).state.progress().is_some())
                    .collect()
            } else {
                SmallVec::new()
            };
        // The markets tabs are showing: the dialog greys out removing one of
        // those, because a tab left on a symbol the catalog no longer offers
        // gets silently retargeted by the next SOURCE correction. Built only
        // while the dialog is open — it is a `String` pair per tab, and no
        // frame should pay for it to be thrown away.
        let open_markets: Vec<(String, String)> =
            if staging || self.surfaces.source_picker.is_open() {
                self.tabs
                    .iter()
                    .map(|tab| (tab.feed_id.clone(), tab.symbol.clone()))
                    .collect()
            } else {
                Vec::new()
            };
        // Split into disjoint borrows: the surfaces are drawn through `&mut`
        // while the environment they read is borrowed from the rest of the
        // application. That the compiler insists on the split is the port
        // working — a surface cannot be handed the trunk it is being kept
        // out of.
        let Self {
            surfaces: registry,
            workspace,
            style,
            footprint_config,
            tabs,
            active_tab,
            indicator_settings,
            indicator_settings_target,
            config,
            added_symbols,
            alert_failure,
            ..
        } = self;
        let focused_tab = &tabs[*active_tab];
        // Read once. `focused_pane` resolves the same side internally, and
        // the answer is not a field lookup — it reads the layout, because
        // focus on a collapsed pane is focus on nothing.
        let focused_side = focused_tab.focused_side();
        let focused_pane = focused_tab.pane(focused_side);
        let surfaces = registry.draw_all(
            ctx,
            &crate::surfaces::SurfaceEnv {
                bookmarks: workspace.session().bookmarks(),
                now,
                indicator_preview_area: indicator_preview_area(
                    tabs,
                    indicator_settings.as_ref(),
                    *indicator_settings_target,
                ),
                focused_chart_area: focused_pane.last_chart_area,
                style,
                footprint: focused_pane.footprint_config(footprint_config),
                footprint_customized: focused_pane.footprint_override.is_some(),
                focused_side,
                config,
                added_symbols,
                open_markets: &open_markets,
                active_tab: focused_tab.id,
                counted_bar_sides: &counted_bar_sides,
                alert_failure: alert_failure.as_deref(),
            },
        );
        if let Some(name) = surfaces.save_workspace_as {
            self.save_named_workspace(&name);
        }
        if let Some(style) = surfaces.style {
            self.style = style;
            self.style_revision = self.style_revision.saturating_add(1);
        }
        // After the assignment, never before: the log line reports the
        // appearance that is now in force, and the revision it landed on.
        if let Some(request) = surfaces.log_style_change {
            self.emit_style_changed(request.applied_preset);
        }
        // The audition goes through the one speaker every armed instance
        // shares, and reports a sound that could not be heard exactly as a
        // missed signal would.
        if let Some(cue) = surfaces.test_alert {
            let outcome = self.alerts.play(&[cue]);
            self.report_alert_attempt(outcome);
        }
        if let Some(request) = surfaces.arm_strategy {
            let outcome = self.arm_strategy_instance(
                request.side,
                request.drawing,
                &request.form,
                request.label,
            );
            self.surfaces.strategy_popup.settle_arm(outcome);
        }
        if let Some(request) = surfaces.market {
            self.apply_market_request(request);
        }
        if let Some(change) = surfaces.footprint {
            self.apply_footprint_change(change);
        }
        if surfaces.undo_drawing {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Same orphan risk as the keyboard undo: the drawing an armed
            // instance rides may just have been taken away.
            pane.sweep_strategy_orphans();
        }
        self.poll_script_files();
        self.maintain_indicator_state();
        self.maintain_chart_layers();
        // This tab's judgement about its own feed, taken once for the frame:
        // the status bar reads it here and the corner reads it below, and two
        // readings a millisecond apart could disagree about whether a budget
        // had run out.
        let stall = self
            .active_tab()
            .stall_at(&self.config, metrics::wall_clock_ms());
        let offline_accent = self.feed_offline_accent(stall.as_ref());
        let status = self.status_model();
        let status_response = statusbar::draw(ctx, &status, &mut self.tz, offline_accent);
        if status_response.open_trading_tab {
            self.dock.open_tab(DockTab::Trading);
        }
        // Above the status bar, below the canvas: the layout tabs.
        self.draw_layout_strip(ctx);
        self.draw_layout_delete_confirm(ctx);
        // The browser window and, while the *active* tab plays a session, its
        // transport bar. A background tab's recording keeps advancing on its
        // own feed thread; what it does not get is the strip, which speaks for
        // one tab at a time (§11).
        let replay_action = {
            let Self {
                replay_view,
                tabs,
                active_tab,
                config,
                ..
            } = self;
            let tab = &tabs[*active_tab];
            // The instruments the download tab offers with one click. A dated
            // contract rolls every couple of months, and typing `WINV26` from
            // memory is not a thing a trader should have to get right to see
            // what they can replay.
            //
            // Filtered by what the download source actually serves, which the
            // source itself answers: offering a Binance pair to a MetaTrader
            // exporter would be a click that can only end in a refusal, and
            // the chart behind this window is often on another venue entirely.
            let serves = replay_view.download_provider();
            let market = crate::replay_view::MarketMenu {
                current: (config.provider_of(&tab.feed_id) == Some(serves))
                    .then_some(tab.symbol.as_str()),
                catalogue: config
                    .feeds
                    .iter()
                    .filter(|feed| feed.provider == serves)
                    .flat_map(|feed| feed.symbols.iter().map(String::as_str))
                    .collect(),
            };
            replay_view.draw(ctx, tab.replay.as_ref(), &market)
        };
        if let Some(action) = replay_action {
            self.apply_replay_action(action);
        }
        // A folder the trader just pointed the browser at is written down on
        // the frame they pointed it, not at exit: "it forgot my folder again"
        // must not be one crash away.
        if let Some(pick) = self.replay_view.take_folder_change() {
            self.write_replay_folder(pick.as_deref());
        }
        // The same, for the tick that decides whether yesterday is on the
        // chart. Either row can have been the one clicked; the browser owns
        // the setting, so there is one place to pick the change up.
        if let Some(enabled) = self.replay_view.take_day_before_change() {
            self.write_replay_day_before(enabled);
        }
        {
            // The focused pane's objects: the toolbox lists and manages what a
            // click on the canvas would act on.
            let side = self.active_tab().focused_side();
            // The flag lives with the window it opens, so it travels through
            // a local rather than a `&mut` handed out of the surface.
            let mut manager_open = self.surfaces.drawing_chrome.manager_open();
            {
                let Self {
                    toolrail,
                    tabs,
                    active_tab,
                    ..
                } = self;
                let tab = &mut tabs[*active_tab];
                toolrail.draw(ctx, &mut tab.pane_mut(side).drawings, &mut manager_open);
            }
            self.surfaces.drawing_chrome.set_manager_open(manager_open);
        }
        // A star clicked this frame is on disk this frame, like the replay
        // folder above: the pinned rail is what the trader reaches for without
        // looking, and rebuilding it after a crash is not a thing anyone
        // should have to do twice.
        if self.toolrail.take_favorites_change() {
            self.write_favorites();
        }
        let dock_response = {
            let Self {
                dock,
                tabs,
                active_tab,
                replay_view,
                tz,
                ..
            } = self;
            // The Trading tab speaks for the market on screen: one tab, one
            // simulator, and the dock reads the active tab's — exactly like
            // the tape and the session panel beside it.
            let Tab {
                flow_pane,
                replay,
                paper,
                ..
            } = &mut tabs[*active_tab];
            let orderflow = flow_pane
                .orderflow
                .as_mut()
                .expect("the flow pane is built with a tape and never drops it");
            dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow,
                    replay_view,
                    replay: replay.as_ref(),
                    paper,
                    tz: *tz,
                },
            )
        };
        // The strategy editor is a window of the active tab's ticket, drawn
        // whatever the dock is showing and whether it is showing at all: it
        // is opened from the Trading tab but it does not belong to it, and a
        // trader who opens it and then looks at the ledger has not asked for
        // it to close.
        if self.active_tab_mut().paper.draw_strategy_editor(ctx) {
            self.persist_order_strategies();
        }
        if dock_response.restart_book_capture {
            self.active_tab_mut().restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            // A click that lost its slot has the trader's next click behind
            // it; only the one-shot hook below cares about the answer.
            let _ = self.apply_replay_action(action);
        }
        // The ledger's jump-to-trade: center the flow pane on the round
        // trip's midpoint, the object manager's own "select and centre".
        //
        // The covering lookup, the same one the marks are painted through:
        // a trade the flow chart's bars do not reach has nowhere to be
        // centred on, and scrolling to the clamped edge instead would land
        // the trader on a bar holding no mark and no explanation. Saying so
        // is the whole of the handling — the row stays in the ledger.
        //
        // The message names the flow chart rather than "the chart": in a
        // split tab the time pane keeps its own, longer window, so the same
        // round trip can be off this one and painted on that one.
        if let Some((opened, closed)) = dock_response.navigate_to_trade {
            let tab = self.active_tab_mut();
            let covered = tab
                .flow_pane
                .covering_slot_at_time(opened)
                .zip(tab.flow_pane.covering_slot_at_time(closed));
            match covered {
                Some((entry, exit)) => {
                    let pane = &mut tab.flow_pane;
                    if let Some(area) = pane.last_chart_area {
                        let slots = pane.slots();
                        let mid = (entry + exit) as f32 / 2.0;
                        pane.viewport.center_on_bar(mid, area.width(), slots);
                    }
                }
                None => {
                    // Said as an event as well as on screen: an operator
                    // driving the ledger without eyes on the toast must be
                    // able to tell a refusal from a silent no-op.
                    tracing::info!(
                        target: "quantick::app",
                        event_code = "TRADE_NAVIGATE_OFF_TAPE",
                        opened_ms = opened,
                        closed_ms = closed,
                        "jump-to-trade refused: the flow chart has no bar for the fills"
                    );
                    tab.paper.show_toast(
                        "This trade is outside the bars on the flow chart - nothing to centre on."
                            .to_owned(),
                    );
                }
            }
        }
        if dock_response.pick_trades_dir {
            self.open_trades_dir_picker();
        }
        if dock_response.order_strategies_changed {
            self.persist_order_strategies();
        }
        if dock_response.cmd_trading_changed {
            self.persist_cmd_trading();
        }
        if dock_response.risk_settings_changed {
            self.persist_risk_settings();
        }
        self.poll_trades_dir_picker();
        self.poll_workspace_picker();
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        self.draw_pinned_inspector(ctx, now);
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        let (tab, config) = self.active_with_config();
        tab.maybe_switch_feed(config);
        // Both deferrals settle here, a frame after the click that armed
        // them, so the frame carrying the change paints its overlay first.
        let Self {
            tabs,
            config,
            style,
            pane_ids,
            ..
        } = self;
        for tab in tabs.iter_mut() {
            tab.apply_pending_layout(config, style, pane_ids);
        }
        // Right after panes appear and markets switch, so a pane built this
        // frame is seeded this frame and a tab that changed symbol swaps its
        // drawings before anything paints them.
        self.maintain_layouts();
        self.active_tab_mut().apply_spec_changes();
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        let replay_loading = self.replay_view.is_loading();
        let book_syncing = self.active_tab().tape().is_syncing();
        let tab = self.active_tab_mut();
        tab.loading
            .set_active(LoadingTask::ReplaySession, replay_loading);
        tab.loading.set_active(LoadingTask::BookSync, book_syncing);

        let mut notice_action = feed_notice::NoticeAction::None;
        // Read before the canvas borrows `self`, and answered after it lets go.
        let popup_tab = self.active_tab().id;
        let popup_open = self.feed_popup_tab == Some(popup_tab);
        let mut chip_clicked = false;
        let mut dismissed = false;
        // Where the corner landed, and so whether there was one at all. A feed
        // that recovered while the popup was open closes it, rather than
        // leaving a stale explanation over a chart that is fine again.
        let mut chip_rect = None;
        // The layer menu offers what this source can produce; resolved once
        // here rather than per pane, per entry, inside the canvas.
        let capabilities = self.active_tab().capabilities(&self.config);
        // Same one-per-frame resolution for the side-honesty label the
        // footprint legend carries.
        let side_inferred = self.active_tab().side_note(&self.config).is_some();
        // Told before the canvas paints, not after: the object holding the
        // words the editor is showing must stand down on the *same* frame,
        // or the note flashes its placeholder under the field for one.
        self.sync_content_editing();
        // Raised by a placement that wants its note typed, and handed to the
        // chrome below: the flag belongs to the editor that owns the caret,
        // not to the canvas that asks for it.
        let mut begin_text_edit = false;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                {
                    let Self {
                        tabs,
                        active_tab,
                        toolrail,
                        drawing_presets,
                        style,
                        tz,
                        layer_actions,
                        footprint_config,
                        ..
                    } = self;
                    let mut chrome = CanvasChrome {
                        toolrail,
                        presets: drawing_presets,
                        begin_text_edit: &mut begin_text_edit,
                        style,
                        tz: *tz,
                        capabilities,
                        side_inferred,
                        footprint: footprint_config,
                        layers: layer_actions,
                    };
                    tabs[*active_tab].draw_canvas(ui, area, &mut chrome);
                }
                // The grid and the indicator state belong to the window, not
                // to the pane whose menu switched them.
                self.apply_layer_actions();
                let tab = self.active_tab();
                // Each wait on the surface it is about. The panes published
                // their rects on the draw just above, so these are this
                // frame's geometry rather than the previous one's.
                let history_note = tab.history_note();
                loading::overlay_scoped(ui, area, &tab.loading, LoadingScope::Whole, history_note);
                // A scope whose surface is not on screen falls back to the
                // canvas rather than dropping its wait: the flow pane is not
                // painted in the Time layout, and a flow-only layout has no
                // time pane, and in both cases the wait is still running. A
                // spinner in the wrong place is a placement complaint; a
                // missing one reads as a frozen application.
                loading::overlay_scoped(
                    ui,
                    tab.flow_pane.last_area.unwrap_or(area),
                    &tab.loading,
                    LoadingScope::Flow,
                    history_note,
                );
                let time_panes: Vec<egui::Rect> = tab
                    .panes()
                    .filter(|(_, side)| matches!(side, PaneSide::Time(_)))
                    .filter_map(|(pane, _)| pane.last_area)
                    .collect();
                if time_panes.is_empty() {
                    loading::overlay_scoped(
                        ui,
                        area,
                        &tab.loading,
                        LoadingScope::TimePanes,
                        history_note,
                    );
                } else {
                    for rect in time_panes {
                        loading::overlay_scoped(
                            ui,
                            rect,
                            &tab.loading,
                            LoadingScope::TimePanes,
                            history_note,
                        );
                    }
                }
                // And the feed's own report, in the corner rather than over
                // the chart. Progress never gets here: a first connection and a
                // history block already have the loading overlay above, and a
                // second badge beside it would be the interface talking about
                // itself twice.
                if let Some(report) = feed_notice::report(&tab.notice, stall.as_ref())
                    && report.is_offline()
                {
                    // Measured once, then handed to everything that needs
                    // it: the chip's own hit test, the popup's anchor, the
                    // dismissal test, and the scene's bounds.
                    let chip = feed_notice::chip_rect(ui.painter(), area);
                    chip_rect = Some(chip);
                    chip_clicked = feed_notice::draw_chip(ui, chip, &report, popup_open);
                    // A pane with nothing on it has room to say why, and a
                    // corner chip alone on a blank canvas is a puzzle. One
                    // muted line, no border and no buttons — the way out is
                    // still the corner.
                    //
                    // Not while the popup is up. The line and the popup carry
                    // the same headline, and on the empty chart that is
                    // exactly where both of them draw: one sentence, twice, a
                    // hand apart.
                    if !popup_open && let Some((pane_rect, 0)) = tab.starved_pane() {
                        feed_notice::draw_empty_pane_note(ui.painter(), pane_rect, &report);
                    }
                    if popup_open {
                        // A click anywhere else puts it away, measured against
                        // the rectangles that were actually drawn — so a click
                        // on the edge of what the trader can see is never read
                        // as a click outside it, and the popup is laid out
                        // once rather than measured again to ask.
                        let popup;
                        (notice_action, popup) = feed_notice::draw_popup(ui, area, chip, &report);
                        dismissed = ui.input(|input| {
                            input.pointer.any_click()
                                && input
                                    .pointer
                                    .interact_pos()
                                    .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
                        });
                    }
                }
            });
        if begin_text_edit {
            self.surfaces.drawing_chrome.request_text_edit();
        }
        // Floating drawing controls must be registered after the opaque
        // central canvas so they stay in front of the chart. That is why the
        // drawing chrome is the one surface `Surfaces::draw_all` does not
        // draw: it is anchored *to* the chart rather than floating over the
        // window, so it is commanded by name from here instead.
        self.draw_drawing_chrome(ctx, now);
        // The menus above may have disarmed a bot over a resting retest
        // limit; its cancel goes to the simulator on this same frame, not
        // on the next print. Every tab, not just the active one: a menu
        // click and a tab switch can land on the same frame, and the old
        // tab's feed keeps running — its cancel must not sit stranded
        // until the tab is looked at again.
        for tab in &mut self.tabs {
            tab.apply_strategy_cleanup();
        }
        self.play_pending_alarms();
        // Both are window chrome reading the active tab, like the offline
        // corner and the transport strip: they speak for one market at a time.
        let tz = self.tz;
        self.active_tab_mut().paper.draw_report_window(ctx, tz);
        self.settle_paper_panels(now);
        // Both controls go through the tab's own methods, which are also what
        // the registered control-plane actions call: a click and a named call
        // must be able to disagree about nothing.
        match notice_action {
            feed_notice::NoticeAction::None => {}
            feed_notice::NoticeAction::Reconnect => {
                let (tab, config) = self.active_with_config();
                let _ = tab.reconnect_feed(config);
            }
            feed_notice::NoticeAction::Reload => {
                let (tab, config) = self.active_with_config();
                let _ = tab.reload_feed(config);
            }
        }
        self.feed_chip_rect = chip_rect;
        self.feed_popup_tab = feed_notice::popup_still_open(
            popup_open,
            chip_clicked,
            chip_rect.is_some(),
            dismissed,
            notice_action,
        )
        .then_some(popup_tab);
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// Every tab takes in what its feed sent this frame, on screen or not.
    ///
    /// §11: switching tabs never tears a feed down, so a background tab has to
    /// keep draining — its channels are bounded, and one left full backs its
    /// feed thread up until the market it is showing is hours behind. The
    /// indicator workers are fed on the same pass, so a tab brought forward is
    /// already current rather than rebuilding on the frame it appears.
    fn drain_tabs(&mut self) {
        let config = &self.config;
        let progressive_history = self.progressive_history;
        let history_reach = self.history_reach;
        let history_reach_span_minutes = self.history_reach_span_minutes;
        let venue_lead_in = self.venue_lead_in;
        let mut trades = 0_u64;
        for tab in &mut self.tabs {
            let before = tab.live_trades;
            tab.drain_feed();
            for pane in tab.panes_mut() {
                pane.apply_indicator_events();
            }
            tab.drain_book_feed();
            tab.drain_notices();
            // Heartbeat for the recorder. The lifecycle calls elsewhere already
            // start it at every point that knows the market changed; this one
            // makes "always recording" true by construction, so a start command
            // lost to a momentarily full channel heals on the next frame
            // instead of leaving the session silently unrecorded. Free while it
            // is running: one bool read and an early return.
            tab.ensure_book_capture(config);
            // MetaTrader narrows its capabilities when the bridge says hello,
            // after the pane may already have asked and been told there was
            // nothing held. Watching the edge is what asks again once the
            // answer can be a real one.
            // The switch lives on the window, the request is phrased by the
            // tab: mirrored here so every tab asks the way the trader last
            // said, including one opened after the choice was made.
            tab.progressive_history = progressive_history;
            tab.history_reach = history_reach;
            tab.history_reach_span_minutes = history_reach_span_minutes;
            // Through the setter, not the field: flipping the lead-in refolds
            // the prefix, and a tab that only had the field written would keep
            // drawing the answer to the previous choice until the next candle
            // landed. Idempotent, so the steady state costs one comparison.
            tab.set_venue_lead_in(venue_lead_in);
            tab.poll_ohlcv_capability(config);
            trades += tab.live_trades - before;
        }
        // What the window ingested, across every market it is holding.
        self.trades_since_summary += trades;
    }

    /// Tab shortcuts (§10): `Ctrl+T` new, `Ctrl+W` close, `Ctrl+Tab` cycle.
    fn handle_tab_keys(&mut self, ctx: &egui::Context) {
        // Focus-gated like `handle_drawing_keys` (audit MINOR-13): typing in
        // the source picker's field with Ctrl held must never close the tab
        // under it — closing is instant and currently irreversible.
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        let (new_tab, close_tab, next, previous) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&NEW_TAB_SHORTCUT),
                input.consume_shortcut(&CLOSE_TAB_SHORTCUT),
                input.consume_shortcut(&NEXT_TAB_SHORTCUT),
                input.consume_shortcut(&PREVIOUS_TAB_SHORTCUT),
            )
        });
        if new_tab {
            self.surfaces.source_picker.open(&self.config);
        }
        if close_tab {
            self.close_tab(self.active_tab);
        }
        if next {
            self.cycle_tab(1);
        }
        if previous {
            self.cycle_tab(-1);
        }
    }

    /// Do what the "Open market" dialog settled on.
    fn apply_market_request(&mut self, request: crate::surfaces::MarketRequest) {
        use crate::surfaces::MarketRequest;
        match request {
            MarketRequest::Open { feed_id, symbol } => self.open_tab(feed_id, symbol, None),
            MarketRequest::Add { feed_id, symbol } => match self.add_symbol(&feed_id, &symbol) {
                Ok(()) => {
                    self.surfaces.source_picker.close();
                    self.open_tab(feed_id, symbol, None);
                }
                // The dialog stays open carrying the reason: the user is one
                // keystroke from a symbol that does fit, and closing would
                // make the refusal look like a crash.
                Err(reason) => self.surfaces.source_picker.refuse(reason),
            },
            MarketRequest::Remove { feed_id, symbol } => self.remove_symbol(&feed_id, &symbol),
        }
    }

    /// Put `symbol` in feed `feed_id`'s catalog and remember it across
    /// restarts. Reports whether the catalog took it.
    ///
    /// The config file itself is never written: it is hand-written, comments
    /// and all, and a program that rewrote it would eat them. The addition
    /// lives in its own sidecar, which the next launch folds back in before
    /// the config is validated (see [`crate::symbols_file`]).
    fn add_symbol(&mut self, feed_id: &str, symbol: &str) -> Result<(), String> {
        // Against the *whole* config, on a copy. A symbol is not just a name
        // in a list: it takes part in every cross-check the config has, and
        // the MetaTrader port map is one where a single mapped symbol offered
        // by two feeds is a configuration the app refuses to load. Persisting
        // one of those would write a file that kills the next launch — and the
        // error would name the config, which is not the file that broke.
        let mut candidate = self.config.clone();
        if !candidate.add_symbol(feed_id, symbol) {
            return Err(format!(
                "{} already offers {symbol}",
                self.config.feed_name(feed_id)
            ));
        }
        candidate.validate()?;
        self.config = candidate;
        self.added_symbols.add(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.workspace.symbols_path(), &self.added_symbols) {
            // The catalog took it for this session either way; what is lost is
            // the next launch, and the user is told which file did not take it.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.workspace.symbols_path().display(),
                error = %error,
                action = "addition_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_ADDED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.workspace.symbols_path().display(),
            action = "open_in_new_tab",
            "a symbol was added from the source picker"
        );
        Ok(())
    }

    /// Take a user-added `symbol` back out of feed `feed_id`'s catalog.
    ///
    /// Only ever a catalog edit: a tab already showing that market keeps
    /// streaming it. The picker will not offer this for a market a tab is on,
    /// which is what stops the selection correction from retargeting it.
    fn remove_symbol(&mut self, feed_id: &str, symbol: &str) {
        if !self.config.remove_symbol(feed_id, symbol) {
            return;
        }
        self.added_symbols.remove(feed_id, symbol);
        if let Err(error) = symbols_file::save(self.workspace.symbols_path(), &self.added_symbols) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_WRITE_FAILED",
                path = %self.workspace.symbols_path().display(),
                error = %error,
                action = "removal_is_session_only",
                "cannot write the added-symbols file"
            );
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "SYMBOL_REMOVED",
            feed = %feed_id,
            symbol = %symbol,
            path = %self.workspace.symbols_path().display(),
            action = "leave_open_tabs_alone",
            "a user-added symbol left the catalog"
        );
    }

    /// Carry out what the tab strip asked for.
    fn apply_tab_action(&mut self, action: TabAction) {
        match action {
            TabAction::Activate(index) => {
                if index < self.tabs.len() {
                    self.active_tab = index;
                }
            }
            TabAction::Close(index) => self.close_tab(index),
            TabAction::New => self.surfaces.source_picker.open(&self.config),
        }
    }
}

/// The interval a saved bar rule names, when it is a time rule at all — a
/// workspace that recorded `tick:50` for a context chart is a file written by
/// hand, and the chart opens on the default rather than on a guess.
fn saved_time_interval(text: Option<&str>) -> Option<i64> {
    text.and_then(|text| match BarSpec::parse(text) {
        Ok(BarSpec::Time(ms)) => Some(ms),
        _ => None,
    })
}

/// Every context chart's opening interval, top to bottom, from the rules a
/// workspace saved. A rule that is not a time rule keeps the default for its
/// slot so the slots after it still line up with their charts. A file written
/// before the stack existed carries only `time_bars`, which is the top chart's.
fn saved_context_intervals(bars: &[String], time_bars: Option<&str>) -> Vec<i64> {
    if bars.is_empty() {
        return saved_time_interval(time_bars).into_iter().collect();
    }
    bars.iter()
        .map(|text| {
            saved_time_interval(Some(text)).unwrap_or(crate::time_header::DEFAULT_INTERVAL_MS)
        })
        .collect()
}

crate::hooks::declare_hooks![
    "QUANTICK_BOOK_AUTOSTART",
    "QUANTICK_BUBBLES_AUTOSTART",
    "QUANTICK_BUBBLE_BUDGET",
    "QUANTICK_CONTROL_ACCESS",
    "QUANTICK_CONTROL_ANNOTATE",
    "QUANTICK_CONTROL_EVIDENCE",
    "QUANTICK_CONTROL_MARK",
    "QUANTICK_CONTROL_NOTIFY",
    "QUANTICK_CONTROL_PANEL",
    "QUANTICK_CONTROL_SCOPES",
    "QUANTICK_DOCK_TAB",
    "QUANTICK_DRAWING_MAGNET",
    "QUANTICK_DRAWING_TOOL",
    "QUANTICK_FOOTPRINT_STYLE",
    "QUANTICK_HISTORY_REACH",
    "QUANTICK_HISTORY_REACH_SPAN_MINUTES",
    "QUANTICK_INDICATORS_AUTOSTART",
    "QUANTICK_INDICATOR_SCRIPTS_AUTOSTART",
    "QUANTICK_INVERTED",
    "QUANTICK_LAYOUT",
    "QUANTICK_LAYOUT_DELETE",
    "QUANTICK_LAYOUT_RENAME",
    "QUANTICK_LAYOUT_TAB",
    "QUANTICK_LEDGER_FOLD",
    "QUANTICK_LEDGER_PAGES",
    "QUANTICK_LEDGER_SCOPE",
    "QUANTICK_LEGEND_COLLAPSED",
    "QUANTICK_LIVE_STRIP_AUTOSTART",
    "QUANTICK_PANE_LAYOUTS",
    "QUANTICK_PAPER_CALENDAR",
    "QUANTICK_PAPER_REPORT_AUTOSTART",
    "QUANTICK_PAPER_REPORT_LIST",
    "QUANTICK_PROGRESSIVE_HISTORY",
    "QUANTICK_REPLAY_AUTOSTART",
    "QUANTICK_REPLAY_BROWSER",
    "QUANTICK_REPLAY_DAY_BEFORE",
    "QUANTICK_REPLAY_SESSION",
    "QUANTICK_REPLAY_SPEED",
    "QUANTICK_TAPE",
    "QUANTICK_TAPE_LAYERS",
    "QUANTICK_TAPE_STARVE_AFTER_MS",
    "QUANTICK_TAPE_WINDOW",
    "QUANTICK_TOOLBAR_SCROLL",
    "QUANTICK_TOOLBOX_DOCK",
    "QUANTICK_TOOLBOX_FLYOUT",
    "QUANTICK_TOOL_FAVORITES",
    "QUANTICK_VENUE_LEAD_IN",
    "QUANTICK_WORKSPACE_EXPORT",
    "QUANTICK_WORKSPACE_IMPORT",
    "QUANTICK_WORKSPACE_SAVE"
];

#[cfg(test)]
mod tests;
