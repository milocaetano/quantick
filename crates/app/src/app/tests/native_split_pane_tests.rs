//! One flow pane: tick candles left of the divider, the WIN tape right of it,
//! one price axis the tape fits. Switched off, the original tick chart.
use super::*;
use quantick_layers::{LayerFacts, LayerState};

/// The WIN source declaration, as a feed switch applies it.
fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions")
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
        ChartLayer::Bubbles,
        ChartLayer::CandleAggression,
    ] {
        assert!(
            LayerState::blocked(layer, facts).is_none_or(|block| !block.code.contains("tape")),
            "{layer:?} still paints left of the divider"
        );
    }

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

/// A drag over the candles pans them through history and, up and down, the
/// shared price axis (trader 2026-09-29: up and down back, as before); it
/// never moves the tape's time. The tape off, the original chart.
#[test]
fn dragging_the_candles_pans_them_and_the_shared_axis_but_never_the_tapes_time() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    run_frame(&mut app, &ctx);
    let (chart, divider, total) = {
        let pane = &app.active_tab().flow_pane;
        let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
        (
            pane.frame.chart_rect.unwrap(),
            pane.frame.lane_divider_x.unwrap(),
            total,
        )
    };
    let window = app.active_tab().tape().live_lane_window();
    let start = egui::pos2((chart.left() + divider) / 2.0, chart.center().y);
    let end = start + egui::vec2(180.0, 90.0);
    let edge = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(total);
    drag_sized(&mut app, &ctx, TEST_WINDOW, start, end);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(
        pane.model.viewport.right_edge_bar(total) < edge,
        "the candles moved back through history"
    );
    assert!(
        !pane.price_view.is_auto(),
        "the shared axis panned: manual Y"
    );
    let tape = app.active_tab().tape();
    assert!(tape.tape_end().is_live(), "the tape's time did not move");
    assert_eq!(tape.live_lane_window(), window);

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

/// Gutter travel per frame in [`flip_travel`]: a steady hand, not a flick.
const DRAG_STEP_PX: f32 = 5.0;

/// The gutter travel main needs to flip from auto-fit:
/// `AXIS_ZOOM_DRAG_PX · ln(FLIP_SPAN_FACTOR · FLIP_REARM_FRACTION)`, about
/// 546px, where the bars are flat enough to turn over.
fn main_flip_travel_px() -> f32 {
    use quantick_chart::price_view::{FLIP_REARM_FRACTION, FLIP_SPAN_FACTOR};
    (150.0 * (FLIP_SPAN_FACTOR * FLIP_REARM_FRACTION).ln()) as f32
}

/// One continuous downward drag on the price gutter from auto-fit, the
/// market going quiet a third of the way in — a burst at one price, two
/// minutes on, so the prints the tape held age out of its window. Returns
/// the travel at which the chart turned over, `None` if 2000px never did.
fn flip_travel(app: &mut QuantickApp) -> Option<f32> {
    let ctx = &egui::Context::default();
    for agg_id in 201..=400 {
        app.active_tab_mut()
            .ingest_live_trade_at(&trade(agg_id), 10_000 + agg_id as i64);
    }
    run_frame(app, ctx);
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(app, ctx);
    run_frame(app, ctx);
    assert!(app.active_tab().flow_pane.price_view.is_auto());
    let gutter = app
        .active_tab()
        .flow_pane
        .frame
        .price_gutter
        .expect("the draw published the gutter");
    let start = gutter.center();
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    let mut travel = 0.0;
    while travel < 2000.0 {
        travel += DRAG_STEP_PX;
        if travel == 180.0 {
            for agg_id in 401..=440 {
                let mut print = trade(agg_id);
                print.timestamp_ms += 120_000;
                print.price = Decimal::from(101);
                app.active_tab_mut()
                    .ingest_live_trade_at(&print, 130_000 + agg_id as i64);
            }
            app.active_tab_mut().tape_mut().flush_for_test();
        }
        let at = start + egui::vec2(0.0, travel);
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)]);
        if app.active_tab().flow_pane.price_view.is_inverted() {
            return Some(travel);
        }
    }
    None
}

/// The flip is a strong squeeze beside the tape exactly as without it
/// (trader 2026-09-30: dragging the axis turned the chart over far too
/// soon). The tape fits the shared axis to a few recent prints; when they
/// narrow, that fit shrinks under the drag, and a threshold measured against
/// it arrived early: 450px here, almost at once on an index tape whose one
/// quiet price fits a single point. With the tape on and off, the drag from
/// auto-fit flips only after main's travel.
#[test]
fn an_expanding_gutter_drag_flips_only_after_mains_travel_with_the_tape_on_or_off() {
    let main = main_flip_travel_px();

    let (mut app, _commands) = app_with_history(200);
    let off = flip_travel(&mut app).expect("the tape off, the drag flips");
    assert!(
        off >= main && off <= main + 3.0 * DRAG_STEP_PX,
        "the tape off flips at main's travel: {off}px, main {main}px"
    );

    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    let on = flip_travel(&mut app).expect("beside the tape, the drag still flips");
    assert!(
        on >= main,
        "beside the tape the flip waits for main's travel: {on}px, main {main}px"
    );
}
