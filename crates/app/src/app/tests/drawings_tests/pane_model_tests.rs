// Pane model integration preserves the existing chart behavior.
use super::*;

#[test]
fn a_drawing_can_be_selected_from_its_stroke_and_moved_without_panning() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

    let before = app.active_tab().flow_pane.drawings.items()[0].points[0];
    let viewport_before = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());
    // Clear of the inspector: the panel is opaque to presses by
    // contract, so a proof about dragging must not start under it.
    let start = canvas_point_clear_of_inspector(&mut app, &ctx, 300.0);
    drag_chart(&mut app, &ctx, start, start + egui::vec2(40.0, 40.0));
    let after = app.active_tab().flow_pane.drawings.items()[0].points[0];

    assert!(
        after.bar > before.bar,
        "dragging right moves the anchor right"
    );
    assert!(
        after.price < before.price,
        "dragging down moves the anchor to a lower price"
    );
    assert_eq!(
        app.active_tab()
            .flow_pane
            .model
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        viewport_before,
        "moving a drawing must not pan the market underneath it"
    );
    assert_eq!(
        app.active_tab().flow_pane.gestures.drag,
        DrawingDrag::None,
        "release ends the move gesture"
    );
}
