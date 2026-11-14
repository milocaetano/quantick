use super::*;
use crate::indicator_worker::IndicatorEvent;
use crate::indicators::IndicatorViews;
use quantick_indicators::{EvalError, IndicatorDescriptor, PlotId, PlotSpec, PlotStyle};

fn descriptor(title: &str) -> IndicatorDescriptor {
    IndicatorDescriptor {
        title: title.to_owned(),
        short_title: None,
        overlay: true,
        plots: vec![PlotSpec {
            id: PlotId::new(0),
            title: "p0".to_owned(),
            style: PlotStyle::Line,
            base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
            width: 1.0,
            offset: 0,
            marker: None,
        }],
        inputs: Vec::new(),
        fills: Vec::new(),
    }
}

fn views_with(build: impl FnOnce(&mut IndicatorViews, SlotId)) -> IndicatorViews {
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    build(&mut views, slot);
    views
}

fn painted(ctx: &egui::Context, views: &IndicatorViews) -> String {
    painted_folded(ctx, views, false)
}

fn painted_folded(ctx: &egui::Context, views: &IndicatorViews, collapsed: bool) -> String {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let mut text = String::new();
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let (actions, _) = draw(ctx, 0, chart, views.all(), None, collapsed);
            assert!(actions.is_empty(), "no clicks, no actions");
        });
        text.clear();
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(galley) = shape.shape {
                text.push_str(galley.galley.text());
                text.push(' ');
            }
        }
    }
    text
}

/// Whoever stacks under this legend — the order-flow key at the same
/// corner — places itself from [`stack_height_px`] before the legend has
/// ever been laid out. The prediction therefore has to cover what the
/// legend really draws, at every row count, or the two print over each
/// other on the frame an indicator is added.
#[test]
fn the_predicted_stack_height_covers_what_the_legend_actually_draws() {
    let ctx = egui::Context::default();
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    for collapsed in [false, true] {
        assert_eq!(
            stack_height_px(&[], collapsed),
            0.0,
            "nothing drawn, nothing claimed"
        );
    }

    // The matrix the fold opened up: rows, times how many of them are
    // unhealthy, times folded or not. A collapsed legend's height is no
    // longer a function of the row count alone — an errored indicator
    // keeps its row through the fold, so the prediction has to count the
    // survivors, and this is what catches it if it ever stops.
    for rows in 1..=3_usize {
        for sick in 0..=rows {
            let mut views = IndicatorViews::new();
            for row in 0..rows {
                let slot = views.allocate_slot("test.indicator");
                views.apply(crate::indicator_worker::event_fixture::rebuilt(
                    slot,
                    descriptor(&format!("EMA({row}, close)")),
                    vec![vec![101.5, 1_234.0]],
                ));
                if row < sick {
                    views.apply(IndicatorEvent::Error {
                        slot,
                        error: EvalError {
                            bar_index: 7,
                            message: "broken".to_owned(),
                        },
                    });
                }
            }
            for collapsed in [false, true] {
                let mut bottom = f32::NEG_INFINITY;
                for _ in 0..2 {
                    let output = ctx.run(egui::RawInput::default(), |ctx| {
                        draw(ctx, 0, chart, views.all(), None, collapsed);
                    });
                    bottom = f32::NEG_INFINITY;
                    for shape in output.shapes {
                        let rect = shape.shape.visual_bounding_rect();
                        if rect.is_positive() {
                            bottom = bottom.max(rect.bottom());
                        }
                    }
                }
                let claimed = chart.top() + stack_height_px(views.all(), collapsed);
                assert!(
                    bottom <= claimed,
                    "{rows} row(s), {sick} broken, collapsed={collapsed}: \
                         the legend reaches {bottom}, the prediction claims {claimed}"
                );
            }
        }
    }
}

/// The fold's whole promise, as a test rather than a comment: an errored
/// indicator is still named, and still says "error", with the legend
/// collapsed. B1 cost the project an indicator that vanished with its
/// message computed and never shown; a fold that could re-hide one would
/// be the same bug wearing a chevron.
#[test]
fn a_folded_legend_still_states_a_broken_indicator() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("zigzag.pine"),
            vec![vec![1.0]],
        ));
        views.apply(IndicatorEvent::Error {
            slot,
            error: EvalError {
                bar_index: 7,
                message: "PINE_NO_SECURITY: security() is not supported".to_owned(),
            },
        });
    });
    let text = painted_folded(&ctx, &views, true);
    assert!(text.contains("zigzag.pine"), "folded: {text}");
    assert!(text.contains("error"), "folded: {text}");
}

