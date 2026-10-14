//! Tick-only activation preserves the approved tape and asset persistence path.
use super::*;
const ACTION: &str = "orderflow.bubbles.tick.set";

#[test]
fn tick_spheres_are_scoped_retry_safe_and_preserve_tape_settings() {
    check_tick_selection("binance", "BTCUSDT", "Binance BTC Spot spheres", "BTC", 2);
    check_tick_selection(
        "metatrader-b3",
        "WINZ26",
        "B3 WIN tick spheres",
        "contracts",
        1,
    );
}

fn check_tick_selection(feed: &str, symbol: &str, preset: &str, unit: &str, choices: usize) {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let config: crate::config::AppConfig =
        toml::from_str(quantick_stores::config::EMBEDDED_DEFAULT).unwrap();
    let presets = quantick_stores::bubble_presets::embedded();
    let (binding, settings) = quantick_stores::bubble_asset_store::AssetBinding::bind(
        quantick_stores::bubble_asset_store::AssetBubblesStore::default().shared(),
        &config,
        (feed, symbol),
        &presets,
        false,
    );
    let owner = app.active_tab_mut().tape_mut();
    owner.bind_asset(binding, &settings);
    owner.edit_config(|config| {
        config.bubbles.max_radius = 48.0;
        config.bubbles.buy_color = Some([12, 230, 45]);
        config.volume_dots.full_quantity = 120.0;
        config.live_lane.native_tape = true;
    });
    let directory = gateway_test_directory("tick-spheres");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let tape = app.active_tab().tape().cached_config().clone();
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "preset": preset,
    });
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let mut invalid = payload.clone();
    invalid["preset"] = json!("missing sphere preset");
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, invalid);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    let mut off = payload.clone();
    off["preset"] = Value::Null;
    let (disabled, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, off.clone());
    assert!(success_result(&disabled)["tick_bubbles"].is_null());
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "tick-first",
        ACTION,
        payload.clone(),
        "tick-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "tick-retry",
        ACTION,
        payload.clone(),
        "tick-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["changed"], true);
    assert_eq!(
        success_result(&first)["tick_bubbles"]["quantity_unit"],
        unit
    );
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&noop)["changed"], false);
    let owner = app.active_tab().tape();
    let mut expected = tape.clone();
    expected.volume_dots.tick_bubbles = owner.cached_config().volume_dots.tick_bubbles;
    assert_eq!(
        *owner.cached_config(),
        expected,
        "only tick appearance changes"
    );
    let (read, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        "snapshot.read",
        json!({"scopes":["orderflow.bubbles"]}),
    );
    let panes = &success_result(&read)["scopes"]["orderflow.bubbles"]["value"]["tabs"][0]["panes"];
    let pane = panes
        .as_array()
        .unwrap()
        .iter()
        .find(|pane| pane["pane_id"] == off["pane_id"])
        .unwrap();
    assert_eq!(
        pane["bubbles"]["tick_bubbles"],
        success_result(&first)["tick_bubbles"]
    );
    assert!(pane["bubbles"]["tick_presets"].as_array().unwrap().len() >= choices);
    off["pane_id"] = json!(u64::MAX.to_string());
    let (stale, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, off);
    assert_eq!(error_code(&stale), Some(codes::INVALID_REQUEST));
    disable_test_gateway(&mut app, &ctx);
}
