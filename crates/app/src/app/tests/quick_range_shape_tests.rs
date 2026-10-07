//! The quick-range bar turns the ruler into a shape: the leg's anchors carry
//! over exactly, and the channel waits for the trader's width click.

use super::bare_canvas::app_with_history as bare_app_with_history;
use super::drawings_tests::{
    click_quick_range_action, drag_quick_range, quick_range_ends, secondary_button,
};
use super::*;
use crate::surfaces::drawing_chrome::QuickRangeAction as Action;
use serde_json::json;

/// A settled ruler, and the feed channel that keeps its series alive.
struct Ruler {
    ctx: egui::Context,
    app: QuantickApp,
    start: egui::Pos2,
    end: egui::Pos2,
    _commands: mpsc::Receiver<FeedCommand>,
}

fn ruler(levelled: bool) -> Ruler {
    let ctx = egui::Context::default();
    let (mut app, commands) = bare_app_with_history(200);
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    let (_, start, end) = quick_range_ends(&app);
    if levelled {
        let shift = egui::Modifiers::SHIFT;
        let moves = [
            vec![
                egui::Event::PointerMoved(start),
                secondary_button(start, true),
            ],
            vec![egui::Event::PointerMoved(end)],
            vec![egui::Event::PointerMoved(end), secondary_button(end, false)],
        ];
        for events in moves {
            run_frame_with_modifiers(&mut app, &ctx, events, shift);
        }
    } else {
        drag_quick_range(&mut app, &ctx, start, end);
    }
    Ruler {
        ctx,
        app,
        start,
        end,
        _commands: commands,
    }
}

/// The wire carries prices at `ANNOTATION_PRICE_DECIMALS`; bars and times exactly.
fn same_price(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-8
}

fn same_points(
    placed: &[crate::drawings::ChartPoint],
    leg: &[crate::drawings::ChartPoint],
) -> bool {
    placed.len() == leg.len()
        && placed
            .iter()
            .zip(leg)
            .all(|(a, b)| a.bar == b.bar && a.time_ms == b.time_ms && same_price(a.price, b.price))
}

fn drawings(app: &QuantickApp) -> &[crate::drawings::Drawing] {
    app.active_tab().flow_pane.drawings.items()
}

#[test]
fn rectangle_and_trend_line_take_the_legs_two_anchors() {
    for (action, tool) in [
        (Action::Rectangle, "rectangle"),
        (Action::TrendLine, "trend-line"),
    ] {
        let Ruler {
            ctx,
            mut app,
            _commands,
            ..
        } = ruler(false);
        let leg = app.drawings.chrome.quick_range.paint_anchors().unwrap();
        click_quick_range_action(&mut app, &ctx, action);
        let placed = drawings(&app);
        assert_eq!(placed.len(), 1, "{action:?}");
        assert_eq!(placed[0].tool.id(), tool);
        assert!(
            same_points(&placed[0].points, &leg),
            "{action:?} keeps the leg: {:?}",
            placed[0].points
        );
    }
}

#[test]
fn horizontal_marks_the_ranges_top_and_bottom() {
    let Ruler {
        ctx,
        mut app,
        _commands,
        ..
    } = ruler(false);
    let leg = app.drawings.chrome.quick_range.paint_anchors().unwrap();
    click_quick_range_action(&mut app, &ctx, Action::Horizontal);
    let prices: Vec<f64> = drawings(&app).iter().map(|d| d.points[0].price).collect();
    assert!(
        drawings(&app)
            .iter()
            .all(|d| d.tool.id() == "horizontal-line")
    );
    let (top, bottom) = (
        leg[0].price.max(leg[1].price),
        leg[0].price.min(leg[1].price),
    );
    assert!(top > bottom);
    assert_eq!(prices.len(), 2);
    assert!(same_price(prices[0], top) && same_price(prices[1], bottom));
}

#[test]
fn a_levelled_range_marks_one_horizontal_line() {
    let Ruler {
        ctx,
        mut app,
        _commands,
        ..
    } = ruler(true);
    let leg = app.drawings.chrome.quick_range.paint_anchors().unwrap();
    assert_eq!(leg[0].price, leg[1].price);
    click_quick_range_action(&mut app, &ctx, Action::Horizontal);
    assert_eq!(drawings(&app).len(), 1);
    assert!(same_price(drawings(&app)[0].points[0].price, leg[0].price));
}

#[test]
fn the_channel_keeps_the_leg_and_takes_its_width_from_the_next_click() {
    for (above, offset) in [(true, -70.0), (false, 70.0)] {
        let Ruler {
            ctx,
            mut app,
            start,
            end,
            _commands,
        } = ruler(false);
        let leg = app.drawings.chrome.quick_range.paint_anchors().unwrap();
        click_quick_range_action(&mut app, &ctx, Action::Channel);
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.drawings.items().is_empty(), "nothing placed yet");
        let draft = pane.drawings.draft().expect("the leg is a pending channel");
        assert_eq!(draft.tool.id(), "parallel-channel");
        assert_eq!(draft.points, leg);
        assert!(crate::app::control_quick_range(&app).is_none(), "box gone");

        let middle = egui::pos2((start.x + end.x) / 2.0, (start.y + end.y) / 2.0 + offset);
        click_chart(&mut app, &ctx, middle);
        let placed = drawings(&app);
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].tool.id(), "parallel-channel");
        assert_eq!(placed[0].points[..2], leg);
        let base = (leg[0].price + leg[1].price) / 2.0;
        assert_eq!(placed[0].points[2].price > base, above, "{above}");
        assert!(app.active_tab().flow_pane.drawings.draft().is_none());
    }
}

