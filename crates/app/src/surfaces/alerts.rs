//! Typed, inert editor surface. Child implementation paints these projections.
use super::{Surface, SurfaceEnv, SurfaceResponse};
use eframe::egui;
use quantick_alerts::{AlertBook, AlertCommand, AlertError, DurableDrawingRef};

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Request {
    pub open: bool,
    pub drawing: Option<DurableDrawingRef>,
    pub command: Option<AlertCommand>,
}

#[derive(Default)]
pub(crate) struct AlertSurface;

impl AlertSurface {
    pub(crate) fn open_drawing(&mut self, _target: Option<DurableDrawingRef>) {
        // Reserved drawing entry; no editor is opened by the scaffold.
    }
    /// Global and drawing entry points use the same typed request boundary.
    pub(crate) fn apply(
        &mut self,
        _book: &mut AlertBook,
        _request: Request,
    ) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }
}

impl Surface for AlertSurface {
    fn id(&self) -> &'static str {
        "alerts"
    }
    fn draw(&mut self, _ctx: &egui::Context, env: &SurfaceEnv<'_>) -> SurfaceResponse {
        let _ = env.alerts;
        SurfaceResponse::default()
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, _env: &SurfaceEnv<'_>) {
        // Reserved QUANTICK_ALERTS hook. The scaffold opens no editor.
    }
}
