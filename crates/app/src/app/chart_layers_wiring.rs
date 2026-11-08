//! Keeping the chart layers and what the panes do with them in step.
//!
//! The layer actions a pane's menu could not apply itself, the footprint
//! change, and the maintain/restore pair that keeps the saved layer states
//! honest. Named `chart_layers_wiring` rather than `chart_layers` because
//! [`crate::chart_layers`] is the layer model itself: this is the window's
//! side of the wire, an adapter over exactly the ports the layer session's
//! effects land on, and one name for both would make the import at the top
//! of `app.rs` ambiguous to a reader.

use crate::chart_layers;
use crate::footprint_config::FootprintConfig;
use crate::style::ChartStyle;
use crate::surfaces::{ChartSettingsSurface, ChartSettingsTab, FootprintChange};
use crate::tab::Tab;
use crate::workspace_store::WorkspaceStore;
use eframe::egui;

use super::arrangement_host::ArrangementHost;

/// The ports a layer effect can land on: the tabs whose panes hold the
/// switches, the store that files them, the shared style the grid lives in,
/// and the footprint's window-wide setup with its settings window.
pub(crate) struct LayerWiring<'a> {
    pub(super) tabs: &'a mut ArrangementHost,
    pub(super) workspace: &'a mut WorkspaceStore,
    pub(super) style: &'a mut ChartStyle,
    pub(super) appearance: &'a quantick_appearance::Appearance,
    pub(super) style_revision: &'a mut u64,
    pub(super) footprint_config: &'a mut FootprintConfig,
    pub(super) chart_settings: &'a mut ChartSettingsSurface,
}

impl LayerWiring<'_> {
    /// Switch one pane's layer the way its menu does, then apply what the
    /// switch left for the window — the control plane's `layers.*` door.
    pub(crate) fn set_visible(
        &mut self,
        tab: usize,
        side: crate::pane::PaneSide,
        layer: chart_layers::ChartLayer,
        visible: bool,
    ) {
        if matches!(
            layer,
            chart_layers::ChartLayer::Grid | chart_layers::ChartLayer::TimeGrid
        ) {
            let candles = &mut self
                .tabs
                .runtime_mut(tab)
                .pane_mut(side)
                .presentation
                .candles;
            if layer == chart_layers::ChartLayer::Grid {
                candles.set_grid(self.style, visible);
            } else {
                candles.set_time_grid(self.style, visible);
            }
            *self.style_revision = self.style_revision.saturating_add(1);
            return;
        }
        self.tabs.runtime_mut(tab).pane_mut(side).set_layer_visible(
            layer,
            visible,
            &mut self.workspace.layers_mut().actions,
        );
        self.apply_actions();
    }

    /// What a pane's layer menu could not switch itself.
    ///
    /// Drained right after the canvas, so the frame that clicked the entry is
    /// the frame that applies it. Each wish reaches the real owner — the shared
    /// style, the indicator state file, the market settings — instead of a
    /// second copy on the pane.
    pub(crate) fn apply_actions(&mut self) {
        let actions = std::mem::take(&mut self.workspace.layers_mut().actions);
        if let Some(on) = actions.bar_progress {
            self.set_bar_progress(self.tabs.id_at(self.tabs.active_index()), on);
        }
        if actions.grid.is_some() || actions.time_grid.is_some() {
            let tab = self.tabs.runtime_mut(self.tabs.active_index());
            let side = match actions.grid_pane {
                Some(id) => tab
                    .panes()
                    .find(|(pane, _)| pane.id == id)
                    .map(|(_, side)| side),
                None => Some(tab.focused_side()),
            };
            if let Some(side) = side {
                let candles = &mut tab.pane_mut(side).presentation.candles;
                if let Some(visible) = actions.grid {
                    candles.set_grid(self.style, visible);
                }
                if let Some(visible) = actions.time_grid {
                    candles.set_time_grid(self.style, visible);
                }
                // The renderer and style log read this revision.
                *self.style_revision = self.style_revision.saturating_add(1);
            }
        }
        if actions.indicators_changed {
            // The same mark `LayoutAdapter::note_indicator_edit_at` leaves:
            // the layouts file carries the indicator state, and the debounce
            // decides when it is written.
            self.workspace
                .layouts_mut()
                .mark_changed(std::time::Instant::now());
        }
        if actions.footprint_changed {
            crate::footprint_config::save(
                self.workspace.footprint_settings_path(),
                self.footprint_config,
            );
        }
        if actions.open_footprint_settings {
            self.open_settings(ChartSettingsTab::Footprint);
        }
        if actions.open_chart_settings {
            let tab_id = self.tabs.active_id();
            let tab = self.tabs.runtime_mut(self.tabs.active_index());
            let requested = tab
                .sides()
                .collect::<Vec<_>>()
                .into_iter()
                .find_map(|side| {
                    let pane = tab.pane_mut(side);
                    pane.presentation.candles.take_editor_request().then_some(
                        quantick_chart::pane_style::StyleTarget {
                            tab_id,
                            pane_id: pane.id,
                        },
                    )
                });
            self.open_target_settings(ChartSettingsTab::Candles, requested);
        }
        let tab = self.tabs.runtime_mut(self.tabs.active_index());
        quantick_chart::pane_header::settings_actions(&mut tab.tape_mut().settings_panel, &actions);
    }

    /// Apply what the footprint settings window settled on.
    ///
    /// Whatever is edited also becomes the window default, which is what a
    /// trader configuring their first chart means; a second chart diverges
    /// only when they configure it too. A tab keeps its own style: see
    /// `quantick_stores::chart_footprint`.
    pub(crate) fn apply_footprint_change(&mut self, change: FootprintChange) {
        let tab = self.tabs.runtime_mut(self.tabs.active_index());
        let side = tab.focused_side();
        match change {
            FootprintChange::Applied(edited) => {
                tab.pane_mut(side)
                    .presentation
                    .footprint
                    .config
                    .set_own(Some((*edited).clone()));
                *self.footprint_config = *edited;
                for pane in self.tabs.iter_mut().flat_map(|tab| tab.panes_mut()) {
                    pane.presentation
                        .footprint
                        .config
                        .follow(self.footprint_config);
                }
                crate::footprint_config::save(
                    self.workspace.footprint_settings_path(),
                    self.footprint_config,
                );
            }
            FootprintChange::ResetToDefault => tab
                .pane_mut(side)
                .presentation
                .footprint
                .config
                .set_own(None),
        }
    }

    /// File the active tab's flow-pane layer mask when it moved, and each
    /// tab's asset bubble settings once the pointer lets go of any drag —
    /// then write them, once per change.
    pub(crate) fn maintain(&mut self, ctx: &egui::Context) {
        let tab_id = self.tabs.id_at(self.tabs.active_index());
        let tab = self.tabs.runtime_mut(self.tabs.active_index());
        chart_layers::maintain(self.workspace, tab_id, &tab.flow_pane, self.style);
        if tab.tape_mut().settle_old_tape_fold() {
            let states = tab.flow_pane.layer_states(self.style);
            chart_layers::save(self.workspace.chart_layers_path(), &states);
        }
        if !ctx.input(|input| input.pointer.any_down()) {
            self.tabs.iter_mut().for_each(Tab::sync_asset_bubbles);
        }
        self.workspace.bubble_assets().borrow_mut().flush();
    }

    /// Put the saved layer states back on every pane.
    pub(crate) fn restore(&mut self) {
        let active_index = self.tabs.active_index();
        chart_layers::restore(self.workspace, self.tabs, active_index, self.style);
    }
}
