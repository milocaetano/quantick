//! The picker is the same sitting through the toolbar and the local gateway.
use super::*;
const CAPABILITY: &str = "indicator.picker.set";
#[test]
fn indicator_picker_control_opens_searches_inspects_and_closes_without_attaching() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("indicator-picker-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options(
            "cockpit",
            &["cockpit", "cockpit.layout", "observe.user_text"],
        ),
    );
    let call = |app: &mut QuantickApp, client: &mut LocalClient, operation| {
        unkeyed_call(app, client, CAPABILITY, json!({ "operation": operation })).0
    };
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::OpenIndicatorPicker);
    let initial = success_result(&call(&mut app, &mut client, json!({"action": "inspect"})));
    assert_eq!(initial["open"], true);
    assert_eq!(initial["pane_id"], app.focused_pane().id.to_string());
    let searched = success_result(&call(
        &mut app,
        &mut client,
        json!({"action":"search", "query":"eMa", "category":"all"}),
    ));
    assert!(
        searched["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["name"].as_str().unwrap().to_lowercase().contains("ema"))
    );
    let invalid = call(
        &mut app,
        &mut client,
        json!({"action":"search", "query":"x", "category":"community"}),
    );
    assert_eq!(error_code(&invalid), Some(codes::INVALID_REQUEST));
    assert_eq!(app.surfaces.indicator_picker.state.query, "eMa");
    let closed = success_result(&call(&mut app, &mut client, json!({"action":"close"})));
    assert_eq!(closed["open"], false);
    assert!(app.focused_pane().indicators.all().is_empty());
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn indicator_picker_owns_keys_before_chrome_and_chart() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let dock = app.dock.visible();
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::OpenIndicatorPicker);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![key_press_with(
            egui::Key::B,
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        )],
    );
    assert_eq!(
        app.dock.visible(),
        dock,
        "modal blocks the unguarded dock shortcut"
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::Text("12".into())]);
    assert_eq!(app.surfaces.indicator_picker.state.query, "12");
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert!(app.surfaces.indicator_picker.state.target.is_none());
    // Escape normally disarms the drawing rail. The modal alone consumed it.
    assert_eq!(
        app.toolrail.tool(),
        Tool::Drawing(drawing_tool("horizontal-line"))
    );
}
