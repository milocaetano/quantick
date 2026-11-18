// The drawing context bar closes on any press outside it and on a lost window
// focus, and reopens only when the selected object is pressed again. The rule
// is `BarDismissal`'s; these tests prove the app feeds it the real press,
// gesture pick and focus.

use super::bare_canvas::app_with_history;
use super::*;

/// A selected rectangle with its bar on screen.
fn selected_rectangle() -> (QuantickApp, egui::Context) {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.surfaces
        .toolrail
        .arm(Tool::Drawing(drawing_tool("rectangle")));
    click_chart(&mut app, &ctx, egui::pos2(500.0, 250.0));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 350.0));
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    assert!(bar_shown(&app), "a selection raises the bar");
    (app, ctx)
}

fn bar_shown(app: &QuantickApp) -> bool {
    app.surfaces
        .drawing_chrome
        .context_bar()
        .last_rect()
        .is_some()
}

/// Pressing the rectangle's left edge, the press that reopens the bar.
fn press_the_rectangle(app: &mut QuantickApp, ctx: &egui::Context) {
    click_chart(app, ctx, egui::pos2(500.0, 300.0));
    run_frame(app, ctx);
}

#[test]
fn a_press_off_the_chart_dismisses_until_the_object_is_pressed() {
    let (mut app, ctx) = selected_rectangle();
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the chart is laid out");
    // The time axis under the chart: chrome, not the object.
    click_chart(
        &mut app,
        &ctx,
        egui::pos2(chart.center().x, chart.bottom() + 6.0),
    );
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    assert!(!bar_shown(&app), "a press elsewhere closes the bar");
    run_frame(&mut app, &ctx);
    assert!(!bar_shown(&app), "and it stays closed");
    press_the_rectangle(&mut app, &ctx);
    assert!(bar_shown(&app), "pressing the object again reopens it");
}

#[test]
fn a_drag_on_the_canvas_dismisses_past_its_release() {
    let (mut app, ctx) = selected_rectangle();
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(300.0, 500.0),
        egui::pos2(360.0, 520.0),
    );
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    assert!(!bar_shown(&app), "a pan is a press elsewhere, not a pause");
}

#[test]
fn losing_window_focus_dismisses() {
    let (mut app, ctx) = selected_rectangle();
    let unfocused = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        focused: false,
        ..Default::default()
    };
    let now = frame_instant(&ctx);
    let _ = ctx.run(unfocused, |ctx| app.draw_frame(ctx, now));
    run_frame(&mut app, &ctx);
    assert!(!bar_shown(&app), "a click in another window closes the bar");
    press_the_rectangle(&mut app, &ctx);
    assert!(bar_shown(&app));
}
