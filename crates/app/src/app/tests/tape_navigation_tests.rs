//! The tape beside the candles moves like the tick chart: the wheel over it
//! zooms its window, a drag pans its price axis and its time, a double click
//! brings it back to live — and none of it moves the candles. The same moves
//! are one named call away, and read back.
use super::*;
use quantick_orderflow::tape_view::TapeEnd;

#[path = "canvas_click_tests.rs"]
mod canvas_click;

const CAPABILITY: &str = "chart.tape_view.set";
/// The first print the fixture's tape retains: `trade(201)`.
const RETAINED_FROM: i64 = 21_100;

fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions", false)
    );
    let config = app.active_tab().tape().cached_config();
    assert!(config.native_tape() && !config.tape_only());
}

/// A native split with its divider laid out, and a point on each side of it.
fn split_app(ctx: &egui::Context) -> (QuantickApp, egui::Pos2, egui::Pos2) {
    let (mut app, _commands) = app_with_history(200);
    native_split(&mut app);
    // The backfill sized the capture grid, and that reset took its prints:
    // the tape retains what arrives from here, 21_100 onwards.
    for agg_id in 201..=400 {
        app.active_tab_mut()
            .ingest_live_trade_at(&trade(agg_id), 10_000 + agg_id as i64);
    }
    run_frame(&mut app, ctx);
    // The worker publishes what it retains; wait for it, then read it.
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, ctx);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let divider = pane.frame.lane_divider_x.expect("the divider");
    let y = chart.center().y;
    let tape = egui::pos2((divider + chart.right()) / 2.0, y);
    let candles = egui::pos2((chart.left() + divider) / 2.0, y);
    (app, tape, candles)
}

/// What the candles show: their zoom and where their right edge is.
fn candles_view(app: &QuantickApp) -> (f32, f32) {
    let pane = &app.active_tab().flow_pane;
    let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
    (
        pane.model.viewport.px_per_bar(),
        pane.model.viewport.right_edge_bar(total),
    )
}

fn wheel(app: &mut QuantickApp, ctx: &egui::Context, at: egui::Pos2) {
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 80.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    run_frame(app, ctx);
}

#[test]
fn the_wheel_over_the_tape_zooms_its_window_and_over_the_candles_the_candles() {
    let ctx = egui::Context::default();
    let (mut app, tape, candles) = split_app(&ctx);
    let window = app.active_tab().tape().live_lane_window();
    let before = candles_view(&app);
    wheel(&mut app, &ctx, tape);
    let zoomed = app.active_tab().tape().live_lane_window();
    assert_ne!(zoomed, window, "the wheel over the tape zooms its window");
    assert_eq!(candles_view(&app), before, "and leaves the candles alone");

    wheel(&mut app, &ctx, candles);
    assert_eq!(
        app.active_tab().tape().live_lane_window(),
        zoomed,
        "the wheel over the candles never zooms the tape"
    );
    assert!(
        candles_view(&app).0 > before.0,
        "it zooms the candles, as it always did"
    );
}

#[test]
fn a_vertical_drag_over_the_tape_takes_manual_y_and_the_candles_follow_the_axis() {
    let ctx = egui::Context::default();
    let (mut app, tape, _) = split_app(&ctx);
    let before = candles_view(&app);
    assert!(app.active_tab().flow_pane.price_view.is_auto());
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        tape,
        tape + egui::vec2(0.0, 90.0),
    );
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(!pane.price_view.is_auto(), "the shared axis is manual now");
    assert_eq!(candles_view(&app), before, "the candles did not pan");
    assert_eq!(app.active_tab().tape().tape_end(), TapeEnd::Live);
}

/// Up and down over the candles pans the shared axis too — primary or
/// middle button, one path with the tape's — and never the tape's time.
#[test]
fn a_vertical_drag_over_the_candles_pans_the_shared_axis_and_never_the_tape() {
    let ctx = egui::Context::default();
    let (mut app, _, candles) = split_app(&ctx);
    let (before, window) = (
        candles_view(&app),
        app.active_tab().tape().live_lane_window(),
    );
    let auto = app.active_tab().flow_pane.frame.auto_range.expect("fitted");
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        candles,
        candles + egui::vec2(0.0, 90.0),
    );
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(!pane.price_view.is_auto(), "manual Y from the candles");
    let primary = pane.price_view.resolve(auto);
    assert_eq!(candles_view(&app), before, "a vertical drag pans no bars");
    let tape = app.active_tab().tape();
    assert_eq!(tape.tape_end(), TapeEnd::Live);
    assert_eq!(tape.live_lane_window(), window);

    let middle = |position: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let end = candles + egui::vec2(0.0, 60.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(candles), middle(candles, true)],
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(end)]);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![egui::Event::PointerMoved(end), middle(end, false)],
    );
    let pane = &app.active_tab().flow_pane;
    assert_ne!(
        pane.price_view.resolve(auto),
        primary,
        "the middle button pans Y"
    );
    assert_eq!(candles_view(&app), before);
    assert_eq!(app.active_tab().tape().tape_end(), TapeEnd::Live);
}

/// A horizontal drag moves the tape back in time and leaves the candles
/// where they were; dragging it back past now re-pins it to live, and a
/// double click over the tape returns it to live without changing framing.
#[test]
fn a_horizontal_drag_pans_the_tape_through_time_and_never_the_candles() {
    let ctx = egui::Context::default();
    let (mut app, tape, candles) = split_app(&ctx);
    let before = candles_view(&app);
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        tape,
        tape + egui::vec2(120.0, 0.0),
    );
    run_frame(&mut app, &ctx);
    let end = app.active_tab().tape().tape_end();
    let TapeEnd::Past { end_ms } = end else {
        panic!("the drag took the tape into the past: {end:?}");
    };
    assert!(end_ms < 41_000, "{end_ms}");
    assert_eq!(candles_view(&app), before, "the candles stayed put");
    assert!(app.active_tab().flow_pane.price_view.is_auto());

    // The candles' own drag never moves the tape back.
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        candles,
        candles + egui::vec2(150.0, 0.0),
    );
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().tape().tape_end(), end);
    assert_ne!(candles_view(&app), before, "the candles panned");

    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        tape,
        tape - egui::vec2(400.0, 0.0),
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().tape().tape_end(),
        TapeEnd::Live,
        "dragged past now, the tape pins to live"
    );

    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        tape,
        tape + egui::vec2(120.0, 60.0),
    );
    run_frame(&mut app, &ctx);
    assert!(!app.active_tab().tape().tape_end().is_live());
    assert!(!app.active_tab().flow_pane.price_view.is_auto());
    let manual = app.active_tab().flow_pane.price_view.manual_range();
    let candles_before = candles_view(&app);
    let window = app.active_tab().tape().live_lane_window();
    click_sized(&mut app, &ctx, TEST_WINDOW, tape);
    click_sized(&mut app, &ctx, TEST_WINDOW, tape);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().tape().tape_end(), TapeEnd::Live);
    assert_eq!(app.active_tab().flow_pane.price_view.manual_range(), manual);
    assert_eq!(candles_view(&app), candles_before);
    assert_eq!(app.active_tab().tape().live_lane_window(), window);
}

/// A press and release of the middle button at `from` and `to`, moving
/// between them in one frame.
fn middle_drag(app: &mut QuantickApp, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    let middle = |position: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(from), middle(from, true)],
    );
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(to)]);
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(to), middle(to, false)],
    );
    run_frame(app, ctx);
}

/// The middle button pans the side it was pressed on, as the primary drag
/// does: a pan pressed on the candles that crosses the divider never moves
/// the tape, and one pressed on the tape never moves the candles.
#[test]
fn a_middle_drag_pans_the_side_it_was_pressed_on() {
    let ctx = egui::Context::default();
    let (mut app, tape, candles) = split_app(&ctx);
    let before = candles_view(&app);
    middle_drag(&mut app, &ctx, candles, tape);
    assert_eq!(
        app.active_tab().tape().tape_end(),
        TapeEnd::Live,
        "pressed on the candles, the pan never moves the tape"
    );
    let panned = candles_view(&app);
    assert_ne!(panned, before, "it pans the candles");

    middle_drag(&mut app, &ctx, tape, candles);
    assert_eq!(
        candles_view(&app),
        panned,
        "pressed on the tape, the pan never moves the candles"
    );
}

