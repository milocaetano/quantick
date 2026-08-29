//! The card that tells the user what the feed is doing, and what to do about
//! it (`docs/ux/ui-design-model.md` §8 — provenance, told at chart scale).
//!
//! An empty chart is ambiguous. It looks the same whether the market is quiet,
//! MetaTrader is closed, the Python package was never installed, or the
//! contract does not exist for this account. The status bar's faint
//! "connecting" dot is honest about *that* it is not connected and silent
//! about *why*, which is fine when the answer is "wait a second" and useless
//! when the answer is "open your terminal".
//!
//! So the card exists to carry the feed's state at a size a person notices,
//! and to obey two rules:
//!
//! - **Never cover a working chart.** A reason the provider itself named is the
//!   one exception: a feed that died after an hour of trading is exactly when
//!   the reason matters. Everything else waits for a pane with nothing in it —
//!   including the stall this application *infers*, because silence is also
//!   what a closed market looks like, and painting an instruction over a
//!   healthy chart at 03:00 would teach the trader to ignore the card.
//! - **Never say something without offering the way out.** The card used to
//!   give a button only to a notice carrying a next step, so a
//!   [`FeedNotice::Working`] that never resolved — "loading WINV26 history from
//!   MetaTrader" — drew a sentence with nothing to press, for as long as the
//!   trader left it there. Their way out was to close the application. Every
//!   card now carries both recovery controls.
//!
//! The two controls are [`Recovery::Reconnect`] and [`Recovery::Reload`], and
//! which of them is offered *first* is decided upstream from what the
//! application already knows (see [`crate::feed::stall`]). The trader is never
//! asked to diagnose their own feed to press the right button.

use eframe::egui;

use crate::feed::FeedNotice;
use crate::feed::stall::{Recovery, Stall};
use crate::theme;

/// Width of the card, in pixels — wide enough for a sentence of instruction
/// without spanning a wide monitor. Clamped to the chart when it is narrower.
const CARD_WIDTH_PX: f32 = 420.0;
/// Floor for the text column, in pixels. A chart narrow enough to drive this
/// negative would otherwise wrap every word onto its own line, or panic the
/// layout; the card is unreadable at that size either way, and this keeps it
/// merely cramped.
const MIN_TEXT_WIDTH_PX: f32 = 40.0;
/// Padding inside the card, in pixels.
const PAD_PX: f32 = 14.0;
/// Gap between the headline and the next step, in pixels.
const LINE_GAP_PX: f32 = 6.0;
/// Diameter of the leading state dot, in pixels.
const DOT_DIAMETER_PX: f32 = 8.0;
/// Gap between the dot and the headline, in pixels.
const DOT_GAP_PX: f32 = 8.0;
/// Headline font size, in points.
const HEADLINE_PT: f32 = 14.0;
/// Next-step font size, in points.
const STEP_PT: f32 = 12.0;
/// Consequence-caption font size, in points. Smaller than the next step: it is
/// there to be read before pressing, not to compete with the instruction.
const CAPTION_PT: f32 = 11.0;
/// Card corner radius, in pixels.
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
/// How far above the chart's vertical centre the card sits, as a fraction of
/// the chart height. Slightly high reads as an overlay rather than content.
const VERTICAL_BIAS: f32 = 0.12;

/// What [`Recovery::Reload`] costs, said on the card rather than behind a
/// confirmation dialog.
///
/// A modal between a trader and a stalled chart is one more thing in the way at
/// the worst moment, and this act is recoverable in the sense that matters —
/// the position it closes is journaled, with its reason, by
/// `PaperTrading::on_timeline_reset`. What is not acceptable is the trader
/// learning about it afterwards, which is what happened while the only button
/// on this card said "Try again" and quietly reset the timeline.
const RELOAD_CAPTION: &str = "Reload rebuilds the chart from scratch: it closes any open paper position and \
     disarms every strategy.";

/// What the person watching asked of the card this frame.
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

/// What the interface has to tell the trader about one feed this frame —
/// however it was arrived at, and whatever it is allowed to cover.
///
/// Both sources of words end up here: the feed's own [`FeedNotice`], and the
/// [`Stall`] this application infers when a notice stops being progress. The
/// card draws a `Report` and knows nothing about which one produced it, so
/// there is one layout, one hit test and one pair of buttons rather than a
/// second card for the second source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report<'a> {
    /// One line: what is happening, or what was observed.
    pub headline: &'a str,
    /// The single next step, when there is one to give.
    pub next_step: Option<&'a str>,
    /// Amber rather than muted: this needs a person, it is not just slow.
    pub needs_user: bool,
    /// May be drawn over a pane that is painting bars.
    ///
    /// True only for a reason the *provider* named. An escalation this
    /// application inferred stays on an empty pane and in the status bar.
    pub may_cover_bars: bool,
    /// The control offered first. The other is always beside it.
    pub primary: Recovery,
}

