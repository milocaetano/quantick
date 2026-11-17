//! A fake window and a fake access: every port a family names, over plain
//! fields a test sets and reads back. The second implementation of each port,
//! after the application's own adapter.

use std::time::Duration;

use quantick_chart::state::ChartState;
use quantick_control::{
    id::{ConnectionId, PrincipalId, RequestId},
    wire::{ActorContext, ActorKind, CanonicalDecimal, WireU64},
};
use quantick_control_host::{
    journal::NewEvent,
    wire::{PaneSideDto, unavailable},
};
use quantick_control_schema::chart::{BarProvenanceContext, ViewportSnapshot};
use quantick_control_schema::interaction::CursorSnapshot;
use quantick_control_schema::notify::AgentPopup;
use quantick_engine::bar_registry::{BUILTIN_BARS, BarConfiguration};
use quantick_engine::{Bar, BarSpec, Side, Trade};
use quantick_session::{
    HistoryAct, HistoryOutcome, HistoryReply, SessionSnapshot, sets_window_default,
};
use quantick_sources::history_reach::{HistoryReach, ReachProgress};
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use rust_decimal::Decimal;

use crate::JournalAccess;
use crate::analysis::{AnalysisPaneEntry, AnalysisPort};
use crate::attention::MarkPort;
use crate::axis_pane::{AxisPane, NativeTape};
use crate::bubble_pane::{AssetSave, BubbleLook, BubblePane, PaneBubblesPort, TickSpherePane};
use crate::chart::{
    AddressedPane, ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane, PaneFraming,
};
use crate::chart_layer::{LayerBlocked, LayerRead};
use crate::deal_recorder::DealRecorder;
use crate::history::HistoryPort;
use crate::notify::{AttentionPort, NotifyAccess};
use crate::pane_marks::PaneMarks;
use crate::recovery::{FeedRecoveryPort, RecoveredFeed};
use crate::tabs::TabDirectory;
use crate::trade::{TradeDesk, TradePort};
use quantick_control::error::ControlError;
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_control_schema::layers::LayerSnapshot;
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_drawings::{DrawingTool, Drawings};
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_replay::recorder::{DealRecordingError, RecordingView};
use quantick_stores::bubble_asset_store::SaveSwitch;
use quantick_stores::candle_presets::PresetError;

/// One pane of a fake tab: a real chart model, an optional venue prefix, and
/// the framing a frame would have laid out.
pub(crate) struct FakePane {
    pub id: u64,
    pub state: ChartState,
    pub prefix: Vec<Bar>,
    pub pagination_revision: u64,
    pub visible_slots: Option<(usize, usize)>,
    pub record_starts_inside: bool,
    /// What the tick sphere rule reads about the pane.
    pub sphere_facts: TickSpherePane,
    /// The pane's order-flow view, which serves its bubble calls.
    pub bubbles: Option<FakeBubbles>,
    /// Every price framing asked of the pane, `None` a fit.
    pub framed: Vec<Option<(f64, f64)>>,
    /// Every time strip act the pane took, by name.
    pub time_acts: Vec<&'static str>,
    /// The pane can show its live bars (overview, centre).
    pub shows_live: bool,
    /// The pane's native tape, when it draws one.
    pub tape: Option<FakeTape>,
    pub drawings: Drawings<DrawingTool>,
    /// The bar rule the pane's marks are cut to.
    pub bars: BarConfiguration,
    /// The registered layers, as the pane's menu reads them.
    pub layers: Vec<(LayerSnapshot, Option<LayerBlocked>)>,
}

/// A fake native tape: what it was last moved to, and the frame it reports.
#[derive(Default)]
pub(crate) struct FakeTape {
    pub window: Option<(LaneWindow, bool)>,
    pub end: Option<TapeEnd>,
    pub frame: Option<TapeViewSnapshot>,
}

/// A fake order-flow view's bubble look: what it last wore and its asset.
#[derive(Default)]
pub(crate) struct FakeBubbles {
    /// The whole look last worn, and whether it brought its spheres.
    pub preset: Option<(String, bool)>,
    pub worn: Option<Option<String>>,
    pub tuning: TickTuning,
    /// The bound asset's key; `None` is a view bound to no asset.
    pub asset: Option<String>,
    pub save_changes: bool,
    pub config: HeatmapConfig,
}

