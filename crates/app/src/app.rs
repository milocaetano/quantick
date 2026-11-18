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

#[cfg(any(feature = "drawing-harness", feature = "quick-range-harness", test))]
use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
use std::time::Instant;

use eframe::egui;

use crate::canvas_layout::PaneIdAllocator;

// Before the modules: both are expanded inside them.
/// The arrangement ports over `$app`'s own fields, borrowed one by one so a
/// caller can lend a sibling field (the replay view) beside them.
macro_rules! arrangement_adapter {
    ($app:expr) => {{
        $app.runtime
            .resolve(&$app.presentation.binding)
            .expect("bound workspace");
        $crate::app::arrangement_adapter::ArrangementAdapter {
            tabs: &mut $app.runtime.tabs,
            config: &$app.config,
            style: &mut $app.runtime.style,
            pane_ids: &mut $app.pane_ids,
            workspace: &mut $app.runtime.workspace,
            indicators: &mut $app.runtime.indicators,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: &$app.chrome.harness,
            toolrail: &mut $app.surfaces.toolrail,
            tz: &mut $app.runtime.tz,
            dock: &mut $app.surfaces.dock,
            show_perf: &mut $app.health.show_perf,
            record_deals: &mut $app.chrome.record_deals,
            history: &mut $app.runtime.history,
            drawing_chrome: &mut $app.surfaces.drawing_chrome,
            toast: &mut $app.surfaces.toast,
            surface: &mut $app.chrome.title_bar.surface,
        }
    }};
}

/// The desk, Settings, REC and workspace owner over `$app`'s own fields: the
/// `DeskHost` and `WorkspaceBookHost` doors' answer, and what the window's own
/// Settings draw and workspace rows act through. `bound` first checks the
/// window shows its workspace, as the desk and Settings always have.
macro_rules! workspace_calls {
    (bound $app:expr) => {
        workspace_calls!($app, $app.runtime.bound_mut(&$app.presentation.binding))
    };
    ($app:expr) => {
        workspace_calls!($app, &mut $app.runtime)
    };
    ($app:expr, $runtime:expr) => {
        $crate::app::workspace_calls::WorkspaceCalls {
            runtime: $runtime,
            presentation: &$app.presentation,
            chrome: &mut $app.chrome,
            pane_ids: &mut $app.pane_ids,
            config: &$app.config,
            control: &mut $app.control,
            surfaces: &mut $app.surfaces,
            health: &mut $app.health,
            drawings: &mut $app.drawings,
        }
    };
}

pub(crate) mod alerts_runtime;
mod arrangement_adapter;
pub(crate) mod canvas_wiring;
mod workspace_calls;
mod workspace_runtime;
use quantick_workspace::lifetime::WorkspaceId;
const WORKSPACE_ID: WorkspaceId = WorkspaceId(0);
pub(crate) mod arrangement_host;
use arrangement_host::ArrangementHost;
mod chart_layers_wiring;
mod chart_settings_wiring;
mod chrome;
pub(crate) mod control_host;
pub(crate) use control_host::{
    ChromePort, ControlWindow, GatewayPort, HealthPort, LayersPort, TabsMutPort, TabsPort,
};
pub(crate) mod deal_recording_wiring;
mod demo_hooks;
pub(crate) mod drawing_controller;

mod frame;
mod frame_tail;
mod health;
mod indicator_manager;
mod indicator_operations;
mod indicator_owner;
mod indicator_wiring;
pub(crate) mod launch_hooks;
mod layout_wiring;
mod script_editor_wiring;
mod script_reload;
pub(crate) use layout_wiring::set_indicator_mouse_vertical_line;
mod home_wiring;
mod menu_bar;
pub(crate) mod native_windows;
pub(crate) mod paper_wiring;
mod replay_and_history;
mod tab_workspaces;
pub(crate) mod title_bar;
mod workspace_bundle_adapter;
mod workspace_restore;
mod workspace_save_adapter;

