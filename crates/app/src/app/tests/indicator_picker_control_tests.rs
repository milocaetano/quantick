//! The picker is the same sitting through the toolbar and the local gateway.
use super::*;
const CAPABILITY: &str = "indicator.picker.set";
#[test]
fn indicator_picker_control_opens_searches_inspects_and_closes_without_attaching() {
    let ctx = egui::Context::default();
    let (mut app, _) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("indicator-picker-control");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,observe.user_text",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut denied_client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let denied = unkeyed_call(
        &mut app,
        &mut denied_client,
        CAPABILITY,
        json!({"operation": {"action": "open"}}),
    )
    .0;
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert!(app.surfaces.indicator_picker.state.target.is_none());
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
    app.open_indicator_picker_on(app.active_tab().focused_side());
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
    app.open_indicator_picker_on(app.active_tab().focused_side());
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

/// `layout.focus.set` then an open reaches the state a click on a pane's `ƒx` does.
#[test]
fn focusing_a_pane_then_opening_the_picker_matches_its_glyph() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = super::candle_aggression::context_fixture(&ctx);
    app.active_tab_mut().focus = PaneSide::Flow;
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("indicator-picker-pane");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,observe.user_text",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options(
            "cockpit",
            &["cockpit", "cockpit.layout", "observe.user_text"],
        ),
    );
    app.control_action(
        "layout.focus.set",
        2,
        crate::control::ActionOrigin::Human,
        json!({"pane": "1"}),
    )
    .unwrap();
    let opened = success_result(
        &unkeyed_call(
            &mut app,
            &mut client,
            CAPABILITY,
            json!({"operation": {"action": "open"}}),
        )
        .0,
    );
    let time_pane = app.active_tab().pane(PaneSide::Time(0)).id;
    assert_eq!(opened["pane_id"], time_pane.to_string());

    let by_script = app.surfaces.indicator_picker.state.target;
    app.surfaces.indicator_picker.state.close();
    app.active_tab_mut().focus = PaneSide::Flow;
    app.active_tab_mut().time_panes[0].request_indicator_picker();
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
    assert_eq!(app.surfaces.indicator_picker.state.target, by_script);
}
