//! Rich text operations pass through the same permission and retry gateway.
use super::*;

pub(super) fn target(app: &QuantickApp) -> Value {
    let pane = &app.active_tab().flow_pane;
    json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": pane.id.to_string(),
        "drawing_id": pane.drawings.items().last().unwrap().id.0.to_string(),
    })
}

#[test]
fn text_repeat_requires_authority_and_key_and_replays_once() {
    const CAPABILITY: &str = "annotate.text.repeat";
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.drawings.place(
        crate::drawings::DrawingTool::by_id("text").unwrap(),
        ChartPoint::at(1.0, 100.0),
    );
    let input = json!({ "source": target(&app), "anchor": newest_anchor(&app) });
    let directory = gateway_test_directory("text-repeat-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let (denied, refused) = keyed_call(
        &mut app,
        &mut observer,
        "denied",
        CAPABILITY,
        input.clone(),
        "denied-key",
    );
    assert!(matches!(
        error_code(&denied),
        Some(codes::SCOPE_DENIED | codes::PERMISSION_DENIED)
    ));
    assert!(refused.iter().all(|request| !request.began));
    let (missing_key, refused) = unkeyed_call(&mut app, &mut client, CAPABILITY, input.clone());
    assert!(matches!(
        missing_key.outcome,
        ResponseOutcome::Failure { .. }
    ));
    assert!(refused.iter().all(|request| !request.began));
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    let properties = json!({
        "size_px": "31.5", "family": "monospace", "bold": false, "italic": false,
        "color": [100, 150, 200, 255], "background": [20, 30, 40, 180],
        "border": [80, 90, 100, 255], "wrap_width_px": "140.25",
    });
    let (styled, _) = keyed_call(
        &mut app,
        &mut client,
        "style",
        "annotate.text.set",
        json!({ "target": input["source"], "properties": properties, "text": "source words" }),
        "style-key",
    );
    assert_eq!(success_result(&styled)["properties"], properties);
    let (inspected, _) = unkeyed_call(
        &mut app,
        &mut client,
        "annotate.text.set",
        json!({ "target": input["source"] }),
    );
    assert_eq!(success_result(&inspected)["properties"], properties);
    let before = app.active_tab().flow_pane.drawings.items()[0].clone();
    let mut invalid = properties.clone();
    invalid["wrap_width_px"] = json!("39.5");
    let (refused, _) = unkeyed_call(
        &mut app,
        &mut client,
        "annotate.text.set",
        json!({ "target": input["source"], "properties": invalid, "text": "must not apply" }),
    );
    assert!(matches!(refused.outcome, ResponseOutcome::Failure { .. }));
    assert_eq!(app.active_tab().flow_pane.drawings.items()[0], before);
    let (first, began) = keyed_call(
        &mut app,
        &mut client,
        "first",
        CAPABILITY,
        input.clone(),
        "repeat-key",
    );
    let placed = success_result(&first)["target"].clone();
    assert_eq!(success_result(&first)["properties"], properties);
    assert!(began.iter().any(|request| request.began));
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "retry",
        CAPABILITY,
        input,
        "repeat-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 2);
    assert_eq!(
        readback(&mut app, &ctx, &mut client, CAPABILITY),
        vec![placed]
    );
    let pane = &mut app.active_tab_mut().flow_pane;
    assert_eq!(
        pane.drawings.items()[1]
            .author
            .as_ref()
            .unwrap()
            .client_name,
        CLIENT_NAME
    );
    pane.drawings.undo();
    assert_eq!(pane.drawings.items().len(), 1);
    pane.drawings.redo();
    assert_eq!(pane.drawings.items().len(), 2);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}
