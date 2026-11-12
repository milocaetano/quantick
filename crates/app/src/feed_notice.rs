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
//! The chart a trader opens before the open has nothing wrong with it, so no
//! panel greets them. The trader's rule: **seeing no data at all is worse than
//! not being connected.**
//!
//! So the news is one word in the status bar, the resting report for every feed
//! problem there is, and **the popup** ([`draw_popup`]): the headline, the next
//! step and the two recovery controls, opened by clicking that word and by
//! nothing else.
//!
//! The two controls are [`Recovery::Reconnect`] and [`Recovery::Reload`], and
//! which of them is offered *first* is decided upstream from what the
//! application already knows (see [`quantick_feed::stall`]). The trader is never
//! asked to diagnose their own feed to press the right button.

use eframe::egui;

use crate::theme;
pub use quantick_feed::notice_report::{NoticeAction, Report, Severity, popup_still_open, report};
use quantick_feed::notice_report::{RELOAD_CAPTION, ROLL_CAPTION, roll_label};
use quantick_feed::stall::Recovery;

/// The word when the chart is not being fed: one word for several faults, which
/// leaves *why* to the popup one click away.
pub const OFFLINE_LABEL: &str = "offline";

/// The next step when the feed's own loop has gone back to "working" while the
/// chart holds offline: a fresh attempt is what a hand can add.
pub const HELD_NEXT_STEP: &str =
    "It is still retrying on its own; Reconnect starts a fresh attempt.";

/// Width of the popup, in pixels.
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
    // A capture hook: compiled only with the scenario harness (or under test).
    #[cfg(any(feature = "scenario-harness", test))]
    {
        popup_open_from(crate::hooks::captured::var("QUANTICK_FEED_POPUP").as_deref())
    }
    #[cfg(not(any(feature = "scenario-harness", test)))]
    {
        false
    }
}

