//! Test-only edge panel registered through the shared catalog.
use eframe::egui;
use quantick_ui_model::chrome_edge_second::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct View {
    pub painted: Option<egui::Rect>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "chrome_edge_second"
    }
    fn paint(
        &mut self,
        state: &State,
        target: &mut crate::surfaces::ViewContext<'_>,
        _: Option<&crate::surfaces::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        self.painted = None;
        if !state.visible {
            return None;
        }
        self.painted = Some(
            egui::TopBottomPanel::top(self.id())
                .exact_height(29.0)
                .resizable(false)
                .frame(egui::Frame::none())
                .show(target.context(), |_| {})
                .response
                .rect,
        );
        Some(Intent::Add(1))
    }
}
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::ChromeEdgeSecond(effects)],
        ..Default::default()
    }
}
