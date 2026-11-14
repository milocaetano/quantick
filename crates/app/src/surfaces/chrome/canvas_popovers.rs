//! Canvas popovers borrow current geometry at the existing toolbar settlement.
use eframe::egui;
use quantick_ui_model::canvas_popovers::{Effects, Intent, Model, State};
#[derive(Default)]
pub(crate) struct CanvasPopoversView;
pub(crate) use CanvasPopoversView as View;
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "canvas_popovers"
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
pub(crate) fn effects(effects: Effects) -> crate::surfaces::SurfaceResponse {
    crate::surfaces::SurfaceResponse {
        model_effects: vec![quantick_ui_model::HostEffect::CanvasPopovers(effects)],
        ..Default::default()
    }
}
impl CanvasPopoversView {
    pub(crate) fn layout(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        ctx: &egui::Context,
        anchor: egui::Rect,
        model: &crate::toolbar::layout_popover::PopoverModel<'_>,
        actions: &mut Vec<crate::toolbar::ToolbarAction>,
    ) -> Vec<crate::toolbar::layout_popover::DrawnCell> {
        if !session.state().projection().layout_open {
            return Vec::new();
        }
        let (cells, outside) = crate::toolbar::layout_popover::draw(ctx, anchor, model, actions);
        let selected = actions
            .iter()
            .any(|action| matches!(action, crate::toolbar::ToolbarAction::SetLayout(_)));
        session.update(Intent::SettleLayout { outside, selected });
        cells
    }
}