/// The rule the hook applies, separated from the reading of it.
///
/// Split so the rule can be tested without setting the real variable. A test
/// that set `QUANTICK_FEED_POPUP` would be setting it for every other test in
/// the process — `cargo test` runs them as threads — and the neighbour that
/// builds a `QuantickApp` reads exactly this variable in its constructor. The
/// failure would land on that neighbour, on a green branch, at random.
#[must_use]
#[cfg(any(feature = "scenario-harness", test))]
fn popup_open_from(value: Option<&str>) -> bool {
    value == Some("1")
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
    /// The roll off an unlisted contract, on its own row above the pair.
    roll: Option<egui::Rect>,
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
    let palette = theme::get(painter.ctx());
    let accent = if report.needs_user {
        palette.loading_title()
    } else {
        palette.loading_meta()
    };

    // Wrap against the width the popup will actually have, not the width it
    // would like: on a narrow chart it is clamped, and text wrapped to the
    // unclamped width spills past its own border.
    let width = POPUP_WIDTH_PX.min(area.width());
    let text_width = (width - 2.0 * PAD_PX - DOT_DIAMETER_PX - DOT_GAP_PX).max(MIN_TEXT_WIDTH_PX);
    let wrap = |text: &str, size: f32, color: egui::Color32| {
        painter.layout(text.to_owned(), palette.body_font(size), color, text_width)
    };
    let headline = wrap(report.headline, HEADLINE_PT, palette.text_primary());
    let step = report
        .next_step_text()
        .map(|step| wrap(&step, STEP_PT, palette.text_muted()));
    let caption = wrap(RELOAD_CAPTION, CAPTION_PT, palette.text_faint());
    // As wide as its label, which names a contract of any length, and never
    // wider than the column: a clipped name is a roll to who knows where.
    let roll = report.roll_target().map(|target| {
        let font = egui::TextStyle::Button.resolve(&painter.ctx().style());
        let label = painter.layout_no_wrap(roll_label(target), font, palette.text_primary());
        let pad = 2.0 * painter.ctx().style().spacing.button_padding.x;
        let width = (label.size().x + pad).max(BUTTON_SIZE_PX.x);
        egui::vec2(width.min(text_width), BUTTON_SIZE_PX.y)
    });

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
    height += roll.map_or(0.0, |size| size.y + BUTTON_SPACING_PX);

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
    let roll = roll
        .map(|size| egui::Rect::from_min_size(egui::pos2(text_left, cursor + BUTTON_GAP_PX), size));
    let button_top = roll.map_or(cursor + BUTTON_GAP_PX, |roll| {
        roll.bottom() + BUTTON_SPACING_PX
    });
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
        roll,
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
    let palette = theme::get(ui.ctx());
    let painter = ui.painter().clone();
    let Geometry {
        popup,
        accent,
        headline,
        step,
        caption,
        primary,
        secondary,
        roll,
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
    painter.rect_filled(
        popup,
        egui::Rounding::same(CORNER_RADIUS_PX),
        palette.chrome(),
    );
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
        palette.text_primary(),
    );
    if let Some(step) = step {
        painter.galley(
            egui::pos2(text_left, headline_top + headline_height + LINE_GAP_PX),
            step,
            palette.text_muted(),
        );
    }

    // The primary is filled and the secondary is not, so the button that fixes
    // *this* stall is the one the eye lands on. Both are always present: the
    // application's guess about which act is needed must never be the reason a
    // trader cannot reach the other one. A roll off an unlisted contract goes
    // above them, filled in the primary's place; the pair stays whole.
    let mut action = NoticeAction::None;
    if let (Some(rect), Some(target)) = (roll, report.roll_target()) {
        let label = egui::RichText::new(roll_label(target)).color(palette.chip_ink());
        let button = ui.put(rect, egui::Button::new(label).fill(accent));
        if button.on_hover_text(ROLL_CAPTION).clicked() {
            action = NoticeAction::Roll(target.to_owned());
        }
    }
    let (primary_rect, primary_recovery) = primary;
    let (fill, ink) = match roll {
        Some(_) => (palette.control(), palette.text_primary()),
        None => (accent, palette.chip_ink()),
    };
    let label = egui::RichText::new(primary_recovery.label()).color(ink);
    if ui
        .put(primary_rect, egui::Button::new(label).fill(fill))
        .on_hover_text(primary_recovery.cost())
        .clicked()
    {
        action = primary_recovery.into();
    }
    let (secondary_rect, secondary_recovery) = secondary;
    if ui
        .put(
            secondary_rect,
            egui::Button::new(secondary_recovery.label()).fill(palette.control()),
        )
        .on_hover_text(secondary_recovery.cost())
        .clicked()
    {
        action = secondary_recovery.into();
    }
    painter.galley(
        egui::pos2(text_left, primary_rect.bottom() + CAPTION_GAP_PX),
        caption,
        palette.text_faint(),
    );
    (action, popup)
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_FEED_POPUP"];

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_feed::{FeedNotice, stall::Stall};

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

    /// An anchor standing where a status word would, inside `area`.
    fn chip_rect(_painter: &egui::Painter, area: egui::Rect) -> egui::Rect {
        let size = egui::vec2(60.0_f32.min(area.width()), 22.0_f32.min(area.height()));
        egui::Rect::from_min_size(area.max - size, size)
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

    /// A roll names a contract of any length, so its button is measured from
    /// its label rather than given the pair's fixed width; and Reconnect and
    /// Reload both stay under it.
    #[test]
    fn a_long_contract_name_fits_its_roll_button_and_the_pair_stays() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
        let notice = FeedNotice::attention("MetaTrader does not list WINFUT_V26", "Add it.")
            .with_unlisted(Some("WINFUT_V26".to_owned()));
        for target in ["WINZ26", "WINFUT_Z26", "WINFUT_Z26_EXTENDED_SERIES"] {
            let report = report(&notice, None)
                .expect("an attention speaks")
                .with_roll(Some(quantick_feed::notice_report::RollNote::To(target)));
            with_painter(|painter| {
                let geometry = geometry_in(painter, area, &report);
                let roll = geometry.roll.expect("a roll on offer gets a button");
                let font = egui::TextStyle::Button.resolve(&painter.ctx().style());
                let label = painter.layout_no_wrap(roll_label(target), font, egui::Color32::WHITE);
                let text_right = geometry.popup.right() - PAD_PX;
                assert!(
                    roll.width() >= label.size().x || roll.right() >= text_right - 0.5,
                    "{target}: the label is {} wide, the button {}",
                    label.size().x,
                    roll.width()
                );
                assert!(
                    roll.right() <= text_right + 0.5,
                    "{target}: inside the popup"
                );
                assert!(
                    roll.width() >= BUTTON_SIZE_PX.x,
                    "never narrower than the pair"
                );
                assert_eq!(geometry.primary.1, Recovery::Reconnect);
                assert_eq!(geometry.secondary.1, Recovery::Reload);
                assert!(
                    geometry.primary.0.top() >= roll.bottom(),
                    "the pair sits under it"
                );
                assert!(geometry.popup.contains_rect(geometry.secondary.0));
            });
        }
        let long = report(&notice, None).unwrap().with_roll(Some(
            quantick_feed::notice_report::RollNote::To("WINFUT_Z26"),
        ));
        let bare = report(&notice, None).unwrap();
        with_painter(|painter| {
            let label = painter.layout_no_wrap(
                roll_label("WINFUT_Z26"),
                egui::TextStyle::Button.resolve(&painter.ctx().style()),
                egui::Color32::WHITE,
            );
            let roll = geometry_in(painter, area, &long).roll.unwrap();
            assert!(roll.width() > label.size().x, "WINFUT_Z26 is not clipped");
            assert!(geometry_in(painter, area, &bare).roll.is_none());
        });
    }
}
