//! The Candles tab: the appearance knobs, the built-in and saved presets,
//! and Reset to the look the window opened on.
//!
//! The decisions are headless — what a preset name means and what a saved
//! file holds are `quantick_stores::candle_presets`, what Reset returns to is
//! `quantick_chart::style::StyleEdit` — so this file is the egui half and the
//! one place both are reached from, by the window and the control plane alike.

use std::path::PathBuf;

use eframe::egui;

use super::{ChartSettingsTab, SettingsTab, TabResponse};
use crate::candle_presets::{CandlePresetStore, MAX_NAME_LEN, SaveRefusal};
use crate::candle_view::{StylePanelResponse, draw_presets, draw_style_contents};
use crate::style::{CandlePreset, ChartStyle, StyleEdit};
use crate::surfaces::{SurfaceEnv, SurfaceResponse};
use crate::theme;

/// The Candles tab's state: the sitting Reset returns to, the saved looks
/// and the half-typed name in the save field.
pub(crate) struct CandlesTab {
    edit: StyleEdit,
    presets: CandlePresetStore,
    presets_path: PathBuf,
    name_draft: String,
    /// Why the last save was refused, shown under the field until the next.
    refusal: Option<&'static str>,
}

impl Default for CandlesTab {
    /// Loads the preset file, so the list is never empty for want of a
    /// second initialiser.
    fn default() -> Self {
        let presets_path = crate::candle_presets::default_path();
        Self {
            edit: StyleEdit::default(),
            presets: CandlePresetStore::load(&presets_path),
            presets_path,
            name_draft: String::new(),
            refusal: None,
        }
    }
}

impl CandlesTab {
    /// Start a sitting on `current`, unless one is under way.
    pub fn begin(&mut self, current: ChartStyle) {
        self.edit.begin(current);
    }

    /// Whether a sitting is under way — whether Reset has anything to
    /// return to.
    pub fn in_sitting(&self) -> bool {
        self.edit.opened_on().is_some()
    }

    /// What Reset applies to `current`, if anything.
    pub fn reset(&self, current: &ChartStyle) -> Option<ChartStyle> {
        self.edit.reset(current)
    }

    /// The look `name` makes of `current`, and the built-in it was, if one.
    pub fn apply(
        &self,
        name: &str,
        current: &ChartStyle,
    ) -> Option<(ChartStyle, Option<CandlePreset>)> {
        let resolved = self.presets.resolve(name)?;
        let preset = match resolved {
            crate::candle_presets::Resolved::BuiltIn(preset) => Some(preset),
            crate::candle_presets::Resolved::Custom(_) => None,
        };
        Some((resolved.apply_to(current), preset))
    }

    /// Save `style` as `name` and write the file. Answers the name it landed
    /// under.
    pub fn save(&mut self, name: &str, style: ChartStyle) -> Result<String, SaveRefusal> {
        let saved = self.presets.insert(name, style)?;
        self.presets.save(&self.presets_path);
        Ok(saved)
    }

    /// The look saved under `name`, if one is.
    pub fn saved(&self, name: &str) -> Option<ChartStyle> {
        self.presets.get(name).copied()
    }

    /// Forget a saved look and write the file; answers whether there was one.
    pub fn delete(&mut self, name: &str) -> bool {
        let removed = self.presets.remove(name);
        if removed {
            self.presets.save(&self.presets_path);
        }
        removed
    }

    /// The name the look answers to, if any.
    pub fn matching(&self, style: &ChartStyle) -> Option<String> {
        self.presets.matching(style).map(str::to_owned)
    }

    /// The saved names, alphabetically.
    pub fn custom_names(&self) -> Vec<String> {
        self.presets.names().map(str::to_owned).collect()
    }

    /// Read the preset file again.
    pub fn reload_presets(&mut self) {
        self.presets = CandlePresetStore::load(&self.presets_path);
    }

    /// The built-ins and the saved looks, one row; Reset beside them.
    fn presets_row(&mut self, ui: &mut egui::Ui, edited: &mut ChartStyle) -> StylePanelResponse {
        let mut response = StylePanelResponse::default();
        ui.horizontal(|ui| {
            ui.heading("Preset");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let target = self.reset(edited);
                if ui
                    .add_enabled(target.is_some(), egui::Button::new("Reset"))
                    .on_hover_text("put back the appearance this window opened on")
                    .clicked()
                    && let Some(target) = target
                {
                    *edited = target;
                    response.changed = true;
                }
            });
        });
        let current = self.matching(edited);
        ui.horizontal_wrapped(|ui| {
            draw_presets(ui, edited, &mut response);
            for name in self.custom_names() {
                let selected = current.as_deref() == Some(name.as_str());
                let label = ui
                    .selectable_label(selected, &name)
                    .on_hover_text("your saved look · click to load · right-click to delete");
                if label.clicked()
                    && let Some((look, _)) = self.apply(&name, edited)
                {
                    *edited = look;
                    response.changed = true;
                }
                if label.secondary_clicked() {
                    self.delete(&name);
                }
            }
        });
        ui.small(format!(
            "current: {}",
            current.as_deref().unwrap_or("Custom")
        ));
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.name_draft)
                    .desired_width(150.0)
                    .char_limit(MAX_NAME_LEN)
                    .hint_text("name this look"),
            );
            let can_save = !self.name_draft.trim().is_empty();
            if ui
                .add_enabled(can_save, egui::Button::new("save preset"))
                .on_hover_text(
                    "store candles and canvas under that name; a name you already used is \
                     overwritten",
                )
                .clicked()
            {
                let name = std::mem::take(&mut self.name_draft);
                self.refusal = match self.save(&name, *edited) {
                    Ok(_) => None,
                    Err(refusal) => {
                        self.name_draft = name;
                        Some(refusal.message())
                    }
                };
            }
        });
        if let Some(refusal) = self.refusal {
            ui.label(egui::RichText::new(refusal).size(10.5).color(theme::WARN));
        }
        response
    }
}

impl SettingsTab for CandlesTab {
    fn tab(&self) -> ChartSettingsTab {
        ChartSettingsTab::Candles
    }

    fn opened(&mut self, env: &SurfaceEnv<'_>) {
        self.begin(*env.style);
    }

    fn closed(&mut self) {
        self.edit.end();
        self.refusal = None;
    }

    fn draw(&mut self, ui: &mut egui::Ui, env: &SurfaceEnv<'_>) -> TabResponse {
        let mut edited = *env.style;
        let row = self.presets_row(ui, &mut edited);
        ui.separator();
        let body = draw_style_contents(ui, &mut edited);
        let changed = (row.changed || body.changed) && edited != *env.style;
        TabResponse {
            surface: SurfaceResponse {
                style: changed.then_some(edited),
                ..SurfaceResponse::default()
            },
            applied_preset: body.applied_preset.or(row.applied_preset),
        }
    }
}
