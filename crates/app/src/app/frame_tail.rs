//! Execute the fixed end-of-frame plan against its actual feature owners.
use crate::{
    config::AppConfig,
    feed_notice,
    surfaces::ToastSurface,
    tab::{LiveFeedSpawn, Tab},
    timezone::TzOffset,
};
use eframe::egui;
use quantick_chart_interaction::frame_tail_plan::FrameTailStage;
use std::time::Instant;

pub(super) struct FrameTailOwners<'a> {
    pub tabs: &'a mut [Tab],
    pub active_tab: usize,
    pub config: &'a AppConfig,
    pub toast: &'a mut ToastSurface,
    pub chip_rect: &'a mut Option<egui::Rect>,
    pub popup_tab: &'a mut Option<u64>,
}
pub(super) struct FrameTailInput<'a> {
    pub ctx: &'a egui::Context,
    pub now: Instant,
    pub tz: TzOffset,
    pub notice_action: feed_notice::NoticeAction,
    pub popup_tab: u64,
    pub popup_open: bool,
    pub chip_clicked: bool,
    pub dismissed: bool,
    pub chip_rect: Option<egui::Rect>,
}
impl FrameTailOwners<'_> {
    pub fn execute(
        self,
        input: FrameTailInput<'_>,
        spawn: &mut LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) {
        for stage in stages {
            match stage {
                FrameTailStage::ApplyNoticeAction => {
                    let tab = &mut self.tabs[self.active_tab];
                    match input.notice_action {
                        feed_notice::NoticeAction::None => {}
                        feed_notice::NoticeAction::Reconnect => {
                            let _ = tab.reconnect_feed_with_spawn(self.config, spawn);
                        }
                        feed_notice::NoticeAction::Reload => {
                            let _ = tab.reload_feed_with_spawn(self.config, spawn);
                        }
                    }
                }
                FrameTailStage::SettlePaperPanels => {
                    settle_paper_panels(self.tabs, self.active_tab, self.toast, input.now)
                }
                FrameTailStage::DrawPaperReport => self.tabs[self.active_tab]
                    .paper
                    .draw_report_window(input.ctx, input.tz),
                FrameTailStage::PublishFeedPopup => {
                    *self.chip_rect = input.chip_rect;
                    *self.popup_tab = feed_notice::popup_still_open(
                        input.popup_open,
                        input.chip_clicked,
                        input.chip_rect.is_some(),
                        input.dismissed,
                        input.notice_action,
                    )
                    .then_some(input.popup_tab);
                }
            }
        }
    }
}

/// Settle every account; watched acknowledgement wins, else first background.
pub(super) fn settle_paper_panels(
    tabs: &mut [Tab],
    active_tab: usize,
    toast: &mut ToastSurface,
    now: Instant,
) {
    let mut watched = None;
    let mut background = None;
    for (index, tab) in tabs.iter_mut().enumerate() {
        tab.paper.settle();
        let Some(message) = tab.paper.take_toast() else {
            continue;
        };
        if index == active_tab {
            watched = Some(message);
        } else if background.is_none() {
            // The interpunct is the window's own separator — the status
            // bar, the tape's axis caption and the layout strip all use
            // it, and the messages themselves already carry a colon
            // (`SIM: …`). A second one would read as two labels.
            background = Some(format!("{} · {message}", tab.symbol));
        }
    }
    if let Some(message) = background {
        toast.note(message, now);
    }
    if let Some(message) = watched {
        toast.note(message, now);
    }
}
