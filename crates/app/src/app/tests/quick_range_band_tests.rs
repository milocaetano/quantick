//! The secondary-button range on an indicator pane: it measures in the
//! pane's own values, its drawings land on that pane, and a press on a
//! divider or a drag into the candles never crosses bands.

use super::bare_canvas::app_with_history as bare_app_with_history;
use super::drawings_tests::{click_quick_range_action, drag_quick_range, quick_range_action};
use super::*;
use crate::surfaces::drawing_chrome::QuickRangeAction as Action;

/// A CVD-like pane whose values (3,000 to 3,398) are nowhere near the
/// candles' prices, so a value read on the wrong axis cannot pass.
const CVD_FIRST: f64 = 3_000.0;
const CVD_LAST: f64 = 3_398.0;

struct Pane {
    ctx: egui::Context,
    app: QuantickApp,
    _commands: mpsc::Receiver<FeedCommand>,
}

fn cvd_pane() -> Pane {
    let ctx = egui::Context::default();
    let (mut app, commands) = bare_app_with_history(200);
    add_pane_indicator(
        &mut app,
        "CVD",
        (0..200).map(|i| CVD_FIRST + f64::from(i) * 2.0).collect(),
    );
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    Pane {
        ctx,
        app,
        _commands: commands,
    }
}

fn x_at(app: &QuantickApp, bar: f32) -> f32 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the chart was laid out");
    let history_right = pane.frame.lane_divider_x.unwrap_or(chart.right());
    pane.model
        .viewport
        .x_at_bar_position(bar, history_right, pane.slots())
}

/// Two points inside the CVD pane's body, clear of its header and divider.
fn cvd_leg(app: &QuantickApp) -> (egui::Pos2, egui::Pos2) {
    let body = pane_body(app, 0);
    (
        egui::pos2(x_at(app, 80.5), body.center().y + 12.0),
        egui::pos2(x_at(app, 160.5), body.center().y - 12.0),
    )
}

fn on_cvd(band: Option<&drawings::DrawingBand>) -> bool {
    matches!(band, Some(drawings::DrawingBand::Indicator(_)))
}

fn cvd_value(price: f64) -> bool {
    (CVD_FIRST - 400.0..=CVD_LAST + 400.0).contains(&price)
}

#[test]
fn a_right_drag_on_the_cvd_pane_measures_its_values_and_places_there() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, end) = cvd_leg(&app);
    drag_quick_range(&mut app, &ctx, start, end);
    let quick = &app.drawings.chrome.quick_range;
    assert!(on_cvd(quick.band()), "the range is the CVD pane's");
    let leg = quick.paint_anchors().expect("a settled range");
    assert!(
        leg.iter().all(|point| cvd_value(point.price)),
        "measured in CVD values, not prices: {leg:?}"
    );
    run_frame(&mut app, &ctx);
    let profile = quick_range_action(&app, Action::Profile);
    assert!(!profile.enabled, "a volume profile is price");
    assert_eq!(profile.unavailable_reason, Some("range_band_price_only"));
    assert!(quick_range_action(&app, Action::TrendLine).enabled);

    click_quick_range_action(&mut app, &ctx, Action::TrendLine);
    let placed = app.active_tab().flow_pane.drawings.items();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].tool.id(), "trend-line");
    assert!(on_cvd(Some(&placed[0].band)), "lands on the CVD pane");
    for (got, want) in placed[0].points.iter().zip(&leg) {
        assert_eq!(got.bar, want.bar);
        assert!((got.price - want.price).abs() < 1e-8, "{got:?} vs {want:?}");
    }
}

#[test]
fn the_channel_seeds_its_draft_on_the_cvd_pane() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, end) = cvd_leg(&app);
    drag_quick_range(&mut app, &ctx, start, end);
    click_quick_range_action(&mut app, &ctx, Action::Channel);
    run_frame(&mut app, &ctx);
    let draft = app
        .active_tab()
        .flow_pane
        .drawings
        .draft()
        .expect("the leg is a pending channel");
    assert!(
        on_cvd(Some(&draft.band)),
        "the width click finishes it there"
    );
}

