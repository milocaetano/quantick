//! A fake window and a fake access: every port a family names, over plain
//! fields a test sets and reads back. The second implementation of each port,
//! after the application's own adapter.

use std::time::Duration;

use quantick_chart::state::ChartState;
use quantick_chart::style::ChartStyle;
use quantick_civil::TzOffset;
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
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_session::paper::{SharedChange, TabPaper, TradeCommand, TradeOutcome};
use quantick_session::{HistoryAct, HistoryOutcome, SessionSnapshot};
use quantick_sim::Order;
use quantick_sources::history_reach::{HistoryReach, ReachProgress};
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use quantick_sources::recovery::Recovery;
use rust_decimal::Decimal;

use crate::JournalAccess;
use crate::addressed_pane::AddressedPanePort;
use crate::analysis::{AnalysisPaneEntry, AnalysisTab};
use crate::annotate::AnnotateAccess;
use crate::attention::MarkTab;
use crate::axis_pane::{AxisPane, NativeTape};
use crate::bubble_pane::{AssetSave, BubbleLook, BubblePane, PaneBubblesPort, TickSpherePane};
use crate::chart::{ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane, PaneFraming};
use crate::chart_layer::{LayerBlocked, LayerRead};
use crate::chart_settings::ChartStylePort;
use crate::contract_roll::AutoRoll;
use crate::deal_recorder::{DealRecorder, DealRecordingPort};
use crate::drawing_magnet::RailMagnetPort;
use crate::indicator_host::{
    AttachedScript, IndicatorControlHost, IndicatorGuidePort, IndicatorOwner, IndicatorPickerPort,
    IndicatorRefusal, NativePort, ScriptEditorPort, ScriptsPort,
};
use crate::market_tab::FeedSelection;
use crate::market_tab::RecoveredFeed;
use crate::market_tab::{LentTab, MarketTab, MarketTabHost};
use crate::notify::{Attention, AttentionHost, NotifyAccess};
use crate::pane_marks::PaneMarks;
use crate::settings::{SettingsAct, SettingsOwner, SettingsReadings, SettleError};
use crate::tabs::{OpenTabs, TabDirectory};
use crate::trade::{PaperHost, PaperOwner, TradeDesk};
use quantick_control::error::ControlError;
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_control_schema::indicator_picker::{PickerInput, PickerResult};
use quantick_control_schema::layers::LayerSnapshot;
use quantick_control_schema::native_indicator as wire;
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_control_schema::script_editor::{
    EditorResult, NumericValue, ScriptDocument, ScriptEditorAction,
};
use quantick_control_schema::tape_view::TapeViewSnapshot;
use quantick_drawings::{DrawingTool, Drawings};
use quantick_orderflow::LaneWindow;
use quantick_orderflow::tape_view::TapeEnd;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_replay::recorder::{DealRecordingError, RecordingView};
use quantick_stores::bubble_asset_store::SaveSwitch;
use quantick_stores::candle_presets::PresetError;
use quantick_stores::config::AppConfig;
use quantick_stores::indicator_state::SavedIndicator;

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
    fn layers(&self, _: &ChartStyle) -> Vec<LayerSnapshot> {
        self.pane
            .layers
            .iter()
            .map(|(layer, _)| layer.clone())
            .collect()
    }
    fn layer(&self, layer_id: &str, _: &ChartStyle) -> Option<LayerRead> {
        let (layer, blocked) = self
            .pane
            .layers
            .iter()
            .find(|(layer, _)| layer.id == layer_id)?;
        Some(LayerRead {
            layer: layer.clone(),
            blocked: *blocked,
        })
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
    pub recoveries: Vec<Recovery>,
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
    pub auto_roll: AutoRoll,
    /// What the last History act on the tab did.
    pub last_history: Option<HistoryOutcome>,
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
            auto_roll: AutoRoll::default(),
            last_history: None,
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

    fn snapshot(&self, outcome: Option<HistoryOutcome>) -> SessionSnapshot {
        let (last_press, last_cancel) = match outcome {
            Some(HistoryOutcome::Pressed(press)) => (Some(press), None),
            Some(HistoryOutcome::Cancelled(cancelled)) => (None, Some(cancelled)),
            None => (None, None),
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

/// The main-click default a fake window opens with.
pub(crate) const FAKE_DEFAULT_REACH: HistoryReach = HistoryReach::Sessions(9);

/// One fake feed: its id, the symbols it offers and the contract each of
/// those rolls onto.
pub(crate) type FakeFeed<'a> = (&'a str, &'a [&'a str], &'a [(&'a str, &'a str)]);

/// A config over `feeds`.
pub(crate) fn fake_config(feeds: &[FakeFeed<'_>]) -> AppConfig {
    let feeds: Vec<serde_json::Value> = feeds
        .iter()
        .map(|(id, symbols, rolls)| {
            let rolls: std::collections::BTreeMap<_, _> = rolls.iter().copied().collect();
            serde_json::json!({
                "id": id,
                "name": id,
                "provider": "binance",
                "symbols": symbols,
                "contract_rolls": rolls,
            })
        })
        .collect();
    let config =
        serde_json::json!({ "default_feed": "fake", "default_symbol": "FAKE", "feeds": feeds });
    serde_json::from_value(config).expect("a valid config")
}

/// The fake tab's market acts over its plain fields.
impl MarketTab for FakeTab {
    fn market(&self) -> FeedSelection {
        FeedSelection {
            feed_id: self.feed_id.clone(),
            symbol: self.symbol.clone(),
            replay: self.replaying,
        }
    }
    fn switch_market(&mut self, _config: &AppConfig, feed_id: &str, symbol: &str) {
        (self.feed_id, self.symbol) = (feed_id.to_owned(), symbol.to_owned());
    }
    fn paper_open(&self) -> bool {
        false
    }
    fn unlisted(&self) -> Option<String> {
        None
    }
    fn roll_memory(&mut self) -> &mut AutoRoll {
        &mut self.auto_roll
    }
    fn recover(&mut self, _config: &AppConfig, recovery: Recovery) -> RecoveredFeed {
        self.recoveries.push(recovery);
        RecoveredFeed {
            symbol: self.symbol.clone(),
            respawned: self.respawns,
        }
    }
    fn pages_history(&self, _config: &AppConfig) -> bool {
        self.history_paging
    }
    fn history(&mut self, _config: &AppConfig, act: HistoryAct) -> HistoryOutcome {
        let outcome = match act {
            HistoryAct::Press(reach) => HistoryOutcome::Pressed(self.press(reach)),
            HistoryAct::Cancel => HistoryOutcome::Cancelled(self.cancel()),
        };
        self.last_history = Some(outcome);
        outcome
    }
    fn session(&self) -> SessionSnapshot {
        self.snapshot(self.last_history)
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
    pub default_reach: HistoryReach,
    /// The config the window lends its tabs.
    pub config: AppConfig,
    /// The tool rail's magnet.
    pub rail_magnet: bool,
    /// What the chart settings owner answers, and every input it was given.
    pub chart_settings: Result<ChartSettingsResult, PresetError>,
    pub settings_asked: Vec<ChartSettingsInput>,
    /// Every layer switch asked: pane, layer and visibility.
    pub layer_switches: Vec<(u64, String, bool)>,
    /// The window's shared chart style.
    pub style: ChartStyle,
    /// The record-by-default setting, once a call kept one.
    pub record_by_default: Option<bool>,
    /// The bound workspace's indicator owner.
    pub indicators: FakeIndicators,
    /// The tabs whose market has the price tag's progress switched off; each
    /// fake tab shows one market.
    pub progress_off: Vec<u64>,
}

/// A fake indicator owner: what each indicator and script call reached it
/// with, and the refusal it answers when a test sets one.
#[derive(Default)]
pub(crate) struct FakeIndicators {
    /// Every script attached: name, source, attached by an operator.
    pub attached: Vec<(String, String, bool)>,
    /// Every guide set: tab, pane, slot, on.
    pub guides: Vec<(u64, u64, u64, bool)>,
    /// Every library script added to the chart.
    pub added: Vec<String>,
    /// Every editor action run.
    pub actions: Vec<ScriptEditorAction>,
    /// Every native edit applied: slot, edit.
    pub applied: Vec<(Option<u64>, wire::Prepared)>,
    /// The saved native indicators every target reads.
    pub saved: Vec<(u64, SavedIndicator, bool)>,
    /// What the picker, guide, editor and `saved` calls answer instead of
    /// succeeding; attach, detach and `apply` keep their own rules.
    pub refusal: Option<IndicatorRefusal>,
}

impl FakeIndicators {
    fn refused(&self) -> Result<(), IndicatorRefusal> {
        self.refusal.clone().map_or(Ok(()), Err)
    }
}

impl IndicatorPickerPort for &mut FakeIndicators {
    fn indicator_picker(&mut self, input: PickerInput) -> Result<PickerResult, IndicatorRefusal> {
        self.refused()?;
        Ok(PickerResult {
            open: true,
            tab_id: None,
            pane_id: None,
            query: String::new(),
            category: String::new(),
            categories: Vec::new(),
            entries: Vec::new(),
            total: 0,
            next_offset: Some(input.offset),
        })
    }
}

impl ScriptsPort for &mut FakeIndicators {
    fn attach_script(&mut self, name: String, source: String, by_operator: bool) -> AttachedScript {
        self.attached.push((name, source, by_operator));
        AttachedScript {
            tab_id: 1,
            pane_side: PaneSideDto::Flow,
            slot: self.attached.len() as u64 - 1,
        }
    }
    fn detach_operator_script(&mut self, slot: u64) -> Result<bool, IndicatorRefusal> {
        let Some((_, _, by_operator)) = self.attached.get(slot as usize) else {
            return Ok(false);
        };
        if *by_operator {
            Ok(true)
        } else {
            Err(IndicatorRefusal::TradersOwn)
        }
    }
}

impl IndicatorGuidePort for &mut FakeIndicators {
    fn set_indicator_guide(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        slot: u64,
        enabled: bool,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.guides.push((tab_id, pane_id, slot, enabled));
        Ok(())
    }
}

impl ScriptEditorPort for &mut FakeIndicators {
    fn set_script_numeric_input(
        &mut self,
        _id: &str,
        _name: &str,
        _value: NumericValue,
    ) -> Result<(), IndicatorRefusal> {
        self.refused()
    }
    fn add_library_script(&mut self, id: &str) -> Result<(), IndicatorRefusal> {
        self.refused()?;
        self.added.push(id.to_owned());
        Ok(())
    }
    fn script_editor(
        &mut self,
        action: ScriptEditorAction,
    ) -> Result<Option<ScriptDocument>, IndicatorRefusal> {
        self.refused()?;
        self.actions.push(action);
        Ok(None)
    }
    fn script_library_state(&self, _offset: usize, _limit: usize) -> EditorResult {
        EditorResult::default()
    }
}

impl NativePort for &mut FakeIndicators {
    fn saved(
        &mut self,
        _target: &wire::Target,
    ) -> Result<Vec<(u64, SavedIndicator, bool)>, IndicatorRefusal> {
        self.refused()?;
        Ok(self.saved.clone())
    }
    fn settings_open(&self) -> bool {
        false
    }
    fn apply(
        &mut self,
        _target: &wire::Target,
        slot: Option<u64>,
        edit: wire::Prepared,
    ) -> Result<u64, IndicatorRefusal> {
        self.applied.push((slot, edit));
        Ok(slot.unwrap_or(7))
    }
}

impl FakeWindow {
    pub fn new(tabs: Vec<FakeTab>) -> Self {
        Self {
            tabs,
            active: 0,
            popups: Vec::new(),
            toasts: Vec::new(),
            speaker_refusal: None,
            default_reach: FAKE_DEFAULT_REACH,
            config: fake_config(&[]),
            rail_magnet: false,
            chart_settings: Err(PresetError::Refused("the fake keeps no chart settings")),
            settings_asked: Vec::new(),
            layer_switches: Vec::new(),
            style: ChartStyle::default(),
            record_by_default: None,
            indicators: FakeIndicators::default(),
            progress_off: Vec::new(),
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

impl IndicatorControlHost for FakeWindow {
    fn indicators(&mut self) -> Box<dyn IndicatorOwner + '_> {
        Box::new(&mut self.indicators)
    }
}

impl MarketTabHost for FakeWindow {
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn market_tab(&self, index: usize) -> Option<&dyn MarketTab> {
        Some(self.tabs.get(index)?)
    }
    fn market_tab_mut(&mut self, index: usize) -> Option<LentTab<'_>> {
        Some(LentTab {
            tab: self.tabs.get_mut(index)?,
            config: &self.config,
            history_default: &mut self.default_reach,
        })
    }
    fn drawing_gesture_on(&self, _feed_id: &str, _from: &str, _to: &str) -> bool {
        false
    }
    fn carry_market_drawings(&mut self, _feed_id: &str, _from: &str, _to: &str) -> usize {
        0
    }
}

/// The fake window's desk: its tabs' recorders and the record-by-default row.
struct FakeDesk<'a> {
    tabs: &'a mut Vec<FakeTab>,
    record_by_default: &'a mut Option<bool>,
}

impl crate::desk::DeskHost for FakeWindow {
    fn desk(&mut self) -> Option<Box<dyn crate::desk::DeskOwner + '_>> {
        let (tabs, record_by_default) = (&mut self.tabs, &mut self.record_by_default);
        Some(Box::new(FakeDesk {
            tabs,
            record_by_default,
        }))
    }
}

impl DealRecordingPort for FakeDesk<'_> {
    fn deal_recorder(&mut self, index: usize) -> Option<&mut dyn DealRecorder> {
        Some(self.tabs.get_mut(index)?)
    }
}

impl SettingsOwner for FakeDesk<'_> {
    fn readings(&self) -> SettingsReadings {
        SettingsReadings {
            record_deals: self.record_by_default.unwrap_or_default(),
            ..SettingsReadings::default()
        }
    }
    fn settle(&mut self, act: SettingsAct) -> Result<(), SettleError> {
        if let SettingsAct::RecordDeals(on) = act {
            *self.record_by_default = Some(on);
        }
        Ok(())
    }
}

impl crate::desk::DeskOwner for FakeDesk<'_> {
    fn act(&mut self, _act: crate::desk::DeskAct) -> bool {
        false
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

/// The fake window lends its own tabs, beside the config it lends a market
/// call and its rail's magnet.
impl OpenTabs for FakeWindow {
    type Tab = FakeTab;
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn open_tab(&self, index: usize) -> Option<&FakeTab> {
        self.tabs.get(index)
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn rail_magnet(&self) -> bool {
        self.rail_magnet
    }
}

/// A fake tab's panes carry no analysis.
impl AnalysisTab for FakeTab {
    fn analysis_panes(&self) -> Vec<AnalysisPaneEntry<'_>> {
        Vec::new()
    }
}

impl RailMagnetPort for FakeWindow {
    fn switch_rail_magnet(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.rail_magnet, on) != on
    }
}

/// The fake window's attention lanes are its own fields, lent for the call.
impl AttentionHost for FakeWindow {
    fn attention(&mut self) -> Box<dyn Attention + '_> {
        Box::new(self)
    }
}

