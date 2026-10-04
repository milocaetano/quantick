//! The order-flow settings surface: the L2 dock tab, the bubbles dock tab,
//! the live-lane and bubble control groups they are built from, and the
//! bubble preset picker with its save and reload.
//!
//! Drawn only while a dock tab is open, never on the chart's own frame.
//! Every control writes the same `HeatmapConfig` field the chart reads, and
//! the root's `commit_config_changes` is the one door a change goes
//! through to reach the worker.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_orderflow::{HeatmapConfig, LiveLaneStyle};
use rust_decimal::prelude::ToPrimitive as _;

use crate::bubble_presets;
use crate::bubble_presets::{BubblePreset, PresetSource};
use crate::orderflow_render::{draw_preview, theme_bubble_rgb};

use super::OrderflowView;
use super::frame::status_color;

mod bubble_sections;
mod l2_sections;
#[cfg(test)]
#[path = "settings/tests/preset_persistence_tests.rs"]
mod preset_persistence_tests;
#[cfg(test)]
#[path = "settings/tests/tape_only_controls.rs"]
mod tape_only_controls;

use bubble_sections::{
    BubbleHealthSection, ClusteringSection, ColoursSection, ConsumptionMarksSection, LabelsSection,
    LiveLaneSection, SizePlacementSection,
};
use l2_sections::{
    BookHealthSection, LiquidityRangesSection, LiquidityResponseSection, ScaleHistorySection,
    VisualLayersSection,
};