/// Tape only is the tape across the whole canvas: a double click anywhere
/// on it returns a held tape to live without changing price framing, as a
/// double click over the tape beside the candles does.
#[test]
fn a_double_click_in_tape_only_returns_the_tape_to_live() {
    let ctx = egui::Context::default();
    let (mut app, tape, _) = split_app(&ctx);
    drag_sized(
        &mut app,
        &ctx,
        TEST_WINDOW,
        tape,
        tape + egui::vec2(120.0, 0.0),
    );
    run_frame(&mut app, &ctx);
    let held = app.active_tab().tape().tape_end();
    assert!(!held.is_live(), "the drag held the tape in the past");
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        quantick_layers::OrderflowSwitch::TapeOnly,
        true,
    );
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().tape().cached_config().tape_only());
    assert_eq!(app.active_tab().tape().tape_end(), held);
    let pane = &mut app.active_tab_mut().flow_pane;
    let auto = pane.frame.auto_range.expect("fitted");
    pane.price_view.pan_screen(40.0, 0.5, auto);
    assert!(!pane.price_view.is_auto());
    let manual = pane.price_view.manual_range();
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let at = chart.center() - egui::vec2(chart.width() / 4.0, 0.0);
    let window = app.active_tab().tape().live_lane_window();
    let candles_before = candles_view(&app);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().tape().tape_end(),
        TapeEnd::Live,
        "the double click returned the tape to live"
    );
    assert_eq!(app.active_tab().flow_pane.price_view.manual_range(), manual);
    assert_eq!(app.active_tab().tape().live_lane_window(), window);
    assert_eq!(candles_view(&app), candles_before);
}

/// A past end stops where the retained tape begins, never before it.
#[test]
fn a_drag_far_into_the_past_stops_at_the_retained_tape() {
    let ctx = egui::Context::default();
    let (mut app, tape, _) = split_app(&ctx);
    for _ in 0..6 {
        drag_sized(
            &mut app,
            &ctx,
            TEST_WINDOW,
            tape,
            tape + egui::vec2(400.0, 0.0),
        );
        run_frame(&mut app, &ctx);
    }
    let view = app.active_tab().tape();
    let window = view.live_lane_window_ms(app.active_tab().flow_pane.state.bars());
    let retained = view
        .tape_retained_from_ms()
        .expect("the tape reports retention");
    assert_eq!(retained, RETAINED_FROM, "the first retained print");
    assert_eq!(
        view.tape_end(),
        TapeEnd::Past {
            end_ms: retained + window / 2
        }
    );
}

fn input(app: &QuantickApp, fields: Value) -> Value {
    let mut payload = json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
    });
    for (key, value) in fields.as_object().unwrap() {
        payload[key] = value.clone();
    }
    payload
}

fn read_tape(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({"scopes":["chart.summary"]}),
    );
    success_result(&response)["scopes"]["chart.summary"]["value"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["pane_id"] == pane_id)
        .unwrap()["viewport"]["tape"]
        .clone()
}

#[test]
fn the_tape_view_call_moves_the_tape_end_and_window_and_reads_them_back() {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let before = candles_view(&app);
    let directory = gateway_test_directory("tape-view-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let live = read_tape(&mut app, &mut client);
    assert_eq!(live["follows_live"], true);
    assert_eq!(live["retained_from_unix_ms"], RETAINED_FROM);

    let payload = input(
        &app,
        json!({
            "end": {"kind":"past","end_unix_ms":35_000},
            "window": {"kind":"fixed","ms":3_000},
        }),
    );
    let (response, _) = keyed_call(
        &mut app,
        &mut client,
        "past",
        CAPABILITY,
        payload.clone(),
        "tape-past",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut client,
        "past-retry",
        CAPABILITY,
        payload,
        "tape-past",
    );
    assert_eq!(response.outcome, retry.outcome);
    let result = success_result(&response);
    assert_eq!(result["tape"]["follows_live"], false);
    assert_eq!(result["tape"]["end_unix_ms"], 35_000);
    assert_eq!(result["tape"]["window"]["fixed_ms"], 3_000);
    assert_eq!(result["tape"]["window_ms"], 3_000);
    run_frame(&mut app, &ctx);
    assert_eq!(read_tape(&mut app, &mut client), result["tape"]);
    assert_eq!(
        app.active_tab().tape().tape_end(),
        TapeEnd::Past { end_ms: 35_000 }
    );
    assert_eq!(
        candles_view(&app),
        before,
        "the call never moves the candles"
    );

    // Asked for an end before the retained tape, it reports where it held.
    let early = input(&app, json!({"end": {"kind":"past","end_unix_ms":0}}));
    let (held, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, early);
    assert_eq!(
        success_result(&held)["tape"]["end_unix_ms"],
        RETAINED_FROM + 1_500
    );

    let back = input(&app, json!({"end": {"kind":"live"}}));
    let (live, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, back);
    assert_eq!(success_result(&live)["tape"]["follows_live"], true);
    run_frame(&mut app, &ctx);
    assert_eq!(read_tape(&mut app, &mut client)["follows_live"], true);

    for fields in [
        json!({"end": {"kind":"past"}}),
        json!({"end": {"kind":"sometime"}}),
        json!({"window": {"kind":"fixed","ms":0}}),
        json!({"window": {"kind":"fixed","ms":-5}}),
        json!({}),
        json!({"end": {"kind":"live"}, "file": true}),
    ] {
        let payload = input(&app, fields);
        let (invalid, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, payload);
        assert_eq!(error_code(&invalid), Some(codes::INVALID_REQUEST));
    }
    let mut stale = input(&app, json!({"end": {"kind":"live"}}));
    stale["pane_id"] = json!(u64::MAX.to_string());
    let (unknown, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, stale);
    assert_eq!(error_code(&unknown), Some(codes::INVALID_REQUEST));
    disable_test_gateway(&mut app, &ctx);
}

/// Tape only is the tape across the whole canvas, so it drags like the tape
/// beside the candles: up and down pans the price axis, sideways its time,
/// primary or middle button (trader 2026-09-30: in tape only a click and
/// drag moved nothing).
#[test]
fn a_drag_in_tape_only_pans_the_price_axis_and_the_tapes_time() {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        quantick_layers::OrderflowSwitch::TapeOnly,
        true,
    );
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().tape().cached_config().tape_only());
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_rect
        .expect("the canvas laid out");
    let at = chart.center();
    let range = |app: &QuantickApp| {
        let pane = &app.active_tab().flow_pane;
        pane.price_view
            .resolve(pane.frame.auto_range.expect("fitted"))
    };
    let before = range(&app);

    drag_sized(&mut app, &ctx, TEST_WINDOW, at, at + egui::vec2(0.0, 90.0));
    run_frame(&mut app, &ctx);
    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "the vertical drag took manual Y"
    );
    let after = range(&app);
    assert!(
        after.0 > before.0 && after.1 > before.1,
        "dragging down pans to higher prices: {before:?} -> {after:?}"
    );
    assert_eq!(app.active_tab().tape().tape_end(), TapeEnd::Live);

    middle_drag(&mut app, &ctx, at, at + egui::vec2(0.0, -90.0));
    run_frame(&mut app, &ctx);
    assert!(range(&app).1 < after.1, "the middle button pans back down");

    drag_sized(&mut app, &ctx, TEST_WINDOW, at, at + egui::vec2(120.0, 0.0));
    run_frame(&mut app, &ctx);
    assert!(
        !app.active_tab().tape().tape_end().is_live(),
        "the sideways drag took the tape into the past"
    );
}

