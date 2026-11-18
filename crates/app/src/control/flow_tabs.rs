//! The window's order-flow tab jobs: resolve the tab a call or a scope names,
//! lend what only the window holds — the footprint setup, the theme's lights
//! and the panel folds — and let the tab's panes and panel answer.
use crate::app::control_host::ControlPort;
use crate::{orderflow_panel::panel_seat, pane::visit_orderflow_panes};
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::orderflow::{OrderflowPaneEntry, OrderflowPort, PanelVisit};
use quantick_control_handlers::orderflow_settings::{OrderflowSettingsPort, PanelSeat};
use quantick_control_handlers::tab_index;
impl OrderflowPort for dyn ControlPort {
    fn visit_orderflow_tabs(&self, visit: &mut dyn FnMut(u64, &[OrderflowPaneEntry<'_>])) {
        let reads = self.tab_reads();
        let window = reads.footprint_config();
        for (tab_id, tab) in reads.tabs().iter_with_ids() {
            visit_orderflow_panes(tab, window, |entries| visit(tab_id, entries));
        }
    }
    fn visit_orderflow_panels(&self, visit: &mut PanelVisit<'_>) {
        let (tabs, lit) = (self.tab_reads().tabs(), self.appearance().has_lit_flow());
        for (tab_id, tab) in tabs.iter_with_ids() {
            let panel = (&tab.tape().settings_panel, lit);
            visit(tab_id, &tab.symbol, panel, tabs.panel_folds());
        }
    }
}
impl OrderflowSettingsPort for dyn ControlPort {
    fn panel_seat(&mut self, tab: u64) -> Result<PanelSeat<'_>, ControlError> {
        let tab_id = Some(WireU64::new(tab));
        let index = tab_index(self, tab_id)?;
        let lights_offered = self.appearance().has_lit_flow();
        let (tab, folds) = self
            .tabs_mut()
            .tab_with_panel_folds(index)
            .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
        Ok(panel_seat(tab, folds, lights_offered))
    }
}
