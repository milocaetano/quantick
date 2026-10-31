//! The indicator owner's worker, layout and control recipients at the frame boundary.
use super::QuantickApp;
use super::indicator_manager::{IndicatorEdit, SettingsChange};
use super::{GatewayPort, LayoutPort};
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::indicator_legend::LegendAction;
use crate::indicator_worker::SlotId;
use crate::pane::PaneSide;
use eframe::egui;
use smallvec::SmallVec;

impl QuantickApp {
    /// Open the indicator library on the pane at `side`. Focus moves there first:
    /// every indicator command lands on the focused pane (§11), so the library
    /// and what it attaches can only agree if the pane it was opened for has focus.
    pub(crate) fn open_indicator_picker_on(&mut self, side: PaneSide) {
        self.active_tab_mut().focus = side;
        let target = (self.runtime.tabs.active_id(), self.focused_pane().id);
        self.surfaces.indicator_picker.state.open(target);
    }

    /// Execute persistence only after the owner has prepared the indicator edit.
    pub(super) fn apply_indicator_edit(&mut self, edit: IndicatorEdit) {
        if self.runtime.validate_target(edit.target()).is_err() {
            return;
        }
        self.layout_adapter().apply_indicator_edit(edit);
    }

    pub(super) fn apply_indicator_settings_change(&mut self, change: SettingsChange) {
        if change
            .target()
            .is_some_and(|target| self.runtime.validate_target(target).is_err())
        {
            return;
        }
        let worker = change.worker_target().and_then(|target| {
            self.runtime
                .tabs
                .by_id(target.tab)
                .map(|tab| &tab.pane(target.side).indicator_worker)
        });
        if let Some(edit) = change.deliver(worker) {
            self.apply_indicator_edit(edit);
        }
    }

    /// Actions carry the pane that drew them, never a later focus lookup.
    pub(super) fn apply_indicator_legend_action(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        action: LegendAction,
    ) {
        let Some(tab) = self.runtime.tabs.by_id_mut(tab_id) else {
            return;
        };
        if let Some(edit) =
            self.runtime
                .indicators
                .legend_action(tab.pane_mut(side), (tab_id, side), action)
        {
            self.apply_indicator_edit(edit);
        }
    }

    /// Existing hook and pane requests are resolved at their original frame phase.
    pub(super) fn service_indicator_requests(&mut self) {
        let tab_id = self.runtime.tabs.active_id();
        #[cfg(any(feature = "scenario-harness", test))]
        {
            let tab = &self.runtime.tabs[self.runtime.tabs.active_index()];
            if self.runtime.indicators.open_autostart(
                tab_id,
                (
                    tab.focused_side(),
                    tab.focused_pane().id,
                    tab.focused_pane().indicators.all(),
                ),
                (tab.flow_pane.id, tab.flow_pane.indicators.all()),
                self.chrome.harness.settings_autostart(),
            ) {
                self.chrome.harness.settings_autostart_opened();
            }
        }
        let requests: SmallVec<[(PaneSide, SlotId); MAX_CANVAS_PANES]> = self
            .active_tab_mut()
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| pane.take_settings_request().map(|slot| (side, slot)))
            .collect();
        for (side, slot) in requests {
            self.apply_indicator_legend_action(tab_id, side, LegendAction::OpenSettings(slot));
        }
        if let Some(side) = self.active_tab_mut().take_indicator_picker_request() {
            self.open_indicator_picker_on(side);
        }
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(index) = self.chrome.harness.indicator_mouse_line() {
            let target = self
                .active_tab()
                .flow_pane
                .indicators
                .all()
                .get(index)
                .map(|view| (self.active_tab().flow_pane.id, view.slot));
            if let Some((pane_id, slot)) = target {
                self.chrome.harness.indicator_mouse_line_opened();
                let _ = self.control_action(crate::control::INDICATOR_GUIDE_CAPABILITY_ID, 1, crate::control::ActionOrigin::Human,
                    serde_json::json!({"tab_id":tab_id.to_string(),"pane_id":pane_id.to_string(),"slot_id":slot.0.to_string(),"enabled":true}));
            }
        }
        // The hook action is admitted before collecting actual pane requests.
        let requests: SmallVec<[(PaneSide, SlotId, bool); MAX_CANVAS_PANES]> = self
            .active_tab_mut()
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| {
                pane.take_indicator_guide_request()
                    .map(|(slot, enabled)| (side, slot, enabled))
            })
            .collect();
        for (side, slot, enabled) in requests {
            let pane_id = self.active_tab().pane(side).id;
            let _ = self.control_action(crate::control::INDICATOR_GUIDE_CAPABILITY_ID, 1, crate::control::ActionOrigin::Human,
                serde_json::json!({"tab_id":tab_id.to_string(),"pane_id":pane_id.to_string(),"slot_id":slot.0.to_string(),"enabled":enabled}));
        }
    }

    pub(super) fn draw_indicator_surfaces(&mut self, ctx: &egui::Context) {
        #[cfg(any(feature = "scenario-harness", test))]
        {
            let tab = &self.runtime.tabs[self.runtime.tabs.active_index()];
            if self.runtime.indicators.open_first_editable(
                self.runtime.tabs.active_id(),
                (tab.flow_pane.id, tab.flow_pane.indicators.all()),
                self.chrome.harness.wants_indicator_settings_dialog(),
            ) {
                self.chrome.harness.indicator_settings_dialog_opened();
            }
        }
        if let Some(change) = self.runtime.draw_indicator_settings(ctx) {
            self.apply_indicator_settings_change(change);
        }
        let tab_id = self.runtime.tabs.active_id();
        let split = self.active_tab().shows_context_charts();
        let shown = self.active_tab().context_panes_shown();
        let sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> = self.active_tab().sides().collect();
        let mut pending = Vec::new();
        for side in sides {
            let visible = !matches!(side, PaneSide::Time(slot) if !split || slot >= shown);
            let pane = self.active_tab().pane(side);
            let position_open = visible
                && pane.frame.chart_area.is_some()
                && pane.paper_hud_anchor().is_some()
                && self.active_tab().paper.position_summary().is_some();
            let pane = self
                .runtime
                .tabs
                .runtime_mut(self.runtime.tabs.active_index())
                .pane_mut(side);
            for action in self.runtime.indicators.draw_legend(
                ctx,
                pane,
                (tab_id, side),
                visible,
                position_open,
            ) {
                pending.push((side, action));
            }
        }
        // Do not interleave mutations with drawing legends on other panes.
        for (side, action) in pending {
            self.apply_indicator_legend_action(tab_id, side, action);
        }
    }
}