/// A hand dragging the chart up and down over the tape, pressed at `at`:
/// every frame moves it 15 px down, then back up past the press, and a few
/// pixels sideways either way. Returns the tape's end after each frame.
fn jittery_vertical_drag(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    at: egui::Pos2,
) -> Vec<TapeEnd> {
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(at), pointer_button(at, true)],
    );
    let sideways = [3.0, -2.0, 4.0, -3.0, 2.0, 3.0, -1.0, 2.0, -2.0, 3.0];
    let vertical = [
        15.0, 15.0, 15.0, 15.0, -15.0, -15.0, -15.0, -15.0, -15.0, 15.0,
    ];
    let mut pointer = at;
    let mut ends = Vec::new();
    for (dx, dy) in sideways.into_iter().zip(vertical) {
        pointer += egui::vec2(dx, dy);
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(pointer)]);
        ends.push(app.active_tab().tape().tape_end());
    }
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(pointer),
            pointer_button(pointer, false),
        ],
    );
    run_frame(app, ctx);
    ends.push(app.active_tab().tape().tape_end());
    ends
}

/// Up and down over the tape pans the price axis, and the few pixels a hand
/// drifts sideways doing it never take the tape into the past — which is
/// what takes the book off it (`OrderflowView::draw_background`). Trader
/// 2026-09-30: dragging the chart up and down over the tape, the book
/// disappeared and came back now and then.
#[test]
fn a_vertical_drag_that_wobbles_sideways_keeps_the_tape_live() {
    let ctx = egui::Context::default();
    let (mut app, tape, _) = split_app(&ctx);
    let before = candles_view(&app);
    let ends = jittery_vertical_drag(&mut app, &ctx, tape);
    assert!(
        ends.iter().all(|end| end.is_live()),
        "the sideways wobble moved the tape off live: {ends:?}"
    );
    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "the drag panned the price axis"
    );
    assert_eq!(candles_view(&app), before, "and never the candles");
}

/// The same wobble in tape only, where the tape is the whole canvas.
#[test]
fn a_vertical_drag_that_wobbles_sideways_keeps_the_tape_live_in_tape_only() {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        quantick_layers::OrderflowSwitch::TapeOnly,
        true,
    );
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().tape().cached_config().tape_only());
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_rect
        .expect("the canvas laid out");
    let ends = jittery_vertical_drag(&mut app, &ctx, chart.center());
    assert!(
        ends.iter().all(|end| end.is_live()),
        "the sideways wobble moved the tape off live: {ends:?}"
    );
    assert!(
        !app.active_tab().flow_pane.price_view.is_auto(),
        "the drag panned the price axis"
    );
}

/// A literal double click returns only the clicked side to live, preserving
/// orientation and the tape's zoom: on the tape the manual prices hold; open
/// canvas on the candles' side keeps their spacing and centres the last price.
/// The next vertical gesture must still follow the hand, including on a
/// narrow Tape-only price window.
#[test]
fn double_click_then_vertical_pan_works_on_both_sides_and_orientations() {
    for inverted in [false, true] {
        for (tape_only, on_tape) in [(false, false), (false, true), (true, true)] {
            for middle in [false, true] {
                for dy in [-60.0, 60.0] {
                    let ctx = egui::Context::default();
                    let (mut app, tape, candles) = split_app(&ctx);
                    for agg_id in 401..=440 {
                        let mut print = trade(agg_id);
                        print.timestamp_ms += 120_000;
                        print.price = Decimal::from(101);
                        app.active_tab_mut()
                            .ingest_live_trade_at(&print, 130_000 + agg_id as i64);
                    }
                    app.active_tab_mut().tape_mut().flush_for_test();
                    // The app frame publishes the new market clock used by
                    // set_tape_end; flushing the worker alone leaves it stale.
                    run_frame(&mut app, &ctx);
                    if tape_only {
                        crate::orderflow_view::layers::set_layer_switch(
                            app.active_tab_mut().tape_mut(),
                            quantick_layers::OrderflowSwitch::TapeOnly,
                            true,
                        );
                        run_frame(&mut app, &ctx);
                        run_frame(&mut app, &ctx);
                    }
                    let manual = if tape_only {
                        (100.995, 101.005)
                    } else {
                        (90.455, 111.545)
                    };
                    let pane = &mut app.active_tab_mut().flow_pane;
                    assert!(pane.price_view.set_manual_range(manual.0, manual.1));
                    pane.price_view.set_inverted(inverted);
                    pane.model.viewport.zoom(1.375);
                    pane.model.viewport.pan_pixels(64.0, pane.slots());
                    app.active_tab_mut()
                        .tape_mut()
                        .set_live_lane_window(quantick_orderflow::LaneWindow::Fixed { ms: 3_000 });
                    app.active_tab_mut()
                        .tape_mut()
                        .set_tape_end(TapeEnd::Past { end_ms: 163_000 });
                    app.active_tab_mut().tape_mut().flush_for_test();
                    run_frame(&mut app, &ctx);
                    let candles_before = candles_view(&app);
                    // Either side keeps the candles' spacing.
                    let candles_px = candles_before.0;
                    let tape_before = app.active_tab().tape().tape_end();
                    let window = app.active_tab().tape().live_lane_window();
                    assert_eq!(
                        app.active_tab().flow_pane.shown_viewport().follows_live(),
                        tape_only
                    );
                    assert!(!tape_before.is_live());
                    let at = if tape_only {
                        app.active_tab()
                            .flow_pane
                            .frame
                            .chart_rect
                            .unwrap()
                            .center()
                    } else if on_tape {
                        tape
                    } else {
                        candles
                    };
                    click_sized(&mut app, &ctx, TEST_WINDOW, at);
                    click_sized(&mut app, &ctx, TEST_WINDOW, at);
                    run_frame(&mut app, &ctx);
                    // Open canvas, on either side, opens no settings: the drag
                    // below reaches the chart.
                    assert!(!app.surfaces.chart_settings.is_open());
                    run_frame(&mut app, &ctx);
                    let pane = &app.active_tab().flow_pane;
                    if on_tape {
                        assert_eq!(
                            pane.price_view.manual_range(),
                            Some(manual),
                            "the tape's double click preserves exact price framing"
                        );
                    } else {
                        assert_last_price_centred(pane);
                    }
                    assert_eq!(pane.price_view.is_inverted(), inverted);
                    assert_eq!(pane.model.viewport.px_per_bar(), candles_px);
                    assert_eq!(app.active_tab().tape().live_lane_window(), window);
                    let expected_tape = if on_tape { TapeEnd::Live } else { tape_before };
                    assert_eq!(app.active_tab().tape().tape_end(), expected_tape);
                    if on_tape {
                        assert_eq!(candles_view(&app), candles_before);
                        assert_eq!(pane.shown_viewport().follows_live(), tape_only);
                    } else {
                        assert!(pane.model.viewport.follows_live());
                        assert_eq!(
                            pane.model.viewport.right_edge_bar(pane.slots()),
                            pane.slots().saturating_sub(1) as f32
                        );
                    }
                    let auto = pane.frame.auto_range.unwrap();
                    assert_eq!(
                        app.active_tab().tape().tape_price_range(),
                        Some((101.0, 101.0)),
                        "the fixture retains only the quiet Tape price"
                    );
                    let height = pane.frame.chart_height;
                    let price = (manual.0 + manual.1) * 0.5;
                    let before_y = pane.price_view.scale(auto, 0.0, height).y(price);
                    if middle {
                        middle_drag(&mut app, &ctx, at, at + egui::vec2(0.0, dy));
                    } else {
                        drag_sized(&mut app, &ctx, TEST_WINDOW, at, at + egui::vec2(0.0, dy));
                    }
                    let pane = &app.active_tab().flow_pane;
                    let after_y = pane.price_view.scale(auto, 0.0, height).y(price);
                    assert!(!pane.price_view.is_auto());
                    assert_eq!(pane.price_view.is_inverted(), inverted);
                    assert!(
                        (after_y - before_y - dy).abs() < 1.0,
                        "screen pan {dy}, actual {}, Tape {on_tape}, Tape only {tape_only}, middle {middle}, inverted {inverted}",
                        after_y - before_y
                    );
                    assert_eq!(app.active_tab().tape().tape_end(), expected_tape);
                    assert_eq!(app.active_tab().tape().live_lane_window(), window);
                    assert_eq!(pane.model.viewport.px_per_bar(), candles_px);
                }
            }
        }
    }
}

