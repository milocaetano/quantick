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
            .apply_source_preset(Some("mini index regions"))
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
/// double click over the tape returns it to live and to automatic Y.
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
    click_sized(&mut app, &ctx, TEST_WINDOW, tape);
    click_sized(&mut app, &ctx, TEST_WINDOW, tape);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().tape().tape_end(), TapeEnd::Live);
    assert!(app.active_tab().flow_pane.price_view.is_auto());
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
/// on it returns a held tape to live and the axis to automatic Y, as a
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
    app.active_tab_mut()
        .tape_mut()
        .set_layer_switch(quantick_layers::OrderflowSwitch::TapeOnly, true);
    run_frame(&mut app, &ctx);
    assert!(app.active_tab().tape().cached_config().tape_only());
    assert_eq!(app.active_tab().tape().tape_end(), held);
    let pane = &mut app.active_tab_mut().flow_pane;
    let auto = pane.frame.auto_range.expect("fitted");
    pane.price_view.pan_screen(40.0, 0.5, auto);
    assert!(!pane.price_view.is_auto());
    let chart = pane.frame.chart_rect.expect("the canvas laid out");
    let at = chart.center() - egui::vec2(chart.width() / 4.0, 0.0);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().tape().tape_end(),
        TapeEnd::Live,
        "the double click returned the tape to live"
    );
    assert!(app.active_tab().flow_pane.price_view.is_auto());
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
