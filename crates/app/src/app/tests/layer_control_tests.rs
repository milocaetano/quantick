//! Real gateway + real chart rendering; no adapter-specific layer vocabulary.
use super::*;
fn layer_input(app: &QuantickApp, id: &str, visible: bool) -> Value {
    json!({ "tab_id": app.tabs.active_id().to_string(), "pane_id": app.active_tab().flow_pane.id.to_string(), "layer_id": id, "visible": visible })
}
fn snapshot(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["layers.visibility"] }),
    );
    success_result(&response)["scopes"]["layers.visibility"]["value"].clone()
}
fn probe_is_painted(output: &egui::FullOutput) -> bool {
    output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(17,83,149)))
}
#[test]
fn additive_layer_traverses_discovery_admission_common_operation_and_real_chart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.active_tab_mut().flow_pane.install_layer_probe();
    assert!(!probe_is_painted(&run_frame(&mut app, &ctx)));
    let directory = gateway_test_directory("layer-probe");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let initial = snapshot(&mut app, &mut observer);
    let layers = initial["panes"][0]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 22);
    assert!(layers.iter().any(|layer| layer["id"] == "test_probe"
        && layer["requested"] == false
        && layer["effective"] == false));
    let payload = layer_input(&app, "test_probe", true);
    let (denied, _) = unkeyed_call(
        &mut app,
        &mut observer,
        "layers.visibility.set",
        payload.clone(),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert!(!probe_is_painted(&run_frame(&mut app, &ctx)));
    for visible in [true, false] {
        let payload = layer_input(&app, "test_probe", visible);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        assert_eq!(success_result(&response)["layer"]["requested"], visible);
        assert_eq!(success_result(&response)["layer"]["effective"], visible);
        assert_eq!(probe_is_painted(&run_frame(&mut app, &ctx)), visible);
    }
    let payload = layer_input(&app, "unknown_layer", true);
    let (unknown, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&unknown), Some(codes::INVALID_REQUEST));
    let mut payload = layer_input(&app, "grid", false);
    payload["pane_id"] = json!(u64::MAX.to_string());
    let (stale, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&stale), Some(codes::INVALID_REQUEST));
    let payload = layer_input(&app, "grid", false);
    let (grid, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(success_result(&grid)["layer"]["scope"], "window");
    assert!(!app.style.canvas.grid_enabled);
    let payload = layer_input(&app, "tape_chart", false);
    let (off, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert!(matches!(off.outcome, ResponseOutcome::Success { .. }));
    let payload = layer_input(&app, "tape_heatmap", true);
    let (blocked, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&blocked), Some(codes::CAPABILITY_UNAVAILABLE));
    let ResponseOutcome::Failure { error } = blocked.outcome else {
        panic!("blocked tape child");
    };
    assert_eq!(error.context.details.unwrap()["reason"], "the_tape_is_off");
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn omitted_pane_mutations_have_correlated_once_only_and_noop_journal_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    // More panes than the snapshot budget, using disconnected in-memory feeds.
    for id in 2..=66 {
        let (other, _, _, _) = test_app();
        let mut tab = other.tabs.into_single_runtime();

        tab.flow_pane.id = id + 1000;
        let opening = app.tabs.plan_open();
        app.tabs.append(opening, tab);
    }
    let target = app.tabs.last().unwrap();
    let tab_id = app.tabs.id_at(app.tabs.len() - 1).to_string();
    let pane_id = target.flow_pane.id.to_string();
    let visible = !target
        .flow_pane
        .layer_switched_on(ChartLayer::Crosshair, &app.style);
    let directory = gateway_test_directory("layers-omitted-readback");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let original_connection = client.handshake().connection_id.clone();
    let captured = snapshot(&mut app, &mut client);
    assert_eq!(captured["panes"].as_array().unwrap().len(), 64);
    assert_ne!(captured["omitted_panes"], "0");
    assert!(
        !captured["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pane| pane["tab_id"] == tab_id)
    );
    let (start, _) = unkeyed_call(
        &mut app,
        &mut client,
        "events.read",
        json!({ "start": "latest" }),
    );
    let cursor = success_result(&start)["next_cursor"].clone();
    let payload = json!({ "tab_id": tab_id, "pane_id": pane_id, "layer_id": "crosshair", "visible": visible });
    let (lost, first) = keyed_call(
        &mut app,
        &mut client,
        "first",
        "layers.visibility.set",
        payload.clone(),
        "layer-key",
    );
    let (retry, second) = keyed_call(
        &mut app,
        &mut client,
        "retry",
        "layers.visibility.set",
        payload.clone(),
        "layer-key",
    );
    assert_eq!(success_result(&lost)["changed"], true);
    assert_eq!(retry.outcome, lost.outcome);
    assert!(first.iter().any(|entry| entry.began));
    assert!(second.is_empty());
    drop(client);
    let mut reconciler = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let (page, _) = unkeyed_call(
        &mut app,
        &mut reconciler,
        "events.read",
        json!({ "cursor": cursor }),
    );
    let events: Vec<_> = success_result(&page)["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "layers.visibility.set")
        .cloned()
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["request_id"], "first");
    assert_eq!(
        events[0]["payload"]["connection_id"],
        json!(original_connection)
    );
    assert_eq!(events[0]["payload"]["result"]["tab_id"], tab_id);
    assert_eq!(events[0]["payload"]["result"]["pane_id"], pane_id);
    assert_eq!(
        events[0]["payload"]["result"]["layer"]["requested"],
        visible
    );
    assert_eq!(events[0]["actor"]["client_name"], CLIENT_NAME);
    let (noop, _) = keyed_call(
        &mut app,
        &mut reconciler,
        "noop",
        "layers.visibility.set",
        payload,
        "another-key",
    );
    assert_eq!(success_result(&noop)["changed"], false);
    let (page, _) = unkeyed_call(
        &mut app,
        &mut reconciler,
        "events.read",
        json!({ "cursor": success_result(&page)["next_cursor"] }),
    );
    let events: Vec<_> = success_result(&page)["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "layers.visibility.set")
        .cloned()
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["request_id"], "noop");
    assert_eq!(events[0]["payload"]["result"]["changed"], false);
    disable_test_gateway(&mut app, &ctx);
}
