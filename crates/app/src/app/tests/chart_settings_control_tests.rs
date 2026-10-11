//! The chart settings window, through the control plane and through the
//! window's own doors: a double click on the candles, the footprint entry
//! points, and the named call — all landing on the one window.
use super::*;
use crate::style::{CandlePreset, ChartStyle};
use crate::surfaces::ChartSettingsTab;

const CAPABILITY: &str = "chart.settings.set";

fn settings_call(
    app: &mut QuantickApp,
    client: &mut LocalClient,
    input: Value,
) -> ResponseEnvelope {
    unkeyed_call(app, client, CAPABILITY, input).0
}

#[test]
fn the_window_and_the_candle_presets_are_driven_by_name() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("chart-settings-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let original = app.style;

    for invalid in [
        json!({"action": "open"}),
        json!({"action": "open", "tab": "drawings"}),
        json!({"action": "close", "x": 1}),
        json!({"action": "apply_candle_preset", "name": "nobody saved this"}),
        json!({"action": "save_candle_preset", "name": "Glass"}),
        json!({"action": "save_candle_preset", "name": "   "}),
        json!({"action": "delete_candle_preset", "name": "classic"}),
        // No sitting yet: there is nothing to reset to.
        json!({"action": "reset_candles"}),
    ] {
        let refused = settings_call(&mut app, &mut client, invalid.clone());
        assert_eq!(
            error_code(&refused),
            Some(codes::INVALID_REQUEST),
            "{invalid}"
        );
        assert_eq!(app.style, original, "a refused call changes nothing");
        assert!(!app.surfaces.chart_settings.is_open());
    }

    let opened = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "open", "tab": "footprint"}),
    ));
    assert_eq!(opened["open"], true);
    assert_eq!(opened["tab"], "footprint");
    assert_eq!(opened["changed"], true);
    assert_eq!(opened["candle_preset"], "Order flow");
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint)
    );
    let again = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "open", "tab": "footprint"}),
    ));
    assert_eq!(
        again["changed"], false,
        "opening an open window is harmless"
    );

    let glass = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "apply_candle_preset", "name": "glass"}),
    ));
    assert_eq!(app.style.candles, CandlePreset::Glass.style());
    assert_eq!(
        app.style.canvas, original.canvas,
        "a built-in keeps the canvas"
    );
    assert_eq!(glass["candle_preset"], "Glass");

    // A look of the trader's own, saved and brought back.
    app.style.candles.gap_px = 4.5;
    app.style.canvas.grid_enabled = !original.canvas.grid_enabled;
    let night: ChartStyle = app.style;
    let saved = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "save_candle_preset", "name": "night"}),
    ));
    assert_eq!(saved["custom_presets"], json!(["night"]));
    assert_eq!(saved["candle_preset"], "night");
    assert_eq!(saved["changed"], true);
    let resaved = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "save_candle_preset", "name": "night"}),
    ));
    assert_eq!(
        resaved["changed"], false,
        "the same look under the same name"
    );
    assert_eq!(
        crate::candle_presets::CandlePresetStore::load(&crate::candle_presets::default_path())
            .get("night"),
        Some(&night),
        "the saved look is on disk"
    );

    settings_call(
        &mut app,
        &mut client,
        json!({"action": "apply_candle_preset", "name": "Classic"}),
    );
    assert_eq!(app.style.candles, CandlePreset::Classic.style());
    settings_call(
        &mut app,
        &mut client,
        json!({"action": "apply_candle_preset", "name": "night"}),
    );
    assert_eq!(app.style, night, "a saved look brings candles and canvas");

    let reset = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "reset_candles"}),
    ));
    assert_eq!(
        app.style, original,
        "reset returns to the look the window opened on"
    );
    assert_eq!(reset["changed"], true);

    let deleted = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "delete_candle_preset", "name": "night"}),
    ));
    assert_eq!(deleted["custom_presets"], json!([]));
    let closed = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "close"}),
    ));
    assert_eq!(closed["open"], false);
    assert!(!app.surfaces.chart_settings.is_open());
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The double click on the bare candles opens the window on the Candles tab;
/// the same gesture on an axis does not.
#[test]
fn a_double_click_on_the_candles_opens_the_candles_tab() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the canvas laid out");
    assert!(!app.surfaces.chart_settings.is_open());
    let at = chart.center() - egui::vec2(chart.width() / 4.0, 0.0);
    click_chart(&mut app, &ctx, at);
    click_chart(&mut app, &ctx, at);
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Candles),
        "the double click at {at:?} opened the chart settings"
    );
}

/// Every footprint entry point opens the same window, on the Footprint tab.
#[test]
fn the_footprint_entry_points_open_the_footprint_tab() {
    let (mut app, _commands) = app_with_history(20);
    app.surfaces.chart_settings.open(ChartSettingsTab::Candles);
    // The pane's layer menu, drained by the window.
    app.workspace.layers_mut().actions.open_footprint_settings = true;
    app.layer_wiring().apply_actions();
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint),
        "one window, switched to the Footprint tab"
    );
    app.surfaces.chart_settings.close();
    // The toolbar's footprint button.
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::OpenFootprintSettings);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint)
    );
    // The appearance button flips the same window to the Candles tab.
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::ToggleAppearance);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Candles)
    );
}