#[test]
fn a_drag_into_the_candles_stays_clamped_to_the_cvd_pane() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, _) = cvd_leg(&app);
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the chart was laid out");
    let into_candles = egui::pos2(x_at(&app, 160.5), chart.center().y);
    let edge = right_edge(&app);
    drag_quick_range(&mut app, &ctx, start, into_candles);
    assert_eq!(right_edge(&app), edge, "measuring the pane does not pan it");
    let quick = &app.drawings.chrome.quick_range;
    assert!(on_cvd(quick.band()), "a range never changes band");
    let leg = quick.paint_anchors().expect("a settled range");
    assert!(
        leg.iter().all(|point| cvd_value(point.price)),
        "the far end reads the CVD pane's top, not a price: {leg:?}"
    );
}

#[test]
fn a_right_drag_from_the_pane_divider_raises_no_range() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let body = pane_body(&app, 0);
    let (_, end) = cvd_leg(&app);
    // Inside the divider's grab tolerance, on the pane's side of it.
    let divider = egui::pos2(x_at(&app, 80.5), body.top() + 2.0);
    drag_quick_range(&mut app, &ctx, divider, end);
    assert!(app.drawings.chrome.quick_range.band().is_none());
    assert!(app.drawings.chrome.quick_range.paint_anchors().is_none());
}

#[test]
fn a_call_names_the_cvd_band_and_one_the_pane_lacks_is_refused() {
    let Pane {
        mut app, _commands, ..
    } = cvd_pane();
    let mut first = anchor_at_slot(&app, 10);
    let mut last = anchor_at_slot(&app, 40);
    first["bar_position"] = serde_json::json!("10.5");
    last["bar_position"] = serde_json::json!("40.5");
    first["price"] = serde_json::json!("3020");
    last["price"] = serde_json::json!("3080");
    let input = |kind: &str| {
        serde_json::json!({
            "anchors": [first, last],
            "band": {"indicator": {"kind": kind, "ordinal": 0}},
        })
    };
    let missing = input("native.cvd");
    let error = app
        .control_action(
            crate::control::TREND_LINE_CAPABILITY_ID,
            1,
            crate::control::ActionOrigin::Human,
            missing,
        )
        .expect_err("this pane shows no native.cvd");
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
    let shown = input("test.indicator");
    let result = app
        .control_action(
            crate::control::TREND_LINE_CAPABILITY_ID,
            1,
            crate::control::ActionOrigin::Human,
            shown,
        )
        .expect("the pane shows that indicator");
    assert_eq!(result["band"]["indicator"]["kind"], "test.indicator");
    let placed = &app.active_tab().flow_pane.drawings.items()[0];
    assert!(on_cvd(Some(&placed.band)));
    assert_eq!(placed.points[0].price, 3_020.0);
}

fn cvd_slot(app: &QuantickApp) -> crate::indicator_worker::SlotId {
    app.active_tab().flow_pane.indicators.all()[0].slot
}

fn cvd_view(app: &mut QuantickApp) -> &mut quantick_chart::indicator_views::IndicatorView {
    let slot = cvd_slot(app);
    app.active_tab_mut()
        .flow_pane
        .indicators
        .view_mut(slot)
        .expect("the CVD view")
}

#[test]
fn a_range_goes_with_its_hidden_pane_and_escape_reaches_the_next_layer() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, end) = cvd_leg(&app);
    drag_quick_range(&mut app, &ctx, start, end);
    assert!(on_cvd(app.drawings.chrome.quick_range.band()));
    cvd_view(&mut app).hidden = true;
    run_frame(&mut app, &ctx);
    assert!(
        app.drawings.chrome.quick_range.band().is_none(),
        "the range went with its pane"
    );
    app.toolrail.arm(Tool::Crosshair);
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert_eq!(
        app.toolrail.tool(),
        Tool::Pointer,
        "Escape is not swallowed by a range nobody can see"
    );
    cvd_view(&mut app).hidden = false;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.drawings.chrome.quick_range.paint_anchors().is_none(),
        "the range does not come back with the pane"
    );
}

#[test]
fn a_range_goes_when_its_pane_collapses() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, end) = cvd_leg(&app);
    drag_quick_range(&mut app, &ctx, start, end);
    cvd_view(&mut app).sizing = crate::indicators::PaneSizing::Collapsed;
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(app.drawings.chrome.quick_range.band().is_none());
}

