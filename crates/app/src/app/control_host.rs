//! The control plane's port onto the window, split by capability family, and
//! the gateway's seat on the window.
//!
//! The gateway, every registered handler and every projector depend on these
//! traits, never on [`super::QuantickApp`]. A handler or projector is generic
//! over the narrowest families it uses — `fn handler<P: TabsPort + ?Sized>`
//! — and is registered instantiated at [`ControlWindow`], so the compiler refuses a handler that reaches a family
//! its signature does not name, and a test drives it with a fake window that
//! implements only those families.
//!
//! The families return views over roots the window owns (`Tab`, the style,
//! the dock), so they live here in `app` rather than in `quantick-control-host`,
//! which cannot name app types. The chrome family also speaks egui geometry.
//! The `QuantickApp` adapters live beside their families in `crate::control`;
//! narrow constructors on the app split the roots each adapter reads. Last
//! comes the gateway's seat — the
//! access it runs in between calls and the `QUANTICK_CONTROL_*` launch hooks
//! that configure it; the scenarios those hooks ask for are headless state in
//! `quantick_control_host::launch`.
//!
//! The shared views stay here because the fake window and every family use
//! the same split-borrow vocabulary.

use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
use eframe::egui;
use quantick_control_handlers::drawing_magnet::RailMagnetPort;
use quantick_control_handlers::indicator_host::IndicatorControlHost;
use quantick_control_handlers::market_tab;
use quantick_control_handlers::workspace_book::cell_menu::CellRow;

use crate::config::AppConfig;
use crate::drawings::{self, DrawingTool, NewDrawing};
use crate::pane::{ChartPane, PaneSide};
use crate::style::ChartStyle;
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;
use quantick_ui_model::dock::State as Dock;

use quantick_feed::history_reach;

use super::QuantickApp;
use super::arrangement_host::ArrangementHost;
use super::workspace_calls::WorkspaceCalls;
use super::workspace_runtime::WorkspaceRuntime;

pub(crate) struct ControlReads<'a> {
    pub(crate) runtime: &'a WorkspaceRuntime,
    pub(crate) presentation: &'a super::workspace_runtime::WindowPresentation,
    pub(crate) chrome: &'a super::chrome::ChromeState,
    pub(crate) added_symbols: &'a crate::symbols_file::AddedSymbols,
    pub(crate) config: &'a AppConfig,
    pub(crate) surfaces: &'a crate::surfaces::Surfaces,
    pub(crate) drawings: &'a super::drawing_controller::DrawingController,
    pub(crate) health: &'a super::health::HealthCounters,
}

pub(crate) struct ControlRoots<'a> {
    pub(crate) runtime: &'a mut WorkspaceRuntime,
    pub(crate) presentation: &'a mut super::workspace_runtime::WindowPresentation,
    pub(crate) chrome: &'a mut super::chrome::ChromeState,
    pub(crate) pane_ids: &'a mut crate::canvas_layout::PaneIdAllocator,
    pub(crate) added_symbols: &'a mut crate::symbols_file::AddedSymbols,
    pub(crate) config: &'a mut AppConfig,
    pub(crate) control: &'a mut ControlState,
    pub(crate) surfaces: &'a mut crate::surfaces::Surfaces,
    pub(crate) drawings: &'a mut super::drawing_controller::DrawingController,
    pub(crate) audio: &'a mut super::replay_and_history::AlertState,
    pub(crate) health: &'a mut super::health::HealthCounters,
}

pub(crate) fn control_roots(app: &QuantickApp) -> ControlReads<'_> {
    ControlReads {
        runtime: &app.runtime,
        presentation: &app.presentation,
        chrome: &app.chrome,
        added_symbols: &app.added_symbols,
        config: &app.config,
        surfaces: &app.surfaces,
        drawings: &app.drawings,
        health: &app.health,
    }
}