/// Build the report for a feed, or `None` when there is nothing to say.
///
/// `stall` is this application's own judgement, already decided against the
/// clock by [`crate::feed::stall::assess`]; it wins over a progress notice,
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
            may_cover_bars: true,
            primary: Recovery::Reconnect,
        });
    }
    if let Some(stall) = stall {
        return Some(Report {
            headline: &stall.headline,
            next_step: Some(&stall.next_step),
            // Amber for a transport that is broken however you read it, muted
            // for a tape that has merely gone quiet — which is also what a
            // closed market looks like, and this card would otherwise wear an
            // alarm every night on a chart with nothing wrong with it.
            needs_user: stall.needs_attention,
            may_cover_bars: false,
            primary: stall.primary,
        });
    }
    match notice {
        FeedNotice::Connected | FeedNotice::Clear | FeedNotice::Attention { .. } => None,
        FeedNotice::Reconnecting { headline } | FeedNotice::Working { headline } => Some(Report {
            headline,
            next_step: None,
            needs_user: false,
            may_cover_bars: false,
            primary: Recovery::Reconnect,
        }),
    }
}

/// Whether `report` may be drawn over a pane holding `bars` bars.
#[must_use]
pub fn should_draw(report: &Report<'_>, bars: usize) -> bool {
    report.may_cover_bars || bars == 0
}

/// Where every piece of the card lands, measured once so drawing and hit
/// testing can never disagree about it.
struct Geometry {
    /// The card's outline.
    card: egui::Rect,
    /// Colour carrying the severity: the dot, the border.
    accent: egui::Color32,
    /// The headline, already wrapped.
    headline: std::sync::Arc<egui::Galley>,
    /// The next step, already wrapped; absent on a progress card.
    step: Option<std::sync::Arc<egui::Galley>>,
    /// The consequence caption, already wrapped.
    caption: std::sync::Arc<egui::Galley>,
    /// The button offered first, and what pressing it means.
    primary: (egui::Rect, Recovery),
    /// The other one.
    secondary: (egui::Rect, Recovery),
}

/// Measure the card for `report` inside `area`.
///
/// Text height depends on the fonts, so this needs a painter — but it takes
/// no other state, and both the drawing and the tests go through it.
fn geometry(painter: &egui::Painter, area: egui::Rect, report: &Report<'_>) -> Geometry {
    let accent = if report.needs_user {
        theme::AMBER
    } else {
        theme::TEXT_MUTED
    };

    // Wrap against the width the card will actually have, not the width it
    // would like: on a narrow chart the card is clamped, and text wrapped to
    // the unclamped width spills past its own border.
    let width = CARD_WIDTH_PX.min(area.width());
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
    // leaving the card.
    //
    // The floor yields to the column rather than overriding it. Clamped up to
    // 44 px unconditionally, each button kept its width while the column
    // shrank, and on a pane under ~140 px the second one was drawn past the
    // card's right edge, over the chart with no chrome behind it. Below that
    // width the labels are unreadable either way, and a control whose edges
    // the trader cannot see is worse than a cramped one.
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

    let card = egui::Rect::from_center_size(
        egui::pos2(
            area.center().x,
            area.center().y - area.height() * VERTICAL_BIAS,
        ),
        egui::vec2(width, height),
    );

    let text_left = card.left() + PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX;
    let mut cursor = card.top() + PAD_PX + headline.size().y;
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
        card,
        accent,
        headline,
        step,
        caption,
        primary: (primary, report.primary),
        secondary: (secondary, report.primary.other()),
    }
}

/// Where the card lands for `report` inside `area` — the outline only.
///
/// The placement rule this file exists to keep is "on the pane that is
/// waiting", and a rule nothing measures is a rule that drifts. Test-only
/// because production has no reason to ask: [`draw`] already puts it there.
#[cfg(test)]
#[must_use]
pub fn card_rect(painter: &egui::Painter, area: egui::Rect, report: &Report<'_>) -> egui::Rect {
    geometry(painter, area, report).card
}

