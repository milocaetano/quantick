//! Real gateway + real chart rendering; no adapter-specific layer vocabulary.
use super::*;
fn layer_input(app: &QuantickApp, id: &str, visible: bool) -> Value {
    json!({ "tab_id": app.runtime.tabs.active_id().to_string(), "pane_id": app.active_tab().flow_pane.id.to_string(), "layer_id": id, "visible": visible })
}
fn snapshot(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["layers.visibility"] }),
    );
    success_result(&response)["scopes"]["layers.visibility"]["value"].clone()
}
/// The registered layer `id` of the first pane in a `layers.visibility` read.
fn listed(snapshot: &Value, id: &str) -> Value {
    snapshot["panes"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == id)
        .unwrap_or_else(|| panic!("{id} is listed"))
        .clone()
}

/// The layer call through the window's adapter reaches the addressed pane's
/// own switches, whatever owns them: an additive probe the registry lists
/// with no adapter vocabulary, the two grids apart (the time grid then the
/// trader's, no longer the theme's); and a tape child the pane blocks is
/// refused there. What
/// the call decides is proven on the fake window in
/// `quantick-control-handlers`.
#[test]
fn the_layer_call_reaches_the_addressed_panes_own_switches() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.active_tab_mut().flow_pane.install_layer_probe();
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("layer-call-wiring");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let initial = snapshot(&mut app, &mut observer);
    assert_eq!(listed(&initial, "test_probe")["requested"], false);
    for (id, persistence) in [
        ("grid", "workspace"),
        ("time_grid", "workspace"),
        ("bubble_overlap_merge", "orderflow_preset"),
        ("tape_only", "orderflow_preset"),
    ] {
        assert_eq!(listed(&initial, id)["persistence"], persistence, "{id}");
    }
    for (id, visible) in [("test_probe", true), ("time_grid", true), ("grid", false)] {
        let payload = layer_input(&app, id, visible);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        success_result(&response);
    }
    run_frame(&mut app, &ctx);
    let pane = &app.active_tab().flow_pane;
    assert!(pane.layer_switched_on(ChartLayer::TimeGrid, &app.runtime.style));
    assert!(!pane.layer_switched_on(ChartLayer::Grid, &app.runtime.style));
    let authored = pane.presentation.candles.authored().unwrap();
    assert_eq!(
        authored.ownership.time_grid,
        quantick_appearance::Inheritance::Explicit,
        "a switched time grid no longer follows the theme"
    );
    let probe = listed(&snapshot(&mut app, &mut observer), "test_probe");
    assert_eq!(
        (&probe["requested"], &probe["effective"]),
        (&json!(true), &json!(true))
    );

    let payload = layer_input(&app, "tape_chart", false);
    let (off, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    success_result(&off);
    let heatmap = listed(&snapshot(&mut app, &mut observer), "tape_heatmap");
    let payload = layer_input(&app, "tape_heatmap", true);
    let (blocked, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&blocked), Some(codes::CAPABILITY_UNAVAILABLE));
    let ResponseOutcome::Failure { error } = blocked.outcome else {
        panic!("blocked tape child");
    };
    assert_eq!(error.context.details.unwrap()["reason"], "the_tape_is_off");
    assert_eq!(
        listed(&snapshot(&mut app, &mut observer), "tape_heatmap"),
        heatmap,
        "the refusal switched nothing"
    );
    disable_test_gateway(&mut app, &ctx);
}

/// Volume dots and tape only are registered display switches: the layer
/// call turns each on and off in the pane's own config, and the bubbles
/// scope reads the same switch back.
#[test]
fn tape_only_is_switched_by_the_layer_call_and_read_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("tape-only-switch");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let switches = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        let config = flow.expect("the flow pane has an engine").cached_config();
        (config.volume_dots.enabled, config.live_lane.tape_only)
    };
    assert_eq!(switches(&app), (false, false), "off until someone asks");
    for on in [true, false] {
        for id in ["bubble_overlap_merge", "tape_only"] {
            let payload = layer_input(&app, id, on);
            let (response, _) =
                unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
            success_result(&response);
        }
        assert_eq!(switches(&app), (on, on), "the pane's own switches");
        let (read, _) = unkeyed_call(
            &mut app,
            &mut observer,
            "snapshot.read",
            json!({ "scopes": ["orderflow.bubbles"] }),
        );
        let value = success_result(&read)["scopes"]["orderflow.bubbles"]["value"].clone();
        let pane = value["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tab| tab["panes"].as_array().unwrap().iter())
            .find(|pane| pane["pane_id"] == pane_id.as_str())
            .expect("the flow pane is in the bubbles scope")
            .clone();
        assert_eq!(pane["bubbles"]["tape_only"], on, "{pane}");
        assert_eq!(pane["bubbles"]["overlap_merge"], on, "{pane}");
    }
    disable_test_gateway(&mut app, &ctx);
}