pub(crate) fn control_roots_mut(app: &mut QuantickApp) -> ControlRoots<'_> {
    ControlRoots {
        runtime: &mut app.runtime,
        presentation: &mut app.presentation,
        chrome: &mut app.chrome,
        pane_ids: &mut app.pane_ids,
        added_symbols: &mut app.added_symbols,
        config: &mut app.config,
        control: &mut app.control,
        surfaces: &mut app.surfaces,
        drawings: &mut app.drawings,
        audio: &mut app.audio,
        health: &mut app.health,
    }
}

impl<'a> ControlRoots<'a> {
    pub(crate) fn workspace_calls(self) -> WorkspaceCalls<'a> {
        WorkspaceCalls {
            runtime: self.runtime,
            presentation: self.presentation,
            chrome: self.chrome,
            pane_ids: self.pane_ids,
            config: self.config,
            control: self.control,
            surfaces: self.surfaces,
            health: self.health,
            drawings: self.drawings,
        }
    }

    pub(crate) fn bound_workspace_calls(self) -> WorkspaceCalls<'a> {
        let runtime = self.runtime.bound_mut(&self.presentation.binding);
        WorkspaceCalls {
            runtime,
            presentation: self.presentation,
            chrome: self.chrome,
            pane_ids: self.pane_ids,
            config: self.config,
            control: self.control,
            surfaces: self.surfaces,
            health: self.health,
            drawings: self.drawings,
        }
    }
}
use quantick_control_handlers::health::FrameReading;

/// The window as the gateway holds it: every family at once, `'static`
/// because the projection registry is keyed on the type it reads.
pub(crate) type ControlWindow = dyn ControlPort;

/// Every family together — what the gateway itself needs to dispatch any
/// capability. Handlers never take this; they name their families.
pub(crate) trait ControlPort:
    quantick_control_handlers::appearance::AppearancePort
    + TabsPort
    + TabsMutPort
    + ChromePort
    + HealthPort
    + quantick_control_handlers::notify::AttentionHost
    + quantick_control_handlers::alerts::AlertDefinitionsPort
    + LayersPort
    + IndicatorControlHost
    + RailMagnetPort
    + GatewayPort
{
}

impl<T> ControlPort for T where
    T: quantick_control_handlers::appearance::AppearancePort
        + TabsPort
        + TabsMutPort
        + ChromePort
        + HealthPort
        + quantick_control_handlers::notify::AttentionHost
        + quantick_control_handlers::alerts::AlertDefinitionsPort
        + LayersPort
        + IndicatorControlHost
        + RailMagnetPort
        + GatewayPort
        + ?Sized
{
}

/// Tabs and panes, read: the markets, their config and drawing defaults.
pub(crate) trait TabsPort {
    fn appearance(&self) -> &quantick_appearance::ResolvedTheme {
        self.tab_reads().appearance
    }
    fn tab_reads(&self) -> TabReads<'_>;
}

/// The window's tab directory, which every tab-scoped family resolves by.
impl quantick_control_handlers::TabDirectory for dyn ControlPort {
    fn active_tab_index(&self) -> usize {
        self.tab_reads().tabs().active_index()
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tab_reads().tabs().position(tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tab_reads().tabs().id_at(index)
    }
}

/// Tabs and panes, changed: the tab or pane an action targets.
pub(crate) trait TabsMutPort {
    fn describe_owners(&mut self) -> (&mut crate::surfaces::Surfaces, &mut ArrangementHost);
    fn tabs_mut(&mut self) -> TabsMut<'_>;
}

/// Chrome and layout: what the window drew around the tab on screen.
pub(crate) trait ChromePort:
    quantick_control_handlers::native_window::NativeWindowHost
    + quantick_control_handlers::home_tab::HomeTabHost
    + quantick_control_handlers::market_palette::MarketPaletteOwners
    + quantick_control_handlers::widgets::WidgetHost
    + quantick_control_handlers::desk::DeskHost
    + quantick_control_handlers::workspace_book::WorkspaceBookHost
    + quantick_control_handlers::canvas::CanvasHost
{
    fn chrome_reads(&self) -> ChromeReads<'_>;
}

/// Health and history: what the window measures, how far it reaches back,
/// and the session flags an operator reads back after setting them.
pub(crate) trait HealthPort {
    fn health_reads(&self) -> HealthReads<'_>;
}

/// The chart-layer owner.
pub(crate) trait LayersPort {
    fn layer_wiring(&mut self) -> super::chart_layers_wiring::LayerWiring<'_>;
}

/// The gateway's own seat on the window, and the doors every in-window
/// caller (the hotkey, the launch hooks, the tests) invokes an action by.
pub(crate) trait GatewayPort {
    /// Where the gateway lives between calls: taken out for the length of
    /// one action, so the action can borrow the window.
    fn gateway_slot(&mut self) -> &mut Option<crate::control::ControlAccess>;
    /// This window as the object the gateway dispatches over.
    fn as_window(&mut self) -> &mut ControlWindow;
    /// The workspace this window shows, to read: its layouts and its book.
    fn workspace(&self) -> &WorkspaceRuntime;
    /// The desk, book, layout, paper and route owners, lent for one act.
    fn workspace_calls(&mut self) -> WorkspaceCalls<'_>;

