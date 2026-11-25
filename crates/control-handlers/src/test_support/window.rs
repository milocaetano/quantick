#![cfg(test)]

use super::{FAKE_DEFAULT_REACH, FakeIndicators, FakePane, FakeTab, fake_config};
use crate::bubble_pane::{BubblePane, PaneBubblesPort};
use crate::drawing_magnet::RailMagnetPort;
use crate::indicator_host::{IndicatorControlHost, IndicatorOwner};
use crate::market_tab::{LentTab, MarketTabHost};
use crate::notify::{Attention, AttentionHost};
use crate::tabs::{OpenTabs, TabDirectory};
use quantick_chart::style::ChartStyle;
use quantick_control::error::ControlError;
use quantick_control::id::{ConnectionId, PrincipalId, RequestId};
use quantick_control::wire::{ActorContext, ActorKind, WireU64};
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_control_schema::notify::AgentPopup;
use quantick_sources::history_reach::HistoryReach;
use quantick_stores::candle_presets::PresetError;
use quantick_stores::config::AppConfig;

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
    pub(super) fn pane_mut(
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