impl FakePane {
    /// A tick-`ticks` pane fed `trades` live.
    pub fn with_trades(id: u64, ticks: u64, trades: impl IntoIterator<Item = Trade>) -> Self {
        let mut state = ChartState::new(BarSpec::Tick(ticks));
        for trade in trades {
            state.ingest_live(&trade);
        }
        Self {
            id,
            state,
            prefix: Vec::new(),
            pagination_revision: 0,
            visible_slots: None,
            record_starts_inside: false,
            sphere_facts: TickSpherePane::new(false, false),
            bubbles: None,
            framed: Vec::new(),
            time_acts: Vec::new(),
            shows_live: true,
            tape: None,
            drawings: Drawings::default(),
            bars: BUILTIN_BARS
                .parse(&format!("tick:{ticks}"))
                .expect("a tick rule"),
            layers: Vec::new(),
        }
    }
}

impl AxisPane for FakePane {
    fn frame_price_range(&mut self, low: f64, high: f64) {
        self.framed.push(Some((low, high)));
    }
    fn fit_prices(&mut self) {
        self.framed.push(None);
    }
    fn reset_time_strip(&mut self, tape: bool) -> bool {
        if tape && self.tape.is_none() {
            return false;
        }
        self.time_acts
            .push(if tape { "tape_opening" } else { "opening" });
        true
    }
    fn show_live_overview(&mut self) -> bool {
        if self.shows_live {
            self.time_acts.push("overview");
        }
        self.shows_live
    }
    fn centre_live(&mut self, zoom_in: bool) -> bool {
        if self.shows_live {
            self.time_acts
                .push(if zoom_in { "centre_zoom_in" } else { "centre" });
        }
        self.shows_live
    }
    fn framing(&self) -> ViewportSnapshot {
        fake_viewport(self)
    }
    fn native_tape(&mut self) -> Option<&mut dyn NativeTape> {
        Some(self.tape.as_mut()?)
    }
}

impl PaneMarks for FakePane {
    fn marks(&mut self) -> &mut Drawings<DrawingTool> {
        &mut self.drawings
    }
    fn marks_bars(&self) -> BarConfiguration {
        self.bars
    }
}

impl NativeTape for FakeTape {
    fn move_tape_window(&mut self, window: LaneWindow, file: bool) {
        self.window = Some((window, file));
    }
    fn move_tape_end(&mut self, end: TapeEnd) {
        self.end = Some(end);
    }
    fn tape_frame(&self) -> Option<TapeViewSnapshot> {
        self.frame.clone()
    }
}

impl BubblePane<()> for FakePane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        self.sphere_facts
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<()>> {
        Some(self.bubbles.as_mut()?)
    }
    fn asset_save(&mut self) -> Option<&mut dyn AssetSave> {
        Some(self.bubbles.as_mut()?)
    }
}

impl BubbleLook<()> for FakeBubbles {
    fn wear_preset(&mut self, name: &str, spheres: bool, _: &()) -> Result<bool, String> {
        self.preset = Some((name.to_owned(), spheres));
        Ok(true)
    }
    fn restore_factory_look(&mut self) -> bool {
        self.preset.take().is_some()
    }
    fn presets_page(&self, _: &ListInput) -> Result<ListResult, String> {
        unreachable!("no fake test lists presets")
    }
    fn wear_tick(&mut self, preset: Option<&str>, tuning: TickTuning) -> Result<bool, String> {
        self.worn = Some(preset.map(str::to_owned));
        self.tuning = tuning;
        Ok(true)
    }
    fn look_snapshot(&self) -> BubblesStateSnapshot {
        BubblesStateSnapshot::from_config(&self.config, Decimal::ZERO, None, &[], None, Vec::new())
    }
    fn look_config(&self) -> &HeatmapConfig {
        &self.config
    }
}

impl AssetSave for FakeBubbles {
    fn asset_key(&self) -> Option<&str> {
        self.asset.as_deref()
    }
    fn switch_asset_save(&mut self, on: bool) -> Option<SaveSwitch> {
        self.asset.as_ref()?;
        let moved = self.save_changes != on;
        self.save_changes = on;
        Some(match (moved, on) {
            (false, _) => SaveSwitch::Unmoved,
            (true, true) => SaveSwitch::ScreenStored,
            (true, false) => SaveSwitch::Off,
        })
    }
}

/// One fake pane read with the tab it sits on, as the window's adapter reads
/// a pane with its tab.
struct FakePaneRead<'a> {
    tab: &'a FakeTab,
    pane: &'a FakePane,
}

