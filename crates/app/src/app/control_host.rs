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
//! The one adapter, `QuantickApp`, follows the views: the only code that
//! knows which root each family reads. Last comes the gateway's seat — the
//! access it runs in between calls and the `QUANTICK_CONTROL_*` launch hooks
//! that configure it; the scenarios those hooks ask for are headless state in
//! `quantick_control_host::launch`.
//!
//! One file, not two: the adapter and the seat name no `egui` type of their
//! own, so a file of them alone would count against the app UI-free ratchet
//! as code that belongs below `app` — which code reading `QuantickApp`'s
//! fields cannot be.

use eframe::egui;

use crate::config::AppConfig;
use crate::dock::Dock;
use crate::drawings;
use crate::pane::ChartPane;
use crate::style::ChartStyle;
use crate::tab::Tab;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

use quantick_feed::history_reach;

use super::arrangement_host::ArrangementHost;
use super::chart_layers_wiring::LayerWiring;
use super::indicator_manager::IndicatorEdit;
use super::layout_wiring::{LayoutAdapter, LayoutRead};
use super::paper_wiring::PaperSettingsAdapter;
use super::{ControlFrameMetrics, QuantickApp};

/// The window as the gateway holds it: every family at once, `'static`
/// because the projection registry is keyed on the type it reads.
pub(crate) type ControlWindow = dyn ControlPort;

/// Every family together — what the gateway itself needs to dispatch any
/// capability. Handlers never take this; they name their families.
pub(crate) trait ControlPort:
    TabsPort
    + TabsMutPort
    + ChromePort
    + HealthPort
    + AlertsPort
    + LayoutPort
    + PaperPort
    + LayersPort
    + ScriptsPort
    + RecordingPort
    + GatewayPort
{
}

impl<T> ControlPort for T where
    T: TabsPort
        + TabsMutPort
        + ChromePort
        + HealthPort
        + AlertsPort
        + LayoutPort
        + PaperPort
        + LayersPort
        + ScriptsPort
        + RecordingPort
        + GatewayPort
        + ?Sized
{
}

/// Tabs and panes, read: the markets, their config and drawing defaults.
pub(crate) trait TabsPort {
    fn tab_reads(&self) -> TabReads<'_>;
}

/// Tabs and panes, changed: the tab or pane an action targets.
pub(crate) trait TabsMutPort {
    fn tabs_mut(&mut self) -> TabsMut<'_>;
}

/// Chrome and layout: what the window drew around the tab on screen.
pub(crate) trait ChromePort {
    fn chrome_reads(&self) -> ChromeReads<'_>;
}

/// Health and history: what the window measures, how far it reaches back,
/// and the session flags an operator reads back after setting them.
pub(crate) trait HealthPort {
    fn health_reads(&self) -> HealthReads<'_>;
}

/// The three lanes the assistant answers on.
pub(crate) trait AlertsPort {
    fn alerts(&mut self) -> Alerts<'_>;
}

/// The pane-layout owners.
pub(crate) trait LayoutPort {
    fn layout_state(&self) -> super::layout_wiring::LayoutRead<'_>;
    fn layout_adapter(&mut self) -> super::layout_wiring::LayoutAdapter<'_>;
}

/// The paper-trading settings owner.
pub(crate) trait PaperPort {
    fn paper_settings(&mut self) -> super::paper_wiring::PaperSettingsAdapter<'_>;
}

/// The chart-layer owner.
pub(crate) trait LayersPort {
    fn layer_wiring(&mut self) -> super::chart_layers_wiring::LayerWiring<'_>;
}

/// Indicator scripts an operator attaches and detaches.
pub(crate) trait ScriptsPort {
    /// Attach `source` to the focused pane of the tab on screen, as an
    /// operator's or the trader's own; answers where it landed.
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> ScriptSlot;
    /// Detach an operator-attached slot. `Err` when the slot is the trader's
    /// own; `Ok(false)` when there was nothing to detach.
    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, ()>;
}