/// The pane holds a manual price range centred on its last price, with
/// every bar of the frame's auto-fit still on the axis.
fn assert_last_price_centred(pane: &crate::pane::ChartPane) {
    use rust_decimal::prelude::ToPrimitive as _;
    let newest = pane.state.partial().or(pane.state.bars().last());
    let last = newest.unwrap().close.to_f64().unwrap();
    let (low, high) = pane.price_view.manual_range().expect("a held frame");
    assert!(
        (f64::midpoint(low, high) - last).abs() < 1e-9,
        "{last} mid-axis in {low}..{high}"
    );
    let auto = pane.frame.auto_range.unwrap();
    assert!(low <= auto.0 && high >= auto.1, "no bar clipped");
}

#[test]
fn an_empty_canvas_double_click_returns_to_live_at_its_spacing_with_the_last_price_centred() {
    for inverted in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        switch_layer(&mut app, ChartLayer::TapeChart, false);
        run_frame(&mut app, &ctx);
        let pane = &mut app.active_tab_mut().flow_pane;
        let manual = (98.25, 104.75);
        assert!(pane.price_view.set_manual_range(manual.0, manual.1));
        pane.price_view.set_inverted(inverted);
        pane.model.viewport.set_px_per_bar(23.0);
        pane.model.viewport.pan_pixels(120.0, pane.slots());
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(!pane.model.viewport.follows_live());
        let at = pane.frame.chart_rect.unwrap().center();
        click_sized(&mut app, &ctx, TEST_WINDOW, at);
        click_sized(&mut app, &ctx, TEST_WINDOW, at);
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.model.viewport.follows_live());
        assert_eq!(
            pane.model.viewport.right_edge_bar(pane.slots()),
            pane.slots().saturating_sub(1) as f32
        );
        assert_eq!(pane.model.viewport.px_per_bar(), 23.0, "spacing kept");
        assert_ne!(pane.price_view.manual_range(), Some(manual));
        assert_last_price_centred(pane);
        assert_eq!(pane.price_view.is_inverted(), inverted);
    }
}

/// Test agent D1 (review round 2, finding 8): the call is navigation, as
/// its descriptor says — the wheel's path. The window it sets moves the view
/// and is never filed as the asset's.
#[test]
fn the_tape_view_window_moves_the_view_and_is_not_filed() {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    app.layer_wiring().maintain(&ctx);
    let asset_window = |app: &QuantickApp| {
        let asset = app.active_tab().tape().asset().expect("bound");
        asset.filed().look.live_lane.window
    };
    let filed = asset_window(&app);
    let window = quantick_orderflow::LaneWindow::Fixed { ms: 3_000 };
    assert_ne!(filed, window);
    let directory = gateway_test_directory("tape-view-navigation");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = input(&app, json!({"window": {"kind":"fixed","ms":3_000}}));
    let (response, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, payload);
    assert_eq!(
        success_result(&response)["tape"]["window"]["fixed_ms"],
        3_000
    );
    app.layer_wiring().maintain(&ctx);
    assert_eq!(app.active_tab().tape().live_lane_window(), window);
    assert_eq!(asset_window(&app), filed, "the asset keeps its window");
    disable_test_gateway(&mut app, &ctx);
}

/// Mesh vertices painted inside `lane`: what the depth map leaves there.
fn lane_mesh_vertices(output: &egui::FullOutput, lane: egui::Rect) -> usize {
    fn walk(shape: &egui::Shape, lane: egui::Rect, count: &mut usize) {
        match shape {
            egui::Shape::Mesh(mesh) => {
                *count += mesh
                    .vertices
                    .iter()
                    .filter(|vertex| lane.contains(vertex.pos))
                    .count();
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, lane, count)),
            _ => {}
        }
    }
    let mut count = 0;
    for clipped in output
        .shapes
        .iter()
        .filter(|clipped| clipped.clip_rect.intersects(lane))
    {
        walk(&clipped.shape, lane, &mut count);
    }
    count
}

/// The trader's WIN layout on a recorded book: the map off on the candles
/// and on on the tape, a bid wall of 50 at 100.5 and an ask wall at 101.0
/// from 21 s, the book's clock ticking every second between the prints, and
/// the bid cut to 7 at 39 s.
fn recorded_book(ctx: &egui::Context) -> QuantickApp {
    use quantick_layers::OrderflowSwitch;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    let (mut app, mut commands) = app_with_history(200);
    native_split(&mut app);
    let generation = take_capture_start(&mut commands);
    let flow = app.active_tab_mut().tape_mut();
    crate::orderflow_view::layers::set_layer_switch(flow, OrderflowSwitch::Depth, false);
    crate::orderflow_view::layers::set_layer_switch(flow, OrderflowSwitch::TapeDepth, true);
    flow.handle_depth_event(DepthEvent::Snapshot {
        symbol: "TESTUSDT".to_owned(),
        generation,
        observed_at_ms: 21_000,
        effective_at_ms: 21_000,
        price_step: None,
        snapshot: BookSnapshot::new(
            10,
            vec![BookLevel::new(Decimal::new(1005, 1), Decimal::from(50)).unwrap()],
            vec![BookLevel::new(Decimal::new(1010, 1), Decimal::from(40)).unwrap()],
            BookCoverage::Limited {
                levels_per_side: 1_000,
            },
        ),
    });
    let (mut update_id, mut book_ms) = (11, 22_000);
    for agg_id in 201..=400 {
        let print = trade(agg_id);
        while book_ms <= print.timestamp_ms {
            let bids = if book_ms == 39_000 {
                vec![BookLevel::new(Decimal::new(1005, 1), Decimal::from(7)).unwrap()]
            } else {
                Vec::new()
            };
            app.active_tab_mut()
                .tape_mut()
                .handle_depth_event(DepthEvent::Update {
                    symbol: "TESTUSDT".to_owned(),
                    generation,
                    event_time_ms: book_ms,
                    delta: BookDelta::new(update_id, update_id, bids, Vec::new()),
                });
            update_id += 1;
            book_ms += 1_000;
        }
        app.active_tab_mut()
            .ingest_live_trade_at(&print, 10_000 + agg_id as i64);
    }
    let flow = app.active_tab_mut().tape_mut();
    flow.set_live_lane_window(quantick_orderflow::LaneWindow::Fixed { ms: 3_000 });
    flow.flush_for_test();
    run_frame(&mut app, ctx);
    app
}

/// Hold the tape at `end_ms` until the engine has published it, and return
/// the next frame with the tape's pane.
fn held_at(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    end_ms: i64,
) -> (egui::FullOutput, egui::Rect) {
    app.active_tab_mut()
        .tape_mut()
        .set_tape_end(TapeEnd::Past { end_ms });
    for _ in 0..3 {
        app.active_tab_mut().tape_mut().flush_for_test();
        run_frame(app, ctx);
    }
    assert!(
        !app.active_tab().tape().tape_end().is_live(),
        "held in the past"
    );
    app.active_tab_mut().tape_mut().flush_for_test();
    let output = run_frame(app, ctx);
    (output, lane_rect(app))
}

fn lane_rect(app: &QuantickApp) -> egui::Rect {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let divider = pane.frame.lane_divider_x.expect("the divider");
    egui::Rect::from_min_max(egui::pos2(divider + 1.0, chart.top()), chart.max)
}