impl ChartPaneSource for FakePaneRead<'_> {
    fn pane_id(&self) -> u64 {
        self.pane.id
    }
    fn state(&self) -> &ChartState {
        &self.pane.state
    }
    fn pagination_revision(&self) -> u64 {
        self.pane.pagination_revision
    }
    fn closed_slots(&self) -> usize {
        self.pane.prefix.len() + self.pane.state.bars().len()
    }
    fn seam_slot(&self) -> usize {
        self.pane.prefix.len()
    }
    fn closed_bar(&self, slot: usize) -> Option<&Bar> {
        self.pane
            .prefix
            .get(slot)
            .or_else(|| self.pane.state.bars().get(slot - self.pane.prefix.len()))
    }
    fn venue_prefix(&self) -> &[Bar] {
        &self.pane.prefix
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.pane.prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self
                .pane
                .state
                .slot_open_time(slot - self.pane.prefix.len()),
        }
    }
    fn visible_slots(&self) -> Option<(usize, usize)> {
        self.pane.visible_slots
    }
    fn viewport(&self) -> ViewportSnapshot {
        fake_viewport(self.pane)
    }
    fn history_paging(&self) -> bool {
        self.tab.history_paging
    }
    fn venue_record_starts_inside(&self, _interval_ms: i64) -> bool {
        self.pane.record_starts_inside
    }
    fn provenance(&self) -> BarProvenanceContext {
        BarProvenanceContext {
            engine_price: "exchange_trade".to_owned(),
            engine_volume: "exchange_trade".to_owned(),
            engine_side: "exchange_aggressor".to_owned(),
            venue_aggressor_split: false,
        }
    }
}

/// A fake pane's framing: the slots a test laid out, the rest fixed.
fn fake_viewport(pane: &FakePane) -> ViewportSnapshot {
    let (start, end) = pane.visible_slots.unwrap_or((0, 0));
    ViewportSnapshot {
        geometry_available: pane.visible_slots.is_some(),
        visible_start_slot: WireU64::new(start as u64),
        visible_end_slot_exclusive: WireU64::new(end as u64),
        pixels_per_bar: CanonicalDecimal::new("8").expect("canonical"),
        right_edge_bar: CanonicalDecimal::new("0").expect("canonical"),
        follows_live: true,
        price_auto_fit: true,
        price_axis_inverted: false,
        price_range: None,
        chart_width_px: None,
        tape: pane.tape.as_ref().and_then(|tape| tape.frame.clone()),
    }
}

/// One fake tab: its market, what its feed and history do when asked, and
/// its panes, the first on the flow side.
pub(crate) struct FakeTab {
    pub id: u64,
    pub symbol: String,
    pub feed_id: String,
    /// Whether a recovery finds a feed session to respawn.
    pub respawns: bool,
    /// Every recovery asked of the tab, `true` for a reconnect.
    pub recoveries: Vec<bool>,
    pub history_paging: bool,
    /// The chart is still filling, so a press waits.
    pub filling: bool,
    /// A started run has nothing to page and ends within the press.
    pub start_finishes: bool,
    pub history: RunStatus,
    pub main_reach: HistoryReach,
    pub panes: Vec<FakePane>,
    /// The tab plays a recorded session.
    pub replaying: bool,
    /// The deal recorder's view; `None` on a feed with no counter.
    pub recording: Option<RecordingView>,
    /// What the recorder refuses to start or load with, when it refuses.
    pub recorder_refusal: Option<DealRecordingError>,
    /// Every recorder act the tab took, by name.
    pub recorder_acts: Vec<String>,
}

impl FakeTab {
    pub fn new(id: u64, symbol: &str) -> Self {
        Self {
            id,
            symbol: symbol.to_owned(),
            feed_id: "fake".to_owned(),
            respawns: true,
            recoveries: Vec::new(),
            history_paging: true,
            filling: false,
            start_finishes: false,
            history: RunStatus::Idle,
            main_reach: HistoryReach::Hours(1),
            panes: Vec::new(),
            replaying: false,
            recording: None,
            recorder_refusal: None,
            recorder_acts: Vec::new(),
        }
    }

    /// The recorder's act `act`, unless it refuses.
    fn record_act(&mut self, act: String) -> Result<(), DealRecordingError> {
        if let Some(refusal) = self.recorder_refusal.clone() {
            return Err(refusal);
        }
        self.recorder_acts.push(act);
        Ok(())
    }

