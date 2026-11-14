//! Borrowed layout dialog painting, at the existing overlay and footer phases.
use quantick_ui_model::layout_dialogs::{Effects, Intent, Model, State};
pub(crate) type LayoutDialogsSurface = crate::surfaces::Adapter<Model, View>;
#[derive(Default)]
pub(crate) struct LayoutDialogsView;
pub(crate) use LayoutDialogsView as View;
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "layout_dialogs"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        None
    }
}
pub(crate) fn effects(effect: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::LayoutDialogs(effect)],
        ..Default::default()
    }
}
impl LayoutDialogsView {
    pub(crate) fn confirm(
        &mut self,
        ctx: &eframe::egui::Context,
        prompt: &quantick_session::prompt::Prompt,
    ) -> Option<bool> {
        crate::widgets::ask(ctx, prompt).map(|answer| answer == 0)
    }
}