fn saved_context_intervals(bars: &[String], time_bars: Option<&str>) -> Vec<i64> {
    let default = crate::time_header::DEFAULT_INTERVAL_MS;
    quantick_stores::ui_state::saved_context_intervals(bars, time_bars, default)
}

use crate::config::AppConfig;
#[cfg(any(feature = "scenario-harness", test))]
use crate::harness::Harness;
use crate::surfaces::replay_browser::ReplayBrowserExt as _;
use crate::symbols_file::{self, AddedSymbols};
use crate::tab::Tab;
use crate::ui_state;
use crate::window_scale;
use quantick_feed::FeedHandle;

use quantick_stores::bubble_asset_store::open_asset_bubbles;

use crate::pane::ChartPane;

/// Id of the tab the window opens with.
const FIRST_TAB_ID: u64 = 0;

/// The natives `QUANTICK_INDICATORS_AUTOSTART` opens with: the overlay and
/// the pane, so a scripted run photographs both shapes. Named by catalog id
/// rather than "all of them", because the hook's contract is a fixed,
/// deterministic pair — a native added later must not silently change what
/// every existing capture shows.
#[cfg(any(feature = "scenario-harness", test))]
const AUTOSTART_NATIVES: &[&str] = &["native.ema", "native.cvd"];

/// The quantick chart window.
///
/// One workspace: N open markets ([`Tab`]) and the chrome around them. The
/// chrome is what is single-instance by nature — one menu bar, one toolbox,
/// one dock, one appearance, one status line — plus the indicator persistence
/// layer, which describes a workspace rather than a market.
pub struct QuantickApp {
    /// One process-owned workspace; a window binding never owns its sessions.
    runtime: workspace_runtime::WorkspaceRuntime,
    presentation: workspace_runtime::WindowPresentation,
    chrome: chrome::ChromeState,
    /// Allocated process-wide, independent of presentation placement.
    pane_ids: PaneIdAllocator,
    added_symbols: AddedSymbols,
    config: AppConfig,
    control: control_host::ControlState,
    surfaces: crate::surfaces::Surfaces,
    drawings: drawing_controller::DrawingController,
    audio: replay_and_history::AlertState,
    health: health::HealthCounters,
}

use quantick_chart::indicator_registry::TabSlot;

/// Constructor-only inputs, consumed at the owners' existing launch phases.
/// This value is never retained on the app or used as a frame context.
#[derive(Default)]
pub(crate) struct AppLaunch {
    #[cfg(any(feature = "scenario-harness", test))]
    pub scenario: crate::hooks::ScenarioInputs,
    #[cfg(any(feature = "control-harness", test))]
    pub control: control_host::ControlLaunch,
    #[cfg(any(feature = "scenario-harness", test))]
    pub window: crate::launch::window::WindowStartupState,
    #[cfg(any(feature = "drawing-harness", test))]
    pub toolrail: crate::toolrail::ToolRailLaunch,
    #[cfg(any(feature = "quick-range-harness", test))]
    pub quick_range: crate::surfaces::drawing_chrome::QuickRangeLaunch,
    #[cfg(any(feature = "drawing-harness", test))]
    pub drawing_chrome: crate::surfaces::drawing_chrome::DrawingChromeLaunch,
}

