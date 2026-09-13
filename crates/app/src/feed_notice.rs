//! What the interface says about the feed, and where it says it
//! (`docs/ux/ui-design-model.md` §8 — provenance, told at the size the news
//! deserves).
//!
//! An empty chart is ambiguous. It looks the same whether the market is quiet,
//! MetaTrader is closed, the Python package was never installed, or the
//! contract does not exist for this account. So there has to be somewhere the
//! reason is written down, and something the trader can press once they have
//! read it.
//!
//! Until now that somewhere was a 420 px card with two buttons, drawn across
//! whichever pane was empty, and it was drawn *because the application decided
//! the trader needed to see it*. That is right for a terminal that froze in
//! the middle of a session. It is wrong every morning: the chart a trader
//! opens before the open is a chart with nothing wrong with it, and greeting
//! them with an error panel over the whole canvas taught them to close the
//! application rather than to read it. Their words, and the rule this module
//! now keeps: **seeing no data at all is worse than not being connected.**
//!
//! So the news comes at three sizes, and the trader chooses which one they
//! want:
//!
//! - **The chip** ([`draw_chip`]) — a dot and one word in the chart's
//!   bottom-right corner, the resting report for every feed problem there is.
//!   It never covers a bar, it is in the same place every time, and it is the
//!   only one of the three drawn without being asked for.
//! - **The popup** ([`draw_popup`]) — the headline, the next step and the two
//!   recovery controls, opened by clicking the chip and by nothing else.
//! - **One line on an empty pane** ([`draw_empty_pane_note`]) — because a
//!   chart with no bars on it has room to say why, and a corner chip alone on
//!   a blank canvas is a puzzle rather than an answer.
//!
//! The two controls are [`Recovery::Reconnect`] and [`Recovery::Reload`], and
//! which of them is offered *first* is decided upstream from what the
//! application already knows (see [`quantick_feed::stall`]). The trader is never
//! asked to diagnose their own feed to press the right button.

use eframe::egui;

use crate::theme;
use quantick_feed::FeedNotice;
use quantick_feed::stall::{Recovery, Stall};

/// The word the chip carries when the chart is not being fed.
///
/// One word for four different faults, and deliberately: it answers the
/// question the trader actually has — *is what I am looking at live?* — and
/// leaves *why* to the popup one click away. Calling a connected socket over a
/// closed market "offline" is a true statement about the thing being reported
/// on, which is the data, not the transport.
pub const OFFLINE_LABEL: &str = "offline";

/// Height of the chip, in pixels. A pill inside the canvas' bottom margin.
pub(crate) const CHIP_HEIGHT_PX: f32 = 22.0;
/// Padding inside the chip, left and right, in pixels.
pub(crate) const CHIP_PAD_PX: f32 = 9.0;
/// Diameter of the chip's state dot, in pixels.
pub(crate) const CHIP_DOT_DIAMETER_PX: f32 = 8.0;
/// Gap between the chip's dot and its word, in pixels.
pub(crate) const CHIP_DOT_GAP_PX: f32 = 7.0;
/// The chip's font size, in points.
pub(crate) const CHIP_LABEL_PT: f32 = 11.0;
/// How far the chip sits from the chart's bottom-right corner, in pixels.
///
/// Far enough not to touch the axis furniture, close enough that it reads as
/// belonging to the corner rather than floating in the chart.
pub(crate) const CHIP_MARGIN_PX: f32 = 10.0;

/// Width of the popup, in pixels.
///
/// Narrower than the card it replaces (420 px). The card had to carry the news
/// to someone who had not gone looking for it; the popup is opened on purpose,
/// by someone already reading the corner it grew out of.
const POPUP_WIDTH_PX: f32 = 340.0;
/// Floor for the popup's text column, in pixels. A chart narrow enough to
/// drive this negative would otherwise wrap every word onto its own line, or
/// panic the layout; the popup is unreadable at that size either way, and this
/// keeps it merely cramped.
const MIN_TEXT_WIDTH_PX: f32 = 40.0;
/// Padding inside the popup, in pixels.
const PAD_PX: f32 = 14.0;
/// Gap between the popup and the chip it opened from, in pixels.
const POPUP_GAP_PX: f32 = 8.0;
/// Gap between the headline and the next step, in pixels.
const LINE_GAP_PX: f32 = 6.0;
/// Diameter of the popup's leading state dot, in pixels.
const DOT_DIAMETER_PX: f32 = 8.0;
/// Gap between that dot and the headline, in pixels.
const DOT_GAP_PX: f32 = 8.0;
/// Headline font size, in points.
const HEADLINE_PT: f32 = 14.0;
/// Next-step font size, in points.
const STEP_PT: f32 = 12.0;
/// Consequence-caption font size, in points. Smaller than the next step: it is
/// there to be read before pressing, not to compete with the instruction.
const CAPTION_PT: f32 = 11.0;
/// Corner radius of the popup, in pixels.
const CORNER_RADIUS_PX: f32 = 6.0;
/// Preferred size of one recovery button, in pixels.
const BUTTON_SIZE_PX: egui::Vec2 = egui::vec2(108.0, 24.0);
/// Floor for a button's width, in pixels, on a chart too narrow for two of
/// them at full size. Below this the label is unreadable anyway; what this
/// protects is the layout, which must never compute a negative width.
const MIN_BUTTON_WIDTH_PX: f32 = 44.0;
/// Gap between the two recovery buttons, in pixels.
const BUTTON_SPACING_PX: f32 = 8.0;
/// Gap between the text above and the button row, in pixels.
const BUTTON_GAP_PX: f32 = 10.0;
/// Gap between the button row and the consequence caption, in pixels.
const CAPTION_GAP_PX: f32 = 6.0;

