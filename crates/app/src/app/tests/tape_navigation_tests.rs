//! The tape beside the candles moves like the tick chart: the wheel over it
//! zooms its window, a drag pans its price axis and its time, a double click
//! brings it back to live — and none of it moves the candles. The same moves
//! are one named call away, and read back.
use super::*;
use quantick_orderflow::tape_view::TapeEnd;

const CAPABILITY: &str = "chart.tape_view.set";
/// The first print the fixture's tape retains: `trade(201)`.
const RETAINED_FROM: i64 = 21_100;

fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions")
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
        pane.viewport.px_per_bar(),
        pane.viewport.right_edge_bar(total),
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
        "tab_id": app.tabs.active_id().to_string(),
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
/// zoom, manual prices and orientation. The next vertical gesture must still
/// follow the hand, including on a narrow Tape-only price window.
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
                    pane.viewport.zoom(1.375);
                    pane.viewport.pan_pixels(64.0, pane.slots());
                    app.active_tab_mut()
                        .tape_mut()
                        .set_live_lane_window(quantick_orderflow::LaneWindow::Fixed { ms: 3_000 });
                    app.active_tab_mut()
                        .tape_mut()
                        .set_tape_end(TapeEnd::Past { end_ms: 163_000 });
                    app.active_tab_mut().tape_mut().flush_for_test();
                    run_frame(&mut app, &ctx);
                    let candles_before = candles_view(&app);
                    let tape_before = app.active_tab().tape().tape_end();
                    let window = app.active_tab().tape().live_lane_window();
                    assert_eq!(
                        app.active_tab().flow_pane.viewport.follows_live(),
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
                    let pane = &app.active_tab().flow_pane;
                    assert_eq!(
                        pane.price_view.manual_range(),
                        Some(manual),
                        "the literal double click preserves exact price framing"
                    );
                    assert_eq!(pane.price_view.is_inverted(), inverted);
                    assert_eq!(pane.viewport.px_per_bar(), candles_before.0);
                    assert_eq!(app.active_tab().tape().live_lane_window(), window);
                    let expected_tape = if on_tape { TapeEnd::Live } else { tape_before };
                    assert_eq!(app.active_tab().tape().tape_end(), expected_tape);
                    if on_tape {
                        assert_eq!(candles_view(&app), candles_before);
                        assert_eq!(pane.viewport.follows_live(), tape_only);
                    } else {
                        assert!(pane.viewport.follows_live());
                        assert_eq!(
                            pane.viewport.right_edge_bar(pane.slots()),
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
                    assert_eq!(pane.viewport.px_per_bar(), candles_before.0);
                }
            }
        }
    }
}

#[test]
fn an_ordinary_canvas_double_click_returns_to_live_at_the_existing_zoom_and_price_range() {
    for inverted in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        switch_layer(&mut app, ChartLayer::TapeChart, false);
        run_frame(&mut app, &ctx);
        let pane = &mut app.active_tab_mut().flow_pane;
        let manual = (98.25, 104.75);
        assert!(pane.price_view.set_manual_range(manual.0, manual.1));
        pane.price_view.set_inverted(inverted);
        pane.viewport.set_px_per_bar(23.0);
        pane.viewport.pan_pixels(120.0, pane.slots());
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.frame.lane_divider_x.is_none());
        assert!(!pane.viewport.follows_live());
        let at = pane.frame.chart_rect.unwrap().center();
        click_sized(&mut app, &ctx, TEST_WINDOW, at);
        click_sized(&mut app, &ctx, TEST_WINDOW, at);
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.viewport.follows_live());
        assert_eq!(
            pane.viewport.right_edge_bar(pane.slots()),
            pane.slots().saturating_sub(1) as f32
        );
        assert_eq!(pane.viewport.px_per_bar(), 23.0);
        assert_eq!(pane.price_view.manual_range(), Some(manual));
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
    flow.set_layer_switch(OrderflowSwitch::Depth, false);
    flow.set_layer_switch(OrderflowSwitch::TapeDepth, true);
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
        app.active_tab_mut()
            .tape_mut()
            .set_layer_switch(OrderflowSwitch::TapeDepth, tape_depth);
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

/// A held tape whose window opens before the first captured book says so
/// from the lane's opening, where the clamp puts the gap's edge.
#[test]
fn a_held_tape_before_the_captured_book_labels_it() {
    let ctx = egui::Context::default();
    let mut app = recorded_book(&ctx);
    let (output, lane) = held_at(&mut app, &ctx, 22_500);
    let texts = lane_texts(&output, lane);
    assert!(
        texts
            .iter()
            .any(|text| text == "L2 unavailable before capture"),
        "the stretch before capture is labelled on the tape: {texts:?}"
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
    let divider = pane.frame.lane_divider_x.expect("the divider");
    (lane.top() as i32..lane.bottom() as i32)
        .step_by(2)
        .filter_map(|y| {
            app.active_tab().tape().control_flow_cell_at(
                chart,
                &pane.viewport,
                pane.slots(),
                chart.right() - divider,
                false,
                egui::pos2(lane.center().x, y as f32),
            )
        })
        .collect()
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