/// Where an attached script landed: tab id, pane side, slot.
pub(crate) type ScriptSlot = (u64, crate::pane::PaneSide, crate::indicator_worker::SlotId);

/// The deal recorder's window-wide default.
pub(crate) trait RecordingPort {
    /// Save the default and apply it to undecided recorders.
    fn set_deal_recording_default(&mut self, enabled: bool);
}

/// The gateway's own seat on the window, and the doors every in-window
/// caller (the hotkey, the launch hooks, the tests) invokes an action by.
pub(crate) trait GatewayPort {
    /// Where the gateway lives between calls: taken out for the length of
    /// one action, so the action can borrow the window.
    fn gateway_slot(&mut self) -> &mut Option<crate::control::ControlAccess>;
    /// This window as the object the gateway dispatches over.
    fn as_window(&mut self) -> &mut ControlWindow;

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
    /// window. The hooks use it so a screenshot shows a real assistant's
    /// object, attribution and all, without a client on the socket.
    #[cfg(any(feature = "control-harness", test))]
    fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
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
        let outcome = access.invoke_local_action(
            self.as_window(),
            capability_id,
            1,
            input,
            crate::control::ActionOrigin::Remote(Box::new(actor)),
        );
        *self.gateway_slot() = Some(access);
        outcome
    }

    /// A launch hook's action, with its failure reported where a scripted run
    /// will see it: the hook is fire-and-forget, so nothing else would.
    #[cfg(any(feature = "control-harness", test))]
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
}

impl<'a> TabReads<'a> {
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
        }
    }

    /// One tab by position, for a control capability that resolved an id.
    pub(crate) fn tab_at(self, index: usize) -> Option<&'a Tab> {
        self.tabs.get(index)
    }

    /// The read side of [`TabsMut::active_paper_mut`], resolved the same
    /// way so a call and its read-back can never name different tabs.
    pub(crate) fn active_paper(self) -> Option<&'a crate::paper_trading::PaperTrading> {
        self.tabs
            .get(self.tabs.active_index())
            .map(|tab| &tab.paper)
    }

    pub(crate) fn tabs(self) -> &'a ArrangementHost {
        self.tabs
    }

    pub(crate) fn active_tab_index(self) -> usize {
        self.tabs.active_index()
    }

    /// What a freshly placed object of `tool` opens with, through the same
    /// door the click path uses — saved defaults, named preset and all.
    pub(crate) fn new_drawing(self, tool: drawings::DrawingTool) -> drawings::NewDrawing {
        drawings::new_drawing_from_defaults(self.presets, tool)
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
}

impl<'a> TabsMut<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(crate) fn new(tabs: &'a mut ArrangementHost, config: &'a AppConfig) -> Self {
        Self { tabs, config }
    }

    /// The mutable twin of [`TabReads::tab_at`].
    pub(crate) fn tab_at_mut(self, index: usize) -> Option<&'a mut Tab> {
        self.tabs.get_mut(index)
    }

    /// One tab beside the configuration it reads, by position — a capability
    /// names the tab it acts on, and respawning a feed needs the feed table
    /// the same way a click in the corner does.
    pub(crate) fn tab_with_config(self, index: usize) -> Option<(&'a mut Tab, &'a AppConfig)> {
        let config = self.config;
        self.tabs.get_mut(index).map(|tab| (tab, config))
    }

    /// The trading host of the tab on screen — where the `trade.*` actions
    /// land. The active tab and not an addressed one: an order belongs to
    /// the symbol the trader is looking at, and a call that could quietly
    /// trade a chart nobody has open is a call nobody should be able to
    /// make.
    pub(crate) fn active_paper_mut(self) -> Option<&'a mut crate::paper_trading::PaperTrading> {
        // Fallible, because the rest of the control code does not trust the
        // invariant either: `annotate::resolve_target` guards an empty tab
        // list and clamps the index, and two more sites clamp it. A
        // `trade.*` call must answer "this window has no chart open" rather
        // than panic the whole trading application, and it must resolve the
        // *same* tab its own read-back resolves.
        let active = self.tabs.active_index();
        self.tabs.get_mut(active).map(|tab| &mut tab.paper)
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
}

