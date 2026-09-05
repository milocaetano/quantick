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

use std::time::Instant;

use eframe::egui;

use crate::canvas_layout::PaneIdAllocator;

mod chart_layers_wiring;
mod control_host;
mod demo_hooks;
mod drawing_chrome_wiring;
mod drawing_input;
mod frame;
mod health;
mod indicator_manager;
pub(crate) mod launch_hooks;
mod layout_wiring;
mod menu_bar;
mod paper_wiring;
mod replay_and_history;
mod tabs;
mod toolbar_wiring;
mod workspace_restore;
mod workspace_save;

// The tab lifecycle took `saved_context_intervals` with it; `workspace_restore`
// and `workspace_save` still reach it through `super::`.
use tabs::saved_context_intervals;
// Named by the paper-trading and drawing tests through `use super::*`, and by
// nothing in production outside the module that owns them, so the imports are
// gated the way the block at the end of this list is.
#[cfg(test)]
use menu_bar::{
    PAPER_BUY_SHORTCUT, PAPER_CANCEL_SHORTCUT, PAPER_FLATTEN_SHORTCUT, PAPER_REVERSE_SHORTCUT,
    PAPER_SELL_SHORTCUT,
};
#[cfg(test)]
use replay_and_history::DUPLICATE_OFFSET_BARS;

use crate::chart_layers;
use crate::config::AppConfig;
use crate::dock::Dock;
use crate::drawings;
use crate::feed_notice;
use crate::harness::{Harness, ScriptedMenu};
use crate::indicator_panel::SettingsDialog;
use crate::indicator_worker::SlotId;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file::{self, SavedKind};
use crate::metrics::FrameStats;
use crate::pane::PaneSide;
use crate::replay_view::ReplayView;
use crate::state::BarSpec;
use crate::style::ChartStyle;
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;
use crate::ui_state;
use crate::window_scale;
use crate::workspace_store::{LayoutStore, StorePaths, WorkspaceStore};
use quantick_feed::FeedHandle;
use quantick_feed::history_reach;
use quantick_orderflow::LaneWindow;

// Names the window's own code no longer reads: the nine modules above took the
// last production read of each with them -- `launch_hooks` and `paper_wiring`
// took the newest five. `app::tests` still reaches every one
// through `use super::*`, so they stay here as gated imports rather than
// becoming an edit to `app/tests/` — the same treatment `ChartLayer` already
// had when `maintain_chart_layers` left for `app::chart_layers_wiring`, and
// `heatmap_lamp_on` has now taken the last read of that one to `app::tabs`.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
#[cfg(test)]
use crate::dock::DockTab;
#[cfg(test)]
use crate::harness::ContextMenuPane;
#[cfg(test)]
use crate::loading::LoadingTask;
#[cfg(test)]
use crate::metrics;
#[cfg(test)]
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX};
#[cfg(test)]
use crate::statusbar;
#[cfg(test)]
use crate::tab::CanvasLayout;
#[cfg(test)]
use crate::tabstrip::TabAction;
#[cfg(test)]
use crate::theme;
#[cfg(test)]
use crate::toolbar::ToolbarAction;
#[cfg(test)]
use crate::toolrail::{Tool, ToolboxDock};
#[cfg(test)]
use quantick_feed::{self as feed, FeedCommand, ReplayControl};

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
        // Every `QUANTICK_*` launch hook, applied to the built window in one
        // place with one name -- see `launch_hooks`, whose doc comment owns
        // the order they are read in.
        app.apply_launch_hooks();
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

#[cfg(test)]
mod tests;
