//! Closed source-editor surface; the editor child implements painting here.
use super::{Surface, SurfaceEnv, SurfaceResponse};
use eframe::egui;

#[derive(Default)]
pub(crate) struct ScriptEditorSurface {
    pub state: quantick_stores::script_editor::ScriptEditor,
}
impl ScriptEditorSurface {
    /// Closed in the skeleton; no input is captured until the editor is enabled.
    pub fn capture_keyboard(&mut self, _ctx: &egui::Context) {}
}
impl Surface for ScriptEditorSurface {
    fn id(&self) -> &'static str {
        "script-editor"
    }
    fn draw(&mut self, _ctx: &egui::Context, _env: &SurfaceEnv<'_>) -> SurfaceResponse {
        SurfaceResponse::default()
    }
}

/// One draw path for the docked editor and alternate surface implementations.
pub(crate) fn draw<S: Surface + ?Sized>(
    surface: &mut S,
    ctx: &egui::Context,
    env: &SurfaceEnv<'_>,
) -> SurfaceResponse {
    surface.draw(ctx, env)
}
