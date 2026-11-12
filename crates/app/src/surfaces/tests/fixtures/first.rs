//! Test view declared through the same catalog entry shape as shipped views.
use super::{SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;

#[derive(Default)]
pub(crate) struct View {
    pub enabled: bool,
    pub hook_intent: Option<(i64, u64)>,
    pub painted: Option<i64>,
    pub intent: Option<(i64, u64)>,
}
impl super::View<quantick_ui_model::first::Model> for View {
    fn apply_env_hook(&mut self, _: &SurfaceEnv<'_>) -> Option<(i64, u64)> {
        self.hook_intent.take()
    }
    fn id(&self) -> &'static str {
        "first-fixture"
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
            egui::Window::new("first fixture").show(target.context(), |ui| {
                ui.label(state.to_string());
            });
        }
        self.intent.take()
    }
}
pub(crate) fn effects((value, now): (i64, u64)) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: vec![super::HostEffect::First((value, now))],
        undo_drawing: value == 8 && now == 17,
        ..Default::default()
    }
}

#[test]
fn hook_updates_state_before_first_paint_and_folds_effects_once() {
    let mut registry = super::Surfaces::default();
    registry.first.view.enabled = true;
    registry.first.view.hook_intent = Some((8, 17));
    let ctx = egui::Context::default();
    let env = SurfaceEnv::quiet(std::time::Instant::now());
    let _ = ctx.run(Default::default(), |ctx| {
        assert!(registry.draw_all(ctx, &env).undo_drawing);
    });
    assert_eq!(registry.first.view.painted, Some(8));
    assert_eq!(registry.first.model_state, 8);
    let _ = ctx.run(Default::default(), |ctx| {
        assert!(!registry.draw_all(ctx, &env).undo_drawing);
    });
    assert_eq!(registry.first.model_state, 8);
}

#[test]
fn unscoped_host_intents_update_only_the_registered_model_and_return_effects() {
    let mut registry = super::Surfaces::default();
    let response = registry.update(super::HostIntent::First(Box::new((8, 17))));
    assert!(response.undo_drawing);
    assert_eq!(registry.first.model_state, 8);
    assert_eq!(registry.second.model_state, 0);
    assert_eq!(*registry.first.scoped(10).0, 0);
}
