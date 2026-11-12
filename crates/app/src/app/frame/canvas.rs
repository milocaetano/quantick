//! The canvas stage's overlays and what the canvas answers the frame tail.
//!
//! The chart itself is drawn by the active tab through its own
//! [`CanvasChrome`](crate::tab::CanvasChrome) borrow; what lives here reads
//! that tab after it drew: the one overlay over the panes with no bars, and the
//! recovery popup the status word opens.

use eframe::egui;
use quantick_control_handlers::contract_roll::RollRefusal;
use quantick_feed::notice_report::RollNote;
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
    /// The roll the open popup offers off a contract the venue no longer
    /// lists, or why it cannot roll; read before the canvas borrows the tabs.
    pub roll: Option<Result<String, RollRefusal>>,
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
        app: &crate::app::QuantickApp,
        popup_tab: u64,
        status: &crate::statusbar::StatusResponse,
    ) -> Self {
        let popup_open = app.chrome.feed_popup_tab == Some(popup_tab);
        let port = app as &dyn crate::app::control_host::ControlPort;
        let index = app.runtime.tabs.active_index();
        Self {
            popup_tab,
            popup_open,
            roll: popup_open
                .then(|| quantick_control_handlers::contract_roll::notice_roll(port, index))
                .flatten(),
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

/// What the overlay calls the source: one with no transport is not the feed the
/// tab was opened on.
fn source_name<'a>(tab: &'a Tab, config: &'a crate::config::AppConfig) -> &'a str {
    if tab.capabilities(config).transport {
        tab.feed_display_name(config)
    } else {
        "recording"
    }
}

/// Measure whether the overlay's control fits before the status bar draws, so
/// exactly one of them offers Reconnect. On last frame's rects: a resize can
/// leave neither for its one frame, never both, since the overlay checks again.
pub(super) fn settle_overlay_control(
    ctx: &egui::Context,
    tab: &mut Tab,
    config: &crate::config::AppConfig,
    now: i64,
) {
    let fits = tab
        .connection
        .overlay(source_name(tab, config), now)
        .zip(tab.empty_panes_rect())
        .is_some_and(|(words, rect)| loading::reconnect_fits(ctx, rect, &tab.symbol, &words));
    tab.connection.set_overlay_control_fits(fits);
}

/// The overlay over whatever panes have no bars, then the popup if it is open,
/// which speaks in the overlay's place so no headline appears twice. The panes
/// published their rects on the draw just before this, so these are this frame's
/// geometry rather than the previous one's.
pub(super) fn overlays(
    ui: &mut egui::Ui,
    area: egui::Rect,
    tab: &Tab,
    config: &crate::config::AppConfig,
    stall: Option<&Stall>,
    now: i64,
    answers: &mut CanvasAnswers,
) {
    let feed_name = source_name(tab, config);
    if !answers.popup_open
        && let Some(words) = tab.connection.overlay(feed_name, now)
        && let Some(rect) = tab.empty_panes_rect()
    {
        let clock = (now, answers.popup_tab);
        let owns = tab.connection.overlay_owns_canvas();
        let palette = owns.then(|| tab.lead_pane().palette(ui.ctx()));
        let _palette = palette.map(|palette| crate::theme::scope(ui.ctx(), palette));
        let (_, pressed) = loading::draw_overlay(ui, rect, &tab.symbol, &words, clock);
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
    let report = match feed_notice::report(&tab.link.notice, stall)
        .filter(feed_notice::Report::is_offline)
    {
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
                roll: None,
            }
        }
    };
    let roll = answers.roll.clone();
    let report = report.with_roll(roll.as_ref().map(|roll| match roll {
        Ok(target) => RollNote::To(target),
        Err(refusal) => RollNote::Refused(refusal.reason()),
    }));
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
