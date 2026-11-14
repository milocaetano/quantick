//! A real catalog row borrowing the same owner as its independent peer.
use crate::surfaces::{BorrowedSurfaceEnv, SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
use quantick_ui_model::borrowed_first::{Effects, Intent, Model, State};
use quantick_ui_model::{Slot, UpdateSession};

#[derive(Default)]
pub(crate) struct View {
    pub painted: Option<egui::Rect>,
    pub immediate: Option<bool>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "borrowed_first"
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
        self.painted = None;
        self.immediate = None;
        session.update(Intent::Clear);
        if !session.state().enabled || slot != Slot::LateEdgePanels {
            return SurfaceResponse::default();
        }
        let Some(BorrowedSurfaceEnv::Paper(env)) = env else {
            return SurfaceResponse::default();
        };
        assert!(env.replay.is_none());
        assert_eq!(env.timezone, crate::timezone::TzOffset::default());
        let mut settings = env.paper.cmd_trading();
        let observed = settings.enabled;
        session.update(Intent::Observe(observed));
        self.immediate = session.state().observed;
        settings.enabled = !observed;
        env.paper.set_cmd_trading(settings);
        self.painted = Some(
            egui::SidePanel::left(self.id())
                .min_width(0.0)
                .exact_width(37.0)
                .resizable(false)
                .frame(egui::Frame::none())
                .show(target.context(), |_| {})
                .response
                .rect,
        );
        let mut response = effects(Effects(Some((observed, "returned"))));
        response.paper_panel = Some(Box::new(crate::surfaces::PaperPanelEffects {
            navigate_to_trade: Some((11, 12)),
            journal_edit: Some(quantick_paper::report::session_edit::JournalEdit::Delete {
                session: quantick_paper::report::sessions::SessionKey::parse("PROBE/first.csv")
                    .unwrap(),
            }),
            close_replay: true,
            cmd_trading_changed: true,
            ..Default::default()
        }));
        response
    }
}
pub(crate) fn effects(value: Effects) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: if value.0.is_some() {
            vec![quantick_ui_model::HostEffect::BorrowedFirst(value)]
        } else {
            vec![]
        },
        ..Default::default()
    }
}
