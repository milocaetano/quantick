//! The indicator owner's worker, layout and control recipients at the frame boundary.
use super::ControlPort;
use super::QuantickApp;
use super::indicator_manager::{IndicatorEdit, SettingsChange};
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::indicator_legend::LegendAction;
use crate::indicator_worker::SlotId;
use crate::pane::PaneSide;
use eframe::egui;
use smallvec::SmallVec;

impl QuantickApp {
    /// Execute persistence only after the owner has prepared the indicator edit.
    pub(super) fn apply_indicator_edit(&mut self, edit: IndicatorEdit) {
        let target = match edit {
            IndicatorEdit::Attached(attached) => {
                if let Some(kind) = attached.layout_entry {
                    self.layout_adapter().mirror_add(attached.target, &kind);
                }
                attached.target
            }
            IndicatorEdit::Hidden(target) => {
                self.layout_adapter().mirror_hidden(target);
                target
            }
            IndicatorEdit::Remove(target) => {
                if !self.tabs.by_id(target.tab).is_some() {
                    return;
                }
                // Read its layout position before either mirror or origin cleanup.
                self.layout_adapter().mirror_remove(target);
                let Some(tab) = self.tabs.by_id_mut(target.tab) else {
                    return;
                };
                self.indicators
                    .remove(Some(tab.pane_mut(target.side)), target);
                target
            }
            IndicatorEdit::Inputs(target, values) => {
                self.layout_adapter().mirror_inputs(target, &values);
                target
            }
            IndicatorEdit::Style(target) => {
                self.layout_adapter().mirror_style(target);
                target
            }
        };
        self.layout_adapter()
            .note_indicator_edit_at(target.tab, target.side);
    }

    pub(super) fn apply_indicator_settings_change(&mut self, change: SettingsChange) {
        let worker = change.worker_target().and_then(|target| {
            self.tabs
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
        let Some(tab) = self.tabs.by_id_mut(tab_id) else {
            return;
        };
        if let Some(edit) =
            self.indicators
                .legend_action(tab.pane_mut(side), (tab_id, side), action)
        {
            self.apply_indicator_edit(edit);
        }
    }

    /// Existing hook and pane requests are resolved at their original frame phase.
    pub(super) fn service_indicator_requests(&mut self) {
        let tab_id = self.tabs.active_id();
        let tab = &self.tabs[self.tabs.active_index()];
        if self.indicators.open_autostart(
            tab_id,
            (tab.focused_side(), tab.focused_pane().indicators.all()),
            tab.flow_pane.indicators.all(),
            self.harness.settings_autostart(),
        ) {
            self.harness.settings_autostart_opened();
        }
        let requests: SmallVec<[(PaneSide, SlotId); MAX_CANVAS_PANES]> = self
            .active_tab_mut()
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| pane.take_settings_request().map(|slot| (side, slot)))
            .collect();
        for (side, slot) in requests {
            self.apply_indicator_legend_action(tab_id, side, LegendAction::OpenSettings(slot));
        }
        if let Some(index) = self.harness.indicator_mouse_line() {
            let target = self
                .active_tab()
                .flow_pane
                .indicators
                .all()
                .get(index)
                .map(|view| (self.active_tab().flow_pane.id, view.slot));
            if let Some((pane_id, slot)) = target {
                self.harness.indicator_mouse_line_opened();
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
        let tab = &self.tabs[self.tabs.active_index()];
        if self.indicators.open_first_editable(
            self.tabs.active_id(),
            tab.flow_pane.indicators.all(),
            self.harness.wants_indicator_settings_dialog(),
        ) {
            self.harness.indicator_settings_dialog_opened();
        }
        if let Some(slot) = self
            .indicators
            .indicator_settings
            .as_ref()
            .map(|dialog| dialog.slot)
        {
            let target = self.indicators.indicator_settings_target;
            let view = self
                .tabs
                .by_id_mut(target.tab)
                .and_then(|tab| tab.pane_mut(target.side).indicators.view_mut(slot));
            let change =
                self.indicators
                    .draw_settings(ctx, view, self.workspace.indicator_presets_path());
            self.apply_indicator_settings_change(change);
        }
        let tab_id = self.tabs.active_id();
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
                .tabs
                .runtime_mut(self.tabs.active_index())
                .pane_mut(side);
            for action in
                self.indicators
                    .draw_legend(ctx, pane, (tab_id, side), visible, position_open)
            {
                pending.push((side, action));
            }
        }
        // Do not interleave mutations with drawing legends on other panes.
        for (side, action) in pending {
            self.apply_indicator_legend_action(tab_id, side, action);
        }
    }
}
