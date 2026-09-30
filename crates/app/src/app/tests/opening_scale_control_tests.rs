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

#[test]
fn candle_opening_scale_is_independent_retry_safe_and_readable_without_a_tape_worker() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = super::candle_aggression::context_fixture(&ctx);
    let pane_id = app.active_tab().time_panes[0].id.to_string();
    let tape_before = format!("{:?}", app.active_tab().tape().cached_config());
    assert!(
        !app.active_tab().time_panes[0]
            .footprint
            .ignore_candle_opening()
    );
    assert!(app.active_tab().time_panes[0].orderflow.is_none());
    let directory = gateway_test_directory("candle-opening-scale");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = json!({"tab_id":app.tabs.active_id().to_string(),"pane_id":pane_id,
        "target":"candle","ignore_opening_burst_in_scale":true});
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert!(
        !app.active_tab().time_panes[0]
            .footprint
            .ignore_candle_opening()
    );
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "candle-opening-first",
        ACTION,
        payload.clone(),
        "candle-opening-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "candle-opening-retry",
        ACTION,
        payload.clone(),
        "candle-opening-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["target"], "candle");
    assert_eq!(success_result(&first)["changed"], true);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload.clone());
    assert_eq!(success_result(&noop)["changed"], false);
    // The preference is readable before painting and while the layer is hidden.
    let (read, _) = unkeyed_call(
        &mut app,
        &mut observer,
        "snapshot.read",
        json!({"scopes":["orderflow.bubbles"]}),
    );
    let result = success_result(&read);
    let pane = result["scopes"]["orderflow.bubbles"]["value"]["tabs"][0]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pane_id"] == pane_id)
        .unwrap();
    assert_eq!(pane["opening_scale"]["candle"], true);
    assert!(pane["opening_scale"]["tape"].is_null());
    assert!(pane["candle_aggression"].is_null());
    app.active_tab_mut().time_panes[0].set_layer_visible(
        quantick_layers::ChartLayer::CandleAggression,
        true,
        &mut Default::default(),
    );
    let opening_output = run_frame(&mut app, &ctx);
    assert!(
        painted_text(&opening_output)
            .iter()
            .any(|text| text == "Opening-only scale fallback")
    );
    let opening = app.active_tab().time_panes[0]
        .footprint
        .candle_aggression()
        .unwrap();
    assert!(!opening.opening_exclusion_effective);
    assert_eq!(opening.recorded_opening_windows_ms, [1000]);
    for index in 0..4 {
        events
            .try_send(FeedEvent::Live(quantick_engine::Trade {
                agg_id: 21 + index,
                timestamp_ms: 1200 + index as i64,
                price: Decimal::from(100 + 5 * index as i64),
                quantity: Decimal::from(if index == 0 { 400 } else { 100 }),
                side: quantick_engine::Side::Buy,
            }))
            .unwrap();
    }
    let output = run_frame(&mut app, &ctx);
    assert!(
        painted_text(&output)
            .iter()
            .any(|text| text == "First recorded burst excluded")
    );
    let projected = app.active_tab().time_panes[0]
        .footprint
        .candle_aggression()
        .unwrap();
    assert!(projected.opening_exclusion_effective);
    assert_eq!(
        projected
            .marks
            .iter()
            .map(|m| m.opening_quantity)
            .sum::<Decimal>(),
        Decimal::from(20)
    );
    assert_eq!(projected.full_quantity, Decimal::from(400));
    assert_eq!(
        format!("{:?}", app.active_tab().tape().cached_config()),
        tape_before
    );
    app.active_tab_mut().time_panes[0].reset_series();
    assert!(
        app.active_tab().time_panes[0]
            .footprint
            .ignore_candle_opening(),
        "same-pane source resets preserve preference"
    );
    assert!(
        app.active_tab().time_panes[0]
            .footprint
            .candle_aggression()
            .is_none()
    );
    let mut off = payload;
    off["ignore_opening_burst_in_scale"] = json!(false);
    let (off, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, off);
    assert_eq!(success_result(&off)["ignore_opening_burst_in_scale"], false);
    assert!(!crate::pane::PaneFootprint::default().ignore_candle_opening());
    disable_test_gateway(&mut app, &ctx);
}