/// Draw the card centred in `area`. Call only when [`should_draw`] agrees.
#[must_use]
pub fn draw(ui: &mut egui::Ui, area: egui::Rect, report: &Report<'_>) -> NoticeAction {
    let painter = ui.painter().clone();
    let Geometry {
        card,
        accent,
        headline,
        step,
        caption,
        primary,
        secondary,
    } = geometry(&painter, area, report);

    // Chrome, not canvas: the card belongs to the interface reporting on the
    // chart rather than to the chart. Opaque, because an instruction someone
    // has to follow should not be read through candles; the accent carries
    // the severity.
    painter.rect_filled(card, egui::Rounding::same(CORNER_RADIUS_PX), theme::CHROME);
    painter.rect_stroke(
        card,
        egui::Rounding::same(CORNER_RADIUS_PX),
        egui::Stroke::new(1.0_f32, accent),
    );

    let text_left = card.left() + PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX;
    let headline_top = card.top() + PAD_PX;
    painter.circle_filled(
        egui::pos2(
            card.left() + PAD_PX + DOT_DIAMETER_PX / 2.0,
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
        .clicked()
    {
        action = secondary_recovery.into();
    }
    painter.galley(
        egui::pos2(text_left, primary_rect.bottom() + CAPTION_GAP_PX),
        caption,
        theme::TEXT_FAINT,
    );
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report for a bare notice, with nothing inferred on top.
    fn plain(notice: &FeedNotice) -> Option<Report<'_>> {
        report(notice, None)
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

    #[test]
    fn a_clear_notice_says_nothing() {
        assert!(plain(&FeedNotice::Clear).is_none());
        assert!(plain(&FeedNotice::Connected).is_none());
    }

    #[test]
    fn progress_never_covers_a_chart_that_has_bars() {
        let working = FeedNotice::working("starting the MetaTrader bridge");
        let report = plain(&working).expect("progress is worth saying");
        assert!(should_draw(&report, 0), "an empty chart explains itself");
        assert!(
            !should_draw(&report, 1),
            "one bar is enough to prefer the chart"
        );

        let reconnecting = FeedNotice::reconnecting("Binance disconnected — reconnecting");
        let report = plain(&reconnecting).expect("a reconnect is worth saying");
        assert!(should_draw(&report, 0));
        assert!(!should_draw(&report, 1));
    }

    #[test]
    fn anything_the_provider_named_is_shown_whatever_is_on_screen() {
        let attention = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        let report = plain(&attention).expect("a named reason is always worth saying");
        assert!(should_draw(&report, 0));
        assert!(
            should_draw(&report, 10_000),
            "a feed that died mid-session still has to say so"
        );
    }

    /// The inferred stall is the one that must not paint over a working chart:
    /// silence is also what a closed market looks like.
    #[test]
    fn an_inferred_stall_waits_for_an_empty_pane() {
        let notice = FeedNotice::Clear;
        let stall = silent_stall();
        let report = report(&notice, Some(&stall)).expect("a stall is worth saying");
        assert!(
            !report.needs_user,
            "a quiet tape is also a closed market: controls, not an alarm"
        );
        assert!(should_draw(&report, 0));
        assert!(
            !should_draw(&report, 1),
            "an overnight market must not be painted over"
        );
        assert_eq!(report.primary, Recovery::Reload);
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
    }

    /// The bug this file's second rule exists for: a progress card used to be
    /// a sentence with nothing to press.
    #[test]
    fn every_card_offers_both_ways_out() {
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
                        let geometry = geometry(ui.painter(), area, &report);
                        let (primary, primary_recovery) = geometry.primary;
                        let (secondary, secondary_recovery) = geometry.secondary;
                        assert_ne!(
                            primary_recovery, secondary_recovery,
                            "two buttons that do the same thing are one button"
                        );
                        assert_eq!(primary_recovery, report.primary);
                        assert!(
                            geometry.card.contains_rect(primary)
                                && geometry.card.contains_rect(secondary),
                            "a button outside its own card cannot be pressed"
                        );
                        assert!(!primary.intersects(secondary), "the buttons overlap");
                    }
                }
            });
        });
    }

    /// Lay the card out for real, off-screen, in every shape — no panicking
    /// widget, no duplicated id, and a long instruction that must wrap.
    #[test]
    fn the_card_lays_out_against_a_real_context() {
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
                        let report = report(notice, None).expect("a card");
                        assert_eq!(
                            draw(ui, area, &report),
                            NoticeAction::None,
                            "an un-clicked frame asks for nothing"
                        );
                    });
                });
            }
        }
    }

    /// Clicking a button is the whole point of it, so the test clicks it —
    /// laying the card out once to learn where it landed, then pressing there.
    /// Both buttons, because the secondary is the one a trader reaches for when
    /// the application guessed wrong, and a secondary that does not fire is
    /// exactly the failure this change exists to remove.
    #[test]
    fn both_buttons_report_a_real_click() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::attention(
            "MetaTrader 5 is not running",
            "Open the terminal and log in.",
        );
        let report = report(&notice, None).expect("a card");

        for secondary in [false, true] {
            let ctx = egui::Context::default();
            // Frame 1: draw, and capture where the buttons were put.
            let mut target = egui::Rect::NOTHING;
            let mut expected = NoticeAction::None;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let action = draw(ui, area, &report);
                    assert_eq!(action, NoticeAction::None, "nothing was pressed yet");
                    let geometry = geometry(ui.painter(), area, &report);
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
                    action = draw(ui, area, &report);
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

    /// The consequence of the destructive control is on the card, so a trader
    /// learns it before pressing rather than from a journal afterwards.
    #[test]
    fn the_card_says_what_reload_costs() {
        assert!(
            RELOAD_CAPTION.contains("paper position") && RELOAD_CAPTION.contains("strateg"),
            "the caption has to name both things Reload ends: {RELOAD_CAPTION}"
        );
    }

    /// Wrapped text must stay inside the card, including when the chart is
    /// narrower than the card wants to be. The instruction is deliberately
    /// long: short strings never wrap, so they cannot catch this.
    #[test]
    fn a_long_instruction_stays_inside_a_narrow_card() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::attention(
            "MetaTrader does not list WINZ99",
            "Add the contract to Market Watch, or pick the exact name your broker uses \
             (front-month contracts look like WINQ26).",
        );
        let report = report(&notice, None).expect("a card");
        // 120 px is the width the old per-button floor overflowed at: two
        // 44 px buttons plus their gap did not fit the text column, and the
        // second one was drawn over the chart outside the card.
        for chart_width in [900.0_f32, 420.0, 300.0, 180.0, 120.0, 90.0] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let area = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(chart_width, 600.0),
                    );
                    let geometry = geometry(ui.painter(), area, &report);
                    assert!(
                        geometry.card.width() <= area.width() + f32::EPSILON,
                        "card {} wider than chart {chart_width}",
                        geometry.card.width()
                    );
                    // The text column plus its padding is what actually has to
                    // fit — the bug this guards let it wrap to 376 px inside a
                    // 180 px card.
                    let fits = |width: f32| {
                        PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX + width + PAD_PX
                            <= geometry.card.width() + 1.0
                    };
                    assert!(
                        fits(geometry.headline.size().x),
                        "headline overflows a {} px card (chart {chart_width})",
                        geometry.card.width()
                    );
                    if let Some(step) = &geometry.step {
                        assert!(
                            fits(step.size().x),
                            "step overflows a {} px card (chart {chart_width})",
                            geometry.card.width()
                        );
                    }
                    assert!(
                        fits(geometry.caption.size().x),
                        "caption overflows a {} px card (chart {chart_width})",
                        geometry.card.width()
                    );
                    // And the pair of buttons, which is the new thing that can
                    // overflow a column the text alone fits in.
                    assert!(
                        geometry.card.contains_rect(geometry.secondary.0),
                        "the second button left a {} px card (chart {chart_width})",
                        geometry.card.width()
                    );
                });
            });
        }
    }

    /// The card explains one pane, so it has to fit inside that pane —
    /// including the tall narrow one a three-pane canvas leaves on the right,
    /// which is the shape it was overflowing in the session this came from.
    #[test]
    fn the_card_stays_inside_the_pane_it_explains() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::working("loading WINV26 history from MetaTrader");
        let stall = silent_stall();
        let panes = [
            // The right-hand flow pane of the screenshot: tall and narrow.
            egui::Rect::from_min_max(egui::pos2(1080.0, 90.0), egui::pos2(1990.0, 1220.0)),
            // A half-height time pane.
            egui::Rect::from_min_max(egui::pos2(20.0, 90.0), egui::pos2(1080.0, 640.0)),
            // And a pane narrower than the card wants to be.
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(240.0, 300.0)),
        ];
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for pane in panes {
                    for stall in [None, Some(&stall)] {
                        let report = report(&notice, stall).expect("a card");
                        let card = card_rect(ui.painter(), pane, &report);
                        assert!(
                            pane.contains_rect(card),
                            "card {card:?} left its pane {pane:?}"
                        );
                    }
                }
            });
        });
    }

    /// A chart narrower than the card must still produce a card inside it.
    #[test]
    fn a_narrow_chart_does_not_overflow_the_card() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::attention("headline", "step");
        let report = report(&notice, None).expect("a card");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 150.0));
                assert_eq!(draw(ui, area, &report), NoticeAction::None);
                // The card is clamped to the chart, and its buttons with it.
                let geometry = geometry(ui.painter(), area, &report);
                assert!(
                    geometry.secondary.0.right() <= area.right(),
                    "button {:?} is outside the chart",
                    geometry.secondary.0
                );
            });
        });
    }
}
