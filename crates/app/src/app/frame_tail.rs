//! Execute the fixed end-of-frame plan against its actual feature owners.
use super::arrangement_host::ArrangementHost;
use crate::{
    config::AppConfig,
    feed_notice,
    surfaces::{
        BorrowedSurfaceEnv, PaperPanelEnv, SurfaceResponse, Surfaces, ToastSurface, ViewContext,
    },
    tab::LiveFeedSpawn,
    timezone::TzOffset,
};
use eframe::egui;
use quantick_chart_interaction::frame_tail_plan::FrameTailStage;
use std::time::Instant;

pub(super) struct FrameTailOwners<'a> {
    pub tabs: &'a mut super::arrangement_host::ArrangementHost,
    pub config: &'a AppConfig,
    pub surfaces: &'a mut Surfaces,
    pub chip_rect: &'a mut Option<egui::Rect>,
}
pub(super) struct FrameTailInput<'a> {
    /// Available only for the workspace's currently bound native view.
    pub presentation: Option<&'a egui::Context>,
    pub now: Instant,
    pub tz: TzOffset,
    pub notice_action: feed_notice::NoticeAction,
    pub popup_tab: u64,
    pub chip_clicked: bool,
    pub dismissed: bool,
    pub chip_rect: Option<egui::Rect>,
}
impl FrameTailOwners<'_> {
    /// A single stage lends the actual owners only until its response returns.
    pub fn execute_stage(
        self,
        input: &FrameTailInput<'_>,
        spawn: &mut LiveFeedSpawn<'_>,
        stage: FrameTailStage,
    ) -> SurfaceResponse {
        self.execute_settling(input, spawn, stage, settle_active_paper_panel)
    }

    fn execute_settling(
        self,
        input: &FrameTailInput<'_>,
        spawn: &mut LiveFeedSpawn<'_>,
        stage: FrameTailStage,
        settle: fn(&mut ArrangementHost, &mut ToastSurface, Instant),
    ) -> SurfaceResponse {
        if stage.needs_presentation() && input.presentation.is_none() {
            return SurfaceResponse::default();
        }
        match stage {
            FrameTailStage::ApplyNoticeAction => {
                let active = self.tabs.active_index();
                let tab = self.tabs.runtime_mut(active);
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
                settle(self.tabs, &mut self.surfaces.toast, input.now)
            }
            FrameTailStage::DrawPaperReport => {
                let Some(ctx) = input.presentation else {
                    return SurfaceResponse::default();
                };
                let tab = self.tabs.runtime_mut(self.tabs.active_index());
                let mut env = BorrowedSurfaceEnv::Paper(PaperPanelEnv {
                    replay: tab.link.replay.as_ref(),
                    paper: &mut tab.paper,
                    timezone: input.tz,
                });
                return self.surfaces.draw_slot_unscoped_with_env(
                    quantick_ui_model::Slot::FrameTailPaperPanels,
                    &mut ViewContext { ctx, ui: None },
                    Some(&mut env),
                );
            }
            FrameTailStage::PublishFeedPopup => {
                *self.chip_rect = input.chip_rect;
                self.surfaces
                    .feed_notice
                    .update(quantick_ui_model::feed_notice::Intent::Settle {
                        tab: input.popup_tab,
                        chip_clicked: input.chip_clicked,
                        offline: input.chip_rect.is_some(),
                        dismissed: input.dismissed,
                        action_taken: input.notice_action != feed_notice::NoticeAction::None,
                    });
            }
        }
        SurfaceResponse::default()
    }
}

#[cfg(test)]
impl FrameTailOwners<'_> {
    /// Legacy owner-level tests settle all accounts; full app tests use run_frame_tail.
    pub fn execute(
        self,
        input: FrameTailInput<'_>,
        spawn: &mut LiveFeedSpawn<'_>,
        stages: impl IntoIterator<Item = FrameTailStage>,
    ) -> Vec<SurfaceResponse> {
        let mut responses = Vec::new();
        for stage in stages {
            let response = FrameTailOwners {
                tabs: self.tabs,
                config: self.config,
                surfaces: self.surfaces,
                chip_rect: self.chip_rect,
            }
            .execute_settling(&input, spawn, stage, settle_paper_panels);
            if response != SurfaceResponse::default() {
                responses.push(response);
            }
        }
        responses
    }
}

/// Settle every account; watched acknowledgement wins, else first background.
#[cfg(test)]
pub(super) fn settle_paper_panels(
    tabs: &mut super::arrangement_host::ArrangementHost,
    toast: &mut ToastSurface,
    now: Instant,
) {
    settle_paper_panels_where(tabs, toast, now, |_| true);
}

pub(super) fn settle_active_paper_panel(
    tabs: &mut super::arrangement_host::ArrangementHost,
    toast: &mut ToastSurface,
    now: Instant,
) {
    let active = tabs.active_id();
    settle_paper_panels_where(tabs, toast, now, |id| id == active);
}

/// The tab each window shows.
fn shown(tabs: &ArrangementHost) -> Vec<u64> {
    let windows = tabs.windows();
    windows
        .windows()
        .iter()
        .map(|window| window.selected().0)
        .collect()
}

pub(super) fn settle_background_paper_panels(
    tabs: &mut ArrangementHost,
    toast: &mut ToastSurface,
    now: Instant,
) {
    let shown = shown(tabs);
    settle_paper_panels_where(tabs, toast, now, |id| !shown.contains(&id));
}

/// Cancels for the disarmed bots of the tabs no window shows.
pub(super) fn clean_up_background_strategies(tabs: &mut ArrangementHost) {
    let shown = shown(tabs);
    for (id, tab) in tabs.iter_with_ids_mut() {
        if !shown.contains(&id) {
            tab.apply_strategy_cleanup();
        }
    }
}

fn settle_paper_panels_where(
    tabs: &mut super::arrangement_host::ArrangementHost,
    toast: &mut ToastSurface,
    now: Instant,
    include: impl Fn(u64) -> bool,
) {
    let active_tab = tabs.active_index();
    let mut watched = None;
    let mut background = None;
    for (index, (id, tab)) in tabs.iter_with_ids_mut().enumerate() {
        if !include(id) {
            continue;
        }
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
        crate::surfaces::toast::note(toast, message, now);
    }
    if let Some(message) = watched {
        crate::surfaces::toast::note(toast, message, now);
    }
}
