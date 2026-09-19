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
use crate::surfaces::{FootprintChange, FootprintSettingsSurface};
use crate::workspace_store::WorkspaceStore;

use super::arrangement_host::ArrangementHost;

/// The ports a layer effect can land on: the tabs whose panes hold the
/// switches, the store that files them, the shared style the grid lives in,
/// and the footprint's window-wide setup with its settings window.
pub(crate) struct LayerWiring<'a> {
    pub(super) tabs: &'a mut ArrangementHost,
    pub(super) workspace: &'a mut WorkspaceStore,
    pub(super) style: &'a mut ChartStyle,
    pub(super) style_revision: &'a mut u64,
    pub(super) footprint_config: &'a mut FootprintConfig,
    pub(super) footprint_settings: &'a mut FootprintSettingsSurface,
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
    /// the frame that applies it. Both wishes reach the real owner — the shared
    /// style, the indicator state file — instead of a second copy on the pane.
    pub(crate) fn apply_actions(&mut self) {
        let actions = std::mem::take(&mut self.workspace.layers_mut().actions);
        if let Some(visible) = actions.grid {
            self.style.canvas.grid_enabled = visible;
            // The appearance panel's own edits bump this; the renderer and the
            // style log both read it to know something moved.
            *self.style_revision = self.style_revision.saturating_add(1);
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
            self.footprint_settings.open();
        }
    }

    /// Apply what the footprint settings window settled on.
    ///
    /// Whatever is edited also becomes the window default, which is what a
    /// trader configuring their first chart means; a second chart diverges
    /// only when they configure it too.
    pub(crate) fn apply_footprint_change(&mut self, change: FootprintChange) {
        let tab = self.tabs.runtime_mut(self.tabs.active_index());
        let side = tab.focused_side();
        match change {
            FootprintChange::Applied(edited) => {
                tab.pane_mut(side).footprint.config = Some((*edited).clone());
                *self.footprint_config = *edited;
                crate::footprint_config::save(
                    self.workspace.footprint_settings_path(),
                    self.footprint_config,
                );
            }
            FootprintChange::ResetToDefault => {
                tab.pane_mut(side).footprint.config = None;
            }
        }
    }

    /// File the active tab's flow-pane layer mask when it moved.
    pub(crate) fn maintain(&mut self) {
        let tab_id = self.tabs.id_at(self.tabs.active_index());
        let tab = &self.tabs[self.tabs.active_index()];
        chart_layers::maintain(self.workspace, tab_id, &tab.flow_pane, self.style);
    }

    /// Put the saved layer states back on every pane.
    pub(crate) fn restore(&mut self) {
        let active_index = self.tabs.active_index();
        chart_layers::restore(self.workspace, self.tabs, active_index, self.style);
    }
}
