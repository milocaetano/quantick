//! Tick-only activation preserves the approved tape and asset persistence path.
use super::*;
const ACTION: &str = "orderflow.bubbles.tick.set";

#[test]
fn tick_spheres_are_scoped_retry_safe_and_preserve_tape_settings() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let config: crate::config::AppConfig =
        toml::from_str(quantick_stores::config::EMBEDDED_DEFAULT).unwrap();
    let presets = quantick_stores::bubble_presets::embedded();
    let (binding, settings) = quantick_stores::bubble_asset_store::AssetBinding::bind(
        quantick_stores::bubble_asset_store::AssetBubblesStore::default().shared(),
        &config,
        ("binance", "BTCUSDT"),
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
        "preset": "Binance BTC Spot spheres",
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
    assert!(pane["bubbles"]["tick_presets"].as_array().unwrap().len() >= 2);
    off["pane_id"] = json!(u64::MAX.to_string());
    let (stale, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, off);
    assert_eq!(error_code(&stale), Some(codes::INVALID_REQUEST));
    disable_test_gateway(&mut app, &ctx);
}

/// Review round 1 #9: the whole-preset door asks the rule tick.set asks. On a
/// volume chart of a mode asset preset.apply wears no spheres and tick.set
/// refuses them; on a tick chart both give the preset's spheres.
#[test]
fn a_whole_preset_and_tick_set_agree_on_spheres() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let config: crate::config::AppConfig =
        toml::from_str(quantick_stores::config::EMBEDDED_DEFAULT).unwrap();
    let (binding, settings) = quantick_stores::bubble_asset_store::AssetBinding::bind(
        quantick_stores::bubble_asset_store::AssetBubblesStore::default().shared(),
        &config,
        ("binance", "BTCUSDT"),
        &quantick_stores::bubble_presets::embedded(),
        false,
    );
    app.active_tab_mut()
        .tape_mut()
        .bind_asset(binding, &settings);
    let directory = gateway_test_directory("tick-sphere-doors");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "preset": "Binance BTC Spot spheres",
    });
    for (spec, tick_chart) in [
        (crate::state::BarSpec::Volume(Decimal::from(5)), false),
        (crate::state::BarSpec::Tick(1), true),
    ] {
        app.active_tab_mut().flow_pane.spec.retain(spec);
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut()
            .tape_mut()
            .edit_config(|config| config.volume_dots.tick_bubbles = None);
        let apply = "orderflow.bubbles.preset.apply";
        let (whole, _) = unkeyed_call(&mut app, &mut cockpit, apply, payload.clone());
        assert_eq!(error_code(&whole), None, "{spec:?}");
        let tape = app.active_tab().tape().cached_config();
        let worn = tape.volume_dots.tick_bubbles.is_some();
        let (tick, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload.clone());
        assert_eq!(worn, tick_chart, "{spec:?}: the whole preset");
        assert_eq!(
            error_code(&tick).is_none(),
            tick_chart,
            "{spec:?}: tick.set"
        );
    }
    disable_test_gateway(&mut app, &ctx);
}