/// The chrome-and-layout read view: what the window drew around the tab on
/// screen, read as values where the frame published them.
#[derive(Clone, Copy)]
pub(crate) struct ChromeReads<'a> {
    tabs: &'a ArrangementHost,
    style: &'a ChartStyle,
    tool_rail: &'a ToolRail,
    dock: &'a Dock,
    tz: TzOffset,
    drawing_chrome: &'a crate::surfaces::drawing_chrome::DrawingChromeSurface,
    drawn: ChromeDrawn,
}

/// What the last frame published about the window chrome.
#[derive(Clone, Copy, Default)]
pub(crate) struct ChromeDrawn {
    /// Where the feed's offline chip was painted, or `None` when it was not.
    pub(crate) feed_chip_rect: Option<egui::Rect>,
    /// The tab whose chip opened the feed's recovery popup, if any.
    pub(crate) feed_popup_tab: Option<u64>,
    /// Whether a recording opens with the session day before it joined.
    pub(crate) replay_day_before: bool,
}

impl<'a> ChromeReads<'a> {
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
    pub(crate) fn feed_offline_accent(
        self,
        stall: Option<&quantick_feed::stall::Stall>,
    ) -> Option<egui::Color32> {
        crate::feed_notice::report(&self.tabs[self.tabs.active_index()].notice, stall)
            .filter(crate::feed_notice::Report::is_offline)
            .map(|report| report.accent())
    }

    /// Where the feed's offline chip was painted, or `None` when it was not.
    ///
    /// The projection reads what was drawn rather than re-deciding it, so the
    /// scene and the screen cannot disagree across the edge of a stall budget.
    pub(crate) fn feed_chip_rect(self) -> Option<egui::Rect> {
        self.drawn.feed_chip_rect
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
    ) -> Option<[crate::surfaces::drawing_chrome::QuickRangeControl; 3]> {
        self.drawing_chrome
            .quick_range
            .controls(self.tabs.id_at(self.tabs.active_index()))
    }
}

/// The health-and-history read view.
#[derive(Clone, Copy)]
pub(crate) struct HealthReads<'a> {
    save_on_exit: bool,
    health: &'a super::health::HealthCounters,
    history: &'a super::tabs::HistorySettings,
}

impl<'a> HealthReads<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(super) fn new(
        save_on_exit: bool,
        health: &'a super::health::HealthCounters,
        history: &'a super::tabs::HistorySettings,
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

    pub(crate) fn frame_metrics(self) -> ControlFrameMetrics {
        ControlFrameMetrics {
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
    agent_popup: &'a mut crate::surfaces::AgentPopupSurface,
    toast: &'a mut crate::surfaces::ToastSurface,
    audio: &'a mut super::replay_and_history::AlertState,
}

impl<'a> Alerts<'a> {
    /// The view over the window's own roots, as [`TabReads::new`].
    pub(super) fn new(
        agent_popup: &'a mut crate::surfaces::AgentPopupSurface,
        toast: &'a mut crate::surfaces::ToastSurface,
        audio: &'a mut super::replay_and_history::AlertState,
    ) -> Self {
        Self {
            agent_popup,
            toast,
            audio,
        }
    }

    /// Open the assistant's popup. One at a time: a second message replaces
    /// the first rather than stacking windows over a chart someone is
    /// trading, and the trader dismisses it.
    pub(crate) fn show_popup(self, popup: crate::control::AgentPopup) {
        self.agent_popup.show(popup);
    }

    /// Post one line to the window's own acknowledgement lane — the same
    /// channel a delete or a workspace save uses, with no Undo: there is
    /// nothing to take back from having been told something.
    pub(crate) fn show_toast(self, message: String) {
        self.toast.note(message, std::time::Instant::now());
    }

    /// Ask for the platform's attention sound, through the same sink the
    /// alarms use, and report honestly when it could not be made rather
    /// than letting a client believe it was heard.
    ///
    /// Straight to the sink, not through the alarms' once-per-run report:
    /// every refused call answers with its reason, and the trader's alarm
    /// failure state is not the assistant's to set or clear.
    pub(crate) fn sound_alert(self) -> Option<String> {
        self.audio
            .alerts
            .play(&[crate::audio::Cue::default()])
            .err()
            .map(ToOwned::to_owned)
    }
}

// `QuantickApp`, the port's one adapter: each family borrows exactly the
// roots it names.

impl TabsPort for QuantickApp {
    fn tab_reads(&self) -> TabReads<'_> {
        let presets = &self.drawings.presets;
        TabReads::new(&self.tabs, &self.config, &self.footprint_config, presets)
    }
}

