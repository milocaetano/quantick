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
mod chrome;
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
use crate::indicator_worker::SlotId;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::preset_file;
use crate::indicators::state_file;
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
    /// The window chrome's transient state — see [`chrome::ChromeState`].
    chrome: chrome::ChromeState,
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

    /// The observer gateway and its launch hooks — see [`control_host::ControlState`].
    control: control_host::ControlState,

    /// The indicator persistence layer — see [`indicator_manager::IndicatorState`].
    indicators: indicator_manager::IndicatorState,

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
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    /// Where a pane's layer menu leaves the grid switch and the "an indicator
    /// was hidden" flag; drained right after the canvas is drawn.
    layer_actions: chart_layers::LayerActions,
    /// The footprint layer's signal tunables — resolved at boot (env >
    /// `config/footprint.toml` preset > saved edits > defaults), edited live
    /// by the layer menu's controls.
    footprint_config: crate::footprint_config::FootprintConfig,
    /// Where a signal alarm is played — see [`replay_and_history::AlertState`].
    alerts: replay_and_history::AlertState,

    /// The chart appearance every renderer reads. The window that edits it
    /// is `surfaces::style_panel`, which hands back a copy rather than
    /// holding a reference to this one.
    style: ChartStyle,
    style_revision: u64,
    /// What the window measures about itself — see [`health::HealthCounters`].
    health: health::HealthCounters,
    /// The window-wide history reach — see [`tabs::HistorySettings`].
    history: tabs::HistorySettings,

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
            chrome: chrome::ChromeState {
                layout_picker_open: false,
                layout_rename: None,
                layout_delete_confirm: None,
                inspector_position_dirty: false,
                surface: None,
                workspace_menu_rect: None,
                history_menu_rect: None,
                feed_chip_rect: None,
                // The hook stands in for a click on the opening tab's chip, which
                // is the first tab there is.
                feed_popup_tab: feed_notice::popup_open_from_env().then_some(FIRST_TAB_ID),
                window_size: None,
            },
            pane_ids,
            added_symbols: symbols_file::load(&symbols_path),
            config,
            control: control_host::ControlState {
                control_access: Some(crate::control::ControlAccess::new()),
                pending_control_access_enable: false,
                pending_control_annotation: None,
                pending_control_notification: None,
                pending_control_evidence: None,
                pending_control_mark: None,
            },
            indicators: indicator_manager::IndicatorState {
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
                last_script_poll: Instant::now(),
                operator_slots: std::collections::BTreeSet::new(),
                indicator_presets: preset_file::PresetStore::load(&indicator_presets_path),
            },
            replay_view: ReplayView::new(
                workspace.replay_folder.as_deref(),
                workspace.replay_day_before,
            ),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            surfaces: crate::surfaces::Surfaces::default(),
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            layer_actions: chart_layers::LayerActions::default(),
            footprint_config: crate::footprint_config::load(&footprint_settings_path),
            alerts: replay_and_history::AlertState {
                alerts: Box::new(crate::audio::Speaker::default()),
                alert_failure: None,
            },
            style: ChartStyle::default(),
            style_revision: 0,
            health: health::HealthCounters {
                show_perf: true,
                frames: FrameStats::new(120),
                cpu_frames: FrameStats::new(120),
                last_frame: None,
                trades_since_summary: 0,
                last_summary: Instant::now(),
            },
            history: tabs::HistorySettings {
                progressive_history: true,
                history_reach: history_reach::HistoryReach::default(),
                history_reach_span_minutes: reach_span_minutes,
                venue_lead_in: false,
            },
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
        self.chrome.surface = window_scale::SurfaceProbe::new(handle);
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(cpu) = frame.info().cpu_usage {
            self.health.cpu_frames.record(cpu * 1000.0);
        }
        self.draw_frame(ctx, Instant::now());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Whatever the debounce was still holding: a level drawn a moment
        // before closing is a level the trader expects back.
        self.flush_layouts();
        if let Some(access) = self.control.control_access.as_mut() {
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
                ScriptedMenu::Workspace => self.chrome.workspace_menu_rect,
                ScriptedMenu::History => self.chrome.history_menu_rect,
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
