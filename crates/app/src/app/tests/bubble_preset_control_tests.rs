//! Exercise whole-look selection through the actual authorized gateway.
use super::*;
const APPLY: &str = "orderflow.bubbles.preset.apply";
const LIST: &str = "orderflow.bubbles.presets.list";

#[test]
fn whole_presets_reapply_drift_and_are_authorized_retry_safe() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let config: crate::config::AppConfig =
        toml::from_str(quantick_stores::config::EMBEDDED_DEFAULT).unwrap();
    let presets = quantick_stores::bubble_presets::embedded();
    let (binding, settings) = quantick_stores::bubble_asset_store::AssetBinding::bind(
        quantick_stores::bubble_asset_store::AssetBubblesStore::default().shared(),
        &config,
        ("binance-futures", "BTCUSDT.p"),
        &presets,
        false,
    );
    app.active_tab_mut()
        .tape_mut()
        .bind_asset(binding, &settings);
    app.active_tab_mut().tape_mut().edit_config(|config| {
        config.live_lane.native_tape = false;
        config.live_lane.stable_size = false;
        config.live_lane.depth_time_averaging = false;
    });
    let directory = gateway_test_directory("whole-preset");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = json!({"tab_id":app.tabs.active_id().to_string(),
        "pane_id":app.active_tab().flow_pane.id.to_string(), "preset":"Binance BTC perpetual spheres"});
    let before = app.active_tab().tape().cached_config().clone();
    let (denied, _) = unkeyed_call(&mut app, &mut observer, APPLY, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    for (field, value) in [
        ("preset", json!("")),
        ("preset", json!("unknown")),
        ("pane_id", json!(u64::MAX.to_string())),
        ("tab_id", json!(u64::MAX.to_string())),
    ] {
        let mut invalid = payload.clone();
        invalid[field] = value;
        let (refused, _) = unkeyed_call(&mut app, &mut cockpit, APPLY, invalid);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        assert_eq!(*app.active_tab().tape().cached_config(), before);
    }
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "whole-first",
        APPLY,
        payload.clone(),
        "whole-key",
    );
    assert_eq!(success_result(&first)["changed"], true);
    assert_eq!(success_result(&first)["stable_size"], true);
    assert_eq!(success_result(&first)["depth_time_averaging"], true);
    assert_eq!(success_result(&first)["native_tape_requested"], true);
    // Drift after the first result proves the keyed retry does not execute again.
    app.active_tab_mut()
        .tape_mut()
        .edit_config(|config| config.live_lane.stable_size = false);
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "whole-retry",
        APPLY,
        payload.clone(),
        "whole-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert!(
        !app.active_tab()
            .tape()
            .cached_config()
            .live_lane
            .stable_size
    );
    let (reapply, _) = unkeyed_call(&mut app, &mut cockpit, APPLY, payload.clone());
    assert_eq!(success_result(&reapply)["changed"], true);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, APPLY, payload.clone());
    assert_eq!(success_result(&noop)["changed"], false);
    let (read, _) = unkeyed_call(
        &mut app,
        &mut observer,
        "snapshot.read",
        json!({"scopes":["orderflow.bubbles"]}),
    );
    let panes = &success_result(&read)["scopes"]["orderflow.bubbles"]["value"]["tabs"][0]["panes"];
    let pane = panes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["pane_id"] == payload["pane_id"])
        .unwrap();
    assert_eq!(pane["bubbles"]["preset"], payload["preset"]);
    assert_eq!(pane["bubbles"]["stable_size"], true);
    let query = json!({"tab_id":payload["tab_id"], "pane_id":payload["pane_id"], "offset":"0", "limit":1, "revision":null});
    let (listed, _) = unkeyed_call(&mut app, &mut observer, LIST, query.clone());
    let page = success_result(&listed);
    assert_eq!(page["names"].as_array().unwrap().len(), 1);
    let mut next = query.clone();
    next["offset"] = page["next_offset"].clone();
    next["revision"] = page["revision"].clone();
    let (continued, _) = unkeyed_call(&mut app, &mut observer, LIST, next.clone());
    assert_ne!(success_result(&continued)["names"], page["names"]);
    for invalid in [json!(0), json!(65)] {
        let mut query = query.clone();
        query["limit"] = invalid;
        let (refused, _) = unkeyed_call(&mut app, &mut observer, LIST, query);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    }
    next["revision"] = json!("stale");
    let (refused, _) = unkeyed_call(&mut app, &mut observer, LIST, next);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    disable_test_gateway(&mut app, &ctx);
}