/// Stale is the same promise: the chart is drawing an older version than
/// the one on disk, and folding the legend must not be a way to stop
/// saying so.
#[test]
fn a_folded_legend_still_states_a_stale_indicator() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor("cvd.pine"),
            columns: vec![vec![2.0]],
            bar_paint: Vec::new(),
            rows: 1,
            inputs: Vec::new(),
            stale: Some("edit has errors; running the previous version".to_owned()),
        });
    });
    let text = painted_folded(&ctx, &views, true);
    assert!(text.contains("cvd.pine"), "folded: {text}");
    assert!(text.contains("stale"), "folded: {text}");
}

/// What the fold *is* allowed to do: take the healthy rows off the chart
/// and leave a count in their place. The puck counts what it folded, not
/// what is on the pane — the broken row below it is still visible and
/// counting it would be claiming to have hidden something that is right
/// there.
#[test]
fn folding_hides_healthy_rows_and_counts_them() {
    let ctx = egui::Context::default();
    let mut views = IndicatorViews::new();
    for name in ["EMA(9, close)", "ATR(14)"] {
        let slot = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(name),
            vec![vec![101.5, 1_234.0]],
        ));
    }
    let broken = views.allocate_slot("test.indicator");
    views.apply(crate::indicator_worker::event_fixture::rebuilt(
        broken,
        descriptor("zigzag.pine"),
        vec![vec![1.0]],
    ));
    views.apply(IndicatorEvent::Error {
        slot: broken,
        error: EvalError {
            bar_index: 7,
            message: "broken".to_owned(),
        },
    });

    let folded = painted_folded(&ctx, &views, true);
    assert!(
        !folded.contains("EMA(9, close)") && !folded.contains("ATR(14)"),
        "the healthy rows fold away: {folded}"
    );
    assert!(
        folded.contains("zigzag.pine") && folded.contains("error"),
        "the broken one stays, and still says why: {folded}"
    );

    // The puck names what it hid with the plots' own colours. Counted as
    // shapes rather than as text on purpose: a bare integer beside the
    // position HUD reads as a quantity, so "there is no naked count here"
    // is part of what this test holds.
    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let mut dots = 0_usize;
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw(ctx, 0, rect, views.all(), None, true);
        });
        dots = output
            .shapes
            .iter()
            .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Circle(_)))
            .count();
    }
    assert_eq!(
        dots, 2,
        "one dot per folded indicator, and none for the row that stayed"
    );

    let open = painted_folded(&ctx, &views, false);
    assert!(open.contains("EMA(9, close)"), "expanded: {open}");
    assert!(open.contains("ATR(14)"), "expanded: {open}");
}

/// Preview rides the puck, and only when the puck actually hid the
/// indicator being previewed.
///
/// Folding must not become a way to stop saying "the chart is showing
/// settings the state file does not hold" — the dialog that holds them
/// may be sitting behind the chart, which is why the chip exists at all.
/// The second half is the honesty half: an errored indicator keeps its
/// own row through the fold, so a puck that announced *its* preview would
/// be describing a state the pane is not in.
#[test]
fn preview_rides_the_puck_and_only_for_what_the_puck_hid() {
    let ctx = egui::Context::default();
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

    let healthy = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("EMA(9, close)"),
            vec![vec![101.5, 1_234.0]],
        ));
    });
    let slot = healthy.all()[0].slot;
    let painted = |views: &IndicatorViews, preview: Option<SlotId>| {
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, 0, chart, views.all(), preview, true);
            });
            text.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
        }
        text
    };

    let folded = painted(&healthy, Some(slot));
    assert!(folded.contains("preview"), "the puck says it: {folded}");
    assert!(
        !folded.contains("EMA(9, close)"),
        "without unfolding the row: {folded}"
    );

    // Same indicator, now broken and previewing: it keeps its own row, so
    // the puck has nothing of its own to announce.
    let broken = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("EMA(9, close)"),
            vec![vec![101.5]],
        ));
        views.apply(IndicatorEvent::Error {
            slot,
            error: EvalError {
                bar_index: 3,
                message: "broken".to_owned(),
            },
        });
    });
    let broken_slot = broken.all()[0].slot;
    let folded = painted(&broken, Some(broken_slot));
    assert!(
        !folded.contains("preview"),
        "the puck folded nothing, so it announces no preview: {folded}"
    );
    assert!(
        folded.contains("error"),
        "the row still states it: {folded}"
    );
}

