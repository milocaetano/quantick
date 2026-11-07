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

fn addressed_style(app: &mut QuantickApp) -> ChartStyle {
    use quantick_control_schema::chart_settings::ChartSettingsInput;
    let inspected = app
        .layer_wiring()
        .chart_settings(ChartSettingsInput::Inspect { target: None })
        .unwrap();
    quantick_control_schema::chart_settings::style_wire::decode(
        inspected.effective_style.expect("live pane style"),
    )
    .unwrap()
    .into()
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
    let original = addressed_style(&mut app);
    let legacy = app.runtime.style;

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
        assert_eq!(
            app.runtime.style, original,
            "a refused call changes nothing"
        );
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
    assert_eq!(
        addressed_style(&mut app).candles,
        CandlePreset::Glass.style()
    );
    assert_eq!(
        addressed_style(&mut app).canvas,
        original.canvas,
        "a built-in keeps the canvas"
    );
    assert_eq!(glass["candle_preset"], "Glass");

    // A look of the trader's own, saved and brought back.
    let mut edited = addressed_style(&mut app);
    edited.candles.gap_px = 4.5;
    edited.canvas.grid_enabled = !original.canvas.grid_enabled;
    let night: ChartStyle = edited.snapshot();
    let document = quantick_control_schema::chart_settings::style_wire::encode(
        &quantick_stores::chart_style::StyleFile::from(&night),
    );
    settings_call(
        &mut app,
        &mut client,
        json!({"action":"update_candles", "style":document}),
    );
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
    assert_eq!(
        addressed_style(&mut app).candles,
        CandlePreset::Classic.style()
    );
    settings_call(
        &mut app,
        &mut client,
        json!({"action": "apply_candle_preset", "name": "night"}),
    );
    assert_eq!(
        addressed_style(&mut app),
        night,
        "a saved look brings candles and canvas"
    );

    let reset = success_result(&settings_call(
        &mut app,
        &mut client,
        json!({"action": "reset_candles"}),
    ));
    assert_eq!(
        addressed_style(&mut app),
        original,
        "reset returns to the look the window opened on"
    );
    assert_eq!(reset["changed"], true);

    // A presets file the disk refuses: the save and the delete say so with
    // a retryable code, and the list stays what the disk holds.
    let unwritable = gateway_test_directory("chart-settings-unwritable")
        .join(crate::candle_presets::PRESETS_FILE);
    std::fs::create_dir_all(unwritable.parent().unwrap()).unwrap();
    let mut held =
        crate::surfaces::chart_settings::CandlesTab::with_presets_path(unwritable.clone());
    held.save("dawn", addressed_style(&mut app))
        .expect("written while the path is a file");
    std::fs::remove_file(&unwritable).unwrap();
    std::fs::create_dir_all(&unwritable).unwrap();
    let working = std::mem::replace(&mut app.surfaces.chart_settings.candles, held);
    for unwritten in [
        json!({"action": "save_candle_preset", "name": "dusk"}),
        json!({"action": "delete_candle_preset", "name": "dawn"}),
    ] {
        let refused = settings_call(&mut app, &mut client, unwritten.clone());
        assert_eq!(
            error_code(&refused),
            Some(codes::CAPABILITY_UNAVAILABLE),
            "{unwritten}"
        );
    }
    assert_eq!(
        app.surfaces.chart_settings.candles.custom_names(),
        ["dawn"],
        "neither change outlived its failed write"
    );
    app.surfaces.chart_settings.candles = working;
    std::fs::remove_dir_all(unwritable.parent().unwrap()).ok();

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
    assert_eq!(
        app.runtime.style, legacy,
        "local settings preserve the legacy global seed"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The double click on a candle opens the window on the Candles tab;
/// the same gesture on an axis does not.
#[test]
fn a_double_click_on_the_candles_opens_the_candles_tab() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    let pane_id = pane.id;
    let chart = pane.frame.chart_area.expect("the canvas laid out");
    assert!(!app.surfaces.chart_settings.is_open());
    // On a candle's body, through the geometry the frame painted.
    let (right, total) = (
        pane.frame.candles_right.unwrap_or(chart.right()),
        pane.slots(),
    );
    let slot = total - 20;
    let bar = pane.closed_bar(slot).expect("a closed bar");
    let scale = pane.price_view.scale(
        pane.frame.auto_range.expect("a price range"),
        pane.frame.chart_top,
        pane.frame.chart_top + pane.frame.chart_height,
    );
    let mid = crate::chart::to_f64(bar.high + bar.low) / 2.0;
    let at = egui::pos2(
        pane.model.viewport.x_center(slot, right, total),
        scale.y(mid),
    );
    let before = (
        pane.model.viewport.px_per_bar(),
        pane.model.viewport.follows_live(),
    );
    click_chart(&mut app, &ctx, at);
    click_chart(&mut app, &ctx, at);
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Candles),
        "the double click at {at:?} opened the chart settings"
    );
    let inspected = app
        .layer_wiring()
        .chart_settings(
            quantick_control_schema::chart_settings::ChartSettingsInput::Inspect { target: None },
        )
        .unwrap();
    assert_eq!(
        inspected.target.expect("live pane target").pane_id.0,
        pane_id,
        "the gesture pins this chart"
    );
    let viewport = &app.active_tab().flow_pane.model.viewport;
    assert_eq!(
        (viewport.px_per_bar(), viewport.follows_live()),
        before,
        "and left the view alone"
    );
}

