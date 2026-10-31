//! Bubble size uses the same independent, bounded preferences over the gateway.
use super::*;

const ACTION: &str = "orderflow.bubbles.size.set";

fn input(app: &QuantickApp, target: &str, scale: f32) -> Value {
    json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "target": target,
        "scale": scale.to_string(),
    })
}

fn scales(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
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
        .expect("the target pane is readable")["bubbles"]["display_scale"]
        .clone()
}

fn enable_bubbles(app: &mut QuantickApp) {
    let owner = app.active_tab_mut().tape_mut();
    assert!(owner.apply_preset("mini index regions", false));
    owner.set_bubbles_enabled(true);
}

#[test]
fn bubble_size_is_named_permission_checked_retry_safe_and_read_back() {
    assert_eq!(
        quantick_control_schema::bubble_scale::descriptor().persistence,
        quantick_control::registry::EffectPersistence::Durable
    );
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    enable_bubbles(&mut app);
    let directory = gateway_test_directory("bubble-size");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    assert_eq!(
        scales(&mut app, &mut observer),
        json!({"tape":"1","candle":"1"})
    );
    let before = app.active_tab().tape().cached_config().clone();
    assert!(before.volume_dots.tick_bubbles.is_none());
    let payload = input(&app, "tape", 2.0);
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert_eq!(*app.active_tab().tape().cached_config(), before);
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "bubble-size-first",
        ACTION,
        payload.clone(),
        "bubble-size-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "bubble-size-retry",
        ACTION,
        payload.clone(),
        "bubble-size-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["scale"], "2");
    assert_eq!(success_result(&first)["target"], "tape");
    assert_eq!(success_result(&first)["changed"], true);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&noop)["changed"], false);
    assert_eq!(
        scales(&mut app, &mut observer),
        json!({"tape":"2","candle":"1"})
    );
    let candle = input(&app, "candle", 0.5);
    let (changed, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, candle);
    assert_eq!(success_result(&changed)["scale"], "0.5");
    assert_eq!(success_result(&changed)["changed"], true);
    assert_eq!(
        scales(&mut app, &mut observer),
        json!({"tape":"2","candle":"0.5"})
    );
    let mut expected = before;
    expected.bubbles.display_scale.tape = 2.0;
    expected.bubbles.display_scale.candle = 0.5;
    assert_eq!(*app.active_tab().tape().cached_config(), expected);
    // Ordinary Tape and historical tick bubbles retain their existing policy.
    app.active_tab_mut().tape_mut().edit_config(|config| {
        config.volume_dots.enabled = false;
        config.live_lane.native_tape = false;
    });
    let mut ordinary = app.active_tab().tape().cached_config().clone();
    for (target, scale) in [("tape", 0.75), ("candle", 1.5)] {
        let payload = input(&app, target, scale);
        let (changed, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
        assert_eq!(success_result(&changed)["scale"], scale.to_string());
        assert_eq!(success_result(&changed)["changed"], true);
    }
    ordinary.bubbles.display_scale.tape = 0.75;
    ordinary.bubbles.display_scale.candle = 1.5;
    assert_eq!(*app.active_tab().tape().cached_config(), ordinary);
    assert_eq!(
        scales(&mut app, &mut observer),
        json!({"tape":"0.75","candle":"1.5"})
    );
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn bubble_size_refuses_invalid_values_stale_targets_and_inactive_context() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    enable_bubbles(&mut app);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().time_panes[0]
            .state
            .tick_membership()
            .is_none()
    );
    let directory = gateway_test_directory("bubble-size-invalid");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = app.active_tab().tape().cached_config().clone();
    let payload = input(&app, "tape", 2.0);
    let mut invalid_inputs = Vec::new();
    for value in [
        json!("0.249"),
        json!("3.001"),
        json!("-1"),
        json!("NaN"),
        json!("1.0"),
        json!(2),
        Value::Null,
    ] {
        let mut invalid = payload.clone();
        invalid["scale"] = value;
        invalid_inputs.push(invalid);
    }
    for field in ["tab_id", "pane_id"] {
        let mut invalid = payload.clone();
        invalid[field] = json!(u64::MAX.to_string());
        invalid_inputs.push(invalid);
    }
    let mut time_chart = input(&app, "candle", 2.0);
    time_chart["pane_id"] = json!(app.active_tab().time_panes[0].id.to_string());
    invalid_inputs.push(time_chart);
    for invalid in invalid_inputs {
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, invalid);
        assert_eq!(error_code(&response), Some(codes::INVALID_REQUEST));
        assert_eq!(*app.active_tab().tape().cached_config(), before);
    }
    app.active_tab_mut().tape_mut().set_bubbles_enabled(false);
    app.active_tab_mut()
        .tape_mut()
        .set_lane_bubbles_enabled(false);
    let disabled = app.active_tab().tape().cached_config().clone();
    for target in ["tape", "candle"] {
        let payload = input(&app, target, 2.0);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
        assert_eq!(error_code(&response), Some(codes::INVALID_REQUEST));
        assert_eq!(
            violated_precondition(&response),
            Some("orderflow.bubbles.display_active"),
            "{target}: switching the bubbles on meets it"
        );
        assert_eq!(*app.active_tab().tape().cached_config(), disabled);
    }
    app.active_tab_mut()
        .flow_pane
        .spec
        .set(crate::state::BarSpec::Volume(Decimal::from(5)));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let membership = app.active_tab().flow_pane.state.tick_membership();
    assert!(membership.is_none(), "the pane now draws volume bars");
    let candle = input(&app, "candle", 2.0);
    let (response, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, candle);
    assert_eq!(
        violated_precondition(&response),
        Some("orderflow.bubbles.tick_chart"),
        "a volume chart fails the tick-chart precondition before the display one"
    );
    disable_test_gateway(&mut app, &ctx);
}
