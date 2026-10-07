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
use rust_decimal::prelude::ToPrimitive as _;

use crate::bubble_presets;
use crate::orderflow_render::{draw_preview, theme_bubble_rgb};

use super::OrderflowView;
use super::frame::status_color;

mod bubble_sections;
mod l2_sections;
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
            let selected = if self.look.look_name().is_empty() {
                "— custom —"
            } else {
                self.look.look_name()
            };
            egui::ComboBox::from_id_salt("bubble_preset")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, preset) in self.look.presets().presets.iter().enumerate() {
                        if ui
                            .selectable_label(self.look.look_name() == preset.name, &preset.name)
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
                self.reload_presets_from(bubble_presets::load());
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(self.look.name_draft_mut())
                    .hint_text("preset name")
                    .desired_width(150.0),
            );
            if ui
                .button("save")
                .on_hover_text("write the current bubble settings to the presets file as a preset any asset can pick")
                .clicked()
            {
                self.look.save_preset(&self.config, bubble_presets::save);
            }
            if ui
                .add_enabled(self.look.draft_is_stored(), egui::Button::new("delete"))
                .on_hover_text("remove this preset from the file")
                .clicked()
            {
                self.look.delete_preset(bubble_presets::save);
            }
        });
        if let Some(index) = chosen
            && let Some(name) = self
                .look
                .presets()
                .presets
                .get(index)
                .map(|preset| preset.name.clone())
        {
            self.apply_preset(&name);
        }
        ui.small(format!("presets · {}", self.look.source()));
        let mut save_switched = None;
        if let Some(asset) = self.look.asset() {
            let mut save = asset.saves_changes();
            if ui
                .checkbox(&mut save, "Save changes for this asset")
                .on_hover_text(format!(
                    "On: a change here is saved for {key} alone and shown in every tab on {key}. \
                     Off: a change stays on this tab for this session only — not saved, not shown \
                     in other tabs; another market and back, or a restart, brings back what is \
                     saved. Back on, what this tab shows is saved for {key} — unless another tab \
                     or an import changed what is saved meanwhile: then this tab shows that.",
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
            self.look.set_save_changes(on);
        }
        if let Some(status) = self.look.status() {
            ui.small(status.to_owned());
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
                    self.config.reset_l2_visuals();
                    self.capture_grouping_draft = self
                        .config
                        .price_grouping
                        .to_f64()
                        .unwrap_or(self.capture_grouping_draft);
                }
            });
        self.commit_config_changes(before)
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
                         The chart layer switch (Ctrl+B), one of this asset's bubble settings",
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
            native_block: super::layers::native_tape_block(config),
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
            self.look.note_lane_set();
        }
    }

    /// Restore the bubble layer's defaults and drop the preset claim, since
    /// no stored preset is on screen any more.
    fn reset_bubble_visuals(&mut self) {
        self.config.reset_bubble_visuals();
        self.look.defaults_restored();
    }
}

/// The panel's controls and buttons, for tests that drive it without
/// drawing it: each door is the operation the control itself calls.
#[cfg(test)]
impl OrderflowView {
    /// A panel control's change, through the door the panel's draw takes.
    pub(crate) fn edit_config_for_test(
        &mut self,
        edit: impl FnOnce(&mut quantick_orderflow::HeatmapConfig),
    ) {
        let before = self.config.clone();
        edit(&mut self.config);
        self.commit_config_changes(before);
    }

    /// Type `name` into the preset name field.
    pub(crate) fn set_preset_name_draft_for_test(&mut self, name: &str) {
        name.clone_into(self.look.name_draft_mut());
    }

    /// "save", with the presets file written by `writer`.
    pub(crate) fn press_save_preset_for_test(
        &mut self,
        writer: impl FnOnce(&bubble_presets::BubblePresetFile) -> Result<std::path::PathBuf, String>,
    ) {
        self.look.save_preset(&self.config, writer);
    }

    /// "delete", with the presets file written by `writer`.
    pub(crate) fn press_delete_preset_for_test(
        &mut self,
        writer: impl FnOnce(&bubble_presets::BubblePresetFile) -> Result<std::path::PathBuf, String>,
    ) {
        self.look.delete_preset(writer);
    }

    /// The reload button, over the presets file read from `path`.
    pub(crate) fn press_reload_presets_for_test(&mut self, path: std::path::PathBuf) {
        self.reload_presets_from(bubble_presets::load_from(Some(path)));
    }

    /// "reset bubble visuals".
    pub(crate) fn press_reset_bubble_visuals_for_test(&mut self) {
        self.reset_bubble_visuals();
    }
}
