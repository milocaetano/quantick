//! A second Paper surface using only the existing model/view/catalog contract.
use crate::surfaces::{
    BorrowedSurfaceEnv, PaperPanelEffects, SurfaceEnv, SurfaceResponse, ViewContext,
};
use quantick_ui_model::paper_tail_probe::{Effects, Intent, Model, State};
use quantick_ui_model::{Slot, UpdateSession};

#[derive(Default)]
pub(crate) struct View;
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "paper_tail_probe"
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
        _: &mut ViewContext<'_>,
        slot: Slot,
        env: Option<&mut BorrowedSurfaceEnv<'_>>,
    ) -> SurfaceResponse {
        if slot != Slot::FrameTailPaperPanels || session.state().armed.is_none() {
            return SurfaceResponse::default();
        }
        let Some(BorrowedSurfaceEnv::Paper(env)) = env else {
            return SurfaceResponse::default();
        };
        session.update(Intent::Visit(env.paper.account().report.is_open()));
        SurfaceResponse::default()
    }
}
pub(crate) fn effects(effects: Effects) -> SurfaceResponse {
    SurfaceResponse {
        paper_panel: effects.navigate.map(|navigate| {
            Box::new(PaperPanelEffects {
                navigate_to_trade: Some(navigate),
                ..Default::default()
            })
        }),
        undo_drawing: effects.mixed,
        ..Default::default()
    }
}