impl Attention for &mut FakeWindow {
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
impl PaperHost for FakeWindow {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(NoDesk)
    }
}

/// A paper owner with no chart open and an empty journal it never writes.
pub(crate) struct NoDesk;

impl PaperOwner for NoDesk {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        None
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        None
    }
    fn persist(&mut self, _change: SharedChange) {}
    fn journal_scope(&self) -> JournalScope {
        JournalScope {
            dir: std::path::PathBuf::from("no-such-trades-folder"),
            live: Vec::new(),
        }
    }
    fn journal_timezone(&self) -> TzOffset {
        TzOffset::new(0)
    }
    fn edit_journal(
        &mut self,
        _edit: &JournalEdit,
    ) -> Result<JournalEditOutcome, SessionEditError> {
        Err(SessionEditError::Io("the fake keeps no journal".to_owned()))
    }
}

/// A test's own paper owner is its own door, lent for the one call.
impl<T: PaperOwner> PaperHost for T {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(self)
    }
}

impl<T: PaperOwner + ?Sized> PaperOwner for &mut T {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        (**self).active_desk()
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        (**self).active_desk_mut()
    }
    fn persist(&mut self, change: SharedChange) {
        (**self).persist(change);
    }
    fn journal_scope(&self) -> JournalScope {
        (**self).journal_scope()
    }
    fn journal_timezone(&self) -> TzOffset {
        (**self).journal_timezone()
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        (**self).edit_journal(edit)
    }
    fn route_call(&mut self) -> Option<quantick_paper::route::RouteCall<'_>> {
        (**self).route_call()
    }
    fn account_route(
        &mut self,
    ) -> Option<quantick_control_schema::trade_account::AccountRouteResult> {
        (**self).account_route()
    }
    fn request_route(
        &mut self,
        request: quantick_paper::route::RouteRequest,
    ) -> Result<(), quantick_paper::route::Refusal> {
        (**self).request_route(request)
    }
    fn broker_write_block(
        &mut self,
        actor: quantick_paper::route::Actor,
    ) -> Option<quantick_paper::route::Refusal> {
        (**self).broker_write_block(actor)
    }
}