    /// Invoke one registered control action from inside the application,
    /// attributed to the human at this window (or to automation when a
    /// control trace replays it). The hotkey, the `QUANTICK_CONTROL_MARK`
    /// hook and the tests all arrive here; there is no second path.
    fn control_action(
        &mut self,
        capability_id: &str,
        capability_version: u32,
        origin: crate::control::ActionOrigin,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.gateway_slot().take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        let outcome = access.invoke_local_action(
            self.as_window(),
            capability_id,
            capability_version,
            input,
            origin,
        );
        *self.gateway_slot() = Some(access);
        outcome
    }

    /// Invoke one registered action as an *agent* would, from inside this
    /// window, so what it places carries a real assistant's attribution
    /// without a client on the socket; `origin` says who the caller is.
    #[cfg(any(feature = "control-harness", feature = "scenario-harness", test))]
    fn agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
        origin: fn(Box<quantick_control::wire::ActorContext>) -> crate::control::ActionOrigin,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        let Some(mut access) = self.gateway_slot().take() else {
            return Err(quantick_control::error::ControlError::invalid_request(
                "control access is not installed",
            ));
        };
        // No identity, no actor to sign with: the same structured refusal an
        // action gets, rather than a panic on the first frame.
        let Some(actor) = access.hook_agent_actor() else {
            *self.gateway_slot() = Some(access);
            return Err(quantick_control::error::ControlError::invalid_request(
                "this window has no control identity to act with",
            ));
        };
        let origin = origin(Box::new(actor));
        let outcome = access.invoke_local_action(self.as_window(), capability_id, 1, input, origin);
        *self.gateway_slot() = Some(access);
        outcome
    }

    /// A hook's action as an agent, `origin` saying which kind of hook, its
    /// failure reported where a scripted run sees it: nothing else would.
    #[cfg(any(feature = "control-harness", feature = "scenario-harness", test))]
    fn run_hook_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
        origin: fn(Box<quantick_control::wire::ActorContext>) -> crate::control::ActionOrigin,
    ) {
        if let Err(error) = self.agent_action(capability_id, input, origin) {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_HOOK_ACTION_FAILED",
                capability = capability_id,
                error_code = %error.code,
                error = %error.message,
                "a hook could not run its action"
            );
        }
    }

    /// The mark hotkey's body: `attention.mark.create` with the resolved
    /// cursor target, attributed to the human.
    fn take_mark(&mut self, note: Option<String>) {
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
}

/// The tabs-and-panes read view: every accessor takes the view by value and
/// hands back the root's own borrow, so a projection keeps what it read for
/// as long as it holds the window.
#[derive(Clone, Copy)]
pub(crate) struct TabReads<'a> {
    tabs: &'a ArrangementHost,
    config: &'a AppConfig,
    footprint_config: &'a crate::footprint_config::FootprintConfig,
    presets: &'a drawings::presets::PresetStore,
    appearance: &'a quantick_appearance::ResolvedTheme,
}