/// Text painted inside `lane`.
fn lane_texts(output: &egui::FullOutput, lane: egui::Rect) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if lane.contains(text.pos) => {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

/// Trader 2026-10-07, WIN with the map on the tape and off the candles:
/// dragging the tape into the past took every heatmap band off it and left
/// the bubbles alone. The history still held the book for that stretch.
#[test]
fn a_tape_panned_into_the_past_keeps_its_book() {
    use quantick_layers::OrderflowSwitch;

    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);

    let painted = |app: &mut QuantickApp, tape_depth: bool| {
        crate::orderflow_view::layers::set_layer_switch(
            app.active_tab_mut().tape_mut(),
            OrderflowSwitch::TapeDepth,
            tape_depth,
        );
        let (output, lane) = held_at(app, &ctx, 35_000);
        lane_mesh_vertices(&output, lane)
    };
    let without = painted(&mut app, false);
    let with = painted(&mut app, true);
    assert!(
        with > without,
        "the past tape draws the book it stood beside: {with} vertices with the map, {without} without"
    );
}

/// A held tape whose window opens before the first captured book marks it from
/// the lane's opening, where the clamp puts the gap's edge, and writes nothing
/// over it: the reason is the hover's and the read's.
#[test]
fn a_held_tape_before_the_captured_book_marks_it_without_words() {
    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);
    let (output, lane) = held_at(&mut app, &ctx, 22_500);
    let texts = lane_texts(&output, lane);
    assert!(
        !texts
            .iter()
            .any(|text| text == "L2 unavailable before capture"),
        "the stretch before capture carries no words: {texts:?}"
    );
}

/// The cursor over a held tape reads the bands painted there: the wall of 50
/// the past stood beside, never today's 7 underneath. Asked the way the
/// control plane's pointer asks, with the geometry the frame painted.
#[test]
fn the_cursor_reads_the_book_a_held_tape_paints() {
    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);
    let (_, lane) = held_at(&mut app, &ctx, 35_000);
    let quantities: Vec<_> = lane_cells(&app, lane)
        .into_iter()
        .map(|cell| (cell.side, cell.quantity))
        .collect();
    assert!(
        quantities.contains(&(quantick_orderbook::BookSide::Bid, Decimal::from(50))),
        "the held wall is under the cursor: {quantities:?}"
    );
    assert!(
        !quantities
            .iter()
            .any(|(_, quantity)| *quantity == Decimal::from(7)),
        "today's book is not: {quantities:?}"
    );
}

/// The cells under a cursor walked down the middle of `lane`, asked the way
/// the control plane's pointer asks, with the geometry the frame painted.
fn lane_cells(app: &QuantickApp, lane: egui::Rect) -> Vec<crate::orderflow_view::FlowCellHit> {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    // The pointer's own reading: the candles' edge, as `control_pointer_hit` passes it.
    let history_right = pane.frame.candles_right.unwrap_or(chart.right());
    (lane.top() as i32..lane.bottom() as i32)
        .step_by(2)
        .filter_map(|y| {
            app.active_tab().tape().control_flow_cell_at(
                chart,
                &pane.model.viewport,
                pane.slots(),
                history_right,
                false,
                egui::pos2(lane.center().x, y as f32),
            )
        })
        .collect()
}

/// The live strip beside the tape narrows the candles, never the tape: the cursor,
/// reading the pane as `control_pointer_hit` does, finds on a live tape with the
/// strip shown the very cells the tape painted right of the divider.
#[test]
fn the_cursor_reads_the_live_tape_unmoved_by_the_strip() {
    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);
    let flow = &mut app.active_tab_mut().flow_pane;
    flow.set_layer_visible(ChartLayer::LiveStrip, true, &mut Default::default());
    for _ in 0..3 {
        app.active_tab_mut().tape_mut().flush_for_test();
        run_frame(&mut app, &ctx);
    }
    assert!(app.active_tab().tape().tape_end().is_live(), "a live tape");
    let strip = crate::live_strip::LIVE_STRIP_WIDTH_PX;
    let pane = &app.active_tab().flow_pane;
    let divider = pane.frame.lane_divider_x.expect("the divider");
    assert_eq!(
        pane.frame.candles_right,
        Some(divider - strip),
        "on the trail"
    );
    assert_eq!(
        app.active_tab().tape().trail_strip_px,
        strip,
        "from a live edge"
    );
    let lane = lane_rect(&app);
    let columns: Vec<_> = (lane.left() as i32..lane.right() as i32)
        .step_by(6)
        .collect();
    let at = |x: &i32| egui::Rect::from_x_y_ranges(*x as f32..=*x as f32, lane.y_range());
    let read: Vec<_> = columns.iter().map(|x| lane_cells(&app, at(x))).collect();
    // The painted tape alone, measured from the divider with no strip to subtract.
    app.active_tab_mut().tape_mut().trail_strip_px = 0.0;
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("laid out");
    let painted: Vec<Vec<_>> = columns
        .iter()
        .map(|x| {
            let rect = at(x);
            (rect.top() as i32..rect.bottom() as i32)
                .step_by(2)
                .filter_map(|y| {
                    let at = egui::pos2(rect.center().x, y as f32);
                    let tape = app.active_tab().tape();
                    tape.control_flow_cell_at(
                        chart,
                        &pane.model.viewport,
                        pane.slots(),
                        divider,
                        false,
                        at,
                    )
                })
                .collect()
        })
        .collect();
    assert!(
        painted.iter().any(|column| !column.is_empty()),
        "the book is under the cursor"
    );
    assert_eq!(read, painted);
}

/// A band on a held tape is the past, never the live lane: the cursor's
/// snapshot says it is not live and names the instant the tape is held at,
/// so a reader cannot take the held book for today's.
#[test]
fn the_cursor_over_a_held_band_reports_the_held_instant_not_live() {
    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);
    let (_, lane) = held_at(&mut app, &ctx, 35_000);
    let held = app.active_tab().tape().tape_end().past_ms();
    assert!(held.is_some(), "held in the past");
    let cells: Vec<_> = lane_cells(&app, lane)
        .into_iter()
        .map(crate::control::flow_cell_snapshot)
        .collect();
    assert!(!cells.is_empty(), "the held book is under the cursor");
    for cell in &cells {
        assert!(!cell.live_lane, "a held band is not live: {cell:?}");
        assert_eq!(
            cell.held_tape_end_unix_ms, held,
            "the snapshot names the held instant: {cell:?}"
        );
    }
}

fn primary_drag(app: &mut QuantickApp, ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2) {
    let middle = egui::pos2((from.x + to.x) / 2.0, from.y);
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
    );
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(middle)]);
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(to)]);
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(to), pointer_button(to, false)],
    );
    run_frame(app, ctx);
}

fn lane_of(app: &QuantickApp) -> quantick_orderflow::LiveLaneStyle {
    app.active_tab().tape().cached_config().live_lane.clone()
}

/// The view the pane's two switches read, on the pane as the hand left it.
fn view_of(app: &QuantickApp) -> quantick_orderflow::LaneView {
    lane_of(app).view()
}

/// With the live strip on the trail, a drag that previews the tape closed and then
/// comes back keeps one chart under the hand: the strip stays off the gutter while
/// the close is only shown, so the chart neither narrows nor resets the drag, and
/// the tape reopens under the hand.
#[test]
fn a_previewed_close_with_the_strip_shown_keeps_the_chart_and_reopens_under_the_hand() {
    use quantick_orderflow::LaneView;
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let flow = &mut app.active_tab_mut().flow_pane;
    flow.set_layer_visible(ChartLayer::LiveStrip, true, &mut Default::default());
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let open = pane.frame.lane_divider_x.expect("the divider");
    let strip = crate::live_strip::LIVE_STRIP_WIDTH_PX;
    assert_eq!(
        pane.frame.candles_right,
        Some(open - strip),
        "the strip is shown"
    );
    let (y, back) = (chart.center().y, open - 20.0);
    let step = |app: &mut QuantickApp, x: f32| {
        run_frame_with_events(app, &ctx, vec![egui::Event::PointerMoved(egui::pos2(x, y))]);
        app.active_tab()
            .flow_pane
            .frame
            .chart_rect
            .expect("laid out")
    };
    let press = egui::pos2(open, y);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(press),
            pointer_button(press, true),
        ],
    );
    for x in [open + 40.0, chart.right() - 30.0, chart.right() - 10.0] {
        assert_eq!(step(&mut app, x), chart, "the chart under the hand at {x}");
    }
    let shown = app
        .active_tab()
        .tape()
        .cached_config()
        .live_lane
        .shown_view();
    assert_eq!(shown, LaneView::CandlesOnly, "the close is previewed");
    for x in [open + 40.0, back] {
        assert_eq!(step(&mut app, x), chart, "the chart under the hand at {x}");
    }
    let release = egui::pos2(back, y);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(release),
            pointer_button(release, false),
        ],
    );
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::Beside);
    let divider = app
        .active_tab()
        .flow_pane
        .frame
        .lane_divider_x
        .expect("reopened");
    assert!(
        (divider - back).abs() < 1.0,
        "under the hand: {divider} for {back}"
    );
}