/// One paper desk, as a window hands the active tab's over: the tab's own
/// paper (an account and the desk's ruler and ticket around it), the Trades
/// ledger's view, and every outcome a control call ran through it, for a
/// parity test to hold against the tab's own surfaces.
pub(crate) struct PaperDesk {
    pub paper: TabPaper,
    pub view: LedgerView,
    pub outcomes: Vec<TradeOutcome>,
}

impl PaperDesk {
    pub fn new(account: PaperAccount) -> Self {
        Self {
            paper: TabPaper::new(account),
            view: LedgerView::default(),
            outcomes: Vec::new(),
        }
    }
}

impl std::ops::Deref for PaperDesk {
    type Target = TabPaper;
    fn deref(&self) -> &TabPaper {
        &self.paper
    }
}

impl std::ops::DerefMut for PaperDesk {
    fn deref_mut(&mut self) -> &mut TabPaper {
        &mut self.paper
    }
}

impl TradeDesk for PaperDesk {
    fn account(&self) -> &PaperAccount {
        &self.account
    }
    fn account_mut(&mut self) -> &mut PaperAccount {
        &mut self.paper.account
    }
    fn ruler_ticks(&self) -> u32 {
        self.desk.ruler.notches
    }
    fn set_ruler_ticks(&mut self, ticks: u32) {
        self.paper.desk.ruler.set_ticks(ticks);
    }
    fn execute(&mut self, command: TradeCommand) -> TradeOutcome {
        let outcome = self.paper.execute(command);
        self.outcomes.push(outcome.clone());
        outcome
    }
    fn mark_price(&self) -> Option<Decimal> {
        self.account.mark_price()
    }
    fn working_orders(&self) -> &[Order] {
        self.account.working_orders()
    }
    fn risk_report(&self) -> (RiskState, bool) {
        self.desk.risk_report(&self.account)
    }
    fn position_summary(&self) -> Option<PositionSummary> {
        self.account.position_summary()
    }
    fn selected_trade_index(&self) -> Option<usize> {
        None
    }
    fn ledger_view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool) {
        let moved = self.view.apply(change);
        (self.view.clone(), moved)
    }
}

