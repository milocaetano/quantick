//! The indicator owner's worker, layout and control recipients at the frame boundary.
use super::indicator_manager::{IndicatorEdit, SettingsChange};
use super::{layout_wiring::LayoutAdapter, workspace_runtime::WorkspaceRuntime};
use crate::canvas_layout::MAX_CANVAS_PANES;
use crate::indicator_legend::LegendAction;
use crate::indicator_worker::SlotId;
use crate::pane::PaneSide;
use eframe::egui;
use smallvec::SmallVec;

/// The ports an indicator edit lands on: the workspace, the library, and the
/// layout chrome an edit can toast or confirm through.
pub(super) struct IndicatorWiring<'a> {
    pub(super) runtime: &'a mut WorkspaceRuntime,
    pub(super) picker: &'a mut crate::surfaces::indicator_picker::IndicatorPickerSurface,
    pub(super) drawing_chrome: &'a mut crate::surfaces::DrawingChromeSurface,
    pub(super) toast: &'a mut crate::surfaces::ToastSurface,
    pub(super) rename: &'a mut Option<super::chrome::LayoutRename>,
    pub(super) delete_confirm: &'a mut Option<crate::layouts::LayoutId>,
    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) harness: &'a mut crate::harness::Harness,
}

impl<'a> IndicatorWiring<'a> {
    /// The layout ports these fields hold: the one place a [`LayoutAdapter`]
    /// is built, for the edits below and for [`super::LayoutPort`].
    pub(super) fn into_layout(self) -> LayoutAdapter<'a> {
        LayoutAdapter {
            active: self.runtime.tabs.active_index(),
            tabs: &mut self.runtime.tabs,
            indicators: &mut self.runtime.indicators,
            store: self.runtime.workspace.layouts_mut(),
            drawing_chrome: self.drawing_chrome,
            toast: self.toast,
            rename: self.rename,
            delete_confirm: self.delete_confirm,
        }
    }
}

impl IndicatorWiring<'_> {
    fn layout(&mut self) -> LayoutAdapter<'_> {
        IndicatorWiring {
            runtime: self.runtime,
            picker: self.picker,
            drawing_chrome: self.drawing_chrome,
            toast: self.toast,
            rename: self.rename,
            delete_confirm: self.delete_confirm,
            #[cfg(any(feature = "scenario-harness", test))]
            harness: self.harness,
        }
        .into_layout()
    }

    /// Open the indicator library on the pane at `side`. Focus moves there first:
    /// every indicator command lands on the focused pane (§11), so the library
    /// and what it attaches can only agree if the pane it was opened for has focus.
    pub(crate) fn open_indicator_picker_on(&mut self, side: PaneSide) {
        self.runtime.active_tab_mut().focus = side;
        let target = (
            self.runtime.tabs.active_id(),
            self.runtime.active_tab().focused_pane().id,
        );
        self.picker
            .update(quantick_ui_model::indicator_picker::Intent::Open(target));
    }

    /// Execute persistence only after the owner has prepared the indicator edit.
    pub(super) fn apply_indicator_edit(&mut self, edit: IndicatorEdit) {
        if self.runtime.validate_target(edit.target()).is_err() {
            return;
        }
        self.layout().apply_indicator_edit(edit);
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

    /// Existing hook and pane requests are resolved at their original frame
    /// phase. Answers the guide switches asked for, as control-action inputs
    /// and the hook's first: the window carries them out through its gateway.
    pub(super) fn service_indicator_requests(&mut self) -> Vec<serde_json::Value> {
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
                self.harness.settings_autostart(),
            ) {
                self.harness.settings_autostart_opened();
            }
        }
        let requests: SmallVec<[(PaneSide, SlotId); MAX_CANVAS_PANES]> = (self.runtime)
            .active_tab_mut()
            .panes_with_sides_mut()
            .filter_map(|(pane, side)| pane.take_settings_request().map(|slot| (side, slot)))
            .collect();
        for (side, slot) in requests {
            self.apply_indicator_legend_action(tab_id, side, LegendAction::OpenSettings(slot));
        }
        let guide = |pane_id: u64, slot: SlotId, enabled: bool| serde_json::json!({"tab_id":tab_id.to_string(),"pane_id":pane_id.to_string(),"slot_id":slot.0.to_string(),"enabled":enabled});
        // The hook's guide goes first. Taking the pane requests before it is
        // sent changes nothing: the guide action writes the slot's guide and
        // the journal, never a pane's pending request.
        let mut guides = Vec::new();
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(index) = self.harness.indicator_mouse_line() {
            let flow = &self.runtime.active_tab().flow_pane;
            let target = flow
                .indicators
                .all()
                .get(index)
                .map(|view| (flow.id, view.slot));
            if let Some((pane_id, slot)) = target {
                self.harness.indicator_mouse_line_opened();
                guides.push(guide(pane_id, slot, true));
            }
        }
        for (pane, _) in self.runtime.active_tab_mut().panes_with_sides_mut() {
            let request = pane.take_indicator_guide_request();
            guides.extend(request.map(|(slot, enabled)| guide(pane.id, slot, enabled)));
        }
        guides
    }

    pub(super) fn draw_indicator_surfaces(&mut self, ctx: &egui::Context) {
        #[cfg(any(feature = "scenario-harness", test))]
        {
            let tab = &self.runtime.tabs[self.runtime.tabs.active_index()];
            if self.runtime.indicators.open_first_editable(
                self.runtime.tabs.active_id(),
                (tab.flow_pane.id, tab.flow_pane.indicators.all()),
                self.harness.wants_indicator_settings_dialog(),
            ) {
                self.harness.indicator_settings_dialog_opened();
            }
        }
        if let Some(change) = self.runtime.draw_indicator_settings(ctx) {
            self.apply_indicator_settings_change(change);
        }
        let tab_id = self.runtime.tabs.active_id();
        let tab = self.runtime.active_tab();
        let (split, shown) = (tab.shows_context_charts(), tab.context_panes_shown());
        let sides: SmallVec<[PaneSide; MAX_CANVAS_PANES]> = tab.sides().collect();
        let mut pending = Vec::new();
        for side in sides {
            let visible = !matches!(side, PaneSide::Time(slot) if !split || slot >= shown);
            let tab = self.runtime.active_tab();
            let pane = tab.pane(side);
            let position_open = visible
                && pane.frame.chart_area.is_some()
                && pane.paper_hud_anchor().is_some()
                && tab.paper.position_summary().is_some();
            let tabs = &mut self.runtime.tabs;
            let pane = tabs.runtime_mut(tabs.active_index()).pane_mut(side);
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