impl<'a> TabReads<'a> {
    pub(crate) fn with_appearance(
        mut self,
        appearance: &'a quantick_appearance::ResolvedTheme,
    ) -> Self {
        self.appearance = appearance;
        self
    }
    /// The view over the window's own roots. The window's adapter builds it,
    /// and so does a test's fake window over roots it owns.
    pub(crate) fn new(
        tabs: &'a ArrangementHost,
        config: &'a AppConfig,
        footprint_config: &'a crate::footprint_config::FootprintConfig,
        presets: &'a drawings::presets::PresetStore,
    ) -> Self {
        Self {
            tabs,
            config,
            footprint_config,
            presets,
            appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
        }
    }

    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn tab_at(self, index: usize) -> Option<&'a Tab> {
        self.tabs.get(index)
    }

    pub(crate) fn tabs(self) -> &'a ArrangementHost {
        self.tabs
    }

    pub(crate) fn active_tab_index(self) -> usize {
        self.tabs.active_index()
    }

    /// What a freshly placed object of `tool` opens with on the pane at `side`
    /// of the tab at `index`, through the same door the click path uses.
    pub(crate) fn new_drawing(self, index: usize, side: PaneSide, tool: DrawingTool) -> NewDrawing {
        let surface = self.tabs[index].pane(side).surface();
        drawings::new_drawing_from_defaults(self.presets, surface, tool)
    }

    pub(crate) fn config(self) -> &'a AppConfig {
        self.config
    }

    /// The window's footprint setup — the one a pane falls back to when it
    /// carries no override of its own.
    pub(crate) fn footprint_config(self) -> &'a crate::footprint_config::FootprintConfig {
        self.footprint_config
    }
}

/// The tabs-and-panes write view, for the cockpit tier: the tab or pane an
/// action changes. Each accessor consumes the view and hands back one borrow,
/// so an action names its target once.
pub(crate) struct TabsMut<'a> {
    tabs: &'a mut ArrangementHost,
    config: &'a AppConfig,
    /// The window's main-click default, which every press sets.
    default_reach: &'a mut history_reach::HistoryReach,
}

impl<'a> TabsMut<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(crate) fn new(
        tabs: &'a mut ArrangementHost,
        config: &'a AppConfig,
        default_reach: &'a mut history_reach::HistoryReach,
    ) -> Self {
        Self {
            tabs,
            config,
            default_reach,
        }
    }

    /// Press History on one tab: the one press the toolbar and
    /// `feed.history.load` both make ([`market_tab::run_history`]).
    pub(crate) fn press_history(self, index: usize, reach: history_reach::HistoryReach) {
        if let Some(lent) = self.lend_tab(index) {
            market_tab::run_history(lent, quantick_session::HistoryAct::Press(reach));
        }
    }

    /// One tab beside the feed table and History default a call lends it.
    pub(crate) fn lend_tab(self, index: usize) -> Option<market_tab::LentTab<'a>> {
        Some(market_tab::LentTab {
            tab: self.tabs.get_mut(index)?,
            config: self.config,
            history_default: self.default_reach,
        })
    }

    /// Fetch one more span of older venue candles on the active tab.
    pub(crate) fn load_older_candles(self) {
        let (active, tab_id) = (self.tabs.active_index(), self.tabs.active_id());
        let act = quantick_session::Act::LoadOlderCandles { tab_id };
        if let Some(tab) = self.tabs.get_mut(active) {
            tab.run_session(Some(self.config), act);
        }
    }

    /// One tab beside the folds its order-flow panel reads, by position.
    pub(crate) fn tab_with_panel_folds(
        self,
        index: usize,
    ) -> Option<(&'a mut Tab, &'a mut crate::ui_state::AssetFolds)> {
        self.tabs.with_panel_folds(index)
    }

    /// The mutable twin of [`TabReads::tab_at`].
    pub(crate) fn tab_at_mut(self, index: usize) -> Option<&'a mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One pane, by tab position and side — the mutable half of
    /// [`TabReads::tabs`], for the actions that place objects.
    pub(crate) fn pane_mut(
        self,
        tab_index: usize,
        side: crate::pane::PaneSide,
    ) -> &'a mut ChartPane {
        self.tabs.runtime_mut(tab_index).pane_mut(side)
    }

    /// File what an action changed for each tab's asset now, so the next
    /// read reports it as the store holds it ([`Tab::sync_asset_bubbles`]).
    pub(crate) fn file_asset_bubbles(self) {
        self.tabs.iter_mut().for_each(Tab::sync_asset_bubbles);
    }
}

