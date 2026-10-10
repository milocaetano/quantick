// Pane model integration preserves the existing chart behavior.
use super::*;

/// (b) A tab is a whole workspace: switching away and back finds its bars,
/// its viewport, its focus and its drawings exactly as they were.
#[test]
fn switching_tabs_preserves_everything_each_one_owns() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(200);

    // Give tab 0 a distinctive state: a drawing, a panned viewport, and
    // the split open with the time pane focused.
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    // Clicking the time pane focuses it and lands the mark there — the
    // real gesture, not a poked field.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .frame
        .chart_area
        .expect("the time pane was laid out")
        .center();
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .model
        .viewport
        .pan_pixels(120.0, slots);
    let first_bars = app.active_tab().flow_pane.state.bars().len();
    let first_edge = app
        .active_tab()
        .flow_pane
        .model
        .viewport
        .right_edge_bar(slots);
    let first_drawings = app.active_tab().focused_pane().drawings.items().len();
    assert_eq!(first_drawings, 1, "the drawing landed on the focused pane");

    let _ends = open_second_tab(&mut app, &ctx, "ETHUSDT");
    assert_eq!(
        app.active_tab().layout,
        CanvasLayout::Single,
        "a new tab opens on the default layout, not the previous tab's"
    );
    assert!(app.active_tab().flow_pane.drawings.items().is_empty());

    app.apply_tab_action(TabAction::Activate(0));
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), first_bars);
    assert_eq!(
        app.active_tab()
            .flow_pane
            .model
            .viewport
            .right_edge_bar(app.active_tab().flow_pane.slots()),
        first_edge,
        "the viewport came back where it was left"
    );
    assert_eq!(app.active_tab().layout, CanvasLayout::TimeAndFlow);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    assert_eq!(
        app.active_tab().focused_pane().drawings.items().len(),
        first_drawings,
        "and its marks with it"
    );
}
