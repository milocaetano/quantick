//! The Footprint tab: the focused chart's footprint knobs and their presets.
//!
//! The knobs live in `footprint_panel.rs`; what this tab holds is the preset
//! store, the "save as" field, and the translation from what the panel
//! returned into what the host has to do about it. The effective
//! configuration stays the host's — every footprint frame reads it — so the
//! tab edits a copy and reports through [`SurfaceResponse::footprint`].

use std::path::PathBuf;

use eframe::egui;

use super::{ChartSettingsTab, SettingsTab, TabResponse};
use crate::footprint_panel::{self, PanelInput, PanelOutcome};
use crate::footprint_presets::PresetStore;
use crate::surfaces::{FootprintChange, SurfaceEnv, SurfaceResponse};

/// The Footprint tab's state.
pub(crate) struct FootprintTab {
    /// The saved setups offered in the tab, and the file they came from.
    presets: PresetStore,
    presets_path: PathBuf,
    /// The "save as" field's text, kept across frames.
    name_draft: String,
}

impl Default for FootprintTab {
    /// Loads the preset file, so the list never ships empty for want of a
    /// second initialiser.
    fn default() -> Self {
        let presets_path = crate::footprint_presets::default_path();
        Self {
            presets: PresetStore::load(&presets_path),
            presets_path,
            name_draft: String::new(),
        }
    }
}

impl FootprintTab {
    /// Read the preset file again, through the loader the constructor uses.
    pub fn reload_presets(&mut self) {
        self.presets = PresetStore::load(&self.presets_path);
    }
}

impl SettingsTab for FootprintTab {
    fn tab(&self) -> ChartSettingsTab {
        ChartSettingsTab::Footprint
    }

    fn draw(&mut self, ui: &mut egui::Ui, env: &SurfaceEnv<'_>) -> TabResponse {
        // Cloned, not borrowed: the panel edits in place and the original is
        // what every footprint frame is reading.
        let mut edited = env.footprint.clone();
        let outcome = footprint_panel::draw(
            ui,
            PanelInput {
                config: &mut edited,
                presets: &mut self.presets,
                presets_path: &self.presets_path,
                name_draft: &mut self.name_draft,
                target: &format!("{} chart", env.focused_side.title().to_lowercase()),
                customized: env.footprint_customized,
            },
        );
        let footprint = match outcome {
            PanelOutcome::Untouched => None,
            PanelOutcome::Changed => Some(FootprintChange::Applied(Box::new(edited))),
            PanelOutcome::ResetToDefault => Some(FootprintChange::ResetToDefault),
        };
        TabResponse {
            surface: SurfaceResponse {
                footprint,
                ..SurfaceResponse::default()
            },
            applied_preset: None,
        }
    }
}