/// Font size of the line drawn on a pane with nothing else on it, in points.
const EMPTY_NOTE_PT: f32 = 13.0;
/// How far above an empty pane's vertical centre that line sits, as a fraction
/// of the pane's height. Slightly high reads as a note about the chart rather
/// than as content in it.
const EMPTY_NOTE_BIAS: f32 = 0.08;

/// What [`Recovery::Reload`] costs, said on the popup rather than behind a
/// confirmation dialog.
///
/// A modal between a trader and a stalled chart is one more thing in the way at
/// the worst moment, and this act is recoverable in the sense that matters —
/// the position it closes is journaled, with its reason, by
/// `PaperTrading::on_timeline_reset`. What is not acceptable is the trader
/// learning about it afterwards, which is what happened while the only button
/// on offer said "Try again" and quietly reset the timeline.
const RELOAD_CAPTION: &str = "Reload rebuilds the chart from scratch: it closes any open paper position and \
     disarms every strategy.";

/// What each control costs, in the trader's terms, on its own hover.
///
/// The caption below the buttons names what the destructive one ends, and for
/// a while that was the only sentence either button had. A warning beside one
/// control and silence beside the other does not read as "that one is safe" —
/// it reads as "one of these is dangerous and I cannot tell which", which
/// pushes a trader toward pressing neither at the moment they most need to
/// press one. This is the status line's old hover text, restored to the
/// surface that inherited its job.
#[must_use]
fn recovery_cost(recovery: Recovery) -> &'static str {
    match recovery {
        Recovery::Reconnect => {
            "Keeps the chart as it is: bars, drawings, strategies and any open paper position."
        }
        Recovery::Reload => {
            "Rebuilds the chart from scratch: closes any open paper position and disarms every strategy."
        }
    }
}

/// What the person watching asked of the popup this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeAction {
    /// Nothing was clicked.
    #[default]
    None,
    /// Respawn the transport and keep the timeline.
    Reconnect,
    /// Throw the timeline away and rebuild it.
    Reload,
}

impl From<Recovery> for NoticeAction {
    fn from(recovery: Recovery) -> Self {
        match recovery {
            Recovery::Reconnect => Self::Reconnect,
            Recovery::Reload => Self::Reload,
        }
    }
}

/// How much of the interface a report is entitled to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Something is under way and nothing is wrong yet: a first connection, a
    /// history block still arriving.
    ///
    /// Says nothing in the corner. The wait already has a surface of its own —
    /// `loading`'s overlay, on the pane that is waiting — and a second badge
    /// beside it would be the interface talking about itself twice.
    Progress,
    /// The chart is not being fed, whatever the socket believes.
    ///
    /// The one state the chip exists for. Four faults reach it — a transport
    /// that never landed, one that dropped, one that stayed open while the
    /// thing behind it went quiet, and a reason the provider named itself —
    /// and the trader is told them apart in the popup, not in the corner.
    Offline,
}

/// What the interface has to tell the trader about one feed this frame —
/// however it was arrived at.
///
/// Both sources of words end up here: the feed's own [`FeedNotice`], and the
/// [`Stall`] this application infers when a notice stops being progress. The
/// chip and the popup draw a `Report` and know nothing about which one
/// produced it, so there is one set of words, one severity and one pair of
/// buttons rather than a second surface for the second source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report<'a> {
    /// One line: what is happening, or what was observed.
    pub headline: &'a str,
    /// The single next step, when there is one to give.
    pub next_step: Option<&'a str>,
    /// Red rather than amber: this needs a person, it is not just quiet.
    pub needs_user: bool,
    /// Whether this is worth a corner at all.
    pub severity: Severity,
    /// The control offered first. The other is always beside it.
    pub primary: Recovery,
}

impl Report<'_> {
    /// The colour carrying this report's severity.
    ///
    /// [`theme::WARN`] for a transport that is broken however you read it,
    /// [`theme::AMBER`] for a tape that has merely gone quiet — which is also
    /// what a closed market looks like, and a red dot every night on a chart
    /// with nothing wrong with it is how a warning stops being read.
    #[must_use]
    pub fn accent(&self) -> egui::Color32 {
        if self.needs_user {
            theme::WARN
        } else {
            theme::AMBER
        }
    }

    /// Whether the corner should carry this report.
    #[must_use]
    pub fn is_offline(&self) -> bool {
        matches!(self.severity, Severity::Offline)
    }
}

/// Build the report for a feed, or `None` when there is nothing to say.
///
/// `stall` is this application's own judgement, already decided against the
/// clock by [`quantick_feed::stall::assess`]; it wins over a progress notice,
/// because a progress notice that has been escalated is exactly the one whose
/// words stopped being true.
#[must_use]
pub fn report<'a>(notice: &'a FeedNotice, stall: Option<&'a Stall>) -> Option<Report<'a>> {
    // The provider named a specific reason. Nothing inferred improves on it,
    // and `assess` already stands down in this case.
    if let FeedNotice::Attention {
        headline,
        next_step,
    } = notice
    {
        return Some(Report {
            headline,
            next_step: Some(next_step),
            needs_user: true,
            severity: Severity::Offline,
            primary: Recovery::Reconnect,
        });
    }
    if let Some(stall) = stall {
        return Some(Report {
            headline: &stall.headline,
            next_step: Some(&stall.next_step),
            needs_user: stall.needs_attention,
            severity: Severity::Offline,
            primary: stall.primary,
        });
    }
    match notice {
        FeedNotice::Connected | FeedNotice::Clear | FeedNotice::Attention { .. } => None,
        FeedNotice::Reconnecting { headline } | FeedNotice::Working { headline } => Some(Report {
            headline,
            next_step: None,
            needs_user: false,
            severity: Severity::Progress,
            primary: Recovery::Reconnect,
        }),
    }
}