/// A fresh Light chart paints its time grid from the theme; the control
/// read, the set call, the menu bar and the pane's paint all agree on it.
#[test]
fn light_time_grid_reads_and_switches_as_the_theme_paints_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.runtime
        .appearance
        .apply_candidate(quantick_appearance::LIGHT, false)
        .unwrap();
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("light-time-grid");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let pane = &app.active_tab().flow_pane;
    let painted = pane
        .plot_style(&app.runtime.style, &ctx)
        .canvas
        .time_grid_rgba()
        .is_some();
    assert!(painted, "Light paints its time grid on a fresh chart");
    let time_grid = |snapshot: &Value| {
        snapshot["panes"][0]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == "time_grid")
            .map(|layer| layer["requested"].clone())
            .unwrap()
    };
    assert_eq!(time_grid(&snapshot(&mut app, &mut observer)), json!(true));
    let payload = layer_input(&app, "time_grid", true);
    let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(
        success_result(&response)["changed"],
        json!(false),
        "switching on what the theme already paints changes nothing"
    );
    disable_test_gateway(&mut app, &ctx);
    let toggle = crate::app::menu_bar::MenuCall::ToggleLayer(ChartLayer::TimeGrid);
    crate::app::menu_bar::apply_menu_command_on(
        &mut app,
        &ctx,
        crate::app::menu_bar::MenuCommand::Call(toggle),
    );
    run_frame(&mut app, &ctx);
    let pane = app.focused_pane();
    assert!(
        !pane
            .plot_style(&app.runtime.style, &ctx)
            .canvas
            .time_grid_rgba()
            .is_some(),
        "the menu bar's first click hides the painted time grid"
    );
    assert!(!pane.layer_switched_on(ChartLayer::TimeGrid, &app.runtime.style));
}

/// The health line speaks for the focused pane's resolved grids, the same
/// answer the `layers.visibility` read gives, on either theme and whichever
/// of the trader's switches the pane holds.
#[test]
fn health_summary_grids_agree_with_the_layers_read_on_light_and_dark() {
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    for (theme, name) in [
        (quantick_appearance::LIGHT, "light"),
        (quantick_appearance::DARK, "dark"),
    ] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(4);
        app.runtime
            .appearance
            .apply_candidate(theme, false)
            .unwrap();
        run_frame(&mut app, &ctx);
        let directory = gateway_test_directory(&format!("health-grids-{name}"));
        grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
        enable_test_gateway(&mut app, &ctx, &directory, 4);
        let mut observer = connect(&directory, &options("observer", &[]));
        let mut cockpit = connect(
            &directory,
            &options("cockpit", &["cockpit", "cockpit.layout"]),
        );
        let mut at = app.health.last_summary;
        let mut read = |app: &mut QuantickApp, observer: &mut LocalClient| {
            let layers = snapshot(app, observer);
            let on = |id: &str| {
                layers["panes"][0]["layers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|layer| layer["id"] == id)
                    .map(|layer| layer["requested"].as_bool().unwrap())
                    .unwrap()
            };
            let log = Arc::new(Mutex::new(Vec::new()));
            let sink = Sink(log.clone());
            let subscriber = tracing_subscriber::fmt()
                .json()
                .without_time()
                .with_writer(move || sink.clone())
                .finish();
            at += std::time::Duration::from_secs(2);
            tracing::subscriber::with_default(subscriber, || app.maybe_emit_summary(at, &ctx));
            let text = String::from_utf8(log.lock().unwrap().clone()).unwrap();
            let row = text
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .find(|row| row["fields"]["event_code"] == "APP_HEALTH_SUMMARY")
                .expect("a health summary was due");
            (
                (on("grid"), on("time_grid")),
                (
                    row["fields"]["chart_grid_enabled"].as_bool().unwrap(),
                    row["fields"]["chart_time_grid_enabled"].as_bool().unwrap(),
                ),
            )
        };
        let (layers, health) = read(&mut app, &mut observer);
        assert_eq!(health, layers, "{name} fresh pane");
        assert_eq!(
            layers.1,
            name == "light",
            "{name} time grid follows the theme"
        );
        for (id, visible) in [
            ("time_grid", !layers.1),
            ("grid", !layers.0),
            ("time_grid", layers.1),
        ] {
            let payload = layer_input(&app, id, visible);
            unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
            run_frame(&mut app, &ctx);
            let (layers, health) = read(&mut app, &mut observer);
            assert_eq!(health, layers, "{name} after {id}={visible}");
        }
        disable_test_gateway(&mut app, &ctx);
    }
}
