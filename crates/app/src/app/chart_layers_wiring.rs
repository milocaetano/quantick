//! Keeping the chart layers and what the panes do with them in step.
//!
//! The layer actions a pane's menu could not apply itself, the footprint
//! change, and the maintain/restore pair that keeps the saved layer states
//! honest. Named `chart_layers_wiring` rather than `chart_layers` because
//! [`crate::chart_layers`] is the layer model itself: this is the window's
//! side of the wire, an adapter over exactly the ports the layer session's
//! effects land on, and one name for both would make the import at the top
//! of `app.rs` ambiguous to a reader.

use crate::candle_presets::PresetError;
use crate::chart_layers;
use crate::footprint_config::FootprintConfig;
use crate::style::ChartStyle;
use crate::surfaces::{ChartSettingsSurface, ChartSettingsTab, FootprintChange};
use crate::tab::Tab;
use crate::workspace_store::WorkspaceStore;
use eframe::egui;
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};

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
        self.tabs.runtime_mut(tab).pane_mut(side).set_layer_visible(
            layer,
            visible,
            &mut self.workspace.layers_mut().actions,
        );
        let _ = self.apply_actions();
    }

    /// The control plane's `chart.settings.set`: the same window and the
    /// same preset store the clicks reach, so a named call and a click cannot
    /// disagree — and a style change leaves the same appearance log line a
    /// click does. `Err` says whether the call was refused or its write
    /// failed; either way nothing changed.
    pub(crate) fn chart_settings(
        &mut self,
        input: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        let settings = &mut *self.chart_settings;
        let window_before = (settings.is_open(), settings.tab());
        let style_before = *self.style;
        let mut list_changed = false;
        let mut applied_preset = None;
        match input {
            ChartSettingsInput::Open { tab } => settings.open(tab, style_before),
            ChartSettingsInput::Close => settings.close(),
            ChartSettingsInput::ApplyCandlePreset { name } => {
                let (look, preset) =
                    settings
                        .candles
                        .apply(&name, &style_before)
                        .ok_or(PresetError::Refused(
                            "no built-in or saved candle preset has that name",
                        ))?;
                *self.style = look;
                applied_preset = preset;
            }
            ChartSettingsInput::SaveCandlePreset { name } => {
                let before = settings.candles.saved(&name);
                settings.candles.save(&name, style_before)?;
                list_changed = before != Some(style_before);
            }
            ChartSettingsInput::DeleteCandlePreset { name } => {
                if crate::style::CandlePreset::from_name(&name).is_some() {
                    return Err(PresetError::Refused("built-in presets cannot be deleted"));
                }
                list_changed = settings.candles.delete(&name)?;
            }
            ChartSettingsInput::ResetCandles => {
                if !settings.candles.in_sitting() {
                    return Err(PresetError::Refused(
                        "the chart settings window is closed; reset restores the appearance it \
                         opened on",
                    ));
                }
                if let Some(look) = settings.candles.reset(&style_before) {
                    *self.style = look;
                }
            }
        }
        let style_changed = *self.style != style_before;
        if style_changed {
            *self.style_revision = self.style_revision.saturating_add(1);
            // A click reaches this line through the window's debounce; a
            // named call is one discrete change, so it is recorded at once.
            super::health::emit_style_changed(self.style, *self.style_revision, applied_preset);
        }
        let settings = &*self.chart_settings;
        Ok(ChartSettingsResult {
            open: settings.is_open(),
            tab: settings.tab(),
            candle_preset: settings.candles.matching(self.style),
            built_in_presets: crate::style::CandlePreset::ALL
                .iter()
                .map(|preset| preset.label().to_owned())
                .collect(),
            custom_presets: settings.candles.custom_names(),
            changed: style_changed
                || list_changed
                || window_before != (settings.is_open(), settings.tab()),
        })
    }

    /// Open the chart settings at `tab` over the look in force — the door
    /// the toolbar, the menus and the pane share, so each starts the sitting
    /// Reset returns to.
    pub(crate) fn open_settings(&mut self, tab: ChartSettingsTab) {
        self.chart_settings.open(tab, *self.style);
    }

    /// The toolbar's flip of the chart settings at `tab`.
    pub(crate) fn toggle_settings(&mut self, tab: ChartSettingsTab) {
        self.chart_settings.toggle(tab, *self.style);
    }

    /// What a pane's layer menu could not switch itself.
    ///
    /// Drained right after the canvas, so the frame that clicked the entry is
    /// the frame that applies it. Both wishes reach the real owner — the shared
    /// style, the indicator state file — instead of a second copy on the pane.
    ///
    /// Answers the dock tab a pane header's glyph asked for: the dock is not
    /// this wiring's to open.
    pub(crate) fn apply_actions(&mut self) -> Option<crate::dock::DockTab> {
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
            self.open_settings(ChartSettingsTab::Footprint);
        }
        if actions.open_chart_settings {
            self.open_settings(ChartSettingsTab::Candles);
        }
        // The strip reads the same book the depth map draws, so one tab
        // configures both.
        actions.open_layer_tab.map(|layer| {
            match layer == crate::chart_layers::ChartLayer::Bubbles {
                true => crate::dock::DockTab::Bubbles,
                false => crate::dock::DockTab::L2,
            }
        })
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

    /// File the active tab's flow-pane layer mask when it moved, and each
    /// tab's asset bubble settings once the pointer lets go of any drag —
    /// then write them, once per change.
    pub(crate) fn maintain(&mut self, ctx: &egui::Context) {
        let tab_id = self.tabs.id_at(self.tabs.active_index());
        let tab = &self.tabs[self.tabs.active_index()];
        chart_layers::maintain(self.workspace, tab_id, &tab.flow_pane, self.style);
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
