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
mod layout_wiring;
mod menu_bar;
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
use crate::dock::{Dock, DockTab};
use crate::drawings;
use crate::feed_notice;
use crate::harness::{Harness, ScriptedMenu};
use crate::indicator_panel::SettingsDialog;
use crate::indicator_worker::{IndicatorSource, SlotId};
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file::{self, SavedKind};
use crate::metrics::FrameStats;
use crate::pane::{self, PaneSide};
use crate::replay_view::ReplayView;
use crate::state::BarSpec;
use crate::style::ChartStyle;
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::{CanvasLayout, Tab};
use crate::timezone::TzOffset;
use crate::toolrail::{Tool, ToolRail, ToolboxDock};
use crate::ui_state;
use crate::window_scale;
use crate::workspace_store::{LayoutStore, StorePaths, WorkspaceStore};
use quantick_feed::FeedHandle;
use quantick_feed::history_reach;
use quantick_orderflow::LaneWindow;

// Names the window's own code no longer reads: the seven modules above took the
// last production read of each with them. `app::tests` still reaches every one
// through `use super::*`, so they stay here as gated imports rather than
// becoming an edit to `app/tests/` — the same treatment `ChartLayer` already
// had when `maintain_chart_layers` left for `app::chart_layers_wiring`, and
// `heatmap_lamp_on` has now taken the last read of that one to `app::tabs`.
#[cfg(test)]
use crate::chart_layers::ChartLayer;
#[cfg(test)]
use crate::harness::ContextMenuPane;
#[cfg(test)]
use crate::loading::LoadingTask;
#[cfg(test)]
use crate::metrics;
#[cfg(test)]
use crate::pane::{ChartPane, DRAWING_ANCHOR_RADIUS_PX};
#[cfg(test)]
use crate::statusbar;
#[cfg(test)]
use crate::tabstrip::TabAction;
#[cfg(test)]
use crate::theme;
#[cfg(test)]
use crate::toolbar::ToolbarAction;
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

impl QuantickApp {
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
