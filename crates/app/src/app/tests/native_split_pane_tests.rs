//! One flow pane: tick candles left of the divider, the WIN tape right of it,
//! one price axis the tape fits. Switched off, the original tick chart.
use super::*;
use quantick_layers::{LayerFacts, LayerState};

/// The WIN source declaration, as a feed switch applies it.
fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_source_preset(Some("mini index regions"))
    );
    let config = app.active_tab().tape().cached_config();
    assert!(config.native_tape() && !config.tape_only());
}

fn facts(app: &QuantickApp) -> LayerFacts {
    let capabilities = app.active_tab().capabilities(&app.config);
    app.active_tab().flow_pane.layer_facts(Some(capabilities))
}

#[test]
fn the_native_tape_sits_beside_the_candles_behind_a_draggable_divider() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let divider = pane
        .frame
        .lane_divider_x
        .expect("a divider between the candles and the tape");
    let lane = app.active_tab().tape().lane_width_px(chart.width());
    assert!((chart.right() - divider - lane).abs() < 1.0);
    assert!(
        divider - chart.left() >= chart.width() * 0.5,
        "the candles keep their side of the pane"
    );
    let facts = facts(&app);
    assert!(facts.native_tape && !facts.tape_only);
    for layer in [
        ChartLayer::Footprint,
        ChartLayer::Drawings,
        ChartLayer::TradePaint,
        ChartLayer::CandleAggression,
    ] {
        assert!(
            LayerState::blocked(layer, facts).is_none_or(|block| !block.code.contains("tape")),
            "{layer:?} still paints left of the divider"
        );
    }
    assert_eq!(
        LayerState::blocked(ChartLayer::Bubbles, facts).map(|block| block.code),
        Some("candle_bubbles_replaced_by_native_tape")
    );

    // The divider is laid out by the first frame's draw, after that frame's
    // gestures ran, so its handle is first registered by the second frame. A
    // press is hit-tested against the handles of the frame before it: one
    // frame with the handle on screen is what a trader always has.
    run_frame(&mut app, &ctx);
    let y = chart.center().y;
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        egui::pos2(divider, y),
        egui::pos2(divider - 100.0, y),
    );
    run_frame(&mut app, &ctx);
    let wider = app.active_tab().tape().lane_width_px(chart.width());
    assert!(
        (wider - (lane + 100.0)).abs() < 2.0,
        "dragging the divider left gives the tape more room: {lane} -> {wider}"
    );
    assert!(app.active_tab().tape().cached_config().native_tape());
}

/// A drag over the candles pans them through history and never touches the
/// axis the tape owns; the tape off, the same drag pans prices again.
#[test]
fn dragging_the_candles_pans_them_and_never_the_tapes_axis() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    run_frame(&mut app, &ctx);
    let (chart, divider, auto, total) = {
        let pane = &app.active_tab().flow_pane;
        let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
        (
            pane.frame.chart_rect.unwrap(),
            pane.frame.lane_divider_x.unwrap(),
            pane.frame.auto_range,
            total,
        )
    };
    let tape_range = app.active_tab().tape().tape_price_range();
    let start = egui::pos2((chart.left() + divider) / 2.0, chart.center().y);
    let end = start + egui::vec2(180.0, 90.0);
    let edge = app.active_tab().flow_pane.viewport.right_edge_bar(total);
    drag_sized(&mut app, &ctx, TEST_WINDOW, start, end);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(
        pane.viewport.right_edge_bar(total) < edge,
        "the candles moved back through history"
    );
    assert!(pane.price_view.is_auto(), "the tape still owns the axis");
    assert_eq!(pane.frame.auto_range, auto, "the axis did not move");
    assert_eq!(app.active_tab().tape().tape_price_range(), tape_range);

    // Tape off: the original tick chart, candle fit and original gestures.
    switch_layer(&mut app, ChartLayer::TapeChart, false);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().flow_pane.frame.lane_divider_x.is_none());
    assert!(!facts(&app).native_tape);
    drag_sized(&mut app, &ctx, TEST_WINDOW, start, end);
    run_frame(&mut app, &ctx);
    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "without the tape a vertical drag pans prices, as it always did"
    );
}

/// The native tape advances on the market clock beside the candles, as the
/// tape-only pane's did; an ordinary lane still never receives it.
#[test]
fn the_native_split_tape_runs_on_the_market_clock() {
    let (mut app, _events, _commands, _book) = test_app();
    native_split(&mut app);
    let print = trade(1);
    app.active_tab_mut().ingest_live_trade_at(&print, 10_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    let first = app.active_tab().tape().lane_now_ms();
    assert!(first.is_some(), "the native tape has a clock");
    app.active_tab_mut().update_tape_clock_at(12_000);
    assert!(app.active_tab().tape().lane_now_ms() > first);
}

/// Each mode starts at its own fit. A manual Y set beside the candles
/// belongs to the split; entering tape only from it starts the full tape at
/// its own fit, as entering it from the ordinary lane always did.
#[test]
fn entering_tape_only_from_the_split_starts_at_the_tapes_own_fit() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    run_frame(&mut app, &ctx);
    let auto = app
        .active_tab()
        .flow_pane
        .frame
        .auto_range
        .expect("the split fits its axis");
    app.active_tab_mut().flow_pane.price_view.pan(25.0, auto);
    run_frame(&mut app, &ctx);
    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "the split keeps the trader's own Y"
    );
    switch_layer(&mut app, ChartLayer::TapeOnly, true);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().flow_pane.price_view.is_auto(),
        "tape only opens at the tape's own fit"
    );
}