/// Both halves of the gesture reach the app as one named outcome, so the
/// chevron, the hotkey and the harness hook cannot disagree about what
/// they toggled from.
#[test]
fn the_chevron_asks_to_fold_and_to_unfold() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("EMA(9, close)"),
            vec![vec![101.5, 1_234.0]],
        ));
    });
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

    for (collapsed, want) in [(false, true), (true, false)] {
        // Aim at the glyph the legend really painted rather than at a
        // position computed from the paddings: the arithmetic would be a
        // second copy of the layout, and a test that agrees with its own
        // copy proves nothing about the one on screen.
        let glyph = if collapsed {
            COLLAPSED_CHEVRON
        } else {
            EXPANDED_CHEVRON
        };
        let mut target = None;
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, 0, chart, views.all(), None, collapsed);
            });
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = &shape.shape
                    && galley.galley.text() == glyph
                {
                    target = Some(galley.visual_bounding_rect().center());
                }
            }
        }
        let target = target.expect("the legend paints its disclosure");
        let mut actions = Vec::new();
        for pressed in [true, false] {
            let mut input = egui::RawInput::default();
            input.events.push(egui::Event::PointerMoved(target));
            input.events.push(egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            });
            let _ = ctx.run(input, |ctx| {
                actions.extend(draw(ctx, 0, chart, views.all(), None, collapsed).0);
            });
        }
        assert!(
            actions.contains(&LegendAction::SetCollapsed(want)),
            "collapsed={collapsed}: the chevron must ask for {want}, got {actions:?}"
        );
    }
}

/// Criterion 1, the legend's target: the *row* opens settings, not just
/// the word.
///
/// The gesture used to be sensed on the label alone, so it worked on the
/// text and failed on the value beside it and on the dot in front of it —
/// a double click that lands a few pixels off and does nothing reads as a
/// broken feature, not as a near miss. Driven at the last value, which is
/// where the old implementation would have missed.
#[test]
fn double_clicking_anywhere_on_a_row_opens_its_settings() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("EMA(9, close)"),
            vec![vec![101.5, 1_234.0]],
        ));
    });
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

    // Lay the legend out, then aim at the last value's glyphs: the
    // right-hand end of the identity region, well past the name.
    let mut target = None;
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            draw(ctx, 0, chart, views.all(), None, false);
        });
        for shape in output.shapes {
            if let egui::epaint::Shape::Text(galley) = &shape.shape
                && galley.galley.text().contains("1.23K")
            {
                target = Some(galley.visual_bounding_rect().center());
            }
        }
    }
    let target = target.expect("the row paints its last value");

    let mut actions = Vec::new();
    for pressed in [true, false, true, false] {
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::PointerMoved(target));
        input.events.push(egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
        let _ = ctx.run(input, |ctx| {
            actions.extend(draw(ctx, 0, chart, views.all(), None, false).0);
        });
    }
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, LegendAction::OpenSettings(_))),
        "a double click on the row's value must open settings: {actions:?}"
    );
}

/// A healthy row reaches pixels: name and last value, in the axis'
/// compact spelling.
#[test]
fn the_legend_names_the_indicator_and_its_last_value() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("EMA(9, close)"),
            vec![vec![101.5, 1_234.0]],
        ));
    });
    let text = painted(&ctx, &views);
    assert!(text.contains("EMA(9, close)"), "painted: {text}");
    assert!(text.contains("1.23K"), "painted: {text}");
}

/// B1: an errored indicator states its error on the chart, message
/// reachable — it must never simply disappear.
#[test]
fn an_errored_indicator_states_its_error_on_the_chart() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor("zigzag.pine"),
            vec![vec![1.0]],
        ));
        views.apply(IndicatorEvent::Error {
            slot,
            error: EvalError {
                bar_index: 7,
                message: "PINE_NO_SECURITY: security() is not supported".to_owned(),
            },
        });
    });
    let text = painted(&ctx, &views);
    assert!(text.contains("zigzag.pine"), "painted: {text}");
    assert!(text.contains("error"), "painted: {text}");
}

/// A stale row wears the amber word; a flat list draws nothing at all.
#[test]
fn stale_rows_say_stale_and_an_empty_list_paints_nothing() {
    let ctx = egui::Context::default();
    let views = views_with(|views, slot| {
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor("cvd.pine"),
            columns: vec![vec![2.0]],
            bar_paint: Vec::new(),
            rows: 1,
            inputs: Vec::new(),
            stale: Some("edit has errors; running the previous version".to_owned()),
        });
    });
    let text = painted(&ctx, &views);
    assert!(text.contains("stale"), "painted: {text}");

    let empty = IndicatorViews::new();
    assert!(painted(&ctx, &empty).is_empty(), "no rows, no chrome");
}
