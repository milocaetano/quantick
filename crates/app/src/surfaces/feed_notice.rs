//! Borrowed recovery popup painting; the registered model owns its lifetime.
use crate::feed_notice::{self, NoticeAction, Report};
use eframe::egui;
use quantick_ui_model::feed_notice::{Intent, Model, State};

pub(crate) type FeedNoticeSurface = super::Adapter<Model, View>;

#[derive(Default)]
pub(crate) struct FeedNoticeView;
pub(crate) use FeedNoticeView as View;
impl super::View<Model> for View {
    fn id(&self) -> &'static str {
        "feed_notice"
    }
    fn paint(
        &mut self,
        _: &State,
        _: &mut super::ViewContext<'_>,
        _: Option<&super::SurfaceEnv<'_>>,
    ) -> Option<Intent> {
        // The existing canvas pass supplies report and geometry by borrow.
        None
    }
}
pub(crate) fn effects((): ()) -> super::SurfaceResponse {
    super::SurfaceResponse::default()
}
impl FeedNoticeView {
    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        chip: egui::Rect,
        report: &Report<'_>,
    ) -> (NoticeAction, bool) {
        let (action, popup) = feed_notice::draw_popup(ui, area, chip, report);
        let dismissed = ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
        });
        (action, dismissed)
    }
}

pub(crate) fn initialize(surface: &mut FeedNoticeSurface, tab: u64) {
    if feed_notice::popup_open_from_env() {
        surface.update(Intent::InitializeOpen(tab));
    }
}