    /// A press judged the way the session judges one: refused while a run
    /// pages, queued while the chart fills, begun otherwise.
    fn press(&mut self, reach: HistoryReach) -> Press {
        let press = match self.history {
            RunStatus::Loading(_) | RunStatus::Paused(_) => return Press::AlreadyRunning,
            _ if self.filling => {
                self.history = RunStatus::Queued(reach);
                Press::Queued
            }
            _ => {
                self.history = if self.start_finishes {
                    RunStatus::Idle
                } else {
                    loading(reach)
                };
                Press::Start
            }
        };
        self.main_reach = reach;
        press
    }

    fn cancel(&mut self) -> Cancelled {
        let cancelled = match self.history {
            RunStatus::Queued(reach) => Cancelled::Queued(reach),
            _ => Cancelled::Nothing,
        };
        self.history = RunStatus::Idle;
        cancelled
    }

    fn snapshot(&self, outcome: HistoryOutcome) -> SessionSnapshot {
        let (last_press, last_cancel) = match outcome {
            HistoryOutcome::Pressed(press) => (Some(press), None),
            HistoryOutcome::Cancelled(cancelled) => (None, Some(cancelled)),
        };
        SessionSnapshot {
            run: self.history,
            main_reach: self.main_reach,
            last_press,
            last_cancel,
            note: None,
            candles_held: 0,
            candles_pending: false,
            older_exhausted: false,
            venue_lead_in: false,
            progressive: true,
        }
    }
}

/// The fake window: tabs in screen order, the one on screen, and what each
/// attention lane received.
pub(crate) struct FakeWindow {
    pub tabs: Vec<FakeTab>,
    pub active: usize,
    pub popups: Vec<AgentPopup>,
    pub toasts: Vec<String>,
    pub speaker_refusal: Option<String>,
    /// The main-click default the last taken press set.
    pub default_reach: Option<HistoryReach>,
    /// The tool rail's magnet.
    pub rail_magnet: bool,
    /// What the chart settings owner answers, and every input it was given.
    pub chart_settings: Result<ChartSettingsResult, PresetError>,
    pub settings_asked: Vec<ChartSettingsInput>,
    /// Every layer switch asked: pane, layer and visibility.
    pub layer_switches: Vec<(u64, String, bool)>,
    /// The record-by-default setting, once a call kept one.
    pub record_by_default: Option<bool>,
}

impl FakeWindow {
    pub fn new(tabs: Vec<FakeTab>) -> Self {
        Self {
            tabs,
            active: 0,
            popups: Vec::new(),
            toasts: Vec::new(),
            speaker_refusal: None,
            default_reach: None,
            rail_magnet: false,
            chart_settings: Err(PresetError::Refused("the fake keeps no chart settings")),
            settings_asked: Vec::new(),
            layer_switches: Vec::new(),
            record_by_default: None,
        }
    }

    /// The pane `pane_id` on the tab `tab_id`.
    fn pane_mut(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut FakePane, ControlError> {
        let tab = self.tabs.iter_mut().find(|tab| tab.id == tab_id.get());
        let tab = tab.ok_or_else(|| ControlError::invalid_request("no open tab has that id"))?;
        let pane = tab.panes.iter_mut().find(|pane| pane.id == pane_id.get());
        pane.ok_or_else(|| ControlError::invalid_request("the call names an unknown pane"))
    }

    /// The actor every fake call is made as.
    pub fn assistant() -> ActorContext {
        use quantick_control::limits::CONTROL_RUNTIME_ID_BYTES;
        ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: PrincipalId::from_bytes([1; CONTROL_RUNTIME_ID_BYTES]),
            client_name: "fake assistant".to_owned(),
            connection_id: ConnectionId::from_bytes([2; CONTROL_RUNTIME_ID_BYTES]),
            request_id: RequestId::new("fake-1").expect("a literal request id is valid"),
            reason: None,
            requested_at_unix_ms: 0,
        }
    }

    /// The trader's own hand on the same door: a call the window makes for a
    /// human gesture.
    pub fn trader() -> ActorContext {
        ActorContext {
            actor_kind: ActorKind::HumanUi,
            client_name: "fake trader".to_owned(),
            ..Self::assistant()
        }
    }
}

impl TabDirectory for FakeWindow {
    fn active_tab_index(&self) -> usize {
        self.active
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tabs[index].id
    }
}

