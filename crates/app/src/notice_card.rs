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
//! So the card exists to carry a [`FeedNotice`] at a size a person notices,
//! and to obey one rule: **never cover a working chart**. It draws when the
//! chart has nothing to show, or when the notice needs the user; a
//! [`FeedNotice::Working`] over a live chart is what the status bar is for.

use eframe::egui;

use crate::feed::FeedNotice;
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
/// Card corner radius, in pixels.
const CORNER_RADIUS_PX: f32 = 6.0;
/// Size of the retry button, in pixels.
const BUTTON_SIZE_PX: egui::Vec2 = egui::vec2(96.0, 24.0);
/// Gap between the next step and the retry button, in pixels.
const BUTTON_GAP_PX: f32 = 10.0;
/// How far above the chart's vertical centre the card sits, as a fraction of
/// the chart height. Slightly high reads as an overlay rather than content.
const VERTICAL_BIAS: f32 = 0.12;

/// What the person watching asked of the card this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeAction {
    /// Nothing was clicked.
    #[default]
    None,
    /// Start the feed over.
    ///
    /// The way out of a bridge that stopped retrying: once the reason is fixed
    /// — terminal opened, package installed — there has to be something to
    /// press that is not "restart quantick".
    Retry,
}

/// Whether `notice` should be drawn over a chart holding `bars` bars.
///
/// Anything needing the user is always shown — a feed that died after an hour
/// of trading is exactly when the reason matters. Progress is shown only while
/// there is nothing else to look at.
#[must_use]
pub fn should_draw(notice: &FeedNotice, bars: usize) -> bool {
    match notice {
        FeedNotice::Connected | FeedNotice::Clear => false,
        FeedNotice::Attention { .. } => true,
        FeedNotice::Working { .. } => bars == 0,
    }
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
    /// The retry button, present exactly when there is a step to act on.
    button: Option<egui::Rect>,
}

/// Measure the card for `notice` inside `area`.
///
/// Text height depends on the fonts, so this needs a painter — but it takes
/// no other state, and both the drawing and the tests go through it.
fn geometry(painter: &egui::Painter, area: egui::Rect, notice: &FeedNotice) -> Option<Geometry> {
    let (accent, headline, next_step) = match notice {
        FeedNotice::Connected | FeedNotice::Clear => return None,
        FeedNotice::Working { headline } => (theme::TEXT_MUTED, headline.as_str(), None),
        FeedNotice::Attention {
            headline,
            next_step,
        } => (theme::AMBER, headline.as_str(), Some(next_step.as_str())),
    };

    // Wrap against the width the card will actually have, not the width it
    // would like: on a narrow chart the card is clamped, and text wrapped to
    // the unclamped width spills past its own border.
    let width = CARD_WIDTH_PX.min(area.width());
    let text_width = (width - 2.0 * PAD_PX - DOT_DIAMETER_PX - DOT_GAP_PX).max(MIN_TEXT_WIDTH_PX);
    let headline_galley = painter.layout(
        headline.to_owned(),
        egui::FontId::proportional(HEADLINE_PT),
        theme::TEXT_PRIMARY,
        text_width,
    );
    let step_galley = next_step.map(|step| {
        painter.layout(
            step.to_owned(),
            egui::FontId::proportional(STEP_PT),
            theme::TEXT_MUTED,
            text_width,
        )
    });

    let mut height = 2.0 * PAD_PX + headline_galley.size().y;
    if let Some(step) = &step_galley {
        height += LINE_GAP_PX + step.size().y;
    }
    // Only something that needs the user gets something to press.
    if step_galley.is_some() {
        height += BUTTON_GAP_PX + BUTTON_SIZE_PX.y;
    }
    let card = egui::Rect::from_center_size(
        egui::pos2(
            area.center().x,
            area.center().y - area.height() * VERTICAL_BIAS,
        ),
        egui::vec2(width, height),
    );

    let text_left = card.left() + PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX;
    let button = step_galley.as_ref().map(|step| {
        let top = card.top() + PAD_PX + headline_galley.size().y + LINE_GAP_PX + step.size().y;
        egui::Rect::from_min_size(egui::pos2(text_left, top + BUTTON_GAP_PX), BUTTON_SIZE_PX)
    });

    Some(Geometry {
        card,
        accent,
        headline: headline_galley,
        step: step_galley,
        button,
    })
}

