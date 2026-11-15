//! A real catalog row borrowing the same owner as its independent peer.
use crate::surfaces::{BorrowedSurfaceEnv, SurfaceEnv, SurfaceResponse, ViewContext};
use eframe::egui;
use quantick_ui_model::borrowed_second::{Effects, Intent, Model, State};
use quantick_ui_model::{Slot, UpdateSession};

#[derive(Default)]
pub(crate) struct View {
    pub painted: Option<egui::Rect>,
    pub immediate: Option<bool>,
}
impl crate::surfaces::View<Model> for View {
    fn id(&self) -> &'static str {
        "borrowed_second"
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
        if !session.state().enabled {
            return SurfaceResponse::default();
        }
        if slot == Slot::PinnedInspector {
            let Some(BorrowedSurfaceEnv::Drawings(env)) = env else {
                return SurfaceResponse::default();
            };
            let observed = env.selected.is_some();
            session.update(Intent::Observe(observed));
            self.immediate = session.state().observed;
            self.painted = Some(
                egui::TopBottomPanel::top("pinned_probe")
                    .min_height(0.0)
                    .exact_height(29.0)
                    .resizable(false)
                    .frame(egui::Frame::none())
                    .show(target.context(), |_| {})
                    .response
                    .rect,
            );
            return effects(Effects(Some((observed, "returned"))));
        }
        if slot != Slot::LateEdgePanels {
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
            egui::TopBottomPanel::top(self.id())
                .min_height(0.0)
                .exact_height(29.0)
                .resizable(false)
                .frame(egui::Frame::none())
                .show(target.context(), |_| {})
                .response
                .rect,
        );
        let mut response = effects(Effects(Some((observed, "returned"))));
        response.paper_panel = Some(Box::new(crate::surfaces::PaperPanelEffects {
            navigate_to_trade: Some((21, 22)),
            journal_edit: Some(quantick_paper::report::session_edit::JournalEdit::Delete {
                session: quantick_paper::report::sessions::SessionKey::parse("PROBE/second.csv")
                    .unwrap(),
            }),
            open_replay_browser: true,
            pick_trades_dir: true,
            order_strategies_changed: true,
            risk_settings_changed: true,
            ..Default::default()
        }));
        response
    }
}
pub(crate) fn effects(value: Effects) -> SurfaceResponse {
    SurfaceResponse {
        model_effects: if value.0.is_some() {
            vec![quantick_ui_model::HostEffect::BorrowedSecond(value)]
        } else {
            vec![]
        },
        ..Default::default()
    }
}
