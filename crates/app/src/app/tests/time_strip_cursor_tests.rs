//! The time strip carries one cursor at a time, and its double-click resets
//! the spacing of the axis it sits under.
use super::*;
use quantick_chart_interaction::viewport::DEFAULT_PX_PER_BAR;

fn strip_app() -> (QuantickApp, egui::Context, egui::Rect, egui::Rect) {
    strip_app_with(AppLaunch::default())
}

fn strip_app_with(launch: AppLaunch) -> (QuantickApp, egui::Context, egui::Rect, egui::Rect) {
    let (mut app, _commands) = app_with_history_and_launch(50, launch);
    app.toolrail.arm(crate::toolrail::Tool::Crosshair);
    let ctx = egui::Context::default();
    pin_frame_clock(&ctx);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let pane = &app.active_tab().flow_pane;
    let history = pane.frame.time_strip.expect("the draw published the strip");
    let chart = pane.frame.chart_area.expect("the canvas laid out");
    let lane = egui::Rect::from_min_max(
        egui::pos2(history.right(), history.top()),
        egui::pos2(chart.right(), history.bottom()),
    );
    assert!(lane.width() > 20.0, "the flow pane shows its tape strip");
    (app, ctx, history, lane)
}

fn hover(app: &mut QuantickApp, ctx: &egui::Context, at: egui::Pos2) -> egui::FullOutput {
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)]);
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(at)])
}

#[test]
fn hovering_either_time_strip_shows_only_the_resize_arrow() {
    let (mut app, ctx, history, lane) = strip_app();
    let in_chart = history.center() - egui::vec2(0.0, 60.0);
    hover(&mut app, &ctx, in_chart);
    assert!(
        app.active_tab().flow_pane.hover_pos.is_some(),
        "the armed crosshair follows the pointer over the candles"
    );
    for at in [history.center(), lane.center()] {
        let output = hover(&mut app, &ctx, at);
        assert_eq!(
            output.platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal,
            "{at:?}"
        );
        assert_eq!(
            app.active_tab().flow_pane.hover_pos,
            None,
            "no painted crosshair beside the arrow at {at:?}"
        );
    }
}

#[test]
fn holding_the_time_strip_swaps_the_arrow_for_the_crosshair() {
    let (mut app, ctx, history, lane) = strip_app();
    for at in [history.center(), lane.center()] {
        hover(&mut app, &ctx, at);
        run_frame_with_events(&mut app, &ctx, vec![pointer_button(at, true)]);
        let held = run_frame_with_events(&mut app, &ctx, vec![]);
        assert_eq!(
            held.platform_output.cursor_icon,
            egui::CursorIcon::Crosshair,
            "{at:?}"
        );
        assert_eq!(app.active_tab().flow_pane.hover_pos, None);
        let dragged = run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(at + egui::vec2(12.0, 0.0))],
        );
        assert_eq!(
            dragged.platform_output.cursor_icon,
            egui::CursorIcon::Crosshair
        );
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![pointer_button(at + egui::vec2(12.0, 0.0), false)],
        );
        let released = hover(&mut app, &ctx, at);
        assert_eq!(
            released.platform_output.cursor_icon,
            egui::CursorIcon::ResizeHorizontal
        );
    }
}

#[test]
fn double_clicking_the_time_strip_resets_the_bar_spacing() {
    let (mut app, ctx, history, _) = strip_app();
    app.active_tab_mut()
        .flow_pane
        .model
        .viewport
        .set_px_per_bar(3.0 * DEFAULT_PX_PER_BAR);
    run_frame(&mut app, &ctx);
    click_chart(&mut app, &ctx, history.center());
    click_chart(&mut app, &ctx, history.center());
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().flow_pane.model.viewport.px_per_bar(),
        DEFAULT_PX_PER_BAR
    );
}

#[test]
fn double_clicking_the_tape_strip_resets_the_lane_window_only() {
    let (mut app, ctx, _, lane) = strip_app();
    let default = app.active_tab().tape().live_lane_window();
    let at = lane.center();
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 240.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    run_frame(&mut app, &ctx);
    assert_ne!(
        app.active_tab().tape().live_lane_window(),
        default,
        "the wheel zoomed the tape"
    );
    let spacing = app.active_tab().flow_pane.model.viewport.px_per_bar();
    click_chart(&mut app, &ctx, at);
    click_chart(&mut app, &ctx, at);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().tape().live_lane_window(), default);
    assert_eq!(
        app.active_tab().flow_pane.model.viewport.px_per_bar(),
        spacing,
        "the tape strip never resets the candles"
    );
}

#[test]
fn a_secondary_press_on_the_strip_keeps_the_resize_arrow() {
    let (mut app, ctx, history, _) = strip_app();
    let at = history.center();
    hover(&mut app, &ctx, at);
    let press = egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    };
    run_frame_with_events(&mut app, &ctx, vec![press]);
    let held = run_frame_with_events(&mut app, &ctx, vec![]);
    assert_eq!(
        held.platform_output.cursor_icon,
        egui::CursorIcon::ResizeHorizontal,
        "the context menu's press is not a primary hold"
    );
}

#[test]
fn a_held_drag_that_leaves_the_strip_never_shows_two_crosshairs() {
    let (mut app, ctx, history, _) = strip_app();
    let at = history.center();
    hover(&mut app, &ctx, at);
    run_frame_with_events(&mut app, &ctx, vec![pointer_button(at, true)]);
    for step in 1..=4 {
        let output = run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(
                at - egui::vec2(0.0, 40.0 * step as f32),
            )],
        );
        let os_crosshair = output.platform_output.cursor_icon == egui::CursorIcon::Crosshair;
        let painted = app.active_tab().flow_pane.hover_pos.is_some();
        assert!(
            os_crosshair != painted,
            "exactly one crosshair at step {step}: os {os_crosshair}, painted {painted}"
        );
    }
}

#[test]
fn the_jump_to_live_chip_shows_only_its_hand() {
    let (mut app, ctx, history, _) = strip_app();
    let pane = &mut app.active_tab_mut().flow_pane;
    let slots = pane.slots();
    pane.model.viewport.pan_pixels(200.0, slots);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let chip = crate::pane::test_live_chip_rect(history);
    let output = hover(&mut app, &ctx, chip.center());
    assert_eq!(
        output.platform_output.cursor_icon,
        egui::CursorIcon::PointingHand
    );
    assert_eq!(app.active_tab().flow_pane.hover_pos, None);
}

#[test]
fn the_double_click_returns_to_the_spacing_a_launch_hook_opened_on() {
    let launch = AppLaunch {
        scenario: crate::hooks::ScenarioInputs::from_pairs(&[("QUANTICK_CANDLE_WIDTH", "20")]),
        ..AppLaunch::default()
    };
    let (mut app, ctx, history, _) = strip_app_with(launch);
    assert_eq!(app.active_tab().flow_pane.model.viewport.px_per_bar(), 20.0);
    app.active_tab_mut().flow_pane.model.viewport.zoom(0.5);
    click_chart(&mut app, &ctx, history.center());
    click_chart(&mut app, &ctx, history.center());
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.model.viewport.px_per_bar(), 20.0);
}
