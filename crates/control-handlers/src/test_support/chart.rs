#![cfg(test)]

use super::pane::fake_viewport;
use super::{FakePane, FakeTab, FakeWindow};
use crate::addressed_pane::AddressedPanePort;
use crate::chart::{ChartPaneEntry, ChartPaneSource, ChartPort, MissingPane, PaneFraming};
use crate::chart_layer::LayerRead;
use crate::chart_settings::ChartStylePort;
use crate::wire::chart::{BarProvenanceContext, ViewportSnapshot};
use crate::wire::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use crate::wire::layers::LayerSnapshot;
use quantick_chart::state::ChartState;
use quantick_chart::style::ChartStyle;
use quantick_control::error::ControlError;
use quantick_control::wire::WireU64;
use quantick_control_host::wire::PaneSideDto;
use quantick_engine::Bar;
use quantick_stores::candle_presets::PresetError;

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