/// Every footprint entry point opens the same window, on the Footprint tab.
#[test]
fn the_footprint_entry_points_open_the_footprint_tab() {
    let (mut app, _commands) = app_with_history(20);
    app.surfaces
        .chart_settings
        .open(ChartSettingsTab::Candles, app.runtime.style);
    // The pane's layer menu, drained by the window.
    app.runtime
        .workspace
        .layers_mut()
        .actions
        .open_footprint_settings = true;
    app.layer_wiring().apply_actions();
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint),
        "one window, switched to the Footprint tab"
    );
    app.surfaces.chart_settings.close();
    // The footprint glyph's right click on the pane's header.
    app.active_tab_mut().flow_pane.apply_header_intent(
        quantick_chart::pane_header::HeaderIntent::OpenLayerSettings(
            quantick_chart::pane_header::LayerToggle::Footprint,
        ),
        &mut quantick_layers::LayerActions::default(),
    );
    app.layer_wiring()
        .open_settings(ChartSettingsTab::Footprint);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Footprint)
    );
    // Settings → Appearance… flips the same window to the Candles tab.
    app.apply_menu_command(crate::app::menu_bar::MenuCommand::OpenAppearance);
    assert!(
        app.surfaces
            .chart_settings
            .is_open_on(ChartSettingsTab::Candles)
    );
}

/// A named call that changes the look leaves the same appearance line a
/// click leaves, naming the built-in it applied; a call that changes nothing
/// leaves none.
#[test]
fn a_named_style_change_leaves_the_appearance_log_line() {
    use super::control_launch_baselines::RecordedEvents;
    use quantick_control_schema::chart_settings::ChartSettingsInput;
    use tracing_subscriber::prelude::*;
    // Keep the general callsite-interest path while subscriber-less test threads
    // register this same log site. This dispatcher is never installed.
    // https://github.com/tokio-rs/tracing/issues/3611 (tracing-core 0.1.36).
    let _parallel_dispatch = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
    let recorded = RecordedEvents::default();
    let subscriber = tracing_subscriber::registry().with(recorded.clone());
    tracing::subscriber::with_default(subscriber, || {
        let (mut app, _commands) = app_with_history(20);
        let original = addressed_style(&mut app);
        let call = |app: &mut QuantickApp, input| {
            app.layer_wiring()
                .chart_settings(input)
                .expect("the call lands")
        };
        call(
            &mut app,
            ChartSettingsInput::Open {
                target: None,
                tab: ChartSettingsTab::Candles,
            },
        );
        assert!(recorded.named("CANDLE_STYLE_CHANGED").is_empty());
        let changed = call(
            &mut app,
            ChartSettingsInput::ApplyCandlePreset {
                target: None,
                name: "glass".to_owned(),
            },
        );
        assert!(changed.changed);
        assert_eq!(
            addressed_style(&mut app).candles,
            CandlePreset::Glass.style()
        );
        let lines = recorded.named("CANDLE_STYLE_CHANGED");
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(lines[0]["preset"], CandlePreset::Glass.log_value());
        assert_eq!(lines[0]["revision"], app.runtime.style_revision.to_string());
        let unchanged = call(
            &mut app,
            ChartSettingsInput::ApplyCandlePreset {
                target: None,
                name: "glass".to_owned(),
            },
        );
        assert!(!unchanged.changed);
        assert_eq!(recorded.named("CANDLE_STYLE_CHANGED").len(), 1);
        assert!(call(&mut app, ChartSettingsInput::ResetCandles { target: None }).changed);
        assert_eq!(addressed_style(&mut app), original);
        assert_eq!(
            recorded.named("CANDLE_STYLE_CHANGED").len(),
            2,
            "reset is a change too"
        );
    });
}

