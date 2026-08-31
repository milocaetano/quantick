//! The footprint settings window, as a [`Surface`].
//!
//! The knobs themselves live in `footprint_panel.rs`; what the trunk held was
//! the window's open flag, its preset store, the "save as" field, and the
//! four-way translation between what the panel returned and what the
//! application had to do about it.
//!
//! # What moved and what did not
//!
//! The **preset store** is the panel's own: nothing else in the application
//! reads it, its file is opened only to fill this list, and the half-typed
//! name in the "save as" field is meaningless outside the window it is typed
//! in. Those live here.
//!
//! The **effective configuration** does not. Every footprint frame reads it,
//! it is written to `footprint.toml` by a path the host owns, and a pane may
//! override it — so the surface edits a copy and reports the outcome through
//! [`SurfaceResponse::footprint`], the same bargain the appearance window
//! makes with the chart style.

use std::path::PathBuf;

use eframe::egui;

use super::{FootprintChange, Surface, SurfaceEnv, SurfaceResponse};
use crate::footprint_panel::{self, PanelInput, PanelOutcome};
use crate::footprint_presets::PresetStore;

/// The footprint settings window and the preset list inside it.
pub(crate) struct FootprintSettingsSurface {
    open: bool,
    /// The saved setups offered in the window, and the file they came from.
    presets: PresetStore,
    presets_path: PathBuf,
    /// The "save as" field's text, kept across frames so a name several
    /// keystrokes long survives being typed.
    name_draft: String,
}

impl Default for FootprintSettingsSurface {
    /// Loads the preset file, because a window whose list is empty until
    /// something calls a second initialiser is a window that ships empty the
    /// one time that call is forgotten.
    fn default() -> Self {
        let presets_path = crate::footprint_presets::default_path();
        Self {
            open: false,
            presets: PresetStore::load(&presets_path),
            presets_path,
            name_draft: String::new(),
        }
    }
}

impl FootprintSettingsSurface {
    /// Put the window on screen — the toolbar button, the pane's layer menu
    /// and the capture hook all come through here.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Read the preset file again, after an imported cockpit replaced it.
    ///
    /// Through the same loader the constructor uses: an import restored by
    /// its own code path is how the two drift.
    pub fn reload_presets(&mut self) {
        self.presets = PresetStore::load(&self.presets_path);
    }
}

impl Surface for FootprintSettingsSurface {
    fn id(&self) -> &'static str {
        "footprint-settings"
    }

    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        if std::env::var("QUANTICK_FOOTPRINT_PANEL").is_ok_and(|value| value == "1") {
            self.open();
        }
    }

    fn draw(&mut self, ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        if !self.open {
            return SurfaceResponse::default();
        }
        // Cloned, not borrowed: the panel edits in place and the original is
        // what every footprint frame is reading. Only on the frames the
        // window is open, which is what keeps this off the frame budget.
        let mut edited = env.footprint.clone();
        let outcome = footprint_panel::draw(
            ctx,
            PanelInput {
                open: &mut self.open,
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
        SurfaceResponse {
            footprint,
            ..SurfaceResponse::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::footprint_config::FootprintConfig;
    use crate::pane::PaneSide;

    fn env(footprint: &FootprintConfig) -> SurfaceEnv<'_> {
        SurfaceEnv {
            footprint,
            focused_side: PaneSide::Flow,
            footprint_customized: false,
            ..SurfaceEnv::quiet(Instant::now())
        }
    }

    /// A closed window reads nothing, clones nothing and asks for nothing.
    #[test]
    fn a_closed_window_asks_for_nothing() {
        let ctx = egui::Context::default();
        let config = FootprintConfig::default();
        let mut surface = FootprintSettingsSurface::default();
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&config));
        });
        assert_eq!(response, SurfaceResponse::default());
    }

    /// An open window that nobody touched leaves the configuration alone.
    /// The panel returns `Untouched` on a quiet frame, and the surface must
    /// not turn that into a write — the file would be rewritten at the
    /// refresh rate.
    #[test]
    fn an_untouched_window_writes_nothing() {
        let ctx = egui::Context::default();
        let config = FootprintConfig::default();
        let mut surface = FootprintSettingsSurface::default();
        surface.open();
        let mut response = SurfaceResponse::default();
        let _ = ctx.run(Default::default(), |ctx| {
            response = surface.draw(ctx, &env(&config));
        });
        assert!(response.footprint.is_none());
    }

    /// The toolbar, the layer menu and the hook all reach the window through
    /// one door, so a click and a named call cannot disagree about it.
    #[test]
    fn the_window_opens_through_its_own_door() {
        let mut surface = FootprintSettingsSurface::default();
        assert!(!surface.open);
        surface.open();
        assert!(surface.open);
    }
}