#[test]
fn a_right_drag_at_the_bottom_of_the_candles_measures_price_as_on_main() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let divider = pane_body(&app, 0).top();
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the chart was laid out");
    // Inside the divider's grab tolerance, but on the candles.
    let start = egui::pos2(x_at(&app, 80.5), divider - 2.0);
    let end = egui::pos2(x_at(&app, 160.5), chart.center().y);
    drag_quick_range(&mut app, &ctx, start, end);
    assert_eq!(
        app.drawings.chrome.quick_range.band(),
        Some(&drawings::DrawingBand::Price)
    );
    assert!(app.drawings.chrome.quick_range.paint_anchors().is_some());
}

#[test]
fn measuring_the_cvd_pane_shows_no_pan_cursor() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    let (start, end) = cvd_leg(&app);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(start),
            super::drawings_tests::secondary_button(start, true),
        ],
    );
    let moving = run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    assert!(
        !matches!(
            moving.platform_output.cursor_icon,
            egui::CursorIcon::Grab | egui::CursorIcon::Grabbing
        ),
        "{:?}",
        moving.platform_output.cursor_icon
    );
}

#[test]
fn a_call_on_a_collapsed_cvd_pane_is_refused_as_the_hand_is() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    cvd_view(&mut app).sizing = crate::indicators::PaneSizing::Collapsed;
    run_frame(&mut app, &ctx);
    let mut first = anchor_at_slot(&app, 10);
    let mut last = anchor_at_slot(&app, 40);
    first["bar_position"] = serde_json::json!("10.5");
    last["bar_position"] = serde_json::json!("40.5");
    first["price"] = serde_json::json!("3020");
    last["price"] = serde_json::json!("3080");
    let error = app
        .control_action(
            crate::control::TREND_LINE_CAPABILITY_ID,
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "anchors": [first, last],
                "band": {"indicator": {"kind": "test.indicator", "ordinal": 0}},
            }),
        )
        .expect_err("a collapsed strip takes no anchor");
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE
    );
    assert!(error.message.contains(crate::bands::COLLAPSED_BAND_REFUSAL));
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());
}

/// The harness hook's value, staged the way a launch stages it.
fn stage(app: &mut QuantickApp, ctx: &egui::Context, value: &str) {
    let launch = crate::surfaces::drawing_chrome::QuickRangeLaunch::capture(|_| Some(value.into()));
    let quick = &mut app.drawings.chrome.quick_range;
    quick.queue_launch(launch);
    quick.apply_launch();
    run_frame(app, ctx);
    run_frame(app, ctx);
}

#[test]
fn the_demo_hook_stages_a_ready_range_on_the_named_indicator_pane() {
    let Pane {
        ctx,
        mut app,
        _commands,
    } = cvd_pane();
    stage(&mut app, &ctx, "ready@test.indicator");
    let quick = &app.drawings.chrome.quick_range;
    assert!(on_cvd(quick.band()), "staged on the CVD pane");
    let leg = quick.paint_anchors().expect("a staged range");
    assert!(
        leg.iter().all(|point| cvd_value(point.price)),
        "anchors read from the pane's own scale: {leg:?}"
    );
    assert!(quick_range_action(&app, Action::TrendLine).enabled);
}

#[test]
fn the_demo_hook_stages_nothing_on_a_pane_the_chart_lacks_or_collapses() {
    for (value, collapse) in [
        ("ready@native.cvd", false),
        ("ready@test.indicator:1", false),
        ("ready@test.indicator", true),
    ] {
        let Pane {
            ctx,
            mut app,
            _commands,
        } = cvd_pane();
        if collapse {
            cvd_view(&mut app).sizing = crate::indicators::PaneSizing::Collapsed;
            run_frame(&mut app, &ctx);
        }
        stage(&mut app, &ctx, value);
        assert!(app.drawings.chrome.quick_range.band().is_none(), "{value}");
        if collapse {
            // The ask was dropped, not parked until the pane opens again.
            cvd_view(&mut app).sizing = crate::indicators::PaneSizing::Auto;
            run_frame(&mut app, &ctx);
            run_frame(&mut app, &ctx);
            assert!(app.drawings.chrome.quick_range.band().is_none());
        }
    }
}
