//! The time strip carries one cursor at a time, and its double-click resets
//! the spacing of the axis it sits under.
use super::*;
use quantick_chart_interaction::viewport::DEFAULT_PX_PER_BAR;

fn strip_app() -> (QuantickApp, egui::Context, egui::Rect, egui::Rect) {
    let (mut app, _commands) = app_with_history(50);
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