/// A fake tab has no pointer: a mark resolved against it names the tab and
/// its first pane and says why the pointer is missing.
impl MarkTab for FakeTab {
    fn cursor_snapshot(&self, tab_id: u64, _config: &AppConfig) -> CursorSnapshot {
        CursorSnapshot {
            active_tab_id: WireU64::new(tab_id),
            focused_pane_id: WireU64::new(self.panes.first().map_or(0, |pane| pane.id)),
            focused_pane_side: PaneSideDto::Flow,
            pointer: None,
            pointer_availability: unavailable("fake_window_has_no_pointer"),
            semantic_scene: unavailable("fake_window_has_no_pointer"),
        }
    }
}

impl AddressedPanePort for FakeWindow {
    type Pane = FakePane;
    fn addressed_pane(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut FakePane, ControlError> {
        self.pane_mut(tab_id, pane_id)
    }
}

impl ChartStylePort for FakeWindow {
    fn apply_chart_settings(
        &mut self,
        input: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        self.settings_asked.push(input);
        self.chart_settings.clone()
    }
    fn chart_style(&self) -> &ChartStyle {
        &self.style
    }
    fn switch_chart_layer(&mut self, tab_id: u64, pane_id: u64, layer_id: &str, visible: bool) {
        let Ok(pane) = self.pane_mut(WireU64::new(tab_id), WireU64::new(pane_id)) else {
            return;
        };
        let layer = pane
            .layers
            .iter_mut()
            .find(|(layer, _)| layer.id == layer_id);
        let Some((layer, _)) = layer else { return };
        layer.requested = visible;
        self.layer_switches
            .push((pane_id, layer_id.to_owned(), visible));
    }
    /// The switch is kept for the pane's tab, the market it shows; a repeat
    /// is no change.
    fn set_price_tag_progress(
        &mut self,
        tab: u64,
        pane: u64,
        on: bool,
    ) -> Result<bool, ControlError> {
        self.pane_mut(WireU64::new(tab), WireU64::new(pane))?;
        let was_on = !self.progress_off.contains(&tab);
        self.progress_off.retain(|off| *off != tab);
        if !on {
            self.progress_off.push(tab);
        }
        Ok(was_on != on)
    }
}

impl ChartPort for FakeWindow {
    fn price_tag_progress(&self, tab_id: u64) -> bool {
        !self.progress_off.contains(&tab_id)
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
    /// The actor and client a replayed run recorded, when the call replays one.
    pub replayed: Option<(ActorKind, String)>,
}

impl FakeAccess {
    pub fn with_budget(budget: usize) -> Self {
        Self {
            budget,
            retry_after: Duration::from_millis(1_500),
            events: Vec::new(),
            replayed: None,
        }
    }
}

impl Default for FakeAccess {
    fn default() -> Self {
        Self::with_budget(0)
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

impl AnnotateAccess for FakeAccess {
    fn recorded_actor(&self) -> Option<(ActorKind, &str)> {
        self.replayed
            .as_ref()
            .map(|(kind, client)| (*kind, client.as_str()))
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
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

/// A window lending its tabs, by id in screen order, beside the config and
/// the rail's magnet: the fake host of every scope that walks [`OpenTabs`].
pub(crate) struct TabsWindow<T> {
    pub tabs: Vec<(u64, T)>,
    pub magnet: bool,
    pub config: AppConfig,
}

impl<T> TabsWindow<T> {
    pub fn new(tabs: Vec<(u64, T)>) -> Self {
        Self {
            tabs,
            magnet: false,
            config: fake_config(&[]),
        }
    }
}

impl<T> TabDirectory for TabsWindow<T> {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|(id, _)| *id == tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tabs[index].0
    }
}

impl<T> OpenTabs for TabsWindow<T> {
    type Tab = T;
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn open_tab(&self, index: usize) -> Option<&T> {
        self.tabs.get(index).map(|(_, tab)| tab)
    }
    fn config(&self) -> &AppConfig {
        &self.config
    }
    fn rail_magnet(&self) -> bool {
        self.magnet
    }
}
