//! Registered access presentation. The borrowed gateway remains the authority.
use super::{BorrowedSurfaceEnv, HostEffect, SurfaceEnv, SurfaceResponse, ViewContext};
use quantick_ui_model::access_panel::{Effects, Intent, Model, State};
use quantick_ui_model::{Slot, UpdateSession};

#[cfg(any(feature = "control-harness", test))]
pub(crate) type AccessPanelSurface = super::Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct View;
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "access_panel"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
    fn paint_borrowed(
        &mut self,
        session: &mut UpdateSession<'_, Model>,
        target: &mut ViewContext<'_>,
        slot: Slot,
        env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> SurfaceResponse {
        if slot == Slot::MenuDialogs
            && session.state().open
            && let Some(BorrowedSurfaceEnv::Access(env)) = env
            && let Some(access) = env.access.as_deref_mut()
        {
            session.update(Intent::Observe(access.panel_facts()));
            paint_window(access, target.context(), session);
        }
        SurfaceResponse::default()
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: (effects != Effects::default())
            .then_some(HostEffect::AccessPanel(effects))
            .into_iter()
            .collect(),
        ..Default::default()
    }
}
#[cfg(test)]
#[path = "access_panel/tests.rs"]
mod tests;

/// Native sizing belongs to the registered View; the body borrows host content.
const CONTROL_PANEL_DEFAULT_WIDTH_PX: f32 = 520.0;
const CONTROL_PANEL_MAX_SCREEN_FRACTION: f32 = 0.8;
pub(crate) fn paint_window(
    access: &mut crate::control::ControlAccess,
    ctx: &eframe::egui::Context,
    session: &mut UpdateSession<'_, Model>,
) {
    let mut open = session.state().open;
    // The body is four consent sections, a button and the client list, and
    // it grew past the window when the analyst tier was added to it: on a
    // 1009-point screen the enabled state reached the bottom edge with the
    // client list under it, and an auto-sized egui window does not scroll
    // by itself, so the disable button could not be reached at all.
    //
    // The whole window, not `available_rect`: by the time the panel draws,
    // the chart and the dock have claimed their space, and a fraction of
    // what is left is a fraction of the wrong rectangle — it made the
    // panel a third of its own content tall.
    let max_height = ctx.screen_rect().height() * CONTROL_PANEL_MAX_SCREEN_FRACTION;
    let id = eframe::egui::Id::new("control_access_panel");
    crate::popup::centred(eframe::egui::Window::new("Local agent access"), id, ctx)
        .open(&mut open)
        .default_width(CONTROL_PANEL_DEFAULT_WIDTH_PX)
        .resizable(true)
        // A scrolling window stops sizing itself to its content, so the
        // default height has to be asked for as well: without it egui
        // opens the panel at its own default and clips the consent text a
        // third of the way in.
        .default_height(max_height)
        .max_height(max_height)
        .vscroll(true)
        .show(ctx, |ui| access.draw_panel_body(ui, session));
    if !open {
        session.update(Intent::Close);
    }
}