/// The divider dropped near the right edge is the Tape switch off, and near the
/// left edge tape only: the one state, each with its rail, and a click on the rail
/// brings both sides back at the default width whatever width the lane had.
#[test]
fn dragging_the_divider_to_an_edge_is_that_view_and_its_rail_reopens_at_the_default() {
    use quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE;
    use quantick_orderflow::{LANE_FOLD_RAIL_PX, LaneView};
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let open = pane.frame.lane_divider_x.expect("the divider");
    let y = chart.center().y;
    let pane_id = pane.id;
    let divider = |app: &QuantickApp| app.active_tab().flow_pane.frame.candles_right;
    let rail = format!("pane.{pane_id}.lane_rail");

    primary_drag(
        &mut app,
        &ctx,
        egui::pos2(open, y),
        egui::pos2(chart.right() - 30.0, y),
    );
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    assert!(!lane_of(&app).enabled, "the very switch the segment reads");
    let edge = divider(&app).expect("the rail's edge");
    assert!(
        (edge - (chart.right() - LANE_FOLD_RAIL_PX)).abs() < 0.5,
        "{edge}"
    );
    let scene = observer_scene(&app);
    assert_eq!(scene_control(&scene, &rail)["label"], "Tape rail");
    assert_eq!(
        scene_control(&scene, &rail)["availability"]["available"],
        true
    );
    assert_eq!(
        scene_control(&scene, &rail)["capability_id"],
        "orderflow.tape.fold.set"
    );

    click_chart(&mut app, &ctx, egui::pos2(chart.right() - 4.0, y));
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::Beside);
    assert_eq!(lane_of(&app).width_share, DEFAULT_LIVE_LANE_SHARE);
    let reopened = divider(&app).expect("the divider");
    let default_x = chart.right() - chart.width() * DEFAULT_LIVE_LANE_SHARE;
    assert!((reopened - default_x).abs() < 0.5, "{reopened}");
    let scene = observer_scene(&app);
    let none = scene["controls"].as_array().unwrap().iter();
    assert!(
        none.clone().all(|control| control["control_id"] != rail),
        "no rail beside the candles"
    );

    primary_drag(
        &mut app,
        &ctx,
        egui::pos2(reopened, y),
        egui::pos2(chart.left() + 30.0, y),
    );
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    assert!(lane_of(&app).tape_only, "the very switch the segment reads");
    let scene = observer_scene(&app);
    assert_eq!(scene_control(&scene, &rail)["label"], "Trail rail");

    click_chart(&mut app, &ctx, egui::pos2(chart.left() + 4.0, y));
    run_frame(&mut app, &ctx);
    assert_eq!(view_of(&app), LaneView::Beside);
    assert_eq!(lane_of(&app).width_share, DEFAULT_LIVE_LANE_SHARE);
}

/// A side closed by button reopens by dragging its rail, and the tape follows the
/// hand: no snap to the default, nor to the width the lane had.
#[test]
fn a_button_closed_side_reopens_under_the_hand_with_no_snap() {
    use quantick_orderflow::LaneView;
    use quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE;
    let ctx = egui::Context::default();
    for (closed, from_right) in [(LaneView::CandlesOnly, true), (LaneView::TapeOnly, false)] {
        let (mut app, _, _) = split_app(&ctx);
        app.active_tab_mut().tape_mut().set_lane_view(closed);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
        let y = chart.center().y;
        let (grab, drop) = if from_right {
            (chart.right() - 4.0, chart.right() - 300.0)
        } else {
            (chart.left() + 4.0, chart.left() + 300.0)
        };
        primary_drag(&mut app, &ctx, egui::pos2(grab, y), egui::pos2(drop, y));
        assert_eq!(view_of(&app), LaneView::Beside, "{closed:?}");
        let divider = app.active_tab().flow_pane.frame.lane_divider_x.unwrap();
        assert!(
            (divider - drop).abs() < 6.0,
            "{closed:?}: {divider} at {drop}"
        );
        assert_ne!(lane_of(&app).width_share, DEFAULT_LIVE_LANE_SHARE);
    }
}

/// Both sides by every door but the hand: the key, the segment's write and the
/// layer call land on the default, from a side the hand closed.
#[test]
fn a_drag_closed_side_reopens_by_the_layer_switch_at_the_default() {
    use quantick_orderflow::LaneView;
    use quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE;
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.unwrap();
    let open = pane.frame.lane_divider_x.unwrap();
    let y = chart.center().y;
    primary_drag(
        &mut app,
        &ctx,
        egui::pos2(open, y),
        egui::pos2(chart.right() - 30.0, y),
    );
    assert_eq!(view_of(&app), LaneView::CandlesOnly);
    let mut actions = quantick_layers::LayerActions::default();
    app.active_tab_mut().flow_pane.set_layer_visible(
        quantick_layers::ChartLayer::TapeChart,
        true,
        &mut actions,
    );
    assert_eq!(view_of(&app), LaneView::Beside);
    assert_eq!(lane_of(&app).width_share, DEFAULT_LIVE_LANE_SHARE);
}

fn read_fold(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let scopes = json!({"scopes":["orderflow.bubbles"]});
    let (response, _) = unkeyed_call(app, client, "snapshot.read", scopes);
    success_result(&response)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap())
        .find(|pane| pane["pane_id"] == pane_id)
        .expect("the flow pane is readable")["bubbles"]["tape_fold"]
        .clone()
}

