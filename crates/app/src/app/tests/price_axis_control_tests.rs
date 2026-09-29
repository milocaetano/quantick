//! Manual tape price framing uses the same owner through gestures and control.
use super::*;

const CAPABILITY: &str = "chart.price_axis.set";

fn enable_tape(app: &mut QuantickApp) {
    for layer in [ChartLayer::TapeChart, ChartLayer::BubbleOverlapMerge, ChartLayer::TapeOnly] {
        app.active_tab_mut().flow_pane.set_layer_visible(layer, true, &mut Default::default());
    }
}

fn input(app: &QuantickApp, mode: Value) -> Value {
    json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "mode": mode,
    })
}

fn read_viewport(app: &mut QuantickApp, client: &mut LocalClient, pane_id: u64) -> Value {
    let (response, _) = unkeyed_call(app, client, "snapshot.read", json!({"scopes":["chart.summary"]}));
    success_result(&response)["scopes"]["chart.summary"]["value"]["panes"]
        .as_array().unwrap().iter()
        .find(|pane| pane["pane_id"] == pane_id.to_string()).unwrap()["viewport"].clone()
}

#[test]
fn the_price_axis_call_holds_manual_tape_framing_and_reads_it_back_without_moving_the_left_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    app.active_tab_mut().set_layout(CanvasLayout::TimeTimeAndFlow);
    enable_tape(&mut app);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let pane_id = app.active_tab().flow_pane.id;
    let left_before = crate::control::chart::viewport_snapshot(&app.active_tab().time_panes[0]);
    let directory = gateway_test_directory("price-axis-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(&directory, &options("cockpit", &["cockpit", "cockpit.layout"]));
    let manual = json!({"kind":"manual","low":"90","high":"110"});
    let payload = input(&app, manual);
    let (response, _) = keyed_call(&mut app, &mut client, "manual", CAPABILITY, payload.clone(), "manual-range");
    let (retry, _) = keyed_call(&mut app, &mut client, "manual-retry", CAPABILITY, payload, "manual-range");
    assert_eq!(response.outcome, retry.outcome);
    let result = success_result(&response);
    assert_eq!(result["viewport"]["price_auto_fit"], false);
    assert_eq!(result["viewport"]["price_range"], json!({"low":"90","high":"110"}));
    run_frame(&mut app, &ctx);
    let mut next = trade(41);
    next.price = Decimal::from(145);
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.state.ingest_live(&next);
        pane.orderflow.as_mut().unwrap().record_trade(&next);
    }
    run_frame(&mut app, &ctx);
    let readback = read_viewport(&mut app, &mut client, pane_id);
    assert_eq!(readback["price_auto_fit"], false);
    assert_eq!(readback["price_range"], result["viewport"]["price_range"]);
    assert_eq!(crate::control::chart::viewport_snapshot(&app.active_tab().time_panes[0]), left_before);
    let payload = input(&app, json!({"kind":"auto"}));
    let (reset, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, payload);
    assert_eq!(success_result(&reset)["viewport"]["price_auto_fit"], true);
    run_frame(&mut app, &ctx);
    let restored = read_viewport(&mut app, &mut client, pane_id);
    assert_eq!(restored["price_auto_fit"], true);
    let high = restored["price_range"]["high"].as_str().unwrap().parse::<f64>().unwrap();
    assert!(high >= 145.0, "returning to auto includes the latest price");
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn the_price_axis_action_validates_authority_target_and_range_before_mutation() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(10);
    enable_tape(&mut app);
    run_frame(&mut app, &ctx);
    let before = crate::control::chart::viewport_snapshot(&app.active_tab().flow_pane);
    let directory = gateway_test_directory("price-axis-validation");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(&directory, &options("cockpit", &["cockpit", "cockpit.layout"]));
    let payload = input(&app, json!({"kind":"manual","low":"90","high":"110"}));
    let (denied, _) = unkeyed_call(&mut app, &mut observer, CAPABILITY, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let mut stale = payload.clone();
    stale["pane_id"] = json!(u64::MAX.to_string());
    let (unknown, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, stale);
    assert_eq!(error_code(&unknown), Some(codes::INVALID_REQUEST));
    for mode in [
        json!({"kind":"manual","low":"110","high":"90"}),
        json!({"kind":"manual","low":"100","high":"100"}),
        json!({"kind":"manual","low":"NaN","high":"110"}),
        json!({"kind":"manual","low":"90"}),
        json!({"kind":"auto","low":"90","high":"110"}),
        json!({"kind":"unknown"}),
    ] {
        let payload = input(&app, mode);
        let (invalid, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, payload);
        assert_eq!(error_code(&invalid), Some(codes::INVALID_REQUEST));
        assert_eq!(crate::control::chart::viewport_snapshot(&app.active_tab().flow_pane), before);
    }
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn the_tape_price_gutter_accepts_drag_and_wheel_without_changing_its_time_window() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    enable_tape(&mut app);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let gutter = pane.plot_areas(pane.frame.plot_area.unwrap(), FeedCapabilities::none()).price_gutter;
    let auto = pane.frame.auto_range.unwrap();
    let window = pane.orderflow.as_ref().unwrap().cached_config().lane_window();
    drag_chart(&mut app, &ctx, gutter.center(), gutter.center() + egui::vec2(0.0, 60.0));
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(!pane.price_view.is_auto(), "the tape's own gutter takes manual control");
    let dragged = pane.price_view.resolve(auto);
    assert!(dragged.1 - dragged.0 > auto.1 - auto.0);
    run_frame_with_events(&mut app, &ctx, vec![
        egui::Event::PointerMoved(gutter.center()),
        egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 80.0), modifiers: egui::Modifiers::NONE },
    ]);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let wheeled = pane.price_view.resolve(auto);
    assert!(wheeled.1 - wheeled.0 < dragged.1 - dragged.0);
    assert_eq!(pane.orderflow.as_ref().unwrap().cached_config().lane_window(), window);
}

#[test]
fn entering_tape_through_layers_or_a_source_preset_discards_candle_manual_framing() {
    for source_preset in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(40);
        run_frame(&mut app, &ctx);
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.price_view.pan(10_000.0, pane.frame.auto_range.unwrap());
        if source_preset {
            assert!(pane.orderflow.as_mut().unwrap().apply_source_preset(Some("mini index regions")));
        } else {
            enable_tape(&mut app);
        }
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(pane.price_view.is_auto(), "every mode-entry door starts at the tape's own fit");
        let range = pane.price_view.resolve(pane.frame.auto_range.unwrap());
        assert!(range.1 < 200.0, "the former candle manual range cannot stretch the tape");
    }
}
