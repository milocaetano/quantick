//! Canvas popovers borrow current geometry at the existing toolbar settlement.
use crate::toolbar::layout_popover as popover;
use eframe::egui;
use quantick_stores::workspaces::WorkspaceColour;
use quantick_ui_model::canvas_popovers::{Effects, Intent, Model, State};
use quantick_ui_model::layout_picker as picker;
use quantick_workspace::canvas::LayoutMarks;
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
    /// The layout picker while open, painted under `anchor` from the model's
    /// picker; what the frame saw goes back, with a repaint while it moves.
    pub(crate) fn layout(
        &mut self,
        session: &mut quantick_ui_model::UpdateSession<'_, Model>,
        (ctx, anchor): (&egui::Context, egui::Rect),
        (workspace, colour, marks): (&str, Option<WorkspaceColour>, LayoutMarks),
    ) -> popover::Drawn {
        if !session.state().projection().layout_open {
            return popover::Drawn::default();
        }
        let (palette, before) = (crate::theme::get(ctx), session.state().picker().clone());
        let shown = picker::View::new(workspace, colour, marks, &before);
        let shown = picker::picker(&shown, &palette.resolved);
        let (drawn, seen) = popover::draw(ctx, anchor, &shown, marks.current);
        session.update(Intent::Layout(seen));
        if let Some(ms) = session.state().picker().repaint_ms(&before, seen.now_ms) {
            ctx.request_repaint_after(std::time::Duration::from_millis(ms));
        }
        drawn
    }
}
