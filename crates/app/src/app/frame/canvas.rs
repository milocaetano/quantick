//! The canvas stage's overlays and what the canvas answers the frame tail.
//!
//! The chart itself is drawn by the active tab through its own
//! [`CanvasChrome`](crate::tab::CanvasChrome) borrow; what lives here reads
//! that tab after it drew: the one overlay over the panes with no bars, and the
//! recovery popup the status word opens.

use eframe::egui;
use quantick_feed::stall::Stall;

use crate::feed_notice;
use crate::loading;
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
    /// Where the status word landed while it is the popup's door. A feed that
    /// recovered while the popup was open closes it, rather than leaving a
    /// stale explanation over a chart that is fine again.
    pub chip_rect: Option<egui::Rect>,
    /// Raised by a placement that wants its note typed, and handed to the
    /// drawing chrome: the flag belongs to the editor that owns the caret,
    /// not to the canvas that asks for it.
    pub begin_text_edit: bool,
}

impl CanvasAnswers {
    /// What the frame already knows before the canvas draws: which tab's popup
    /// this is, and what the status bar was just asked.
    pub(in crate::app) fn seeded(
        popup_tab: u64,
        open_popup: Option<u64>,
        status: &crate::statusbar::StatusResponse,
    ) -> Self {
        Self {
            popup_tab,
            popup_open: open_popup == Some(popup_tab),
            chip_clicked: status.word_clicked,
            chip_rect: status.word_rect,
            notice_action: if status.reconnect {
                feed_notice::NoticeAction::Reconnect
            } else {
                feed_notice::NoticeAction::None
            },
            ..Self::default()
        }
    }
}

/// The overlay over whatever panes have no bars, then the popup if it is open.
/// The panes published their rects on the draw just before this, so these are
/// this frame's geometry rather than the previous one's.
pub(super) fn overlays(
    ui: &mut egui::Ui,
    area: egui::Rect,
    tab: &Tab,
    config: &crate::config::AppConfig,
    stall: Option<&Stall>,
    answers: &mut CanvasAnswers,
) {
    let now = crate::metrics::wall_clock_ms();
    let feed_name = tab.feed_display_name(config);
    if let Some(words) = tab.connection.overlay(feed_name, now)
        && let Some(rect) = tab.empty_panes_rect()
    {
        let (_, pressed) = loading::draw_overlay(ui, rect, &tab.symbol, &words, now);
        if pressed {
            answers.notice_action = feed_notice::NoticeAction::Reconnect;
        }
    }
    if let Some(chip) = answers.chip_rect
        && answers.popup_open
    {
        draw_recovery_popup(ui, area, chip, tab, feed_name, stall, answers);
    }
}

fn draw_recovery_popup(
    ui: &mut egui::Ui,
    area: egui::Rect,
    chip: egui::Rect,
    tab: &Tab,
    feed_name: &str,
    stall: Option<&Stall>,
    answers: &mut CanvasAnswers,
) {
    let held;
    let report =
        match feed_notice::report(&tab.notice, stall).filter(feed_notice::Report::is_offline) {
            Some(report) => report,
            // Latched offline while the feed's own loop has gone back to "working":
            // the reason it gave is gone, the door still opens.
            None => {
                held = format!("{feed_name} has not come back");
                feed_notice::Report {
                    headline: &held,
                    next_step: Some(feed_notice::HELD_NEXT_STEP),
                    needs_user: true,
                    severity: feed_notice::Severity::Offline,
                    primary: quantick_feed::stall::Recovery::Reconnect,
                }
            }
        };
    // A click anywhere else puts it away, measured against the rectangles that
    // were actually drawn — so a click on the edge of what the trader can see
    // is never read as a click outside it, and the popup is laid out once
    // rather than measured again to ask.
    let (action, popup) = feed_notice::draw_popup(ui, area, chip, &report);
    if action != feed_notice::NoticeAction::None {
        answers.notice_action = action;
    }
    answers.dismissed = ui.input(|input| {
        input.pointer.any_click()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !popup.contains(at) && !chip.contains(at))
    });
}