/// The chrome-and-layout read view: what the window drew around the tab on
/// screen, read as values where the frame published them.
#[derive(Clone, Copy)]
pub(crate) struct ChromeReads<'a> {
    pub(crate) alerts_open: bool,
    tabs: &'a ArrangementHost,
    style: &'a ChartStyle,
    tool_rail: &'a ToolRail,
    dock: &'a Dock,
    tz: TzOffset,
    drawing_chrome: &'a crate::surfaces::drawing_chrome::DrawingChromeSurface,
    drawn: ChromeDrawn,
    pub(crate) title_bar: Option<&'a super::title_bar::TitleBarState>,
    pub(crate) window_menu: Option<&'a super::menu_bar::WindowMenuState>,
    pub(crate) settings: Option<quantick_control_schema::settings_sections::SettingsSection>,
    /// The toolbar's list door, capsule and layout popover, as drawn.
    pub(crate) toolbar: &'a [crate::toolbar::DrawnControl],
    /// The Trading panel's route block, while broker routes are offered.
    pub(crate) route: Option<&'a quantick_paper::route::Sheet>,
}

/// What the last frame published about the window chrome.
#[derive(Clone, Copy, Default)]
pub(crate) struct ChromeDrawn {
    /// Where the feed's offline chip and the tape cell's Reconnect were painted.
    pub(crate) feed_drawn: quantick_ui_model::feed_notice::Drawn<egui::Rect>,
    /// The tab whose chip opened the feed's recovery popup, if any.
    pub(crate) feed_popup_tab: Option<u64>,
    /// Whether a recording opens with the session day before it joined.
    pub(crate) replay_day_before: bool,
}