impl TabsMutPort for QuantickApp {
    fn tabs_mut(&mut self) -> TabsMut<'_> {
        TabsMut::new(&mut self.tabs, &self.config)
    }
}

impl ChromePort for QuantickApp {
    fn chrome_reads(&self) -> ChromeReads<'_> {
        let drawn = ChromeDrawn {
            feed_chip_rect: self.chrome.feed_chip_rect,
            feed_popup_tab: self.chrome.feed_popup_tab,
            replay_day_before: self.replay_view.day_before(),
        };
        let (rail, chrome) = (&self.toolrail, &self.drawings.chrome);
        ChromeReads::new(
            &self.tabs,
            &self.style,
            rail,
            &self.dock,
            self.tz,
            chrome,
            drawn,
        )
    }
}

impl HealthPort for QuantickApp {
    fn health_reads(&self) -> HealthReads<'_> {
        let save_on_exit = self.workspace.session().save_on_exit();
        HealthReads::new(save_on_exit, &self.health, &self.history)
    }
}

impl AlertsPort for QuantickApp {
    fn alerts(&mut self) -> Alerts<'_> {
        let surfaces = &mut self.surfaces;
        Alerts::new(
            &mut surfaces.agent_popup,
            &mut surfaces.toast,
            &mut self.audio,
        )
    }
}

impl LayoutPort for QuantickApp {
    fn layout_state(&self) -> LayoutRead<'_> {
        let session = self.workspace.layouts().session();
        LayoutRead {
            tabs: &self.tabs,
            active: self.tabs.active_index(),
            session,
        }
    }

    fn layout_adapter(&mut self) -> LayoutAdapter<'_> {
        LayoutAdapter {
            active: self.tabs.active_index(),
            tabs: &mut self.tabs,
            indicators: &mut self.indicators,
            store: self.workspace.layouts_mut(),
            drawing_chrome: &mut self.drawings.chrome,
            toast: &mut self.surfaces.toast,
            rename: &mut self.chrome.layout_rename,
            delete_confirm: &mut self.chrome.layout_delete_confirm,
        }
    }
}

impl PaperPort for QuantickApp {
    fn paper_settings(&mut self) -> PaperSettingsAdapter<'_> {
        PaperSettingsAdapter {
            tabs: &mut self.tabs,
            workspace: &mut self.workspace,
        }
    }
}

impl LayersPort for QuantickApp {
    fn layer_wiring(&mut self) -> LayerWiring<'_> {
        LayerWiring {
            tabs: &mut self.tabs,
            workspace: &mut self.workspace,
            style: &mut self.style,
            style_revision: &mut self.style_revision,
            footprint_config: &mut self.footprint_config,
            footprint_settings: &mut self.surfaces.footprint_settings,
        }
    }
}

