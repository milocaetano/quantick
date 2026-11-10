//! The Candles tab: the appearance knobs, the built-in and saved presets,
//! and Reset to the look the window opened on.
//!
//! The decisions are headless — what a preset name means and what a saved
//! file holds are `quantick_stores::candle_presets`, what Reset returns to is
//! `quantick_chart::style::StyleEdit` — so this file is the egui half and the
//! one place both are reached from, by the window and the control plane alike.

use super::{ChartSettingsTab, SettingsTab, TabResponse};
use crate::candle_presets::MAX_NAME_LEN;
#[cfg(test)]
use crate::candle_presets::PresetError;
use crate::candle_view::{StylePanelResponse, draw_presets, draw_style_contents};
use crate::style::ChartStyle;
use crate::surfaces::{SurfaceEnv, SurfaceResponse};
use crate::theme;
use eframe::egui;
use quantick_stores::candle_settings::CandleSettings;

pub(crate) struct CandlesTab {
    pub state: CandleSettings,
    name_draft: String,
    refusal: Option<String>,
}
impl std::ops::Deref for CandlesTab {
    type Target = CandleSettings;
    fn deref(&self) -> &CandleSettings {
        &self.state
    }
}
impl std::ops::DerefMut for CandlesTab {
    fn deref_mut(&mut self) -> &mut CandleSettings {
        &mut self.state
    }
}
impl Default for CandlesTab {
    fn default() -> Self {
        Self {
            state: CandleSettings::open(crate::candle_presets::default_path()),
            name_draft: String::new(),
            refusal: None,
        }
    }
}
impl CandlesTab {
    #[cfg(test)]
    pub fn with_presets_path(path: std::path::PathBuf) -> Self {
        Self {
            state: CandleSettings::open(path),
            ..Self::default()
        }
    }

    /// A warning is about the last attempt: a new name typed, or anything
    /// that then went right, retires it.
    fn settle(&mut self, moved_on: bool) {
        if moved_on {
            self.refusal = None;
        }
    }

    /// The built-ins and the saved looks, one row; Reset beside them.
    fn presets_row(&mut self, ui: &mut egui::Ui, edited: &mut ChartStyle) -> StylePanelResponse {
        let palette = theme::get(ui.ctx());
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
            let saved = self.custom_names();
            if !saved.is_empty() {
                // The trader's own looks, told apart from the built-ins.
                ui.separator();
                ui.label(theme::supporting_text(ui, "Saved").small());
            }
            for name in saved {
                let selected = current.as_deref() == Some(name.as_str());
                let label = ui
                    .selectable_label(selected, &name)
                    .on_hover_text("your saved look · click to load · right-click for more");
                if label.clicked()
                    && let Some((look, _)) = self.apply(&name, edited)
                {
                    *edited = look;
                    response.changed = true;
                }
                label.context_menu(|ui| {
                    if ui.button("Delete").clicked() {
                        self.refusal = self.delete(&name).err().map(|error| error.message());
                        ui.close_menu();
                    }
                });
            }
        });
        ui.small(format!(
            "current: {}",
            current.as_deref().unwrap_or("Custom")
        ));
        ui.horizontal(|ui| {
            let hint = crate::theme::supporting_text(ui, "name this look");
            let typed = crate::theme::preset_input(
                ui,
                egui::TextEdit::singleline(&mut self.name_draft)
                    .desired_width(150.0)
                    .char_limit(MAX_NAME_LEN)
                    .hint_text(hint),
            )
            .changed();
            self.settle(typed);
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
        if let Some(refusal) = &self.refusal {
            ui.label(
                egui::RichText::new(refusal)
                    .font(palette.body_font(10.5))
                    .color(palette.warn()),
            );
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
        self.state.end();
        self.refusal = None;
    }

    fn draw(&mut self, ui: &mut egui::Ui, env: &SurfaceEnv<'_>) -> TabResponse {
        let mut edited = *env.style;
        let row = self.presets_row(ui, &mut edited);
        ui.separator();
        let body = draw_style_contents(ui, &mut edited, env.style_surface);
        let changed = (row.changed || body.changed) && edited != *env.style;
        self.settle(changed);
        TabResponse {
            surface: SurfaceResponse {
                style: changed.then_some(edited),
                ..SurfaceResponse::default()
            },
            applied_preset: body.applied_preset.or(row.applied_preset),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_warning_lasts_until_the_trader_moves_on() {
        let refused = PresetError::from(crate::candle_presets::SaveRefusal::BuiltInName);
        let mut tab = CandlesTab {
            refusal: Some(refused.message()),
            ..CandlesTab::default()
        };
        tab.settle(false);
        assert!(tab.refusal.is_some(), "a quiet frame keeps it");
        tab.settle(true);
        assert_eq!(
            tab.refusal, None,
            "a new name or a landed change retires it"
        );
    }
}
