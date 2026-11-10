//! Actual loopback admission, persistence and retry/readback proofs.
use super::*;
use quantick_control::error::codes;
use quantick_control_local::client::{LocalClient, discover_in};
use quantick_control_schema::alerts as schema;
use serde_json::{Value, json};

fn connect_alerts(directory: &std::path::Path, cockpit: bool) -> LocalClient {
    let options = if cockpit {
        cockpit_test_options()
    } else {
        gateway_test_options()
    };
    discover_in(directory, &options)
        .unwrap()
        .select(None)
        .unwrap()
}

fn draft(app: &QuantickApp) -> Value {
    let context = super::super::alerts_runtime::context(
        app.active_tab(),
        &app.active_tab().flow_pane,
        app.runtime.tabs.active_id(),
        0,
    );
    json!({"version":1,"id":0,"name":"Socket alert","market":context.market,
        "conditions":[{"operator":"GreaterThan","target":{"Value":"100"}}],
        "frequency":"Once","expiration":"Never","message":"Socket threshold",
        "notification":{"popup":false,"toast":false,"sound":null,"sound_seconds":null}})
}

/// Retry each mutation and reconcile its single journal receipt over the socket.
fn mutation(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut LocalClient,
    operation: &str,
    payload: Value,
    key: &str,
) -> Value {
    let before = remote_call(app, ctx, client, "events.read", json!({"start":"latest"}));
    let cursor = success_result(&before)["next_cursor"].clone();
    let first = remote_call_with_key(app, ctx, client, key, operation, payload.clone(), key);
    let receipt = success_result(&first);
    let retry = remote_call_with_key(
        app,
        ctx,
        client,
        &format!("{key}-retry"),
        operation,
        payload,
        key,
    );
    assert_eq!(first.outcome, retry.outcome);
    let page = remote_call(app, ctx, client, "events.read", json!({"cursor":cursor}));
    let result = success_result(&page);
    let events: Vec<_> = result["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == schema::CHANGED_EVENT)
        .collect();
    assert_eq!(
        events.len(),
        1,
        "{operation} has exactly one mutation receipt"
    );
    assert_eq!(events[0]["payload"]["operation"], operation);
    assert_eq!(events[0]["payload"]["request_id"], key);
    assert_eq!(events[0]["payload"]["receipt"], receipt);
    receipt
}

#[test]
fn alert_socket_crud_retry_and_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let directory = gateway_test_directory("alerts-crud");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,cockpit.alert.delete",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect_alerts(&directory, false);
    let mut cockpit = connect_alerts(&directory, true);
    let spec = draft(&app);
    let denied = remote_call_with_key(
        &mut app,
        &ctx,
        &mut observer,
        "denied",
        schema::CREATE,
        json!({"spec":spec}),
        "denied",
    );
    assert_eq!(
        response_error(&denied).code.as_str(),
        codes::PERMISSION_DENIED
    );
    assert!(app.runtime.alerts.core.book.views().unwrap().is_empty());
    let no_key = remote_call(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::CREATE,
        json!({"spec":spec}),
    );
    assert!(matches!(
        no_key.outcome,
        quantick_control::wire::ResponseOutcome::Failure { .. }
    ));
    let valid = remote_call(
        &mut app,
        &ctx,
        &mut observer,
        schema::VALIDATE,
        json!({"spec":spec}),
    );
    assert_eq!(success_result(&valid), json!({"valid":true}));
    let mut invalid = spec.clone();
    invalid["conditions"][0]["operator"] = json!("Inside");
    let invalid = remote_call(
        &mut app,
        &ctx,
        &mut observer,
        schema::VALIDATE,
        json!({"spec":invalid}),
    );
    assert_eq!(
        response_error(&invalid).context.details.as_ref().unwrap()["field"],
        "operator"
    );
    let created = mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::CREATE,
        json!({"spec":spec}),
        "create",
    );
    let id = created["id"].clone();
    let read = remote_call(
        &mut app,
        &ctx,
        &mut observer,
        schema::READ,
        json!({"id":id}),
    );
    let mut saved = success_result(&read)["spec"].clone();
    saved["message"] = json!("Edited socket threshold");
    let edited = mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::EDIT,
        json!({"spec":saved,"expected_revision":created["revision"]}),
        "edit",
    );
    let stale = remote_call_with_key(
        &mut app,
        &ctx,
        &mut cockpit,
        "stale",
        schema::PAUSE,
        json!({"id":id,"expected_revision":created["revision"]}),
        "stale",
    );
    assert_eq!(
        response_error(&stale).code.as_str(),
        codes::REVISION_CONFLICT
    );
    let mut revision = edited["revision"].clone();
    for (operation, key) in [
        (schema::PAUSE, "pause"),
        (schema::RESUME, "resume"),
        (schema::REARM, "rearm"),
    ] {
        revision = mutation(
            &mut app,
            &ctx,
            &mut cockpit,
            operation,
            json!({"id":id,"expected_revision":revision}),
            key,
        )["revision"]
            .clone();
    }
    app.runtime
        .alerts
        .core
        .reload(crate::metrics::wall_clock_ms())
        .unwrap();
    let read = remote_call(
        &mut app,
        &ctx,
        &mut observer,
        schema::READ,
        json!({"id":id}),
    );
    assert_eq!(success_result(&read)["revision"], revision);
    assert_eq!(
        success_result(&read)["spec"]["message"],
        "Edited socket threshold"
    );
    let persisted = std::fs::read(&app.runtime.alerts.core.store.path).unwrap();
    for (operation, payload, key) in [
        (
            schema::DELETE,
            json!({"id":id,"expected_revision":revision}),
            "deny-delete",
        ),
        (schema::CLEAR_HISTORY, json!({}), "deny-clear"),
    ] {
        let denied =
            remote_call_with_key(&mut app, &ctx, &mut cockpit, key, operation, payload, key);
        assert_eq!(
            response_error(&denied).code.as_str(),
            codes::PERMISSION_DENIED
        );
    }
    assert_eq!(
        std::fs::read(&app.runtime.alerts.core.store.path).unwrap(),
        persisted
    );
    assert_eq!(app.runtime.alerts.core.book.views().unwrap().len(), 1);
    let mut options = cockpit_test_options();
    options
        .requested_scopes
        .insert(quantick_control_host::authority::permission(
            quantick_control_host::authority::COCKPIT_ALERT_DELETE_PERMISSION_ID,
        ));
    let mut cockpit = discover_in(&directory, &options)
        .unwrap()
        .select(None)
        .unwrap();
    mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::DELETE,
        json!({"id":id,"expected_revision":revision}),
        "delete",
    );
    mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::CLEAR_HISTORY,
        json!({}),
        "clear",
    );
    let list = remote_call(&mut app, &ctx, &mut observer, schema::LIST, json!({}));
    assert!(
        success_result(&list)["alerts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let history = remote_call(&mut app, &ctx, &mut observer, schema::HISTORY, json!({}));
    assert!(
        success_result(&history)["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        app.runtime.alerts.core.events.is_empty(),
        "commands never notify"
    );
    assert!(app.active_tab().paper.working_orders().is_empty());
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn alert_socket_target_binding_and_reopen() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "rectangle",
        &[egui::pos2(500.0, 300.0), egui::pos2(650.0, 420.0)],
    );
    app.toolrail.arm(Tool::Pointer);
    app.drawing_pane_mut().drawings.select(Some(index));
    run_frame(&mut app, &ctx);
    let selector = schema::DrawingSelector {
        tab_id: app.runtime.tabs.active_id(),
        pane_id: app.active_tab().drawing_pane().id,
        drawing_id: app.active_tab().drawing_pane().drawings.items()[index].id.0,
    };
    let directory = gateway_test_directory("alerts-target");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect_alerts(&directory, false);
    let mut cockpit = connect_alerts(&directory, true);
    let denied = remote_call_with_key(
        &mut app,
        &ctx,
        &mut observer,
        "target-denied",
        schema::TARGET_RESOLVE,
        json!(selector),
        "target-denied",
    );
    assert_eq!(
        response_error(&denied).code.as_str(),
        codes::PERMISSION_DENIED
    );
    let missing = remote_call_with_key(
        &mut app,
        &ctx,
        &mut cockpit,
        "target-missing",
        schema::TARGET_RESOLVE,
        json!(schema::DrawingSelector {
            drawing_id: u64::MAX,
            ..selector
        }),
        "target-missing",
    );
    assert_eq!(
        response_error(&missing).code.as_str(),
        codes::INVALID_REQUEST
    );
    assert_eq!(
        response_error(&missing).context.details.as_ref().unwrap()["reason"],
        "missing"
    );
    let resolved = mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::TARGET_RESOLVE,
        json!(selector),
        "resolve",
    );
    let target = resolved["target"].clone();
    // The selected drawing action uses the same persistent bridge as admission.
    let output = run_frame(&mut app, &ctx);
    let button =
        painted_text_center(&output, "Create alert").expect("selected drawing alert action");
    click_chart(&mut app, &ctx, button);
    run_frame(&mut app, &ctx);
    assert_eq!(
        json!(app.active_tab().drawing_pane().drawings.items()[index].durable_identity),
        target
    );
    let mut spec = draft(&app);
    spec["conditions"][0] = json!({"operator":"Inside","target":{"Drawing":target}});
    let created = mutation(
        &mut app,
        &ctx,
        &mut cockpit,
        schema::CREATE,
        json!({"spec":spec}),
        "drawing-create",
    );
    // Reopen both owners from their committed documents, never copying live drawings.
    app.layout_adapter()
        .reload_layouts(&["layouts", "market_settings"]);
    app.runtime
        .alerts
        .core
        .reload(crate::metrics::wall_clock_ms())
        .unwrap();
    run_frame(&mut app, &ctx);
    let matches = app
        .runtime
        .tabs
        .iter_with_ids()
        .flat_map(|(_, tab)| tab.panes())
        .flat_map(|(pane, _)| pane.drawings.items())
        .filter(|drawing| json!(drawing.durable_identity) == target)
        .count();
    assert!(
        matches > 0,
        "reopened workspace retains the exact durable drawing"
    );
    let read = remote_call(
        &mut app,
        &ctx,
        &mut observer,
        schema::READ,
        json!({"id":created["id"]}),
    );
    assert_eq!(
        success_result(&read)["spec"]["conditions"][0]["target"]["Drawing"],
        target
    );
    assert!(
        app.runtime.alerts.core.events.is_empty(),
        "reopen never replays notifications"
    );
    disable_test_gateway(&mut app, &ctx);
}