impl OrderflowView {
    /// Picker, save and reload for the named bubble looks.
    ///
    /// Every change is kept for the asset on screen; saving names the look
    /// as a preset any asset can pick, never over a look other assets open on.
    fn draw_bubble_presets(&mut self, ui: &mut egui::Ui) {
        // The picker reads the stored presets while the closure below wants to
        // mutate them, so it hands back an index and the name is read after.
        // Cloning every name each frame would be the other way out, and this
        // runs on the render thread.
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("preset");
            let selected = if self.look_name.is_empty() {
                "— custom —"
            } else {
                self.look_name.as_str()
            };
            egui::ComboBox::from_id_salt("bubble_preset")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, preset) in self.presets.presets.iter().enumerate() {
                        if ui
                            .selectable_label(self.look_name == preset.name, &preset.name)
                            .clicked()
                        {
                            chosen = Some(index);
                        }
                    }
                });
            if ui
                .small_button(icons::ARROW_CLOCKWISE)
                .on_hover_text("reload the presets file from disk; an asset with settings of its own keeps them")
                .clicked()
            {
                self.reload_presets();
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.preset_name_draft)
                    .hint_text("preset name")
                    .desired_width(150.0),
            );
            if ui
                .button("save")
                .on_hover_text("write the current bubble settings to the presets file as a preset any asset can pick")
                .clicked()
            {
                self.save_preset();
            }
            let removable = !self.preset_name_draft.trim().is_empty()
                && self.presets.get(self.preset_name_draft.trim()).is_some();
            if ui
                .add_enabled(removable, egui::Button::new("delete"))
                .on_hover_text("remove this preset from the file")
                .clicked()
            {
                self.delete_preset_with(bubble_presets::save);
            }
        });
        if let Some(index) = chosen
            && let Some(name) = self
                .presets
                .presets
                .get(index)
                .map(|preset| preset.name.clone())
        {
            self.apply_preset(&name);
        }
        ui.small(format!("presets · {}", self.presets_source));
        let mut save_switched = None;
        if let Some(asset) = &self.asset {
            let mut save = asset.saves_changes();
            if ui
                .checkbox(&mut save, "Save changes for this asset")
                .on_hover_text(format!(
                    "On: a change here is saved for {key} alone and shown in every tab on {key}. \
                     Off: a change applies to this tab for this session only — not saved, not \
                     shown in other tabs, which keep what is saved; switching this tab to another \
                     market and back, or restarting, brings back what is saved. Turning it back on \
                     saves what this tab shows now for {key}.",
                    key = asset.key()
                ))
                .changed()
            {
                save_switched = Some(save);
            }
            let saved = asset
                .unsaved()
                .map_or_else(String::new, |why| format!(" — not saved: {why}"));
            ui.small(format!(
                "settings kept for asset {} only ({}){saved}",
                asset.key(),
                asset.source().as_str()
            ));
        }
        if let Some(on) = save_switched {
            self.set_save_asset_changes(on);
        }
        if let Some(status) = &self.preset_status {
            ui.small(status.clone());
        }
    }

    /// Apply the stored preset called `name`, reporting whether it exists.
    ///
    /// The choice is the asset's own: it is filed for the asset on screen.
    /// An unknown name changes nothing and returns `false`; the caller
    /// decides how loudly to say so.
    pub(crate) fn apply_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self.presets.get(name).cloned() else {
            return false;
        };
        preset.apply_to(&mut self.config);
        self.look_name = preset.name.clone();
        self.preset_name_draft = preset.name.clone();
        self.preset_status = Some(format!("'{}' applied", preset.name));
        self.note_asset_lane_set();
        true
    }

    /// Refuse saving over or removing `name` when other assets open on it.
    fn refused_preset_name(&mut self, name: &str) -> bool {
        let asset = self.asset.as_ref();
        let refusal = asset.and_then(|asset| asset.refuses_preset_name(name, &self.presets));
        refusal
            .map(|refusal| self.preset_status = Some(refusal))
            .is_some()
    }

    fn save_preset(&mut self) {
        self.save_preset_with(bubble_presets::save);
    }

    pub(crate) fn save_preset_with(
        &mut self,
        writer: impl FnOnce(&bubble_presets::BubblePresetFile) -> Result<std::path::PathBuf, String>,
    ) {
        let name = self.preset_name_draft.trim().to_owned();
        if name.is_empty() {
            self.preset_status = Some("name the preset before saving".to_owned());
            return;
        }
        if self.refused_preset_name(&name) {
            return;
        }
        self.presets
            .upsert(BubblePreset::capture(&name, &self.config));
        self.look_name = name.clone();
        self.note_asset_change();
        self.persist_presets(format!("'{name}' saved"), writer);
    }

    /// Remove the preset the name field holds from the presets file, unless
    /// some asset opens on it.
    pub(crate) fn delete_preset_with(
        &mut self,
        writer: impl FnOnce(&bubble_presets::BubblePresetFile) -> Result<std::path::PathBuf, String>,
    ) {
        let name = self.preset_name_draft.trim().to_owned();
        if self.refused_preset_name(&name) {
            return;
        }
        self.presets.remove(&name);
        self.persist_presets(format!("preset '{name}' removed"), writer);
    }

    fn persist_presets(
        &mut self,
        success: String,
        writer: impl FnOnce(&bubble_presets::BubblePresetFile) -> Result<std::path::PathBuf, String>,
    ) {
        // `active` stays the file's: the look an undeclared asset opens on,
        // never the asset on screen's choice.
        match writer(&self.presets) {
            Ok(path) => {
                self.presets_source = PresetSource::WorkingDir(path.clone());
                self.preset_status = Some(format!("{success} → {}", path.display()));
            }
            Err(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_NOT_SAVED",
                    error = message.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble presets could not be written; the current look is in memory only"
                );
                self.preset_status = Some(format!("not saved — {message}"));
            }
        }
    }

    fn reload_presets(&mut self) {
        self.reload_presets_from(bubble_presets::load());
    }

    pub(crate) fn reload_presets_from(
        &mut self,
        (presets, source, error): (
            bubble_presets::BubblePresetFile,
            PresetSource,
            Option<String>,
        ),
    ) {
        self.presets = presets;
        self.presets_source = source;
        self.follow_declared_look(true);
        match error {
            Some(message) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BUBBLE_PRESETS_UNREADABLE",
                    error = message.as_str(),
                    action = "using_built_in_presets",
                    "bubble presets file could not be read; built-in presets are in use"
                );
                self.preset_status = Some(format!("presets not loaded — {message}"));
            }
            None if self.asset.is_some() => {}
            None => {
                let active = self.look_name.clone();
                if active.is_empty() || self.presets.get(&active).is_none() {
                    self.preset_status = Some("presets reloaded".to_owned());
                } else {
                    self.apply_preset(&active);
                    self.preset_status = Some(format!("reloaded · '{active}' applied"));
                }
            }
        }
    }

    /// The L2 dock tab's body: everything the depth map owns. Returns
    /// whether capture must restart because the base capture resolution
    /// changed.
    ///
    /// The layer's *toggle* lives in the toolbar; this tab is settings only —
    /// opening it never starts capture (looking is not enabling).
    pub fn draw_l2_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
        egui::ScrollArea::vertical()
            .id_salt("orderflow_l2_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(self.published.status.label())
                        .small()
                        .color(status_color(&self.published.status)),
                );
                ui.small(
                    "Brightness is resting liquidity. Green/red bubbles are confirmed trades.",
                );
                ui.small(
                    "A bite means a compatible L2 reduction; a violet tail is an unattributed withdrawal.",
                );
                draw_preview(ui, &self.config).on_hover_text(
                    "Deterministic preview: persistent wall, aligned depletion, full withdrawal and clustered trades.",
                );
                ui.separator();

                LiquidityRangesSection {
                    config: &mut self.config,
                }
                .show(ui);
                ui.separator();
                VisualLayersSection {
                    config: &mut self.config,
                }
                .show(ui);
                ui.separator();
                LiquidityResponseSection {
                    config: &mut self.config,
                }
                .show(ui);
                ui.separator();
                ScaleHistorySection {
                    config: &mut self.config,
                    capture_grouping_draft: &mut self.capture_grouping_draft,
                }
                .show(ui);
                ui.separator();
                BookHealthSection {
                    published: &self.published,
                }
                .show(ui);
                if ui.button("reset L2 visuals").clicked() {
                    self.reset_l2_visuals();
                }
            });
        self.commit_config_changes(before)
    }

    /// Restore the depth map's defaults, keeping the capture bucket and the
    /// whole bubble layer as they are.
    fn reset_l2_visuals(&mut self) {
        let price_grouping = self.config.price_grouping;
        self.capture_grouping_draft = price_grouping
            .to_f64()
            .unwrap_or(self.capture_grouping_draft);
        self.config = HeatmapConfig {
            enabled: self.config.enabled,
            price_grouping,
            // The bubble layer owns its own panel and its own reset,
            // so its whole look (and the preset it came from) stays.
            show_aggressions: self.config.show_aggressions,
            bubble_cluster_ms: self.config.bubble_cluster_ms,
            bubble_dust_merge_ms: self.config.bubble_dust_merge_ms,
            bubble_candle_summary: self.config.bubble_candle_summary,
            volume_dots: self.config.volume_dots,
            bubble_region_rows: self.config.bubble_region_rows,
            bubble_region_ms: self.config.bubble_region_ms,
            bubbles: self.config.bubbles.clone(),
            live_lane: self.config.live_lane.clone(),
            ..HeatmapConfig::default()
        };
    }

    /// The Bubbles dock tab's body: the aggression layer's settings.
    /// Everything here is independent of L2 capture — bubbles are built from
    /// the aggregate-trade stream. Same return contract as
    /// [`Self::draw_l2_tab`].
    pub fn draw_bubbles_tab(&mut self, ui: &mut egui::Ui) -> bool {
        self.sync_published();
        let before = self.config.clone();
        egui::ScrollArea::vertical()
            .id_salt("orderflow_bubbles_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.small(
                    "Confirmed executions from the trade stream. Colour is the aggressor side; area is quantity.",
                );
                ui.small("This layer is independent: it keeps drawing with L2 capture off.");
                draw_preview(ui, &self.config).on_hover_text(
                    "Deterministic preview: every control below shows its effect here, without waiting for a trade.",
                );
                ui.separator();

                self.draw_bubble_presets(ui);
                ui.separator();

                ui.checkbox(&mut self.config.show_aggressions, "show aggression bubbles")
                    .on_hover_text(
                        "records and projects confirmed trades; does not start or stop L2 depth capture. \
                         A chart layer switch (Ctrl+B), kept for every market — not an asset setting",
                    );
                ui.add_enabled_ui(self.config.show_aggressions, |ui| {
                    self.draw_bubble_controls(ui);
                });

                ui.separator();
                BubbleHealthSection {
                    health: &self.published.health,
                    show_aggressions: self.config.show_aggressions,
                }
                .show(ui);
                if ui
                    .button("reset bubble visuals")
                    .on_hover_text("only this tab; L2 and history settings stay as they are")
                    .clicked()
                {
                    self.reset_bubble_visuals();
                }
            });
        self.commit_config_changes(before)
    }

    /// Every visual choice for the bubbles, in the order the tab shows them:
    /// how prints fold, the live lane, then the bubbles themselves — size and
    /// placement, the marks a consuming print leaves, labels, and colour.
    fn draw_bubble_controls(&mut self, ui: &mut egui::Ui) {
        let theme_rgb = theme_bubble_rgb(self.config.theme);
        let config = &mut self.config;
        ClusteringSection {
            config: &mut *config,
        }
        .show(ui);
        // Read after the clustering section drew: a history window picked
        // this frame is the one the live lane's "Same as history" inherits.
        let lane_set = LiveLaneSection {
            inherited_cluster_ms: config.bubble_cluster_ms,
            volume_dots: config.volume_dots.enabled,
            native_block: OrderflowView::native_tape_block(config),
            lane: &mut config.live_lane,
        }
        .show(ui);
        let native_tape = config.native_tape() && config.volume_dots.enabled;
        let bubbles = &mut config.bubbles;
        SizePlacementSection {
            bubbles: &mut *bubbles,
            native_tape,
        }
        .show(ui);
        ConsumptionMarksSection {
            bubbles: &mut *bubbles,
        }
        .show(ui);
        LabelsSection {
            bubbles: &mut *bubbles,
        }
        .show(ui);
        ColoursSection { bubbles, theme_rgb }.show(ui);
        if lane_set {
            self.note_asset_lane_set();
        }
    }

    /// Restore the bubble layer's defaults and drop the preset claim, since
    /// no stored preset is on screen any more.
    pub(crate) fn reset_bubble_visuals(&mut self) {
        let defaults = HeatmapConfig::default();
        self.config.bubble_cluster_ms = defaults.bubble_cluster_ms;
        self.config.bubble_dust_merge_ms = defaults.bubble_dust_merge_ms;
        self.config.bubble_candle_summary = defaults.bubble_candle_summary;
        self.config.volume_dots = defaults.volume_dots;
        self.config.bubble_region_rows = defaults.bubble_region_rows;
        self.config.bubble_region_ms = defaults.bubble_region_ms;
        self.config.bubbles = defaults.bubbles;
        // The native tape and tape only are the pane's mode, not its look.
        self.config.live_lane = LiveLaneStyle {
            native_tape: self.config.live_lane.native_tape,
            tape_only: self.config.live_lane.tape_only,
            ..defaults.live_lane
        };
        // No stored preset is on screen any more, so the
        // picker must not keep claiming one.
        self.look_name.clear();
        self.note_asset_lane_set();
        self.preset_status = Some("bubble defaults restored for the asset on screen".to_owned());
    }
}
