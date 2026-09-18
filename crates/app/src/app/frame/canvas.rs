//! The canvas stage's overlays and what the canvas answers the frame tail.
//!
//! The chart itself is drawn by the active tab through its own
//! [`CanvasChrome`](crate::tab::CanvasChrome) borrow; what lives here reads
//! that tab after it drew: the loading waits over the surfaces they are
//! about, the deal-recording corner, and the feed's offline chip and popup.

use eframe::egui;
use quantick_feed::stall::Stall;

use crate::feed_notice;
use crate::loading::{self, LoadingScope};
use crate::pane::PaneSide;
use crate::tab::Tab;

/// What the canvas answers for the stages after it: the feed chip and popup
/// the tail publishes, and whether a placement asked for its note typed.
#[derive(Debug, Default)]
pub(in crate::app) struct CanvasAnswers {
    /// The tab the popup speaks for, read before the canvas borrows the tabs.
    pub popup_tab: u64,
    pub popup_open: bool,
    pub notice_action: feed_notice::NoticeAction,
    pub chip_clicked: bool,
    pub dismissed: bool,
    /// Where the corner landed, and so whether there was one at all. A feed
    /// that recovered while the popup was open closes it, rather than
    /// leaving a stale explanation over a chart that is fine again.
    pub chip_rect: Option<egui::Rect>,
    /// Raised by a placement that wants its note typed, and handed to the
    /// drawing chrome: the flag belongs to the editor that owns the caret,
    /// not to the canvas that asks for it.
    pub begin_text_edit: bool,
}

/// Each wait on the surface it is about, then the feed's own report in the
/// corner. The panes published their rects on the draw just before this, so
/// these are this frame's geometry rather than the previous one's.
pub(super) fn draw_overlays(
    ui: &mut egui::Ui,
    area: egui::Rect,
    tab: &Tab,
    stall: Option<&Stall>,
    answers: &mut CanvasAnswers,
) {
    draw_loading_waits(ui, area, tab);
    // The feed's own report goes in the corner rather than over the chart.
    // Progress never gets here: a first connection and a history block
    // already have the loading overlay above, and a second badge beside it
    // would be the interface talking about itself twice.
    // The deal-recording chip, left of the offline chip's corner.
    let corner_area = tab
        .panes()
        .filter_map(|(pane, _)| pane.frame.area)
        .max_by(|left, right| {
            left.right()
                .total_cmp(&right.right())
                .then(left.bottom().total_cmp(&right.bottom()))
        })
        .unwrap_or(area);
    crate::deal_recording_ui::draw_corner(ui, corner_area, tab, stall);
    if let Some(report) = feed_notice::report(&tab.notice, stall)
        && report.is_offline()
    {
        draw_offline_notice(ui, area, corner_area, tab, &report, answers);
    }
}

fn draw_loading_waits(ui: &mut egui::Ui, area: egui::Rect, tab: &Tab) {
    let history_note = tab.history_note();
    loading::overlay_scoped(ui, area, &tab.loading, LoadingScope::Whole, history_note);
    // A scope whose surface is not on screen falls back to the canvas rather
    // than dropping its wait: the flow pane is not painted in the Time
    // layout, and a flow-only layout has no time pane, and in both cases the
    // wait is still running. A spinner in the wrong place is a placement
    // complaint; a missing one reads as a frozen application.
    loading::overlay_scoped(
        ui,
        tab.flow_pane.frame.area.unwrap_or(area),
        &tab.loading,
        LoadingScope::Flow,
        history_note,
    );
    let mut time_panes = tab
        .panes()
        .filter(|(_, side)| matches!(side, PaneSide::Time(_)))
        .filter_map(|(pane, _)| pane.frame.area)
        .peekable();
    if time_panes.peek().is_none() {
        loading::overlay_scoped(
            ui,
            area,
            &tab.loading,
            LoadingScope::TimePanes,
            history_note,
        );
    }
    for rect in time_panes {
        loading::overlay_scoped(
            ui,
            rect,
            &tab.loading,
            LoadingScope::TimePanes,
            history_note,
        );
    }
}

fn draw_offline_notice(
    ui: &mut egui::Ui,
    area: egui::Rect,
    corner_area: egui::Rect,
    tab: &Tab,
    report: &feed_notice::Report<'_>,
    answers: &mut CanvasAnswers,
) {
    let popup_open = answers.popup_open;
    // Measured once, then handed to everything that needs it: the chip's
    // own hit test, the popup's anchor, the dismissal test, and the scene's
    // bounds.
    let chip = feed_notice::chip_rect(ui.painter(), corner_area);
    answers.chip_rect = Some(chip);
    answers.chip_clicked = feed_notice::draw_chip(ui, chip, report, popup_open);
    // A pane with nothing on it has room to say why, and a corner chip alone
    // on a blank canvas is a puzzle. One muted line, no border and no buttons
    // — the way out is still the corner.
    //
    // Not while the popup is up. The line and the popup carry the same
    // headline, and on the empty chart that is exactly where both of them
    // draw: one sentence, twice, a hand apart.
    if !popup_open && let Some((pane_rect, 0)) = tab.starved_pane() {
        feed_notice::draw_empty_pane_note(ui.painter(), pane_rect, report);
    }
    if popup_open {
        // A click anywhere else puts it away, measured against the
        // rectangles that were actually drawn — so a click on the edge of
        // what the trader can see is never read as a click outside it, and
        // the popup is laid out once rather than measured again to ask.
        let popup;
        (answers.notice_action, popup) = feed_notice::draw_popup(ui, area, chip, report);
        answers.dismissed = ui.input(|input| {
            input.pointer.any_click()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
        });
    }
}
