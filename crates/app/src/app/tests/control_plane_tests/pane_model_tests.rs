// Pane model integration preserves the existing chart behavior.
use super::*;

/// The scripted pan (`QUANTICK_PAN_PX`) is the gesture, not a teleport: it
/// re-applies every frame and settles exactly where the projection margin
/// holds it, which is how a screenshot reaches that state at all.
#[test]
fn the_scripted_pan_settles_on_the_projection_margin() {
    let (mut app, _cmd_rx) = app_with_history(400);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let slots = app.active_tab().flow_pane.slots();
    let newest = (slots - 1) as f32;

    app.chrome.harness.arm_candle_width(40.0);
    app.chrome.harness.arm_pan_px(-9_000.0);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let settled = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(slots);
    assert_eq!(app.active_tab().flow_pane.model.viewport.px_per_bar(), 40.0);
    assert!(!app.active_tab().flow_pane.model.viewport.follows_live());
    assert!(
        settled > newest + 1.0,
        "the chart is out in the empty canvas: {settled}"
    );

    // And it stays there. The margin is a wall, not a slope — a hook that
    // kept sliding would screenshot a different chart every frame.
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let again = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(slots);
    assert!((again - settled).abs() < 0.001, "{again} vs {settled}");
}