/// The fold is the view by the side it closes, one named call away: permission-
/// checked, safe to retry, read back, allowed from every view.
#[test]
fn the_tape_fold_is_an_alias_over_the_view_retry_safe_and_read_back() {
    use quantick_orderflow::LaneView;
    use quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE;
    const FOLD: &str = "orderflow.tape.fold.set";
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let share = lane_of(&app).width_share;
    let directory = gateway_test_directory("tape-fold-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    assert_eq!(read_fold(&mut app, &mut observer), "open");

    let candles = input(&app, json!({"fold": "candles"}));
    let (denied, _) = unkeyed_call(&mut app, &mut observer, FOLD, candles.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "fold",
        FOLD,
        candles.clone(),
        "fold-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "fold-retry",
        FOLD,
        candles.clone(),
        "fold-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["fold"], "candles");
    assert_eq!(success_result(&first)["changed"], true);
    let (again, _) = unkeyed_call(&mut app, &mut cockpit, FOLD, candles.clone());
    assert_eq!(success_result(&again)["changed"], false);
    assert_eq!(read_fold(&mut app, &mut observer), "candles");
    assert_eq!(read_view(&mut app, &mut observer), "tape", "tape only");
    assert_eq!(view_of(&app), LaneView::TapeOnly);
    assert_eq!(lane_of(&app).width_share, share);

    let left = input(&app, json!({"fold": "left"}));
    let (bad, _) = unkeyed_call(&mut app, &mut cockpit, FOLD, left);
    assert!(error_code(&bad).is_some());

    // No refusal to meet: tape only straight to the candles alone, and back.
    let tape = input(&app, json!({"fold": "tape"}));
    let (moved, _) = unkeyed_call(&mut app, &mut cockpit, FOLD, tape);
    assert_eq!(success_result(&moved)["changed"], true);
    assert_eq!(read_fold(&mut app, &mut observer), "tape");
    assert_eq!(
        read_view(&mut app, &mut observer),
        "candles",
        "the tape off"
    );
    assert!(!lane_of(&app).enabled);
    assert_eq!(lane_of(&app).width_share, share);

    let open = input(&app, json!({"fold": "open"}));
    let (opened, _) = unkeyed_call(&mut app, &mut cockpit, FOLD, open.clone());
    assert_eq!(success_result(&opened)["changed"], true);
    assert_eq!(read_fold(&mut app, &mut observer), "open");
    assert_eq!(view_of(&app), LaneView::Beside);
    assert_eq!(lane_of(&app).width_share, DEFAULT_LIVE_LANE_SHARE);
    let (again, _) = unkeyed_call(&mut app, &mut cockpit, FOLD, open);
    assert_eq!(success_result(&again)["changed"], false);
}

/// Text painted in the bottom time strip whose clip or anchor lies in
/// `x_from..x_to`: what a folded side would crowd its rail with. Its glyphs
/// must reach the pane's own strip: the drawing rail's icons, left of the
/// pane at the same height, are not the pane's labels.
fn strip_texts(app: &QuantickApp, output: &egui::FullOutput, x: (f32, f32)) -> Vec<String> {
    let frame = &app.active_tab().flow_pane.frame;
    let strip = frame.time_strip.expect("a strip");
    let pane = frame.area.expect("the pane drew");
    let band = egui::Rect::from_x_y_ranges(x.0..=x.1, strip.y_range());
    let pane_strip = egui::Rect::from_x_y_ranges(pane.x_range(), strip.y_range());
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text)
                if text.visual_bounding_rect().intersects(pane_strip)
                    && (band.contains(text.pos) || band.contains(clipped.clip_rect.center())) =>
            {
                Some(text.galley.text().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn set_fold(app: &mut QuantickApp, fold: quantick_orderflow::LaneFold) {
    app.active_tab_mut().tape_mut().set_lane_fold(fold);
}

/// Closed candles keep the view the trader left: the rail is never a window
/// the view is clamped to, so reopening at that width returns it. Their time labels
/// and jump-to-live chip leave the strip to the rail meanwhile.
#[test]
fn closed_candles_keep_their_view_and_leave_the_strip_to_their_rail() {
    use quantick_orderflow::{LANE_FOLD_RAIL_PX, LaneFold};
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let pane = &mut app.active_tab_mut().flow_pane;
    let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
    // Pushed into the projection margin: room only the full candle pane has.
    pane.model.viewport.pan_pixels(-300.0, total);
    run_frame(&mut app, &ctx);
    let before = candles_view(&app);
    assert!(
        before.1 > (total - 1) as f32 + 1.0,
        "{before:?} past {total}"
    );
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    let output = run_frame(&mut app, &ctx);
    let candle_strip = (chart.left() - 100.0, chart.left() + LANE_FOLD_RAIL_PX);
    assert!(lane_texts(&output, egui::Rect::EVERYTHING).contains(&"» live".to_owned()));

    set_fold(&mut app, LaneFold::Candles);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let output = run_frame(&mut app, &ctx);
    assert_eq!(candles_view(&app), before, "the rail clamped the view");
    assert!(!lane_texts(&output, egui::Rect::EVERYTHING).contains(&"» live".to_owned()));
    let crowded = strip_texts(&app, &output, candle_strip);
    assert!(crowded.is_empty(), "{crowded:?}");

    set_fold(&mut app, LaneFold::Open);
    run_frame(&mut app, &ctx);
    assert_eq!(candles_view(&app), before, "reopening returns the view");
}

/// A closed tape writes no time under its rail: the lane's labels go with the
/// lane and come back with it.
#[test]
fn a_closed_tape_leaves_the_strip_to_its_rail() {
    use quantick_orderflow::{LANE_FOLD_RAIL_PX, LaneFold};
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    let divider = app.active_tab().flow_pane.frame.lane_divider_x.unwrap();
    let output = run_frame(&mut app, &ctx);
    let lane = strip_texts(&app, &output, (divider + 1.0, chart.right()));
    assert!(!lane.is_empty(), "an open tape labels its strip");

    set_fold(&mut app, LaneFold::Tape);
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let rail = (chart.right() - LANE_FOLD_RAIL_PX, chart.right() + 100.0);
    let crowded = strip_texts(&app, &output, rail);
    assert!(crowded.is_empty(), "{crowded:?}");
}

fn read_view(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let scopes = json!({"scopes":["orderflow.bubbles"]});
    let (response, _) = unkeyed_call(app, client, "snapshot.read", scopes);
    success_result(&response)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap())
        .find(|pane| pane["pane_id"] == pane_id)
        .expect("the flow pane is readable")["bubbles"]["tape_view"]
        .clone()
}

/// The view is one named call away: permission-checked, safe to retry, allowed
/// from any view, read back, refused only where there is no lane at all, and it
/// is the state `orderflow.tape.fold.set` names.
#[test]
fn the_tape_view_is_named_retry_safe_refused_without_a_lane_and_read_back() {
    const VIEW: &str = "orderflow.tape.view.set";
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let share = lane_of(&app).width_share;
    let directory = gateway_test_directory("tape-view-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    assert_eq!(read_view(&mut app, &mut observer), "beside");

    let tape = input(&app, json!({"view": "tape"}));
    let (denied, _) = unkeyed_call(&mut app, &mut observer, VIEW, tape.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "view",
        VIEW,
        tape.clone(),
        "view-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "view-retry",
        VIEW,
        tape.clone(),
        "view-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["view"], "tape");
    assert_eq!(success_result(&first)["changed"], true);
    let (again, _) = unkeyed_call(&mut app, &mut cockpit, VIEW, tape);
    assert_eq!(success_result(&again)["changed"], false);
    assert_eq!(read_view(&mut app, &mut observer), "tape");
    assert_eq!(lane_of(&app).width_share, share);

    // From tape only straight to candles only: no view stands in the way.
    let candles = input(&app, json!({"view": "candles"}));
    let (moved, _) = unkeyed_call(&mut app, &mut cockpit, VIEW, candles);
    assert_eq!(success_result(&moved)["changed"], true);
    assert_eq!(read_view(&mut app, &mut observer), "candles");
    assert!(!lane_of(&app).enabled, "the view is the tape's own switch");

    let open = input(&app, json!({"view": "open"}));
    let (bad, _) = unkeyed_call(&mut app, &mut cockpit, VIEW, open);
    assert!(error_code(&bad).is_some());
    let tab_id = app.runtime.tabs.active_id().to_string();
    let nowhere = json!({"tab_id": tab_id, "pane_id": "424242", "view": "beside"});
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, VIEW, nowhere);
    assert!(error_code(&refused).is_some(), "no pane, no lane");
    assert_eq!(read_view(&mut app, &mut observer), "candles");

    assert_eq!(read_fold(&mut app, &mut observer), "tape", "the one state");
    let beside = input(&app, json!({"view": "beside"}));
    let (back, _) = unkeyed_call(&mut app, &mut cockpit, VIEW, beside);
    assert_eq!(success_result(&back)["changed"], true);
    assert_eq!(read_view(&mut app, &mut observer), "beside");
    assert_eq!(read_fold(&mut app, &mut observer), "open");
    assert_eq!(
        lane_of(&app).width_share,
        quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE,
        "to beside is the default width"
    );
}

fn header_rect(
    app: &QuantickApp,
    control: quantick_chart::pane_header::HeaderControl,
) -> egui::Rect {
    app.active_tab()
        .flow_pane
        .frame
        .header_controls
        .iter()
        .find_map(|(painted, rect)| (*painted == control).then_some(*rect))
        .expect("the header painted it")
}

/// The selector and the call with `file` are one setting: a press files the window
/// and reads back through the call's scope, the call's filing shows on the selector
/// and outlives a double click's reset, and `auto` follows.
#[test]
fn the_tape_window_selector_and_the_filing_call_are_one_setting() {
    use quantick_chart::pane_header::HeaderControl;
    use quantick_orderflow::LaneWindow;
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    app.layer_wiring().maintain(&ctx);
    let directory = gateway_test_directory("tape-window-selector");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    // What a double click on the tape returns to: the window filed as the asset's.
    let setting = |app: &mut QuantickApp| {
        app.active_tab_mut().tape_mut().reset_live_lane_window();
        app.active_tab().tape().live_lane_window()
    };

    let selector = header_rect(&app, HeaderControl::TapeWindow);
    click_chart(&mut app, &ctx, selector.center());
    let open = run_frame(&mut app, &ctx);
    let row = painted_text_center(&open, "30 s").expect("the list offers the presets");
    click_chart(&mut app, &ctx, row);
    run_frame(&mut app, &ctx);
    let tape = read_tape(&mut app, &mut client);
    assert_eq!(tape["window_ms"], 30_000);
    assert_eq!(
        setting(&mut app),
        LaneWindow::Fixed { ms: 30_000 },
        "a pick files"
    );

    let window = json!({"kind":"fixed","ms":120_000});
    let payload = input(&app, json!({"window": window, "file": true}));
    keyed_call(&mut app, &mut client, "two", CAPABILITY, payload, "two");
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.starts_with("2 min")),
        "the selector says what the call wrote: {texts:?}"
    );
    let two = LaneWindow::Fixed { ms: 120_000 };
    assert_eq!(
        setting(&mut app),
        two,
        "the call files as the selector does"
    );
    let wheel = input(&app, json!({"window": {"kind":"fixed","ms":5_000}}));
    keyed_call(&mut app, &mut client, "nav", CAPABILITY, wheel, "nav");
    assert_eq!(
        setting(&mut app),
        two,
        "without `file` the call only navigates"
    );

    // Navigated to `auto` and zoomed over the filed 2 min: filing `auto` files it,
    // as the selector's pick does, judged against the setting and not the view.
    let auto = json!({"kind":"auto"});
    let zoomed = LaneWindow::Auto { zoom: 2.0 };
    app.active_tab_mut()
        .tape_mut()
        .navigate_live_lane_window(zoomed);
    let payload = input(&app, json!({"window": auto, "file": true}));
    keyed_call(&mut app, &mut client, "auto", CAPABILITY, payload, "auto");
    let filed = || app.active_tab().tape().lane_window_setting();
    assert_eq!(
        filed(),
        LaneWindow::default(),
        "auto over a filed fixed files"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        texts.iter().any(|text| text.starts_with("auto")),
        "auto reads the label's words: {texts:?}"
    );

    // Filed and shown, `auto` again files nothing and keeps the zoom; the
    // readback says so.
    app.active_tab_mut()
        .tape_mut()
        .navigate_live_lane_window(zoomed);
    let payload = input(&app, json!({"window": auto, "file": true}));
    let (response, _) = keyed_call(&mut app, &mut client, "again", CAPABILITY, payload, "again");
    assert_eq!(
        app.active_tab().tape().live_lane_window(),
        zoomed,
        "zoom kept"
    );
    assert_eq!(
        app.active_tab().tape().lane_window_setting(),
        LaneWindow::default()
    );
    let window = &success_result(&response)["tape"]["window"];
    assert_eq!(window["mode"], "auto");
    assert_eq!(window["zoom"], "2", "the result reports the zoom in force");
    disable_test_gateway(&mut app, &ctx);
}

/// The bar reaches up through the side headers and takes the hand there too, the
/// headers following within the frame; a drag elsewhere on a header moves nothing.
#[test]
fn the_divider_drags_from_its_header_part_and_a_header_never_pans() {
    use quantick_chart::flow_band::SIDE_HEADER_HEIGHT_PX;
    use quantick_chart::pane_header::{HeaderControl, LayerToggle};
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    let area = app.active_tab().flow_pane.frame.area.unwrap();
    let divider = app.active_tab().flow_pane.frame.candles_right.unwrap();
    let y = area.top() - SIDE_HEADER_HEIGHT_PX / 2.0;
    assert!(area.top() - SIDE_HEADER_HEIGHT_PX < y);

    let view = candles_view(&app);
    let empty = egui::pos2(divider - 300.0, y);
    primary_drag(&mut app, &ctx, empty, egui::pos2(divider - 200.0, y));
    assert_eq!(candles_view(&app), view, "a header starts no pan");
    assert_eq!(
        app.active_tab().flow_pane.frame.candles_right,
        Some(divider)
    );

    // The same grab through the header folds at the edge as it does on the chart.
    let chart = app.active_tab().flow_pane.frame.chart_rect.unwrap();
    let edge = egui::pos2(chart.right() - 30.0, y);
    primary_drag(&mut app, &ctx, egui::pos2(divider, y), edge);
    assert_eq!(view_of(&app), quantick_orderflow::LaneView::CandlesOnly);
    let footprint = header_rect(&app, HeaderControl::Layer(LayerToggle::Footprint));
    assert!(
        footprint.right() > divider,
        "one header runs the full width"
    );
}

/// With the order-flow panel open on its bubble settings, the wheel over the
/// tape still zooms the tape's window and never the candles beside it.
#[test]
fn the_wheel_over_the_tape_zooms_it_with_the_bubble_settings_open() {
    for preset in ["mini index regions", "Crypto default"] {
        wheel_over_the_tape_with_the_bubble_settings_open(preset);
    }
}

fn wheel_over_the_tape_with_the_bubble_settings_open(preset: &str) {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    // `split_app` already wears the mini index look; any other one is put on.
    if preset != "mini index regions" {
        assert!(app.active_tab_mut().tape_mut().apply_preset(preset, false));
    }
    run_frame(&mut app, &ctx);
    crate::orderflow_panel::open_on(app.active_tab_mut(), crate::ui_state::PanelSection::Bubbles);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().tape().settings_panel.is_open());
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let divider = pane.frame.lane_divider_x.expect("the divider");
    let tape = egui::pos2((divider + chart.right()) / 2.0, chart.center().y);
    let window = app.active_tab().tape().live_lane_window();
    let before = candles_view(&app);
    // The pointer comes off the panel onto the tape, as the hand does, and
    // the panel is still open when the wheel lands.
    let panel = egui::pos2(chart.right() + 40.0, chart.center().y);
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(panel)]);
    assert!(app.active_tab().tape().settings_panel.is_open());
    wheel(&mut app, &ctx, tape);
    assert_ne!(
        app.active_tab().tape().live_lane_window(),
        window,
        "the wheel over the tape zooms its window with the panel open"
    );
    assert!(app.active_tab().tape().settings_panel.is_open());
    assert_eq!(candles_view(&app), before, "and leaves the candles alone");
}

/// The panel's "Native tape" switch off leaves the ordinary lane beside the
/// candles: the wheel over it still zooms the lane's window, never the
/// candles, with the bubble settings open.
#[test]
fn the_wheel_over_the_ordinary_lane_zooms_its_window_not_the_candles() {
    let ctx = egui::Context::default();
    let (mut app, _, _) = split_app(&ctx);
    crate::orderflow_panel::open_on(app.active_tab_mut(), crate::ui_state::PanelSection::Bubbles);
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .edit_config(|config| config.live_lane.native_tape = false)
    );
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let config = app.active_tab().tape().cached_config();
    assert!(
        config.lane_enabled() && !config.native_tape(),
        "the ordinary lane"
    );
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let divider = pane
        .frame
        .lane_divider_x
        .expect("the lane keeps its divider");
    let lane = egui::pos2((divider + chart.right()) / 2.0, chart.center().y);
    let window = app.active_tab().tape().live_lane_window();
    let before = candles_view(&app);
    wheel(&mut app, &ctx, lane);
    assert_ne!(
        app.active_tab().tape().live_lane_window(),
        window,
        "the wheel over the lane zooms its window"
    );
    assert_eq!(candles_view(&app), before, "and leaves the candles alone");
}
