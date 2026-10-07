//! "Save changes for this asset" through the real local gateway: the
//! asset's switch is permission-checked, retry-safe and read back. What it
//! promises is proven in `asset_bubbles_audit_tests`.
use super::*;

const ACTION: &str = "orderflow.bubbles.save_changes.set";

fn maintain(app: &mut QuantickApp) {
    app.layer_wiring().maintain(&egui::Context::default());
}

fn input(app: &QuantickApp, save_changes: bool) -> Value {
    json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "save_changes": save_changes,
    })
}

/// The flow pane's `asset` block in `orderflow.bubbles`.
fn asset_read(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["orderflow.bubbles"] }),
    );
    success_result(&response)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .expect("tabs")
        .iter()
        .flat_map(|tab| tab["panes"].as_array().expect("panes"))
        .find(|pane| pane["pane_id"] == pane_id)
        .expect("the flow pane is readable")["bubbles"]["asset"]
        .clone()
}

#[test]
fn save_changes_is_the_assets_permission_checked_retry_safe_and_read_back() {
    assert_eq!(
        quantick_control_schema::bubble_save::descriptor().persistence,
        quantick_control::registry::EffectPersistence::Durable,
        "the switch is stored for the asset"
    );
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("save-changes");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = asset_read(&mut app, &mut observer);
    assert_eq!(before["save_changes"], true, "on until switched off");

    let payload = input(&app, false);
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert_eq!(asset_read(&mut app, &mut observer), before);
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "save-first",
        ACTION,
        payload.clone(),
        "save-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "save-retry",
        ACTION,
        payload,
        "save-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    let result = success_result(&first);
    assert_eq!(result["changed"], true);
    assert_eq!(result["save_changes"], false);
    assert_eq!(result["asset"], before["key"]);
    maintain(&mut app);
    let off = asset_read(&mut app, &mut observer);
    assert_eq!(off["save_changes"], false);
    assert_eq!(off["saved"], true, "nothing on screen differs yet");

    assert!(
        app.active_tab_mut()
            .tape_mut()
            .edit_config(|config| config.set_ignore_opening_burst_in_scale(true))
    );
    maintain(&mut app);
    let held = asset_read(&mut app, &mut observer);
    assert_eq!(held["saved"], false, "a held edit is not saved");
    let why = held["save_error"].as_str().expect("why").to_owned();
    assert!(why.contains("saving is off"), "{why}");
    let payload = input(&app, false);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&noop)["changed"], false);

    let mut context = input(&app, true);
    context["pane_id"] = json!(app.active_tab().time_panes[0].id.to_string());
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, context);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    let mut unknown = input(&app, true);
    unknown["every_asset"] = json!(true);
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, unknown);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    assert_eq!(asset_read(&mut app, &mut observer)["save_changes"], false);

    let payload = input(&app, true);
    let (on, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&on)["changed"], true);
    maintain(&mut app);
    let saved = asset_read(&mut app, &mut observer);
    assert_eq!(saved["save_changes"], true);
    assert_eq!(saved["saved"], true, "what the pane shows is saved");
    assert_eq!(saved["source"], "stored");
    disable_test_gateway(&mut app, &ctx);
}