impl ScriptsPort for QuantickApp {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> ScriptSlot {
        let target = (self.tabs.active_id(), self.active_tab().focused_side());
        let pane = self
            .tabs
            .runtime_mut(self.tabs.active_index())
            .pane_mut(target.1);
        let attached = self
            .indicators
            .attach_script(pane, target, name, source, by_operator);
        let owner = attached.target;
        self.apply_indicator_edit(IndicatorEdit::Attached(attached));
        (owner.tab, owner.side, owner.slot)
    }

    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, ()> {
        let Some(target) = self.indicators.operator_target(slot)? else {
            return Ok(false);
        };
        self.apply_indicator_edit(IndicatorEdit::Remove(target));
        self.indicators.operator_slots.remove(&target);
        Ok(true)
    }
}

impl RecordingPort for QuantickApp {
    fn set_deal_recording_default(&mut self, enabled: bool) {
        super::deal_recording_wiring::set_default(self, enabled);
    }
}

impl GatewayPort for QuantickApp {
    fn gateway_slot(&mut self) -> &mut Option<crate::control::ControlAccess> {
        &mut self.control.control_access
    }

    fn as_window(&mut self) -> &mut ControlWindow {
        self
    }
}

#[cfg(any(feature = "control-harness", test))]
pub(super) use quantick_control_host::launch::{
    CONTROL_EVIDENCE_HOOK_FRAMES, EvidenceStep, LaunchScenarios, NotificationStep,
};

/// The ordinary local gateway and its opt-in launch scenarios.
pub(super) struct ControlState {
    pub(super) control_access: Option<crate::control::ControlAccess>,
    #[cfg(any(feature = "control-harness", test))]
    pub(super) scenarios: LaunchScenarios,
}

/// The temporary range's visible action button on the tab on screen, if the
/// current frame has laid it out. Test support over the window's own roots.
#[cfg(test)]
pub(crate) fn control_quick_range(
    app: &QuantickApp,
) -> Option<crate::surfaces::drawing_chrome::QuickRangeControl> {
    app.drawings
        .chrome
        .quick_range
        .control(app.tabs.id_at(app.tabs.active_index()))
}

/// Control-only launch inputs, captured before owner construction.
#[cfg(any(feature = "control-harness", test))]
#[derive(Default)]
pub(crate) struct ControlLaunch {
    panel: bool,
    scopes: Option<String>,
    scenarios: LaunchScenarios,
}

#[cfg(any(feature = "control-harness", test))]
impl ControlLaunch {
    pub(crate) fn capture(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Self {
        let mut value = |name| lookup(name).and_then(|raw| raw.into_string().ok());
        let panel = value("QUANTICK_CONTROL_PANEL").is_some_and(|raw| raw == "1");
        let scopes = value("QUANTICK_CONTROL_SCOPES");
        let access = value("QUANTICK_CONTROL_ACCESS").is_some_and(|raw| raw == "1");
        let mark = value("QUANTICK_CONTROL_MARK")
            .filter(|raw| !raw.trim().is_empty())
            .map(|raw| if raw == "1" { String::new() } else { raw });
        let evidence = value("QUANTICK_CONTROL_EVIDENCE").filter(|raw| !raw.trim().is_empty());
        let annotation = value("QUANTICK_CONTROL_ANNOTATE").filter(|raw| !raw.trim().is_empty());
        let notification = value("QUANTICK_CONTROL_NOTIFY").filter(|raw| !raw.trim().is_empty());
        Self {
            panel,
            scopes,
            scenarios: LaunchScenarios::new(quantick_control_host::launch::LaunchRequests {
                enable_access: access,
                annotation,
                notification,
                evidence,
                mark,
            }),
        }
    }
}

#[cfg(any(feature = "control-harness", test))]
impl ControlState {
    pub(super) fn apply_launch(&mut self, launch: ControlLaunch) {
        if launch.panel
            && let Some(access) = self.control_access.as_mut()
        {
            access.open_panel();
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