impl QuantickApp {
    pub(crate) fn workspace_bundle_adapter(
        &mut self,
    ) -> workspace_bundle_adapter::WorkspaceBundleAdapter<'_> {
        workspace_bundle_adapter::WorkspaceBundleAdapter {
            arrangement: arrangement_adapter!(self),
            alerts: &mut self.runtime.alerts.core,
            replay_view: &self.surfaces.replay_browser,
            dialogs: &mut self.surfaces.layout_dialogs,
            added_symbols: &mut self.added_symbols,
            drawing_presets: &mut self.drawings.presets,
            footprint_config: &mut self.runtime.footprint_config,
            chart_settings: &mut self.surfaces.chart_settings.model_state,
            indicator_settings: &mut self.surfaces.indicator_settings,
        }
    }
    pub(crate) fn arrangement_adapter(&mut self) -> arrangement_adapter::ArrangementAdapter<'_> {
        arrangement_adapter!(self)
    }
    fn layout_adapter(&mut self) -> layout_wiring::LayoutAdapter<'_> {
        self.indicator_wiring().into_layout()
    }
    fn indicator_wiring(&mut self) -> indicator_wiring::IndicatorWiring<'_> {
        indicator_wiring::IndicatorWiring {
            runtime: self.runtime.bound_mut(&self.presentation.binding),
            surfaces: &mut self.surfaces,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: &mut self.chrome.harness,
        }
    }
    pub(crate) fn workspace_save_adapter(
        &mut self,
    ) -> workspace_save_adapter::WorkspaceSaveAdapter<'_> {
        arrangement_adapter!(self).into_save(&self.surfaces.replay_browser)
    }

    pub(crate) fn symbol_catalog(&mut self) -> quantick_stores::symbol_catalog::SymbolCatalog<'_> {
        quantick_stores::symbol_catalog::SymbolCatalog {
            config: &mut self.config,
            added: &mut self.added_symbols,
            path: self.runtime.workspace.symbols_path(),
        }
    }

    /// The active tab beside the config it reads.
    ///
    /// Split here, once, because almost every tab operation needs both and
    /// `self.runtime.tabs[i].f(&self.config)` is a borrow error at every call site.
    pub(super) fn active_with_config(&mut self) -> (&mut Tab, &AppConfig) {
        (self.runtime.active_tab_mut(), &self.config)
    }

    /// The tab on screen.
    pub(super) fn active_tab(&self) -> &Tab {
        self.runtime.bound(&self.presentation.binding).active_tab()
    }

    /// See [`Self::active_tab`].
    pub(super) fn active_tab_mut(&mut self) -> &mut Tab {
        self.runtime
            .bound_mut(&self.presentation.binding)
            .active_tab_mut()
    }

    /// The pane the chrome speaks for: the active tab's focused pane (§11).
    pub(super) fn focused_pane(&self) -> &ChartPane {
        self.active_tab().focused_pane()
    }

    /// See [`Self::focused_pane`].
    pub(super) fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().focused_pane_mut()
    }

    /// The pane every drawing surface speaks for: the one holding the
    /// selection, which is the focused pane unless a shared mark was taken
    /// from the chart it is mirrored on (see [`Tab::drawing_side`]).
    ///
    /// The inspector, the keyboard, the object manager and the toast all read
    /// through here, so an object selected on either of its two charts is
    /// edited and deleted from either of them.
    #[cfg(test)]
    pub(super) fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.active_tab_mut().drawing_pane_mut()
    }

    /// Launch scenarios run the label and notification handlers as a trusted
    /// local agent: traced on a replay, never through remote-grant admission.
    #[cfg(any(feature = "control-harness", test))]
    pub(super) fn apply_control_annotate_hooks(&mut self) {
        let remote = crate::control::ActionOrigin::Remote;
        if self.control.scenarios.has_annotation() {
            let pane = self.active_tab().drawing_pane();
            let slot = pane.slots().saturating_sub(1);
            let anchor = match (pane.slot_open_time(slot), pane.closed_bar(slot)) {
                (Some(time), Some(bar)) => Some(serde_json::json!({
                    "time_unix_ms": time,
                    "price": rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap_or(1.0).to_string(),
                })),
                _ => None,
            };
            if let Some(input) = self.control.scenarios.annotation(anchor) {
                self.run_hook_action("annotate.label.create", input, remote);
            }
        }
        match self.control.scenarios.notification() {
            Some(control_host::NotificationStep::Ready { capability, input }) => {
                self.run_hook_action(capability, input, remote)
            }
            Some(control_host::NotificationStep::Refused { channel }) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_NOTIFY_HOOK_REFUSED",
                channel = %channel,
                "QUANTICK_CONTROL_NOTIFY names no notification channel"
            ),
            None => {}
        }
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
        spec: impl Into<crate::state::BarConfiguration>,
        feed: FeedHandle,
        workspace: ui_state::Workspace,
        #[cfg_attr(
            not(any(
                feature = "scenario-harness",
                feature = "control-harness",
                feature = "drawing-harness",
                feature = "quick-range-harness",
                test
            )),
            allow(unused_variables)
        )]
        launch: AppLaunch,
    ) -> Self {
        let state_path = crate::paper_state::default_path();
        // Read before `config` is moved into the struct below: this seeds the
        // window's own copy of the span, which the trader then edits.
        let reach_span_minutes = config.history.reach_span_minutes;
        let paper_state = crate::paper_state::load(&state_path);
        let (trades_dir, consolidated) = crate::paper_home::startup_home(
            config.paper.trades_dir.as_deref(),
            paper_state.trades_dir.as_deref(),
            &state_path,
        );
        let mut pane_ids = PaneIdAllocator::new();
        let mut tab = Tab::new(
            pane_ids.alloc(),
            feed_id.into(),
            symbol.into(),
            spec,
            feed,
            trades_dir.clone(),
        );
        // The ticket settings the trader left, unless a launch hook outranks them.
        tab.paper.restore_settings(&paper_state);
        crate::paper_state::restore_money(tab.paper.account_mut(), &paper_state);
        let mut runtime =
            workspace_runtime::WorkspaceRuntime::new(tab, trades_dir, reach_span_minutes, &config);
        let added_symbols = symbols_file::load(runtime.workspace.symbols_path());
        let presentation = workspace_runtime::WindowPresentation::new(&mut runtime);
        let mut app = Self {
            runtime,
            presentation,
            chrome: chrome::ChromeState {
                title_bar: Default::default(),
                #[cfg(any(feature = "scenario-harness", test))]
                harness: Harness::capture(&launch.scenario),
                #[cfg(any(feature = "scenario-harness", test))]
                window_startup: launch.window,
                record_deals: None,
                add_menu: Default::default(),
                capsule: Default::default(),
                toolbar_controls: Vec::new(),
                surface: None,
                window_menu: Default::default(),
                history_menu_rect: None,
                feed_chip_rect: None,
                // The hook stands in for a click on the opening tab's chip, which
                // is the first tab there is.
            },
            pane_ids,
            added_symbols,
            config,
            control: control_host::ControlState {
                control_access: Some(crate::control::ControlAccess::new()),
                #[cfg(any(feature = "control-harness", test))]
                scenarios: Default::default(),
            },
            surfaces: crate::surfaces::Surfaces::default(),
            drawings: drawing_controller::DrawingController::new(),
            audio: replay_and_history::AlertState {
                alerts: Box::new(crate::audio::Speaker::default()),
                alert_failure: None,
            },
            health: health::HealthCounters::new(),
        };
        app.surfaces.replay_browser.initialize(
            workspace.replay_folder.as_deref(),
            workspace.replay_day_before,
        );
        crate::surfaces::feed_notice::initialize(&mut app.surfaces.feed_notice, FIRST_TAB_ID);
        // Recording is not a display choice: it starts with the feed, so
        // hiding the map later never leaves a hole in what was captured.
        let config = app.config.clone();
        app.active_tab_mut().refresh_chip_label(&config);
        app.active_tab_mut().ensure_book_capture(&config);
        let assets = open_asset_bubbles(app.runtime.workspace.bubble_assets(), &config);
        app.active_tab_mut().bind_asset_bubbles(&config, &assets);
        // A feed read by timeframe can open straight on the timeframe chart.
        app.active_tab_mut().apply_feed_declared_layout(&config);
        // The code's own baseline, and nothing more: what a launch actually
        // opens with is `config/chart-layers.toml`, applied by
        // `LayerWiring::restore` immediately below and shipping the map on.
        // This line is what remains if that config is ever unreadable — a
        // layer nobody requested costing no projection. Capture is already
        // running either way, so it is a display choice and nothing else.
        app.active_tab_mut().tape_mut().set_depth_visible(false);
        // What the user last had on the canvas, applied over those defaults and
        // under the autostart hooks below: an env var is an explicit request
        // for this run and must still win (see `LayerWiring::restore`).
        app.layer_wiring().restore();
        // And the workspace itself — the tab strip, each tab's canvas, and the
        // chrome around them. After the config defaults (a saved cockpit is
        // the user's own answer to what a feed declares) and before the
        // autostart hooks, which are explicit requests for this one run.
        app.arrangement_adapter().restore_workspace(workspace);
        // Install staged drawing inputs before applying the remaining legacy
        // hooks and the captured rail inputs. `launch_hooks` owns their
        // construction-phase order after workspace restoration.
        #[cfg(any(feature = "quick-range-harness", test))]
        app.surfaces
            .drawing_chrome
            .quick_range()
            .queue_launch(launch.quick_range);
        #[cfg(any(feature = "drawing-harness", test))]
        app.surfaces
            .drawing_chrome
            .queue_launch(launch.drawing_chrome);
        launch_hooks::apply_launch_phase(
            &mut app,
            #[cfg(any(feature = "scenario-harness", test))]
            &launch.scenario,
            #[cfg(any(feature = "control-harness", test))]
            launch.control,
            #[cfg(any(feature = "drawing-harness", test))]
            launch.toolrail,
        );
        if let Some(notice) = crate::store_home::rescue_notice() {
            app.runtime.tabs.runtime_mut(0).paper.show_toast(notice);
        }
        if let Some(summary) = consolidated
            && summary.imported() > 0
        {
            // A silent rescue would look like the app moved files on its
            // own; the toast says what happened and that copies were made.
            app.runtime
                .tabs
                .runtime_mut(0)
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
        self.presentation.native.pump.attach(handle);
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
        self.presentation.native.pump.set_active(false);
        if !self.runtime.lifetime.begin_shutdown() {
            return;
        }
        // Whatever the debounce was still holding: a level drawn a moment
        // before closing is a level the trader expects back.
        self.layout_adapter().flush_layouts();
        let _ = self.runtime.alerts.core.flush();
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
    #[cfg(any(feature = "scenario-harness", test))]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let chart_layers = self.active_tab().flow_pane.chart_layers_menu_center();
        if self
            .chrome
            .harness
            .push_context_menu_followup(raw_input, chart_layers)
        {
            return;
        }
        // The menu bar's own button, clicked. A menu is a popup egui owns, so
        // there is no state to set that would not be a second way of opening
        // it; the hook supplies the press, and every line after it is what a
        // trader's click runs. The rect is published by the draw, so the
        // first frame has none — wait for it rather than guess.
        if title_bar::push_scripted_menu(self, ctx, raw_input) {
            return;
        }
        // The parked pointer, re-delivered — before the menu branch and not
        // inside its `else`, because a menu whose position never resolves
        // returns early for ever. `QUANTICK_CONTEXT_MENU=tape` on a canvas
        // with no lane is exactly that, and it used to take the pointer hook
        // down with it: the capture showed no compass, no crosshair and no
        // hover readout at all, and read as "the compass does not draw"
        // rather than "the menu never opened".
        let harness = &mut self.chrome.harness;
        harness.push_scripted_input(&mut self.runtime.tabs, raw_input);
        let Some(pane) = self.chrome.harness.context_menu() else {
            return;
        };
        // The divider is published by the draw, so the first frame has none:
        // wait for it rather than guess where the tape is.
        let Some(position) = pane.scripted_position(&self.active_tab().flow_pane) else {
            return;
        };
        self.chrome.harness.context_menu_pressed(position);
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