impl PaneBubblesPort for FakeWindow {
    type Theme = ();
    fn bubble_theme(&self) {}
    fn pane_bubbles(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut dyn BubblePane<()>, ControlError> {
        Ok(self.pane_mut(tab_id, pane_id)?)
    }
}

impl FeedRecoveryPort for FakeWindow {
    fn recover_feed(&mut self, index: usize, keep_timeline: bool) -> Option<RecoveredFeed> {
        let tab = self.tabs.get_mut(index)?;
        tab.recoveries.push(keep_timeline);
        Some(RecoveredFeed {
            symbol: tab.symbol.clone(),
            respawned: tab.respawns,
        })
    }
    fn deal_recorder(&mut self, index: usize) -> Option<&mut dyn DealRecorder> {
        Some(self.tabs.get_mut(index)?)
    }
    fn record_deals_by_default(&mut self, on: bool) {
        self.record_by_default = Some(on);
    }
}

impl DealRecorder for FakeTab {
    fn replaying(&self) -> bool {
        self.replaying
    }
    fn recorded_symbol(&self) -> &str {
        &self.symbol
    }
    fn recorder_view(&self) -> Option<RecordingView> {
        self.recording.clone()
    }
    fn load_recorded_day(&mut self, index: usize) -> Result<(), DealRecordingError> {
        self.record_act(format!("load {index}"))
    }
    fn start_recording(&mut self) -> Result<(), DealRecordingError> {
        self.record_act("start".to_owned())
    }
    fn stop_recording(&mut self) {
        self.recorder_acts.push("stop".to_owned());
    }
}

/// The fake window's panes carry no analysis: it serves the rail's magnet.
impl AnalysisPort for FakeWindow {
    fn drawing_magnet(&self) -> bool {
        self.rail_magnet
    }
    fn switch_rail_magnet(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.rail_magnet, on) != on
    }
    fn visit_analysis_tabs(&self, _visit: &mut dyn FnMut(u64, &[AnalysisPaneEntry<'_>])) {}
}

impl HistoryPort for FakeWindow {
    fn history_paging(&self, index: usize) -> Option<bool> {
        self.tabs.get(index).map(|tab| tab.history_paging)
    }
    /// The press and cancel over plain fields; a taken press moves the
    /// default as `TabsMut::press_history` does.
    fn dispatch_history(&mut self, index: usize, act: HistoryAct) -> Option<HistoryReply> {
        let tab = self.tabs.get_mut(index)?;
        let outcome = match act {
            HistoryAct::Press(reach) => {
                let press = tab.press(reach);
                if sets_window_default(press) {
                    self.default_reach = Some(reach);
                }
                HistoryOutcome::Pressed(press)
            }
            HistoryAct::Cancel => HistoryOutcome::Cancelled(tab.cancel()),
        };
        Some((outcome, tab.snapshot(outcome)))
    }
}

impl AttentionPort for FakeWindow {
    fn show_popup(&mut self, popup: AgentPopup) {
        self.popups.push(popup);
    }
    fn show_toast(&mut self, message: String) {
        self.toasts.push(message);
    }
    fn sound_alert(&mut self) -> Option<String> {
        self.speaker_refusal.clone()
    }
}

/// The fake window keeps no paper desk: every trade action finds no
/// chart open.
impl TradePort for FakeWindow {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        None
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        None
    }
    fn persist_order_strategies(&mut self) {}
    fn persist_risk_settings(&mut self) {}
}

/// The fake window has no pointer: a mark resolved against it names the
/// active tab's first pane and says why the pointer is missing.
impl MarkPort for FakeWindow {
    fn cursor_snapshot(&self) -> CursorSnapshot {
        let tab = self.tabs.get(self.active);
        CursorSnapshot {
            active_tab_id: WireU64::new(tab.map_or(0, |tab| tab.id)),
            focused_pane_id: WireU64::new(
                tab.and_then(|tab| tab.panes.first())
                    .map_or(0, |pane| pane.id),
            ),
            focused_pane_side: PaneSideDto::Flow,
            pointer: None,
            pointer_availability: unavailable("fake_window_has_no_pointer"),
            semantic_scene: unavailable("fake_window_has_no_pointer"),
        }
    }
}

impl ChartPort for FakeWindow {
    fn price_tag_progress(&self, _tab_id: u64) -> bool {
        true
    }
    fn addressed_pane(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut dyn AddressedPane, ControlError> {
        Ok(self.pane_mut(tab_id, pane_id)?)
    }
    fn apply_chart_settings(
        &mut self,
        input: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        self.settings_asked.push(input);
        self.chart_settings.clone()
    }
    fn chart_layer(
        &self,
        tab_id: WireU64,
        pane_id: WireU64,
        layer_id: &str,
    ) -> Result<LayerRead, ControlError> {
        let tab = self.tabs.iter().find(|tab| tab.id == tab_id.get());
        let tab = tab.ok_or_else(|| ControlError::invalid_request("no open tab has that id"))?;
        let pane = tab.panes.iter().find(|pane| pane.id == pane_id.get());
        let pane = pane.ok_or_else(|| ControlError::invalid_request("unknown pane"))?;
        let (layer, blocked) = pane
            .layers
            .iter()
            .find(|(layer, _)| layer.id == layer_id)
            .ok_or_else(|| ControlError::invalid_request("unknown layer"))?;
        Ok(LayerRead {
            layer: layer.clone(),
            blocked: *blocked,
        })
    }
    fn switch_chart_layer(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
        layer_id: &str,
        visible: bool,
    ) -> Result<(), ControlError> {
        let pane = self.pane_mut(tab_id, pane_id)?;
        let layer = pane
            .layers
            .iter_mut()
            .find(|(layer, _)| layer.id == layer_id);
        let (layer, _) = layer.ok_or_else(|| ControlError::invalid_request("unknown layer"))?;
        layer.requested = visible;
        self.layer_switches
            .push((pane_id.get(), layer_id.to_owned(), visible));
        Ok(())
    }
    fn visit_chart_panes(&self, visit: &mut dyn FnMut(ChartPaneEntry<'_>, PaneFraming)) {
        for (index, tab) in self.tabs.iter().enumerate() {
            for (slot, pane) in tab.panes.iter().enumerate() {
                let shown = index == self.active;
                let framing = PaneFraming {
                    visible: shown,
                    focused: shown && slot == 0,
                };
                let read = FakePaneRead { tab, pane };
                visit(entry(tab, slot, &read), framing);
            }
        }
    }
    fn visit_chart_pane(
        &self,
        tab_id: u64,
        pane_id: u64,
        visit: &mut dyn FnMut(ChartPaneEntry<'_>),
    ) -> Result<(), MissingPane> {
        let tab = self
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .ok_or(MissingPane::Tab)?;
        let (slot, pane) = tab
            .panes
            .iter()
            .enumerate()
            .find(|(_, pane)| pane.id == pane_id)
            .ok_or(MissingPane::Pane)?;
        visit(entry(tab, slot, &FakePaneRead { tab, pane }));
        Ok(())
    }
}

fn entry<'a>(tab: &'a FakeTab, slot: usize, read: &'a FakePaneRead<'a>) -> ChartPaneEntry<'a> {
    let side = if slot == 0 {
        PaneSideDto::Flow
    } else {
        PaneSideDto::Time
    };
    ChartPaneEntry {
        tab_id: tab.id,
        side,
        pane_index: slot,
        feed_id: &tab.feed_id,
        symbol: &tab.symbol,
        pane: read,
        market_clock_ms: None,
    }
}

/// The fixed time a fake access tells a handler it runs at.
pub(crate) const FAKE_NOW_UNIX_MS: i64 = 100;

/// A fake access: a fixed number of notifications it allows, then the wait
/// it reports, and every event it journaled.
pub(crate) struct FakeAccess {
    pub budget: usize,
    pub retry_after: Duration,
    pub events: Vec<NewEvent>,
}

impl FakeAccess {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            retry_after: Duration::from_millis(1_500),
            events: Vec::new(),
        }
    }
}

impl JournalAccess for FakeAccess {
    fn record_event(&mut self, event: NewEvent) -> WireU64 {
        self.events.push(event);
        WireU64::new(self.events.len() as u64)
    }
    fn now_unix_ms(&self) -> i64 {
        FAKE_NOW_UNIX_MS
    }
}

impl NotifyAccess for FakeAccess {
    fn allow_notification(&mut self, _actor: &ActorContext) -> Result<(), Duration> {
        if self.budget == 0 {
            return Err(self.retry_after);
        }
        self.budget -= 1;
        Ok(())
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}

/// A run paging toward `reach`, nothing pulled yet.
pub(crate) fn loading(reach: HistoryReach) -> RunStatus {
    RunStatus::Loading(ReachProgress {
        reach,
        oldest_ms: 0,
        sessions_reached: 0,
        traded_ms: 0,
        prints_pulled: 0,
        pages: 0,
    })
}

/// A buy print at `price`, the `id`th second of the epoch.
pub(crate) fn trade(id: u64, price: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: i64::try_from(id).expect("small id") * 1_000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}