#[test]
fn horizontal_levels_capability_places_one_line_per_distinct_price() {
    for (prices, expected) in [(["101", "99"], 2), (["100", "100"], 1)] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let mut anchors = [anchor_at_slot(&app, 5), anchor_at_slot(&app, 1)];
        for (anchor, (price, bar)) in anchors
            .iter_mut()
            .zip(prices.into_iter().zip(["5.5", "1.5"]))
        {
            anchor["price"] = json!(price);
            anchor["bar_position"] = json!(bar);
        }
        let result = app
            .control_action(
                crate::control::HORIZONTAL_LEVELS_CAPABILITY_ID,
                1,
                crate::control::ActionOrigin::Human,
                json!({ "anchors": anchors }),
            )
            .unwrap();
        let annotations = result["annotations"].as_array().unwrap();
        assert_eq!(annotations.len(), expected);
        assert_eq!(annotations[0]["anchors"][0]["slot"], "1", "the earlier bar");
        assert_eq!(annotations[0]["anchors"][0]["price"], prices[0]);
        let placed = &app.active_tab().drawing_pane().drawings;
        assert_eq!(placed.items().len(), expected);
        assert!(
            placed
                .items()
                .iter()
                .all(|d| d.tool.id() == "horizontal-line")
        );
    }
}

#[test]
fn each_shape_capability_places_its_own_tool() {
    for (capability, version, tool, count) in [
        (crate::control::RECTANGLE_CAPABILITY_ID, 1, "rectangle", 2),
        (crate::control::TREND_LINE_CAPABILITY_ID, 1, "trend-line", 2),
        (
            crate::control::PARALLEL_CHANNEL_CAPABILITY_ID,
            1,
            "parallel-channel",
            3,
        ),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(8);
        run_frame(&mut app, &ctx);
        let mut anchors = vec![anchor_at_slot(&app, 1), anchor_at_slot(&app, 5)];
        anchors.extend((count == 3).then(|| anchor_at_slot(&app, 3)));
        for (anchor, bar) in anchors.iter_mut().zip(["1.5", "5.5", "3.5"]) {
            anchor["bar_position"] = json!(bar);
        }
        let result = app
            .control_action(
                capability,
                version,
                crate::control::ActionOrigin::Human,
                json!({ "anchors": anchors }),
            )
            .unwrap();
        assert_eq!(result["tool_id"], tool, "{capability}");
        let placed = app.active_tab().drawing_pane().drawings.items();
        assert_eq!(placed.len(), 1);
        assert_eq!(placed[0].points.len(), count);
    }
}

#[test]
fn horizontal_levels_journal_one_created_event_per_line() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    let mut anchors = [anchor_at_slot(&app, 1), anchor_at_slot(&app, 5)];
    for (anchor, price) in anchors.iter_mut().zip(["101", "99"]) {
        anchor["price"] = json!(price);
    }
    let result = app
        .control_action(
            crate::control::HORIZONTAL_LEVELS_CAPABILITY_ID,
            1,
            crate::control::ActionOrigin::Human,
            json!({ "anchors": anchors }),
        )
        .unwrap();
    let placed: Vec<_> = result["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|annotation| annotation["annotation_id"].clone())
        .collect();
    assert_eq!(placed.len(), 2);
    let journaled: Vec<_> = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 64, 1 << 20)
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "annotate.object.created")
        .map(|event| {
            let annotation = &event.payload["annotation"];
            assert_eq!(annotation["tool_id"], "horizontal-line");
            assert!(
                annotation.get("annotations").is_none(),
                "one line, not a set"
            );
            annotation["annotation_id"].clone()
        })
        .collect();
    assert_eq!(
        journaled, placed,
        "one standard event per line, in placement order"
    );
}

#[test]
fn a_failed_later_line_takes_back_the_lines_already_placed() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    let tool = crate::drawings::DrawingTool::by_id("horizontal-line").unwrap();
    let mut fresh = Some(app.tab_reads().new_drawing(tool));
    let pane = app.active_tab_mut().drawing_pane_mut();
    let depth = pane.drawings.undo_depth();
    let refused = crate::control::install_all(pane, [true, false], |pane, places| {
        if !places {
            return Err(quantick_control::error::ControlError::invalid_request(
                "the second line fails",
            ));
        }
        let point = crate::drawings::ChartPoint::at_time(1.5, 100.0, None);
        let band = crate::drawings::DrawingBand::Price;
        assert!(
            pane.drawings
                .place_with(tool, &band, point, |_| fresh.take().unwrap())
        );
        Ok((pane.drawings.items().last().unwrap().id.0, String::new()))
    });
    assert!(refused.is_err());
    let pane = app.active_tab().drawing_pane();
    assert!(pane.drawings.items().is_empty(), "no orphan first line");
    assert_eq!(
        pane.drawings.undo_depth(),
        depth,
        "and no undo step that brings it back"
    );
}
