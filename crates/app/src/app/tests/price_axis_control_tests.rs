//! Manual tape price framing uses the same owner through gestures and control.
use super::*;

const CAPABILITY: &str = "chart.price_axis.set";

#[test]
fn a_manual_price_range_is_reported_before_the_first_automatic_fit() {
    let (mut app, _events, _commands, _book) = test_app();
    let pane = &mut app.active_tab_mut().flow_pane;
    assert!(pane.frame.auto_range.is_none());
    assert!(pane.price_view.set_manual_range(90.0, 110.0));
    let snapshot = crate::control::chart::viewport_snapshot(pane);
    assert!(!snapshot.price_auto_fit);
    let range = snapshot
        .price_range
        .expect("manual framing exists before the first print");
    assert_eq!((range.low.as_str(), range.high.as_str()), ("90", "110"));
}

#[test]
fn the_price_range_launch_hook_uses_the_same_manual_range_and_rejects_invalid_bounds() {
    for (value, expected) in [
        ("90:110", Some((90.0, 110.0))),
        (" 90.5 : 110.5 ", Some((90.5, 110.5))),
        ("110:90", None),
        ("100:100", None),
        ("NaN:110", None),
        ("90:inf", None),
        ("90:110:120", None),
        ("", None),
    ] {
        let launch = AppLaunch {
            scenario: crate::hooks::ScenarioInputs::from_pairs(&[("QUANTICK_PRICE_RANGE", value)]),
            ..AppLaunch::default()
        };
        let (app, _events, _commands, _book) = test_app_with_launch(launch);
        assert_eq!(
            app.active_tab().flow_pane.price_view.manual_range(),
            expected,
            "{value}"
        );
    }
    let (app, _events, _commands, _book) = test_app();
    assert!(
        app.active_tab().flow_pane.price_view.is_auto(),
        "an absent hook preserves defaults"
    );
}

fn enable_tape(app: &mut QuantickApp) {
    for layer in [
        ChartLayer::TapeChart,
        ChartLayer::BubbleOverlapMerge,
        ChartLayer::TapeOnly,
    ] {
        app.active_tab_mut()
            .flow_pane
            .set_layer_visible(layer, true, &mut Default::default());
    }
}

fn input(app: &QuantickApp, mode: Value) -> Value {
    json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "mode": mode,
    })
}

fn read_viewport(app: &mut QuantickApp, client: &mut LocalClient, pane_id: u64) -> Value {
    let pane_id = pane_id.to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({"scopes":["chart.summary"]}),
    );
    success_result(&response)["scopes"]["chart.summary"]["value"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["pane_id"] == pane_id)
        .unwrap()["viewport"]
        .clone()
}

/// The axis calls through the window's adapter reach the pane they address:
/// a manual price range holds there and reads back while the tab's other pane
/// keeps its framing, auto lets the pane fit again, and a time reset puts the
/// candles back on the spacing the pane opened on. What each call decides is
/// proven on the fake window in `quantick-control-handlers`.
#[test]
fn the_axis_calls_reach_the_addressed_window_pane() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    enable_tape(&mut app);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let pane_id = app.active_tab().flow_pane.id;
    let opened = app.active_tab().flow_pane.model.viewport.px_per_bar();
    let left_before = crate::control::chart::viewport_snapshot(&app.active_tab().time_panes[0]);
    let directory = gateway_test_directory("axis-calls-wiring");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let manual = input(&app, json!({"kind":"manual","low":"90","high":"110"}));
    let (framed, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, manual);
    success_result(&framed);
    let flow = &app.active_tab().flow_pane;
    assert_eq!(flow.price_view.manual_range(), Some((90.0, 110.0)));
    run_frame(&mut app, &ctx);
    let readback = read_viewport(&mut app, &mut client, pane_id);
    assert_eq!(readback["price_range"], json!({"low":"90","high":"110"}));
    assert_eq!(
        crate::control::chart::viewport_snapshot(&app.active_tab().time_panes[0]),
        left_before,
        "the tab's other pane keeps its framing"
    );
    let auto = input(&app, json!({"kind":"auto"}));
    let (auto, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, auto);
    success_result(&auto);
    assert!(app.active_tab().flow_pane.price_view.is_auto());
    app.active_tab_mut().flow_pane.model.viewport.zoom(3.0);
    assert_ne!(
        app.active_tab().flow_pane.model.viewport.px_per_bar(),
        opened
    );
    let tab_id = app.runtime.tabs.active_id().to_string();
    let reset = json!({"tab_id": tab_id, "pane_id": pane_id.to_string(), "axis": "candles"});
    let (reset, _) = unkeyed_call(&mut app, &mut client, "chart.time_axis.reset", reset);
    success_result(&reset);
    let px_per_bar = app.active_tab().flow_pane.model.viewport.px_per_bar();
    assert_eq!(px_per_bar, opened);
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
    let gutter = pane.frame.price_gutter.expect("the painted price gutter");
    let auto = pane.frame.auto_range.unwrap();
    let window = pane
        .orderflow
        .as_ref()
        .unwrap()
        .cached_config()
        .lane_window();
    drag_chart(
        &mut app,
        &ctx,
        gutter.center(),
        gutter.center() + egui::vec2(0.0, 60.0),
    );
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(
        !pane.price_view.is_auto(),
        "the tape's own gutter takes manual control"
    );
    let dragged = pane.price_view.resolve(auto);
    assert!(dragged.1 - dragged.0 > auto.1 - auto.0);
    run_frame_with_events(
        &mut app,
        &ctx,
        vec![
            egui::Event::PointerMoved(gutter.center()),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 80.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let wheeled = pane.price_view.resolve(auto);
    assert!(wheeled.1 - wheeled.0 < dragged.1 - dragged.0);
    assert_eq!(
        pane.orderflow
            .as_ref()
            .unwrap()
            .cached_config()
            .lane_window(),
        window
    );
}

#[test]
fn entering_tape_through_layers_or_a_source_preset_discards_candle_manual_framing() {
    for source_preset in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(40);
        run_frame(&mut app, &ctx);
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.price_view
            .pan(10_000.0, pane.frame.auto_range.unwrap());
        if source_preset {
            assert!(
                pane.orderflow
                    .as_mut()
                    .unwrap()
                    .apply_preset("mini index regions", false)
            );
        } else {
            enable_tape(&mut app);
        }
        run_frame(&mut app, &ctx);
        let pane = &app.active_tab().flow_pane;
        assert!(
            pane.price_view.is_auto(),
            "every mode-entry door starts at the tape's own fit"
        );
        let range = pane.price_view.resolve(pane.frame.auto_range.unwrap());
        assert!(
            range.1 < 200.0,
            "the former candle manual range cannot stretch the tape"
        );
    }
}