/// Whether `QUANTICK_FEED_POPUP` asks the chart to open the popup.
///
/// The popup opens on a click and on nothing else, which is exactly the rule
/// a scripted run cannot follow: a capture has no pointer. So the hook stands
/// in for the click and for nothing else — what it opens is the popup a click
/// opens, drawn from the same report, closing the same ways.
///
/// Read once, at startup, so a run that goes on to click the chip is not
/// fighting an environment variable. It shows nothing on its own: the popup
/// draws only while there is an offline report to draw, so a capture pairs
/// this with `QUANTICK_FEED_STALL` — which is honest rather than awkward, a
/// popup about a healthy feed being a thing the application must never show.
#[must_use]
pub fn popup_open_from_env() -> bool {
    popup_open_from(std::env::var("QUANTICK_FEED_POPUP").ok().as_deref())
}

/// The rule the hook applies, separated from the reading of it.
///
/// Split so the rule can be tested without setting the real variable. A test
/// that set `QUANTICK_FEED_POPUP` would be setting it for every other test in
/// the process — `cargo test` runs them as threads — and the neighbour that
/// builds a `QuantickApp` reads exactly this variable in its constructor. The
/// failure would land on that neighbour, on a green branch, at random.
#[must_use]
fn popup_open_from(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Whether the recovery popup is still up after a frame.
///
/// The chip is its only door, in both directions — the rule the trader asked
/// for, after a card that opened itself over the chart every morning.
/// Everything else here closes it: a click that landed somewhere else, a
/// control that was pressed and is now under way, and a feed that recovered on
/// its own while the trader was reading about it.
///
/// Pure, because the rule is worth a table rather than four frames of
/// clicking, and because the frame that applies it has three other jobs on the
/// same line.
///
/// `offline` is checked before anything else, and deliberately: no chip, no
/// popup, in every combination. Today the frame cannot report a click on a
/// chip it did not draw, so the case is unreachable — which is exactly why it
/// is worth pinning here rather than leaving to hold by luck. A popup carries
/// a `Report`; with no report there is nothing to draw in it.
#[must_use]
pub fn popup_still_open(
    was_open: bool,
    chip_clicked: bool,
    offline: bool,
    dismissed: bool,
    action: NoticeAction,
) -> bool {
    if !offline {
        return false;
    }
    if chip_clicked {
        return !was_open;
    }
    was_open && !dismissed && action == NoticeAction::None
}

/// Where the chip lands inside `area`.
///
/// Measured from the word it carries rather than fixed, so a future state word
/// cannot silently overflow the pill.
///
/// **Call this once a frame and pass the answer down.** Laying out even a
/// seven-character static string allocates it, and the corner is drawn on
/// every frame of a chart that is not being fed — which, since the opening
/// backfill learned to reach the last session, is the ordinary state of a
/// chart opened outside market hours and can be hours at a time. Passing the
/// rectangle rather than re-deriving it also makes the drawing, the hit test
/// and the popup's anchor share one measurement by construction rather than
/// by three callers agreeing.
#[must_use]
pub fn chip_rect(painter: &egui::Painter, area: egui::Rect) -> egui::Rect {
    let label = painter.layout_no_wrap(
        OFFLINE_LABEL.to_owned(),
        egui::FontId::proportional(CHIP_LABEL_PT),
        theme::TEXT_PRIMARY,
    );
    let width = 2.0 * CHIP_PAD_PX + CHIP_DOT_DIAMETER_PX + CHIP_DOT_GAP_PX + label.size().x.ceil();
    // Clamped to the chart, so a pane narrower than the chip keeps it inside
    // rather than hanging it off the left edge.
    let width = width.min(area.width());
    egui::Rect::from_min_size(
        egui::pos2(
            area.right() - CHIP_MARGIN_PX - width,
            area.bottom() - CHIP_MARGIN_PX - CHIP_HEIGHT_PX,
        ),
        egui::vec2(width, CHIP_HEIGHT_PX),
    )
    // A chart shorter than the margins keeps the chip on screen rather than
    // above the canvas' own top edge.
    .intersect(area)
}

/// Draw the chip in the bottom-right corner of `area`. Answers whether it was
/// clicked this frame.
///
/// `open` is whether the popup it opens is showing, which the chip carries as
/// a pressed look — the trader must be able to tell the click landed even
/// though the thing it opened is above their pointer rather than under it.
///
/// The reason rides on the hover. The chip is one word by design, and one word
/// answers "am I live?" and nothing else; a trader mid-session who wants *why*
/// should not have to spend a click and a dismissal to read it. `on_hover_ui`
/// rather than `on_hover_text`, for the reason the status line gives: the
/// closure runs only while someone is pointing at it.
#[must_use]
pub fn draw_chip(ui: &mut egui::Ui, rect: egui::Rect, report: &Report<'_>, open: bool) -> bool {
    let response = ui
        .interact(
            rect,
            ui.id().with("feed_offline_chip"),
            egui::Sense::click(),
        )
        .on_hover_ui(|ui| {
            ui.label(report.headline);
            if let Some(step) = report.next_step {
                ui.label(egui::RichText::new(step).color(theme::TEXT_MUTED));
            }
        });
    let accent = report.accent();
    let hovered = response.hovered();
    let painter = ui.painter();

    let fill = if open || hovered {
        theme::CONTROL
    } else {
        theme::CHROME
    };
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, egui::Rounding::same(radius), fill);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(radius),
        egui::Stroke::new(1.0_f32, accent),
    );
    painter.circle_filled(
        egui::pos2(
            rect.left() + CHIP_PAD_PX + CHIP_DOT_DIAMETER_PX / 2.0,
            rect.center().y,
        ),
        CHIP_DOT_DIAMETER_PX / 2.0,
        accent,
    );
    painter.text(
        egui::pos2(
            rect.left() + CHIP_PAD_PX + CHIP_DOT_DIAMETER_PX + CHIP_DOT_GAP_PX,
            rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        OFFLINE_LABEL,
        egui::FontId::proportional(CHIP_LABEL_PT),
        theme::TEXT_PRIMARY,
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.clicked()
}

/// One muted line on a pane with nothing else on it.
///
/// The chip answers "am I live?" in a corner, which is enough on a chart full
/// of bars and not enough on a blank one: there the trader has come to read
/// something and there is nothing to read. This is the headline and no more —
/// no border, no fill, no buttons. The way out is still the corner.
pub fn draw_empty_pane_note(painter: &egui::Painter, pane: egui::Rect, report: &Report<'_>) {
    let galley = painter.layout(
        report.headline.to_owned(),
        egui::FontId::proportional(EMPTY_NOTE_PT),
        theme::TEXT_MUTED,
        (pane.width() - 2.0 * PAD_PX).max(MIN_TEXT_WIDTH_PX),
    );
    let size = galley.size();
    painter.galley(
        egui::pos2(
            pane.center().x - size.x / 2.0,
            pane.center().y - pane.height() * EMPTY_NOTE_BIAS - size.y / 2.0,
        ),
        galley,
        theme::TEXT_MUTED,
    );
}

/// Where every piece of the popup lands, measured once so drawing and hit
/// testing can never disagree about it.
struct Geometry {
    /// The popup's outline.
    popup: egui::Rect,
    /// Colour carrying the severity: the dot, the border.
    accent: egui::Color32,
    /// The headline, already wrapped.
    headline: std::sync::Arc<egui::Galley>,
    /// The next step, already wrapped; absent when there is none.
    step: Option<std::sync::Arc<egui::Galley>>,
    /// The consequence caption, already wrapped.
    caption: std::sync::Arc<egui::Galley>,
    /// The button offered first, and what pressing it means.
    primary: (egui::Rect, Recovery),
    /// The other one.
    secondary: (egui::Rect, Recovery),
}

/// Measure the popup for `report` inside `area`.
///
/// Text height depends on the fonts, so this needs a painter — but it takes no
/// other state, and both the drawing and the tests go through it.
fn geometry(
    painter: &egui::Painter,
    area: egui::Rect,
    chip: egui::Rect,
    report: &Report<'_>,
) -> Geometry {
    let accent = report.accent();

    // Wrap against the width the popup will actually have, not the width it
    // would like: on a narrow chart it is clamped, and text wrapped to the
    // unclamped width spills past its own border.
    let width = POPUP_WIDTH_PX.min(area.width());
    let text_width = (width - 2.0 * PAD_PX - DOT_DIAMETER_PX - DOT_GAP_PX).max(MIN_TEXT_WIDTH_PX);
    let wrap = |text: &str, size: f32, color: egui::Color32| {
        painter.layout(
            text.to_owned(),
            egui::FontId::proportional(size),
            color,
            text_width,
        )
    };
    let headline = wrap(report.headline, HEADLINE_PT, theme::TEXT_PRIMARY);
    let step = report
        .next_step
        .map(|step| wrap(step, STEP_PT, theme::TEXT_MUTED));
    let caption = wrap(RELOAD_CAPTION, CAPTION_PT, theme::TEXT_FAINT);

    // Two buttons share the text column. On a chart too narrow for both at
    // their preferred width they shrink together rather than one of them
    // leaving the popup.
    //
    // The floor yields to the column rather than overriding it. Clamped up to
    // 44 px unconditionally, each button kept its width while the column
    // shrank, and on a pane under ~140 px the second one was drawn past the
    // right edge, over the chart with no chrome behind it. Below that width
    // the labels are unreadable either way, and a control whose edges the
    // trader cannot see is worse than a cramped one.
    let half_column = ((text_width - BUTTON_SPACING_PX) / 2.0).max(1.0);
    let button_width = if half_column < MIN_BUTTON_WIDTH_PX {
        half_column
    } else {
        half_column.min(BUTTON_SIZE_PX.x)
    };
    let button_size = egui::vec2(button_width, BUTTON_SIZE_PX.y);

    let mut height = 2.0 * PAD_PX + headline.size().y;
    if let Some(step) = &step {
        height += LINE_GAP_PX + step.size().y;
    }
    height += BUTTON_GAP_PX + button_size.y + CAPTION_GAP_PX + caption.size().y;

    // Anchored to the chip, not to the middle of the chart: the popup grows
    // out of the thing that was clicked, so the eye does not have to travel to
    // find the answer it just asked for. Clamped into the chart, because a
    // popup taller than the pane it explains would otherwise start above it.
    let bottom = (chip.top() - POPUP_GAP_PX).max(area.top() + height);
    let popup = egui::Rect::from_min_size(
        egui::pos2(
            (chip.right() - width).max(area.left()),
            (bottom - height).max(area.top()),
        ),
        egui::vec2(width, height),
    );

    let text_left = popup.left() + PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX;
    let mut cursor = popup.top() + PAD_PX + headline.size().y;
    if let Some(step) = &step {
        cursor += LINE_GAP_PX + step.size().y;
    }
    let button_top = cursor + BUTTON_GAP_PX;
    let primary = egui::Rect::from_min_size(egui::pos2(text_left, button_top), button_size);
    let secondary = egui::Rect::from_min_size(
        egui::pos2(primary.right() + BUTTON_SPACING_PX, button_top),
        button_size,
    );

    Geometry {
        popup,
        accent,
        headline,
        step,
        caption,
        primary: (primary, report.primary),
        secondary: (secondary, report.primary.other()),
    }
}

/// Where the popup lands for `report` inside `area` — the outline only.
///
/// Test-only, like the `card_rect` before it. Production has no reason to ask:
/// [`draw_popup`] hands back the rectangle it already measured, so the frame
/// can tell a click *outside* the popup from one inside it without laying the
/// whole thing out a second time. What the tests want is the placement rule
/// measured rather than eyeballed, and that is worth a function the shipped
/// binary does not carry.
#[cfg(test)]
#[must_use]
pub fn popup_rect(
    painter: &egui::Painter,
    area: egui::Rect,
    chip: egui::Rect,
    report: &Report<'_>,
) -> egui::Rect {
    geometry(painter, area, chip, report).popup
}

/// Draw the popup above the chip in `area`. Call only while it is open.
///
/// Answers what was pressed *and where the popup landed*, because the caller
/// needs the rectangle to tell a click on the popup from a click that
/// dismisses it — and measuring it a second time would re-wrap three strings
/// on a path that runs every frame the chart is offline.
#[must_use]
pub fn draw_popup(
    ui: &mut egui::Ui,
    area: egui::Rect,
    chip: egui::Rect,
    report: &Report<'_>,
) -> (NoticeAction, egui::Rect) {
    let painter = ui.painter().clone();
    let Geometry {
        popup,
        accent,
        headline,
        step,
        caption,
        primary,
        secondary,
    } = geometry(&painter, area, chip, report);

    // The body claims the pointer before anything is drawn into it. A painted
    // rectangle registers no widget, so without this a click on the headline
    // — or a drag across the caption — reaches the chart underneath and pans
    // it, or drops a drawing anchor on it. The card this replaced could
    // mostly ignore that by only ever covering a pane with no bars; the
    // corner's whole point is to work on a chart that is full.
    //
    // Registered before the buttons so they win inside their own rectangles,
    // and after the canvas so this wins over it.
    let _ = ui.interact(
        popup,
        ui.id().with("feed_recovery_popup"),
        egui::Sense::click_and_drag(),
    );

    // Chrome, not canvas: this belongs to the interface reporting on the chart
    // rather than to the chart. Opaque, because an instruction someone has to
    // follow should not be read through candles; the accent carries the
    // severity.
    painter.rect_filled(popup, egui::Rounding::same(CORNER_RADIUS_PX), theme::CHROME);
    painter.rect_stroke(
        popup,
        egui::Rounding::same(CORNER_RADIUS_PX),
        egui::Stroke::new(1.0_f32, accent),
    );

    let text_left = popup.left() + PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX;
    let headline_top = popup.top() + PAD_PX;
    painter.circle_filled(
        egui::pos2(
            popup.left() + PAD_PX + DOT_DIAMETER_PX / 2.0,
            headline_top + HEADLINE_PT / 2.0,
        ),
        DOT_DIAMETER_PX / 2.0,
        accent,
    );
    let headline_height = headline.size().y;
    painter.galley(
        egui::pos2(text_left, headline_top),
        headline,
        theme::TEXT_PRIMARY,
    );
    if let Some(step) = step {
        painter.galley(
            egui::pos2(text_left, headline_top + headline_height + LINE_GAP_PX),
            step,
            theme::TEXT_MUTED,
        );
    }

    // The primary is filled and the secondary is not, so the button that fixes
    // *this* stall is the one the eye lands on. Both are always present: the
    // application's guess about which act is needed must never be the reason a
    // trader cannot reach the other one.
    let mut action = NoticeAction::None;
    let (primary_rect, primary_recovery) = primary;
    if ui
        .put(
            primary_rect,
            egui::Button::new(egui::RichText::new(primary_recovery.label()).color(theme::CHIP_INK))
                .fill(accent),
        )
        .on_hover_text(recovery_cost(primary_recovery))
        .clicked()
    {
        action = primary_recovery.into();
    }
    let (secondary_rect, secondary_recovery) = secondary;
    if ui
        .put(
            secondary_rect,
            egui::Button::new(secondary_recovery.label()).fill(theme::CONTROL),
        )
        .on_hover_text(recovery_cost(secondary_recovery))
        .clicked()
    {
        action = secondary_recovery.into();
    }
    painter.galley(
        egui::pos2(text_left, primary_rect.bottom() + CAPTION_GAP_PX),
        caption,
        theme::TEXT_FAINT,
    );
    (action, popup)
}

crate::hooks::declare_hooks!["QUANTICK_FEED_POPUP"];

#[cfg(test)]
mod tests {
    use super::*;

    /// The report for a bare notice, with nothing inferred on top.
    fn plain(notice: &FeedNotice) -> Option<Report<'_>> {
        report(notice, None)
    }

    /// A feed with nothing of its own to say, so the stall is the whole story.
    const CLEAR: FeedNotice = FeedNotice::Clear;

    /// The report an inferred stall produces on such a feed.
    fn from_stall(stall: &Stall) -> Report<'_> {
        report(&CLEAR, Some(stall)).expect("a stall speaks")
    }

    /// The stall the tests below escalate into.
    fn silent_stall() -> Stall {
        Stall {
            headline: "MetaTrader 5 — B3 has sent nothing for 4 min".to_owned(),
            next_step: "If the market is open, check the terminal.".to_owned(),
            primary: Recovery::Reload,
            needs_attention: false,
        }
    }

    /// Draw the chip the way the frame does: one measurement, handed down.
    fn chip_at(ui: &mut egui::Ui, area: egui::Rect, report: &Report<'_>, open: bool) -> bool {
        let rect = chip_rect(ui.painter(), area);
        draw_chip(ui, rect, report, open)
    }

    /// The popup, anchored on that same measurement. Answers what was
    /// pressed; the rectangle it also returns is the caller's business in
    /// production and nobody's here.
    fn popup_at(ui: &mut egui::Ui, area: egui::Rect, report: &Report<'_>) -> NoticeAction {
        let rect = chip_rect(ui.painter(), area);
        draw_popup(ui, area, rect, report).0
    }

    /// The popup's geometry inside `area`, with the chip it grows out of.
    fn geometry_in(painter: &egui::Painter, area: egui::Rect, report: &Report<'_>) -> Geometry {
        geometry(painter, area, chip_rect(painter, area), report)
    }

    /// A painter over a headless context, for the geometry tests.
    fn with_painter<R>(f: impl FnOnce(&egui::Painter) -> R) -> R {
        let ctx = egui::Context::default();
        // `run` takes an `FnMut`, and the body must run exactly once: the
        // measurement is what is being tested, so a second call would either
        // move a consumed closure or quietly overwrite the answer.
        let mut once = Some(f);
        let mut out = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            if let Some(f) = once.take() {
                out = Some(f(&painter));
            }
        });
        out.expect("the frame ran")
    }

    #[test]
    fn a_clear_notice_says_nothing() {
        assert!(plain(&FeedNotice::Clear).is_none());
        assert!(plain(&FeedNotice::Connected).is_none());
    }

    #[test]
    fn progress_never_reaches_the_corner() {
        let working = FeedNotice::working("starting the MetaTrader bridge");
        let report = plain(&working).expect("progress is worth saying");
        assert_eq!(
            report.severity,
            Severity::Progress,
            "a first connection is not an outage"
        );
        assert!(
            !report.is_offline(),
            "the loading overlay owns a wait; the corner would say it twice"
        );

        let reconnecting = FeedNotice::reconnecting("Binance disconnected — reconnecting");
        let report = plain(&reconnecting).expect("a reconnect is worth saying");
        assert!(!report.is_offline());
    }

    #[test]
    fn every_fault_lands_in_the_corner() {
        let attention = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        let named = plain(&attention).expect("a named reason is always worth saying");
        assert!(
            named.is_offline(),
            "the reason the provider named is an outage like any other"
        );
        assert!(named.needs_user, "and it needs a person");

        let silent = silent_stall();
        let inferred = from_stall(&silent);
        assert!(
            inferred.is_offline(),
            "so is a tape that stopped without anyone saying why"
        );
    }

    #[test]
    fn a_quiet_tape_is_amber_and_a_broken_one_is_red() {
        let silent = silent_stall();
        let quiet = from_stall(&silent);
        assert_eq!(
            quiet.accent(),
            theme::AMBER,
            "a closed market wears no alarm"
        );

        let attention = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        let broken = plain(&attention).expect("a named reason speaks");
        assert_eq!(broken.accent(), theme::WARN);
    }

    #[test]
    fn the_chip_sits_in_the_bottom_right_corner_and_covers_nothing_else() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 700.0));
        let chip = with_painter(|painter| chip_rect(painter, area));

        assert!(area.contains_rect(chip), "the chip stays inside the chart");
        assert!(
            (area.right() - chip.right() - CHIP_MARGIN_PX).abs() < 0.5,
            "pinned to the right edge: {chip:?}"
        );
        assert!(
            (area.bottom() - chip.bottom() - CHIP_MARGIN_PX).abs() < 0.5,
            "pinned to the bottom edge: {chip:?}"
        );
        // The rule the trader asked for, measured: what it covers is a corner,
        // not a chart. Anything approaching the old card's 420 px would mean
        // the demotion had quietly been undone.
        assert!(
            chip.width() < 100.0 && chip.height() <= CHIP_HEIGHT_PX,
            "the chip is a chip: {chip:?}"
        );
    }

    #[test]
    fn a_chart_narrower_than_the_chip_still_holds_it() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(40.0, 30.0));
        let chip = with_painter(|painter| chip_rect(painter, area));
        assert!(
            area.contains_rect(chip),
            "a cramped chip beats one hanging off the canvas: {chip:?}"
        );
    }

    #[test]
    fn the_popup_grows_out_of_the_chip() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1200.0, 700.0));
        let stall = silent_stall();
        let report = from_stall(&stall);
        let (chip, popup) = with_painter(|painter| {
            (
                chip_rect(painter, area),
                geometry_in(painter, area, &report).popup,
            )
        });

        assert!(
            area.contains_rect(popup),
            "the popup stays inside the chart: {popup:?}"
        );
        assert!(
            popup.bottom() <= chip.top(),
            "it opens above the chip, never over it: {popup:?} vs {chip:?}"
        );
        assert!(
            (popup.right() - chip.right()).abs() < 0.5,
            "and shares its right edge, so the eye travels straight up: {popup:?}"
        );
    }

    #[test]
    fn a_short_pane_keeps_the_popup_on_screen() {
        // Shorter than the popup is tall: the anchor would put its top above
        // the chart, which is the one placement a trader cannot recover from.
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 90.0));
        let stall = silent_stall();
        let report = from_stall(&stall);
        let popup = with_painter(|painter| geometry_in(painter, area, &report).popup);
        assert!(
            popup.top() >= area.top() - 0.5,
            "the popup never starts above the chart: {popup:?}"
        );
    }

    /// A stall replaces the words of the progress notice it escalated, rather
    /// than appearing beside them.
    #[test]
    fn a_stall_wins_over_the_progress_it_escalated() {
        let notice = FeedNotice::working("connecting to MetaTrader 5");
        let stall = Stall {
            headline: "MetaTrader 5 — B3 has not connected in 30 s".to_owned(),
            next_step: "Check that MetaTrader 5 is running.".to_owned(),
            primary: Recovery::Reconnect,
            needs_attention: true,
        };
        let report = report(&notice, Some(&stall)).expect("a stall is worth saying");
        assert_eq!(report.headline, stall.headline);
        assert_eq!(report.next_step, Some(stall.next_step.as_str()));
        assert!(
            report.is_offline(),
            "an escalated progress notice is no longer progress"
        );
    }

    /// A quiet tape leads with Reload and wears no alarm: silence is also what
    /// a closed market looks like, and this branch fires two minutes after
    /// every close.
    #[test]
    fn a_quiet_tape_gets_controls_and_not_an_alarm() {
        let stall = silent_stall();
        let report = from_stall(&stall);
        assert!(!report.needs_user);
        assert_eq!(report.primary, Recovery::Reload);
    }

    /// The bug the second rule exists for: a progress card used to be a
    /// sentence with nothing to press. Every report that reaches the popup
    /// still carries both ways out.
    #[test]
    fn every_popup_offers_both_ways_out() {
        let ctx = egui::Context::default();
        let stall = silent_stall();
        let notices = [
            FeedNotice::working("loading WINV26 history from MetaTrader"),
            FeedNotice::reconnecting("Hyperliquid disconnected — reconnecting"),
            FeedNotice::attention("MetaTrader does not list WINQ26", "Add it to Market Watch."),
        ];
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
                for notice in &notices {
                    for stall in [None, Some(&stall)] {
                        let Some(report) = report(notice, stall) else {
                            continue;
                        };
                        let geometry = geometry_in(ui.painter(), area, &report);
                        let (primary, primary_recovery) = geometry.primary;
                        let (secondary, secondary_recovery) = geometry.secondary;
                        assert_ne!(
                            primary_recovery, secondary_recovery,
                            "two buttons that do the same thing are one button"
                        );
                        assert_eq!(primary_recovery, report.primary);
                        assert!(
                            geometry.popup.contains_rect(primary)
                                && geometry.popup.contains_rect(secondary),
                            "a button outside its own popup cannot be pressed"
                        );
                        assert!(!primary.intersects(secondary), "the buttons overlap");
                    }
                }
            });
        });
    }

    /// Lay both surfaces out for real, off-screen, in every shape — no
    /// panicking widget, no duplicated id, and a long instruction that must
    /// wrap.
    #[test]
    fn both_surfaces_lay_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let notices = [
            FeedNotice::working("starting the MetaTrader bridge"),
            FeedNotice::reconnecting("Hyperliquid disconnected — reconnecting"),
            FeedNotice::attention(
                "MetaTrader does not list WINQ26",
                "Add the contract to Market Watch, or pick the exact name your broker \
                 uses (front-month contracts look like WINQ26). This sentence is \
                 deliberately long so the wrap path is exercised too.",
            ),
        ];
        for notice in &notices {
            for _ in 0..2 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = egui::Rect::from_min_max(
                            egui::pos2(0.0, 0.0),
                            egui::pos2(900.0, 600.0),
                        );
                        let report = report(notice, None).expect("something to say");
                        assert!(
                            !chip_at(ui, area, &report, false),
                            "an un-clicked frame presses nothing"
                        );
                        assert_eq!(
                            popup_at(ui, area, &report),
                            NoticeAction::None,
                            "an un-clicked frame asks for nothing"
                        );
                    });
                });
            }
        }
    }

    /// Clicking a button is the whole point of it, so the test clicks it —
    /// laying the popup out once to learn where it landed, then pressing
    /// there. Both buttons, because the secondary is the one a trader reaches
    /// for when the application guessed wrong, and a secondary that does not
    /// fire is exactly the failure this pair exists to remove.
    #[test]
    fn both_buttons_report_a_real_click() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::attention(
            "MetaTrader 5 is not running",
            "Open the terminal and log in.",
        );
        let report = report(&notice, None).expect("a named reason speaks");

        for secondary in [false, true] {
            let ctx = egui::Context::default();
            // Frame 1: draw, and capture where the buttons were put.
            let mut target = egui::Rect::NOTHING;
            let mut expected = NoticeAction::None;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let action = popup_at(ui, area, &report);
                    assert_eq!(action, NoticeAction::None, "nothing was pressed yet");
                    let geometry = geometry_in(ui.painter(), area, &report);
                    let (rect, recovery) = if secondary {
                        geometry.secondary
                    } else {
                        geometry.primary
                    };
                    target = rect;
                    expected = recovery.into();
                });
            });
            assert!(
                area.contains(target.center()),
                "button {target:?} escaped the chart"
            );

            // Frame 2: press and release inside it.
            let click_at = target.center();
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(click_at),
                    egui::Event::PointerButton {
                        pos: click_at,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::default(),
                    },
                    egui::Event::PointerButton {
                        pos: click_at,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            };
            let mut action = NoticeAction::None;
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = popup_at(ui, area, &report);
                });
            });
            assert_eq!(
                action,
                expected,
                "a click on the {} button must mean {expected:?}",
                if secondary { "secondary" } else { "primary" }
            );
        }
    }

    /// The chip is a control, so a click on it has to register as one.
    #[test]
    fn the_chip_reports_a_real_click() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        let report = report(&notice, None).expect("a named reason speaks");
        let ctx = egui::Context::default();

        let mut target = egui::Pos2::ZERO;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!chip_at(ui, area, &report, false));
                target = chip_rect(ui.painter(), area).center();
            });
        });

        let input = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(target),
                egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: target,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };
        let mut clicked = false;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                clicked = chip_at(ui, area, &report, false);
            });
        });
        assert!(clicked, "the corner has to answer the pointer");
    }

    /// The consequence of the destructive control is on the popup, so a trader
    /// learns it before pressing rather than from a journal afterwards.
    #[test]
    fn the_popup_says_what_reload_costs() {
        assert!(
            RELOAD_CAPTION.contains("paper position") && RELOAD_CAPTION.contains("strateg"),
            "the caption has to name both things Reload ends: {RELOAD_CAPTION}"
        );
    }

    /// Wrapped text must stay inside the popup, including when the chart is
    /// narrower than the popup wants to be. The instruction is deliberately
    /// long: short strings never wrap, so they cannot catch this.
    #[test]
    fn a_long_instruction_stays_inside_a_narrow_popup() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::attention(
            "MetaTrader does not list WINZ99",
            "Add the contract to Market Watch, or pick the exact name your broker uses \
             (front-month contracts look like WINQ26).",
        );
        let report = report(&notice, None).expect("a named reason speaks");
        // 120 px is the width the old per-button floor overflowed at: two
        // 44 px buttons plus their gap did not fit the text column, and the
        // second one was drawn over the chart outside the popup.
        for chart_width in [900.0_f32, 420.0, 300.0, 180.0, 120.0, 90.0] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let area = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(chart_width, 600.0),
                    );
                    let geometry = geometry_in(ui.painter(), area, &report);
                    assert!(
                        geometry.popup.width() <= area.width() + f32::EPSILON,
                        "popup {} wider than chart {chart_width}",
                        geometry.popup.width()
                    );
                    // The text column plus its padding is what actually has to
                    // fit — the bug this guards let it wrap to 376 px inside a
                    // 180 px card.
                    let fits = |width: f32| {
                        PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX + width + PAD_PX
                            <= geometry.popup.width() + 1.0
                    };
                    assert!(
                        fits(geometry.headline.size().x),
                        "headline overflows a {} px popup (chart {chart_width})",
                        geometry.popup.width()
                    );
                    if let Some(step) = &geometry.step {
                        assert!(
                            fits(step.size().x),
                            "step overflows a {} px popup (chart {chart_width})",
                            geometry.popup.width()
                        );
                    }
                    assert!(
                        fits(geometry.caption.size().x),
                        "caption overflows a {} px popup (chart {chart_width})",
                        geometry.popup.width()
                    );
                    // And the pair of buttons, which is the thing that can
                    // overflow a column the text alone fits in.
                    assert!(
                        geometry.popup.contains_rect(geometry.secondary.0),
                        "the second button left a {} px popup (chart {chart_width})",
                        geometry.popup.width()
                    );
                });
            });
        }
    }

    /// The popup explains one chart, so it has to fit inside it — including
    /// the tall narrow canvas a three-pane layout leaves, and one narrower
    /// than the popup wants to be.
    #[test]
    fn the_popup_stays_inside_the_chart_it_explains() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::working("loading WINV26 history from MetaTrader");
        let stall = silent_stall();
        let charts = [
            // The right-hand flow pane of the screenshot: tall and narrow.
            egui::Rect::from_min_max(egui::pos2(1080.0, 90.0), egui::pos2(1990.0, 1220.0)),
            // A half-height canvas.
            egui::Rect::from_min_max(egui::pos2(20.0, 90.0), egui::pos2(1080.0, 640.0)),
            // And one narrower than the popup wants to be.
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(240.0, 300.0)),
        ];
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for chart in charts {
                    for stall in [None, Some(&stall)] {
                        let report = report(&notice, stall).expect("something to say");
                        let popup = geometry_in(ui.painter(), chart, &report).popup;
                        assert!(
                            chart.contains_rect(popup),
                            "popup {popup:?} left its chart {chart:?}"
                        );
                        let chip = chip_rect(ui.painter(), chart);
                        assert!(
                            chart.contains_rect(chip),
                            "chip {chip:?} left its chart {chart:?}"
                        );
                    }
                }
            });
        });
    }

    /// A chart narrower than the popup must still produce one inside it.
    #[test]
    fn a_narrow_chart_does_not_overflow_the_popup() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::attention("headline", "step");
        let report = report(&notice, None).expect("a named reason speaks");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 150.0));
                assert_eq!(popup_at(ui, area, &report), NoticeAction::None);
                // The popup is clamped to the chart, and its buttons with it.
                let geometry = geometry_in(ui.painter(), area, &report);
                assert!(
                    geometry.secondary.0.right() <= area.right(),
                    "button {:?} is outside the chart",
                    geometry.secondary.0
                );
            });
        });
    }

    /// Every way the popup can close, and the one way it opens.
    #[test]
    fn every_way_the_popup_closes_and_the_one_way_it_opens() {
        let shut = |chip, offline, dismissed, action| {
            popup_still_open(false, chip, offline, dismissed, action)
        };
        let open = |chip, offline, dismissed, action| {
            popup_still_open(true, chip, offline, dismissed, action)
        };

        assert!(
            shut(true, true, false, NoticeAction::None),
            "the chip opens it"
        );
        assert!(
            !shut(false, true, false, NoticeAction::None),
            "and nothing else does"
        );
        assert!(
            !open(true, true, false, NoticeAction::None),
            "the chip closes it again"
        );
        assert!(
            open(false, true, false, NoticeAction::None),
            "an untouched frame leaves it up"
        );
        assert!(
            !open(false, true, true, NoticeAction::None),
            "a click somewhere else puts it away"
        );
        assert!(
            !open(false, false, false, NoticeAction::None),
            "so does a feed that came back on its own"
        );
        for action in [NoticeAction::Reconnect, NoticeAction::Reload] {
            assert!(
                !open(false, true, false, action),
                "a control that was pressed is under way, not still being read: {action:?}"
            );
        }
        // No chip, no popup, whatever else is true on the frame. A click on a
        // chip that was never drawn cannot reach this today; the rule holds it
        // anyway rather than resting on that.
        assert!(!shut(true, false, false, NoticeAction::None));
        assert!(!open(true, false, false, NoticeAction::None));
    }

    /// The hook stands in for a click and for nothing else.
    ///
    /// The rule, not the reading: setting the real variable here would set it
    /// for every other test in the process.
    #[test]
    fn the_popup_hook_asks_for_exactly_one_value() {
        assert!(!popup_open_from(None), "unset means closed");
        assert!(!popup_open_from(Some("")), "and so does empty");
        assert!(!popup_open_from(Some("0")), "a typo must not open it");
        assert!(!popup_open_from(Some("true")), "nor a near miss");
        assert!(popup_open_from(Some("1")));
    }
}
