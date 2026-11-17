//! One drawing's scope through the control plane: the same switch as "Show on
//! all timeframes", by a named call.
use super::*;

const CAPABILITY: &str = "annotate.drawing_scope.set";

fn scope_call(app: &QuantickApp, index: usize, scope: &str) -> serde_json::Value {
    let pane = &app.active_tab().flow_pane;
    json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": pane.id.to_string(),
        "drawing_id": pane.drawings.items()[index].id.0.to_string(),
        "scope": scope,
    })
}

fn cockpit(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    name: &str,
) -> (std::path::PathBuf, LocalClient) {
    let directory = gateway_test_directory(name);
    grant_annotate_for_test(app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(app, ctx, &directory, 4);
    let client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    (directory, client)
}

/// `all_charts` with `all_windows` reaches the pane's marks on the layout the
/// pane shows; `this_chart` pins the drawing to the bars its chart is cut to:
/// a re-cut hides it and the way back restores it. Naming the scope it stands
/// in is a no-op.
#[test]
fn a_drawing_is_kept_to_its_chart_by_a_named_call() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    place_level(&mut app, PaneSide::Flow, 100.0);
    run_frame(&mut app, &ctx);
    let (directory, mut client) = cockpit(&mut app, &ctx, "drawing-scope-control");
    let drawn_at = *app.active_tab().flow_pane.series.state.spec();

    let mut wide = scope_call(&app, 0, "all_charts");
    wide["all_windows"] = json!(true);
    let (shared, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, wide);
    let result = success_result(&shared);
    assert_eq!(result["all_windows"], json!(true));
    let layout = app.layout_state().focused_pane_layout();
    assert_eq!(result["layout_id"], json!(layout.0.to_string()));

    let call = scope_call(&app, 0, "this_chart");
    let (kept, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, call.clone());
    let result = success_result(&kept);
    assert_eq!(result["scope"], json!("this_chart"));
    assert_eq!(result["changed"], json!(true));
    assert_eq!(result["pinned_bars"], json!(drawn_at.to_config_string()));
    let (again, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, call);
    assert_eq!(success_result(&again)["changed"], json!(false), "a repeat");
    run_frame(&mut app, &ctx);

    let flow = PaneSide::Flow;
    app.active_tab_mut()
        .pane_mut(flow)
        .set_spec(BarSpec::Tick(77));
    app.active_tab_mut().apply_spec_changes();
    run_frame(&mut app, &ctx);
    assert!(drawings_on(&app, PaneSide::Flow).is_empty(), "other bars");
    app.active_tab_mut().pane_mut(flow).set_spec(drawn_at);
    app.active_tab_mut().apply_spec_changes();
    run_frame(&mut app, &ctx);
    assert_eq!(drawings_on(&app, PaneSide::Flow), vec![100.0]);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
