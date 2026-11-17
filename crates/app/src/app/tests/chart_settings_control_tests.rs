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
    assert!(!app.surfaces.chart_settings.model_state.is_open());
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
            .model_state
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
    app.layer_wiring().open_settings(ChartSettingsTab::Candles);
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
            .model_state
            .is_open_on(ChartSettingsTab::Footprint),
        "one window, switched to the Footprint tab"
    );
    app.surfaces.chart_settings.model_state.close();
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
            .model_state
            .is_open_on(ChartSettingsTab::Footprint)
    );
    // Settings → Appearance… flips the same window to the Candles tab.
    let appearance = quantick_control_handlers::settings::SettingsAct::OpenAppearance;
    quantick_control_handlers::settings::SettingsOwner::settle(&mut desk_adapter!(app), appearance)
        .unwrap();
    assert!(
        app.surfaces
            .chart_settings
            .model_state
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

/// The look of the pane `target` names, as `inspect` reads it back.
fn inspected(app: &mut QuantickApp, client: &mut LocalClient, target: &Value) -> Value {
    let inspect = json!({"action":"inspect", "target":target});
    success_result(&settings_call(app, client, inspect))["effective_style"].clone()
}

/// The settings call through the window's adapter reaches the window and the
/// addressed pane's own look: it opens the window there, an update lands on
/// that pane alone while the legacy global seed stays, a look saved by name
/// lands in the app's own presets file, and close closes it.
/// What the call decides is proven on a sitting and the fake window in
/// `quantick-control-handlers`.
#[test]
fn the_settings_call_reaches_the_window_and_the_addressed_panes_look_only() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 20);
    let tab_id = app.runtime.tabs.active_id().to_string();
    let time_pane = app.active_tab().time_pane().unwrap().id.to_string();
    let target = json!({"tab_id": tab_id, "pane_id": time_pane});
    let sibling = json!({"tab_id": tab_id,
        "pane_id": app.active_tab().flow_pane.id.to_string()});
    let directory = gateway_test_directory("chart-settings-wiring");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let legacy = app.runtime.style;
    let sibling_before = inspected(&mut app, &mut cockpit, &sibling);
    let open = json!({"action":"open", "tab":"footprint", "target":target});
    success_result(&settings_call(&mut app, &mut cockpit, open));
    let window = &app.surfaces.chart_settings.model_state;
    assert!(window.is_open_on(ChartSettingsTab::Footprint));

    let mut style = inspected(&mut app, &mut cockpit, &target);
    style["candles"]["gap_px"] = json!("5");
    let revision = app.runtime.style_revision;
    let update = json!({"action":"update_candles", "target":target, "style":style});
    success_result(&settings_call(&mut app, &mut cockpit, update));
    assert!(app.runtime.style_revision > revision);
    assert_eq!(inspected(&mut app, &mut cockpit, &target), style);
    assert_eq!(
        inspected(&mut app, &mut cockpit, &sibling),
        sibling_before,
        "the sibling pane keeps its look"
    );
    assert_eq!(
        app.runtime.style, legacy,
        "local settings preserve the legacy global seed"
    );
    let save = json!({"action":"save_candle_preset", "name":"night", "target":target});
    success_result(&settings_call(&mut app, &mut cockpit, save));
    let night: ChartStyle = quantick_control_schema::chart_settings::style_wire::decode(style)
        .unwrap()
        .into();
    assert_eq!(
        crate::candle_presets::CandlePresetStore::load(&crate::candle_presets::default_path())
            .get("night"),
        Some(&night.snapshot()),
        "the saved look is in the app's presets file"
    );
    let close = json!({"action":"close"});
    success_result(&settings_call(&mut app, &mut cockpit, close));
    assert!(!app.surfaces.chart_settings.model_state.is_open());
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
        !app.surfaces.chart_settings.model_state.is_open(),
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
    let old = crate::drawings::new_drawing_from_defaults(
        &app.drawings.presets,
        quantick_appearance::ChartSurface::Paper,
        tool,
    );
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
        theme.color(Token::ChartCanvas).to_array()
    );
    assert_eq!(app.runtime.style.candles.bull_fill, old_fill);
    assert_eq!(app.runtime.style.candles.wick_width, 3.0);
    let new = crate::drawings::new_drawing_from_defaults(
        &app.drawings.presets,
        quantick_appearance::ChartSurface::Paper,
        tool,
    );
    assert_eq!(new.style.color, theme.color(Token::DrawingDefault));
    assert_ne!(new.style.color, old.style.color);
    assert_eq!(old.style, tool.default_style());
    let revision = app.runtime.style_revision;
    run_frame(&mut app, &ctx);
    assert_eq!(app.runtime.style_revision, revision);
}
