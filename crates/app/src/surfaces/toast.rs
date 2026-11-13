//! Window acknowledgement view and supplied host-time conversion.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use crate::theme;
use eframe::egui;
use quantick_ui_model::toast::{Intent, Model, State};
use std::borrow::Cow;
use std::time::Instant;
const BOTTOM_MARGIN_PX: f32 = 44.0;
pub(crate) type ToastSurface = super::Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct View {
    anchor: Option<Instant>,
    #[cfg(test)]
    undo_rect: Option<egui::Rect>,
}
impl View {
    /// Signed nanoseconds retain timestamps supplied before the first anchor.
    fn time(&mut self, now: Instant) -> i128 {
        let anchor = *self.anchor.get_or_insert(now);
        if now >= anchor {
            now.duration_since(anchor).as_nanos() as i128
        } else {
            -(anchor.duration_since(now).as_nanos() as i128)
        }
    }
    #[cfg(test)]
    pub fn undo_rect(&self) -> Option<egui::Rect> {
        self.undo_rect
    }
}
#[cfg(test)]
pub(crate) fn new() -> ToastSurface {
    super::Adapter::new(View::default(), effects)
}
pub(crate) fn note(
    surface: &mut ToastSurface,
    message: impl Into<Cow<'static, str>>,
    now: Instant,
) {
    let now = surface.view.time(now);
    surface.update(Intent::Note {
        message: message.into(),
        now,
    });
}
pub(crate) fn note_with_undo(
    surface: &mut ToastSurface,
    message: impl Into<Cow<'static, str>>,
    now: Instant,
) {
    let now = surface.view.time(now);
    surface.update(Intent::NoteWithUndo {
        message: message.into(),
        now,
    });
}
pub(crate) fn effects(effect: quantick_ui_model::toast::Effects) -> SurfaceResponse {
    SurfaceResponse {
        undo_drawing: effect.undo_drawing,
        ..Default::default()
    }
}
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "toast"
    }
    #[cfg(any(feature = "scenario-harness", test))]
    fn apply_env_hook(&mut self, env: &SurfaceEnv<'_>) -> Option<Intent> {
        let now = self.time(env.now);
        match crate::hooks::captured::var("QUANTICK_TOAST").as_deref() {
            Some("plain") => Some(Intent::Note {
                message: "Workspace saved.".into(),
                now,
            }),
            Some("undo") => Some(Intent::NoteWithUndo {
                message: "Trend line deleted.".into(),
                now,
            }),
            _ => None,
        }
    }
    fn paint_updated(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        env: Option<&SurfaceEnv<'_>>,
    ) {
        if let Some(env) = env {
            session.update(Intent::Tick(self.time(env.now)));
        }
        if let Some(intent) = self.paint(session.state(), target, env) {
            session.update(intent);
        }
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        #[cfg(test)]
        {
            self.undo_rect = None;
        }
        let message = state.message()?;
        let offers_undo = state.offers_undo();
        let ctx = target.context();
        let palette = theme::get(ctx);
        let id = self.id();
        let mut undo_clicked = false;
        #[cfg(test)]
        let mut undo_rect = None;
        egui::Area::new(egui::Id::new(id))
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -BOTTOM_MARGIN_PX),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(palette.tag_bg())
                    .stroke(egui::Stroke::new(1.0_f32, palette.border()))
                    .rounding(6.0_f32)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(message);
                            if offers_undo {
                                let undo = ui.button("Undo");
                                #[cfg(test)]
                                {
                                    undo_rect = Some(undo.rect);
                                }
                                undo_clicked = undo.clicked();
                            }
                        });
                    });
            });
        #[cfg(test)]
        {
            self.undo_rect = undo_rect;
        }
        undo_clicked.then_some(Intent::Undo)
    }
}
#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_TOAST"];
#[cfg(test)]
#[path = "toast/tests.rs"]
mod tests;
