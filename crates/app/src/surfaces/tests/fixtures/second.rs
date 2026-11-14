//! Test view declared through the same catalog entry shape as shipped views.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;

#[derive(Default)]
pub(crate) struct View {
    pub enabled: bool,
    pub painted: Option<i64>,
    pub intent: Option<(i64, u64)>,
}
impl super::View<quantick_ui_model::second::Model> for View {
    fn id(&self) -> &'static str {
        "second-fixture"
    }
    fn paint(
        &mut self,
        state: &i64,
        target: &mut ViewContext<'_>,
        _: Option<&SurfaceEnv<'_>>,
    ) -> Option<(i64, u64)> {
        if !self.enabled {
            return None;
        }
        self.painted = Some(*state);
        if let Some(ui) = target.ui.as_mut() {
            ui.label(state.to_string());
        } else {
            egui::Window::new("second fixture").show(target.context(), |ui| {
                ui.label(state.to_string());
            });
        }
        self.intent.take()
    }
}
pub(crate) fn effects((value, now): (i64, u64)) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: vec![super::HostEffect::Second((value, now))],
        undo_drawing: value == 8 && now == 17,
        ..Default::default()
    }
}
