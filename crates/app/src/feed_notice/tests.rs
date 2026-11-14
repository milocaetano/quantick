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
                    let area =
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(900.0, 600.0));
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
                let area =
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(chart_width, 600.0));
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