const PROGRESS: &str = "chart.price_tag.progress.set";

/// Every pane of the active tab as `chart.summary` reports it.
fn summary_panes(app: &mut QuantickApp, client: &mut LocalClient) -> Vec<Value> {
    let read = json!({"scopes":["chart.summary"]});
    let (response, _) = unkeyed_call(app, client, "snapshot.read", read);
    let panes = &success_result(&response)["scopes"]["chart.summary"]["value"]["panes"];
    panes.as_array().unwrap().clone()
}

/// The call through the window's adapter reaches the market settings of the
/// market the pane shows: every pane of the tab reads it back, and the
/// setting is kept for that market only. The answer, a repeat and the
/// refusals are proven on the handler in `quantick-control-handlers`.
#[test]
fn the_price_tag_progress_call_switches_the_market_and_reads_it_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let directory = gateway_test_directory("price-tag-progress");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let tab = app.runtime.tabs.active_id().to_string();
    let time = app.active_tab().pane(PaneSide::Time(0)).id.to_string();
    for pane in summary_panes(&mut app, &mut client) {
        assert_eq!(pane["price_tag_progress"], true, "on by default");
        let progress = &pane["bar_progress"];
        assert!(
            progress["done"].is_string() && progress["unit"].is_string(),
            "{pane}"
        );
    }
    let off = json!({"tab_id": tab, "pane_id": time, "on": false});
    let (first, _) = unkeyed_call(&mut app, &mut client, PROGRESS, off);
    success_result(&first);
    for pane in summary_panes(&mut app, &mut client) {
        assert_eq!(
            pane["price_tag_progress"], false,
            "the whole market follows: {pane}"
        );
    }
    let (venue, symbol) = app.active_tab().active.clone();
    let market = app.runtime.workspace.layouts().market();
    assert!(
        !market.bar_progress_on(&venue, &symbol),
        "kept in the market settings"
    );
    assert!(
        market.bar_progress_on("another venue", &symbol),
        "and for that market only"
    );
    disable_test_gateway(&mut app, &ctx);
}

/// The axis menu's switch lands in the market settings of the tab's market
/// on the next frame, and never as a layout edit.
#[test]
fn the_axis_menu_switch_is_kept_for_that_market_only() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let (venue, symbol) = app.active_tab().active.clone();
    run_frame(&mut app, &ctx);
    let layouts = app.runtime.workspace.layouts().revision();
    app.runtime.workspace.layers_mut().actions.bar_progress = Some(false);
    run_frame(&mut app, &ctx);
    let market = app.runtime.workspace.layouts().market();
    assert!(!market.bar_progress_on(&venue, &symbol));
    assert!(market.bar_progress_on("another venue", &symbol));
    assert!(market.bar_progress_on(&venue, "another symbol"));
    let after = app.runtime.workspace.layouts().revision();
    assert_eq!(after, layouts, "not a layout edit");
    assert_eq!(
        app.runtime.workspace.layers_mut().actions.bar_progress,
        None,
        "taken once"
    );
}

/// `apply_actions` is the one drain of the pane menus' wishes: any caller of
/// it, not only the frame, writes the price tag's switch to the market
/// rather than dropping it with the rest of the taken actions.
#[test]
fn every_drain_of_the_layer_actions_writes_the_progress_switch() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let (venue, symbol) = app.active_tab().active.clone();
    app.runtime.workspace.layers_mut().actions.bar_progress = Some(false);
    app.layer_wiring().apply_actions();
    let market = app.runtime.workspace.layouts().market();
    assert!(!market.bar_progress_on(&venue, &symbol));
    assert_eq!(
        app.runtime.workspace.layers_mut().actions.bar_progress,
        None
    );
}
