//! A fake window and a fake access: every port a family names, over plain
//! fields a test sets and reads back. The second implementation of each port,
//! after the application's own adapter.

use std::time::Duration;

use quantick_chart::state::ChartState;
use quantick_control::{
    id::{ConnectionId, PrincipalId, RequestId},
    wire::{ActorContext, ActorKind, CanonicalDecimal, WireU64},
};
use quantick_control_host::{journal::NewEvent, wire::PaneSideDto};
use quantick_control_schema::chart::{BarProvenanceContext, ViewportSnapshot};
use quantick_control_schema::notify::AgentPopup;
use quantick_engine::{Bar, BarSpec, Side, Trade};
use quantick_sources::history_reach::HistoryReach;
use quantick_sources::history_run::{Cancelled, Press, RunStatus};
use rust_decimal::Decimal;

use crate::chart::{ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane};
use crate::history::{HistoryPort, HistoryRun};
use crate::notify::{AttentionPort, NotifyAccess};
use crate::recovery::{FeedRecoveryPort, RecoveredFeed};
use crate::tabs::TabDirectory;

/// One pane of a fake tab: a real chart model, an optional venue prefix, and
/// the framing a frame would have laid out.
pub(crate) struct FakePane {
    pub id: u64,
    pub state: ChartState,
    pub prefix: Vec<Bar>,
    pub pagination_revision: u64,
    pub visible_slots: Option<(usize, usize)>,
    pub record_starts_inside: bool,
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
        }
    }
}

impl ChartPaneSource for FakePane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn state(&self) -> &ChartState {
        &self.state
    }
    fn pagination_revision(&self) -> u64 {
        self.pagination_revision
    }
    fn closed_slots(&self) -> usize {
        self.prefix.len() + self.state.bars().len()
    }
    fn seam_slot(&self) -> usize {
        self.prefix.len()
    }
    fn closed_bar(&self, slot: usize) -> Option<&Bar> {
        self.prefix
            .get(slot)
            .or_else(|| self.state.bars().get(slot - self.prefix.len()))
    }
    fn venue_prefix(&self) -> &[Bar] {
        &self.prefix
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self.state.slot_open_time(slot - self.prefix.len()),
        }
    }
    fn visible_slots(&self) -> Option<(usize, usize)> {
        self.visible_slots
    }
    fn viewport(&self) -> ViewportSnapshot {
        let (start, end) = self.visible_slots.unwrap_or((0, 0));
        ViewportSnapshot {
            geometry_available: self.visible_slots.is_some(),
            visible_start_slot: WireU64::new(start as u64),
            visible_end_slot_exclusive: WireU64::new(end as u64),
            pixels_per_bar: CanonicalDecimal::new("8").expect("canonical"),
            right_edge_bar: CanonicalDecimal::new("0").expect("canonical"),
            follows_live: true,
            price_auto_fit: true,
            price_axis_inverted: false,
            price_range: None,
            chart_width_px: None,
            tape: None,
        }
    }
    fn history_paging(&self) -> bool {
        true
    }
    fn venue_record_starts_inside(&self, _interval_ms: i64) -> bool {
        self.record_starts_inside
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
    pub history: RunStatus,
    pub main_reach: HistoryReach,
    pub panes: Vec<FakePane>,
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
            history: RunStatus::Idle,
            main_reach: HistoryReach::Hours(1),
            panes: Vec::new(),
        }
    }

    fn run(&self) -> HistoryRun {
        HistoryRun {
            status: self.history,
            main_reach: self.main_reach,
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
}

impl FakeWindow {
    pub fn new(tabs: Vec<FakeTab>) -> Self {
        Self {
            tabs,
            active: 0,
            popups: Vec::new(),
            toasts: Vec::new(),
            speaker_refusal: None,
        }
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

impl FeedRecoveryPort for FakeWindow {
    fn recover_feed(&mut self, index: usize, keep_timeline: bool) -> Option<RecoveredFeed> {
        let tab = self.tabs.get_mut(index)?;
        tab.recoveries.push(keep_timeline);
        Some(RecoveredFeed {
            symbol: tab.symbol.clone(),
            respawned: tab.respawns,
        })
    }
}

impl HistoryPort for FakeWindow {
    fn history_paging(&self, index: usize) -> Option<bool> {
        self.tabs.get(index).map(|tab| tab.history_paging)
    }
    fn press_history(&mut self, index: usize, reach: HistoryReach) -> Option<(Press, HistoryRun)> {
        let tab = self.tabs.get_mut(index)?;
        tab.main_reach = reach;
        let press = match tab.history {
            RunStatus::Idle => {
                tab.history = RunStatus::Queued(reach);
                Press::Start
            }
            _ => Press::AlreadyRunning,
        };
        Some((press, tab.run()))
    }
    fn cancel_history(&mut self, index: usize) -> Option<(Cancelled, HistoryRun)> {
        let tab = self.tabs.get_mut(index)?;
        let cancelled = match tab.history {
            RunStatus::Queued(reach) => Cancelled::Queued(reach),
            _ => Cancelled::Nothing,
        };
        tab.history = RunStatus::Idle;
        Some((cancelled, tab.run()))
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

impl ChartPort for FakeWindow {
    fn chart_panes(&self) -> Vec<ChartPaneEntry<'_>> {
        self.tabs
            .iter()
            .enumerate()
            .flat_map(|(index, tab)| {
                tab.panes
                    .iter()
                    .enumerate()
                    .map(move |(slot, pane)| entry(tab, slot, pane, index == self.active))
            })
            .collect()
    }
    fn chart_pane(&self, tab_id: u64, pane_id: u64) -> Result<ChartPaneEntry<'_>, MissingPane> {
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
        Ok(entry(tab, slot, pane, false))
    }
}

fn entry<'a>(tab: &'a FakeTab, slot: usize, pane: &'a FakePane, shown: bool) -> ChartPaneEntry<'a> {
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
        visible: shown,
        focused: shown && slot == 0,
        pane: Box::new(FakePaneRef(pane)),
    }
}

/// A borrowed pane, so an entry can lend the fake's own.
struct FakePaneRef<'a>(&'a FakePane);

impl ChartPaneSource for FakePaneRef<'_> {
    fn pane_id(&self) -> u64 {
        self.0.pane_id()
    }
    fn state(&self) -> &ChartState {
        self.0.state()
    }
    fn pagination_revision(&self) -> u64 {
        self.0.pagination_revision()
    }
    fn closed_slots(&self) -> usize {
        self.0.closed_slots()
    }
    fn seam_slot(&self) -> usize {
        self.0.seam_slot()
    }
    fn closed_bar(&self, slot: usize) -> Option<&Bar> {
        self.0.closed_bar(slot)
    }
    fn venue_prefix(&self) -> &[Bar] {
        self.0.venue_prefix()
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        self.0.slot_open_time(slot)
    }
    fn visible_slots(&self) -> Option<(usize, usize)> {
        self.0.visible_slots()
    }
    fn viewport(&self) -> ViewportSnapshot {
        self.0.viewport()
    }
    fn history_paging(&self) -> bool {
        self.0.history_paging()
    }
    fn venue_record_starts_inside(&self, interval_ms: i64) -> bool {
        self.0.venue_record_starts_inside(interval_ms)
    }
    fn provenance(&self) -> BarProvenanceContext {
        self.0.provenance()
    }
}

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