impl<'a> ChromeReads<'a> {
    pub(crate) fn title_bar(self) -> Option<&'a super::title_bar::TitleBarState> {
        self.title_bar
    }

    /// The section Settings is showing, while it is up.
    pub(crate) fn settings_section(
        self,
    ) -> Option<quantick_control_schema::settings_sections::SettingsSection> {
        self.settings
    }

    /// The workspace cell menu's rows in view, as last drawn.
    pub(crate) fn cell_menu(self) -> impl Iterator<Item = &'a (CellRow, egui::Rect)> {
        (self.window_menu.into_iter())
            .flat_map(|menu| menu.cell_rows.iter().chain(&menu.dialog_rows))
    }

    /// The view over the window's own roots, as [`TabReads::new`].
    pub(crate) fn new(
        tabs: &'a ArrangementHost,
        style: &'a ChartStyle,
        tool_rail: &'a ToolRail,
        dock: &'a Dock,
        tz: TzOffset,
        drawing_chrome: &'a crate::surfaces::drawing_chrome::DrawingChromeSurface,
        drawn: ChromeDrawn,
    ) -> Self {
        Self {
            tabs,
            style,
            tool_rail,
            dock,
            tz,
            drawing_chrome,
            drawn,
            alerts_open: false,
            title_bar: None,
            window_menu: None,
            settings: None,
            toolbar: &[],
            route: None,
        }
    }

    /// The window's shared chart style, which owns the layers no pane does.
    pub(crate) fn style(self) -> &'a ChartStyle {
        self.style
    }

    /// The drawing tool rail: which tool is armed, and whether it is shown.
    pub(crate) fn tool_rail(self) -> &'a ToolRail {
        self.tool_rail
    }

    /// Where the feed's offline chip and the tape cell's Reconnect were painted.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn feed_drawn(self) -> quantick_ui_model::feed_notice::Drawn<egui::Rect> {
        self.drawn.feed_drawn
    }

    /// The word the status bar is painting about the active tab's feed.
    pub(crate) fn feed_word(self) -> Option<&'static str> {
        let tab = &self.tabs[self.tabs.active_index()];
        tab.connection
            .status_word(crate::metrics::wall_clock_ms())
            .map(|word| word.word)
    }

    /// Whether a one-click Reconnect stands outside the popup; see
    /// `quantick_control_handlers::connection::reconnect_offered`.
    pub(crate) fn feed_reconnect_offered(self) -> bool {
        let tab = &self.tabs[self.tabs.active_index()];
        let link = tab.connection.state().link;
        quantick_control_handlers::connection::reconnect_offered(link, tab.paper.trade_gate())
    }

    /// Whether the recovery popup that chip opens is showing, on the chart
    /// the trader is looking at.
    pub(crate) fn feed_popup_open(self) -> bool {
        self.drawn.feed_popup_tab == Some(self.tabs.active_id())
    }

    /// The right-hand dock: whether it is shown, and which tab is open.
    pub(crate) fn dock(self) -> &'a Dock {
        self.dock
    }

    pub(crate) fn timezone(self) -> TzOffset {
        self.tz
    }

    /// Whether a recording opens with the session day before it joined in
    /// front, and a download fetches that day's tape too — a choice an
    /// operator must be able to read back after setting it.
    pub(crate) fn replay_day_before(self) -> bool {
        self.drawn.replay_day_before
    }

    /// All drawing actions in the temporary range's visible action bar.
    pub(crate) fn quick_range_actions(
        self,
    ) -> Option<
        [crate::surfaces::drawing_chrome::QuickRangeControl;
            crate::surfaces::drawing_chrome::QUICK_RANGE_ACTION_COUNT],
    > {
        self.drawing_chrome
            .quick_range_read()
            .controls(self.tabs.id_at(self.tabs.active_index()))
    }
}

/// The health-and-history read view.
#[derive(Clone, Copy)]
pub(crate) struct HealthReads<'a> {
    save_on_exit: bool,
    health: &'a super::health::HealthCounters,
    history: &'a quantick_session::HistorySettings,
}

impl<'a> HealthReads<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(crate) fn new(
        save_on_exit: bool,
        health: &'a super::health::HealthCounters,
        history: &'a quantick_session::HistorySettings,
    ) -> Self {
        Self {
            save_on_exit,
            health,
            history,
        }
    }

    /// Save-on-exit, the performance overlay, progressive history.
    pub(crate) fn workspace_flags(self) -> (bool, bool, bool) {
        (
            self.save_on_exit,
            self.health.show_perf,
            self.history.progressive_history,
        )
    }

    /// What the `by time` reach's span is now, for an operator reading back
    /// what it set.
    pub(crate) fn history_reach_span_minutes(self) -> u32 {
        self.history.history_reach_span_minutes
    }

    /// How far the window's *load older* press reaches, and whether a chart
    /// cut by trades carries the venue's candles.
    ///
    /// Both are choices an operator without a mouse has to be able to read
    /// back after setting them — the reach especially, since it decides
    /// whether one press is one request or a run of them.
    pub(crate) fn history_settings(self) -> (history_reach::HistoryReach, bool) {
        (self.history.history_reach, self.history.venue_lead_in)
    }

    pub(crate) fn frame_metrics(self) -> FrameReading {
        FrameReading {
            wall_average_ms: self.health.frames.avg_ms(),
            wall_worst_ms: self.health.frames.worst_ms(),
            frames_per_second: self.health.frames.fps(),
            cpu_average_ms: self.health.cpu_frames.avg_ms(),
            cpu_worst_ms: self.health.cpu_frames.worst_ms(),
        }
    }
}

/// The three lanes the assistant answers on: its popup, the window's
/// acknowledgement toast, and the attention sound.
pub(crate) struct Alerts<'a> {
    surfaces: &'a mut crate::surfaces::Surfaces,
    audio: &'a mut super::replay_and_history::AlertState,
}