/// Draw the card centred in `area`. Call only when [`should_draw`] agrees.
#[must_use]
pub fn draw(ui: &mut egui::Ui, area: egui::Rect, notice: &FeedNotice) -> NoticeAction {
    let painter = ui.painter().clone();
    let Some(geometry) = geometry(&painter, area, notice) else {
        return NoticeAction::None;
    };
    let Geometry {
        card,
        accent,
        headline,
        step,
        button,
    } = geometry;

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
    let Some(button) = button else {
        return NoticeAction::None;
    };
    if ui.put(button, egui::Button::new("Try again")).clicked() {
        NoticeAction::Retry
    } else {
        NoticeAction::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the retry button lands for `notice`, or `None` when it has none.
    /// Goes through [`geometry`] exactly as [`draw`] does, so a test that
    /// clicks here clicks what the user clicks.
    fn retry_button_rect(
        ui: &egui::Ui,
        area: egui::Rect,
        notice: &FeedNotice,
    ) -> Option<egui::Rect> {
        geometry(ui.painter(), area, notice).and_then(|geometry| geometry.button)
    }

    #[test]
    fn a_clear_notice_draws_nothing() {
        assert!(!should_draw(&FeedNotice::Clear, 0));
        assert!(!should_draw(&FeedNotice::Clear, 500));
    }

    #[test]
    fn progress_never_covers_a_chart_that_has_bars() {
        let working = FeedNotice::working("starting the MetaTrader bridge");
        assert!(should_draw(&working, 0), "an empty chart explains itself");
        assert!(
            !should_draw(&working, 1),
            "one bar is enough to prefer the chart"
        );
    }

    #[test]
    fn anything_needing_the_user_is_shown_whatever_is_on_screen() {
        let attention = FeedNotice::attention("MetaTrader 5 is not running", "Open the terminal.");
        assert!(should_draw(&attention, 0));
        assert!(
            should_draw(&attention, 10_000),
            "a feed that died mid-session still has to say so"
        );
    }

    /// Lay the card out for real, off-screen, in both shapes — no panicking
    /// widget, no duplicated id, and a long instruction that must wrap.
    #[test]
    fn the_card_lays_out_against_a_real_context() {
        let ctx = egui::Context::default();
        let notices = [
            FeedNotice::working("starting the MetaTrader bridge"),
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
                        assert_eq!(
                            draw(ui, area, notice),
                            NoticeAction::None,
                            "an un-clicked frame asks for nothing"
                        );
                    });
                });
            }
        }
    }

    /// Clicking the button is the whole point of it, so the test clicks it —
    /// laying the card out once to learn where it landed, then pressing there.
    #[test]
    fn the_retry_button_reports_a_real_click() {
        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::attention(
            "MetaTrader 5 is not running",
            "Open the terminal and log in.",
        );

        // Frame 1: draw, and capture where the button was put.
        let mut button = egui::Rect::NOTHING;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let action = draw(ui, area, &notice);
                assert_eq!(action, NoticeAction::None, "nothing was pressed yet");
                button = retry_button_rect(ui, area, &notice).expect("attention has a button");
            });
        });
        assert!(
            area.contains(button.center()),
            "button {button:?} escaped the chart"
        );

        // Frame 2: press and release inside it.
        let click_at = button.center();
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
                action = draw(ui, area, &notice);
            });
        });
        assert_eq!(action, NoticeAction::Retry, "a click on it must mean retry");
    }

    #[test]
    fn a_progress_card_has_no_button_to_click() {
        let ctx = egui::Context::default();
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::working("starting the MetaTrader bridge");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert_eq!(retry_button_rect(ui, area, &notice), None);
            });
        });
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
        for chart_width in [900.0_f32, 420.0, 300.0, 180.0] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let area = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(chart_width, 600.0),
                    );
                    let geometry = geometry(ui.painter(), area, &notice).expect("a card");
                    assert!(
                        geometry.card.width() <= area.width() + f32::EPSILON,
                        "card {} wider than chart {chart_width}",
                        geometry.card.width()
                    );
                    // The text column plus its padding is what actually has to
                    // fit — the bug this guards let it wrap to 376 px inside a
                    // 180 px card.
                    let text_right_edge =
                        PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX + geometry.headline.size().x + PAD_PX;
                    assert!(
                        text_right_edge <= geometry.card.width() + 1.0,
                        "headline needs {text_right_edge} px inside a {} px card (chart {chart_width})",
                        geometry.card.width()
                    );
                    if let Some(step) = &geometry.step {
                        let step_right_edge =
                            PAD_PX + DOT_DIAMETER_PX + DOT_GAP_PX + step.size().x + PAD_PX;
                        assert!(
                            step_right_edge <= geometry.card.width() + 1.0,
                            "step needs {step_right_edge} px inside a {} px card (chart {chart_width})",
                            geometry.card.width()
                        );
                    }
                });
            });
        }
    }

    /// A chart narrower than the card must still produce a card inside it.
    #[test]
    fn a_narrow_chart_does_not_overflow_the_card() {
        let ctx = egui::Context::default();
        let notice = FeedNotice::attention("headline", "step");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 150.0));
                assert_eq!(draw(ui, area, &notice), NoticeAction::None);
                // The card is clamped to the chart, and its button with it.
                let button = retry_button_rect(ui, area, &notice).expect("a button");
                assert!(
                    button.width() <= area.width(),
                    "button {button:?} is wider than the chart"
                );
            });
        });
    }
}