#[test]
fn a_keyed_local_candle_update_checks_authority_and_retries_without_reapplying() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 20);
    let target = json!({"tab_id":app.runtime.tabs.active_id().to_string(),
        "pane_id":app.active_tab().time_pane().unwrap().id.to_string()});
    let directory = gateway_test_directory("local-candle-retry");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = success_result(&settings_call(
        &mut app,
        &mut cockpit,
        json!({"action":"inspect", "target":target}),
    ));
    let mut style = before["effective_style"].clone();
    style["candles"]["gap_px"] = json!("5");
    let input = json!({"action":"update_candles", "target":target, "style":style});
    let denied = settings_call(&mut app, &mut observer, input.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let revision = app.runtime.style_revision;
    let (first, served) = keyed_call(
        &mut app,
        &mut cockpit,
        "local-style-first",
        CAPABILITY,
        input.clone(),
        "local-style",
    );
    assert_eq!(error_code(&first), None);
    assert!(served.iter().any(|request| request.began));
    assert!(app.runtime.style_revision > revision);
    let sibling = json!({"tab_id":app.runtime.tabs.active_id().to_string(),
        "pane_id":app.active_tab().flow_pane.id.to_string()});
    settings_call(
        &mut app,
        &mut cockpit,
        json!({"action":"apply_candle_preset", "target":sibling, "name":"classic"}),
    );
    let sibling_before = success_result(&settings_call(
        &mut app,
        &mut cockpit,
        json!({"action":"inspect", "target":sibling}),
    ))["effective_style"]
        .clone();
    let revision = app.runtime.style_revision;
    let (retry, served) = keyed_call(
        &mut app,
        &mut cockpit,
        "local-style-retry",
        CAPABILITY,
        input,
        "local-style",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert!(
        served.is_empty(),
        "the keyed retry never reaches the mutation adapter"
    );
    assert_eq!(app.runtime.style_revision, revision);
    let after = success_result(&settings_call(
        &mut app,
        &mut cockpit,
        json!({"action":"inspect", "target":target}),
    ));
    assert_eq!(after["effective_style"], style);
    let sibling_after = success_result(&settings_call(
        &mut app,
        &mut cockpit,
        json!({"action":"inspect", "target":sibling}),
    ));
    assert_eq!(sibling_after["effective_style"], sibling_before);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A double click on the lane divider is a grab of the handle, not a request
/// for the chart settings — the same exclusion the drag makes.
#[test]
fn a_double_click_on_the_lane_divider_opens_no_settings() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    app.active_tab_mut().tape_mut().set_lane_enabled(true);
    app.active_tab_mut().tape_mut().record_trade(&trade(201));
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the canvas laid out");
    let divider = pane.frame.lane_divider_x.expect("the tape lane is visible");
    // Just on the candles' side of the line, inside the handle.
    let at = egui::pos2(divider - 1.0, chart.center().y);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    click_sized(&mut app, &ctx, TEST_WINDOW, at);
    run_frame_at(&mut app, &ctx, TEST_WINDOW);
    assert!(
        !app.surfaces.chart_settings.is_open(),
        "the double click at {at:?} on the divider at {divider} opened the settings"
    );
}

#[test]
fn the_frame_publishes_chart_and_new_drawing_defaults_together() {
    use quantick_appearance::{Inheritance, Token};
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let tool = crate::drawings::DrawingTool::by_id("text").unwrap();
    let old = crate::drawings::new_drawing_from_defaults(&app.drawings.presets, tool);
    let old_fill = app.runtime.style.candles.bull_fill;
    app.runtime.style.ownership.bull_fill = Inheritance::Explicit;
    app.runtime.style.candles.wick_width = 3.0;
    app.runtime
        .appearance
        .apply_candidate(quantick_appearance::LIGHT, false)
        .unwrap();
    run_frame(&mut app, &ctx);
    let theme = app.runtime.appearance.resolved();
    assert_eq!(crate::theme::get(&ctx).resolved, *theme);
    assert_eq!(
        app.runtime.style.canvas.background_rgba(),
        theme.color(Token::Canvas).to_array()
    );
    assert_eq!(app.runtime.style.candles.bull_fill, old_fill);
    assert_eq!(app.runtime.style.candles.wick_width, 3.0);
    let new = crate::drawings::new_drawing_from_defaults(&app.drawings.presets, tool);
    assert_eq!(new.style.color, theme.color(Token::DrawingDefault));
    assert_ne!(new.style.color, old.style.color);
    assert_eq!(old.style, tool.default_style());
    let revision = app.runtime.style_revision;
    run_frame(&mut app, &ctx);
    assert_eq!(app.runtime.style_revision, revision);
}