impl<'a> Alerts<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(crate) fn new(
        surfaces: &'a mut crate::surfaces::Surfaces,
        audio: &'a mut super::replay_and_history::AlertState,
    ) -> Self {
        Self { surfaces, audio }
    }
}

impl quantick_control_handlers::notify::Attention for Alerts<'_> {
    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    fn show_popup(&mut self, popup: crate::control::AgentPopup) {
        self.surfaces
            .update(crate::surfaces::HostIntent::AgentPopup(Box::new(
                quantick_ui_model::agent_popup::Intent::Show(popup),
            )));
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    fn show_toast(&mut self, message: String) {
        crate::surfaces::toast::note(&mut self.surfaces.toast, message, std::time::Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    ///
    /// Straight to the sink, not through the alarms' once-per-run report:
    /// every refused call answers with its reason, and the trader's alarm
    /// failure state is not the assistant's to set or clear.
    fn sound_alert(&mut self) -> Option<String> {
        self.audio
            .alerts
            .play(&[crate::audio::Cue::default()])
            .err()
            .map(ToOwned::to_owned)
    }
}

#[cfg(any(feature = "control-harness", test))]
pub(crate) use quantick_control_host::launch::ControlLaunch;
#[cfg(any(feature = "control-harness", test))]
pub(super) use quantick_control_host::launch::{
    CONTROL_EVIDENCE_HOOK_FRAMES, EvidenceStep, LaunchScenarios, NotificationStep,
};

/// The ordinary local gateway and its opt-in launch scenarios.
pub(crate) struct ControlState {
    pub(crate) control_access: Option<crate::control::ControlAccess>,
    #[cfg(any(feature = "control-harness", test))]
    pub(super) scenarios: LaunchScenarios,
}

/// The temporary range's visible action button on the tab on screen, if the
/// current frame has laid it out. Test support over the window's own roots.
#[cfg(test)]
pub(crate) fn control_quick_range(
    app: &QuantickApp,
) -> Option<crate::surfaces::drawing_chrome::QuickRangeControl> {
    app.surfaces
        .drawing_chrome
        .quick_range_read()
        .control(app.runtime.tabs.id_at(app.runtime.tabs.active_index()))
}

#[cfg(any(feature = "control-harness", test))]
impl ControlState {
    pub(super) fn apply_launch(
        &mut self,
        launch: ControlLaunch,
        panel: &mut crate::surfaces::access_panel::AccessPanelSurface,
    ) {
        if launch.panel && self.control_access.is_some() {
            panel.update(quantick_ui_model::access_panel::Intent::Open);
        }
        if let Some(scopes) = launch.scopes
            && let Some(access) = self.control_access.as_mut()
            && let Err(error) = access.configure_scopes(&scopes)
        {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_SCOPE_HOOK_REFUSED",
                error = %error,
                "QUANTICK_CONTROL_SCOPES named something this build does not register"
            );
        }
        self.scenarios = launch.scenarios;
    }

    /// Take the pending evidence request, expanded against the grants the
    /// gateway holds now.
    pub(super) fn prepare_evidence(
        &mut self,
        access: &crate::control::ControlAccess,
    ) -> Option<quantick_control_host::launch::EvidenceCapture> {
        self.scenarios.prepare_evidence(
            || {
                access
                    .readable_scopes()
                    .into_iter()
                    .map(|scope| scope.to_string())
                    .collect()
            },
            access.grants_screenshot(),
        )
    }
}

#[cfg(any(feature = "control-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_CONTROL_ACCESS",
    "QUANTICK_CONTROL_ANNOTATE",
    "QUANTICK_CONTROL_EVIDENCE",
    "QUANTICK_CONTROL_MARK",
    "QUANTICK_CONTROL_NOTIFY",
    "QUANTICK_CONTROL_PANEL",
    "QUANTICK_CONTROL_SCOPES"
];

#[cfg(test)]
pub(crate) mod tests;
