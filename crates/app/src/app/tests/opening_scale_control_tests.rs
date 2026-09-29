//! Opening-burst scaling is an explicit, readable display preference.
use super::*;

const ACTION: &str = "orderflow.tape.opening_scale.set";

fn input(app: &QuantickApp, enabled: bool) -> Value {
    json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "ignore_opening_burst_in_scale": enabled,
    })
}

fn bubbles(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["orderflow.bubbles"] }),
    );
    success_result(&response)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap())
        .find(|pane| pane["pane_id"] == pane_id)
        .expect("the target pane is readable")["bubbles"]
        .clone()
}

#[test]
fn opening_scale_is_default_off_named_permission_checked_and_read_back() {
    assert_eq!(
        quantick_control_schema::opening_scale::descriptor().persistence,
        quantick_control::registry::EffectPersistence::Transient,
        "changing the preference does not save a preset implicitly"
    );
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("opening-scale");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = bubbles(&mut app, &mut observer);
    assert_eq!(before["ignore_opening_burst_in_scale"], false);
    let payload = input(&app, true);
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert_eq!(bubbles(&mut app, &mut observer), before);
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "opening-first",
        ACTION,
        payload.clone(),
        "opening-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "opening-retry",
        ACTION,
        payload,
        "opening-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["changed"], true);
    assert_eq!(
        success_result(&first)["ignore_opening_burst_in_scale"],
        true
    );
    let after = bubbles(&mut app, &mut observer);
    assert_eq!(after["ignore_opening_burst_in_scale"], true);
    assert_eq!(after["tape_only"], before["tape_only"]);
    assert_eq!(after["overlap_merge"], before["overlap_merge"]);
    assert!(after["recorded_opening_windows_ms"].is_array());
    let payload = input(&app, true);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&noop)["changed"], false);
    let payload = input(&app, false);
    let (off, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&off)["ignore_opening_burst_in_scale"], false);
    assert_eq!(
        bubbles(&mut app, &mut observer)["ignore_opening_burst_in_scale"],
        false
    );
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn opening_scale_refuses_stale_targets_without_changing_the_active_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("opening-scale-stale");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    for field in ["tab_id", "pane_id"] {
        let mut payload = input(&app, true);
        payload[field] = json!(u64::MAX.to_string());
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
        assert_eq!(error_code(&response), Some(codes::INVALID_REQUEST));
        assert_eq!(
            bubbles(&mut app, &mut cockpit)["ignore_opening_burst_in_scale"],
            false
        );
    }
    disable_test_gateway(&mut app, &ctx);
}
