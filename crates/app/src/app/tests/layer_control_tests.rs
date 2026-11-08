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
#[test]
fn additive_layer_traverses_discovery_admission_and_common_operation() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.active_tab_mut().flow_pane.install_layer_probe();
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("layer-probe");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let initial = snapshot(&mut app, &mut observer);
    let layers = initial["panes"][0]["layers"].as_array().unwrap();
    assert_eq!(layers.len(), 27);
    assert!(layers.iter().any(|layer| layer["id"] == "test_probe"
        && layer["requested"] == false
        && layer["effective"] == false));
    let payload = layer_input(&app, "test_probe", true);
    let (denied, _) = unkeyed_call(
        &mut app,
        &mut observer,
        "layers.visibility.set",
        payload.clone(),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    for visible in [true, false] {
        let payload = layer_input(&app, "test_probe", visible);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        assert_eq!(success_result(&response)["layer"]["requested"], visible);
        assert_eq!(success_result(&response)["layer"]["effective"], visible);
    }
    let payload = layer_input(&app, "unknown_layer", true);
    let (unknown, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&unknown), Some(codes::INVALID_REQUEST));
    let mut payload = layer_input(&app, "grid", false);
    payload["pane_id"] = json!(u64::MAX.to_string());
    let (stale, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&stale), Some(codes::INVALID_REQUEST));
    let payload = layer_input(&app, "grid", false);
    let (grid, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(success_result(&grid)["layer"]["scope"], "pane");
    assert!(
        !app.active_tab()
            .flow_pane
            .layer_switched_on(ChartLayer::Grid, &app.runtime.style)
    );
    let payload = layer_input(&app, "tape_chart", false);
    let (off, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert!(matches!(off.outcome, ResponseOutcome::Success { .. }));
    let payload = layer_input(&app, "tape_heatmap", true);
    let (blocked, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(error_code(&blocked), Some(codes::CAPABILITY_UNAVAILABLE));
    let ResponseOutcome::Failure { error } = blocked.outcome else {
        panic!("blocked tape child");
    };
    assert_eq!(error.context.details.unwrap()["reason"], "the_tape_is_off");
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn omitted_pane_mutations_have_correlated_once_only_and_noop_journal_readback() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    // More panes than the snapshot budget, using disconnected in-memory feeds.
    for id in 2..=66 {
        let (other, _, _, _) = test_app();
        let mut tab = other.runtime.tabs.into_single_runtime();

        tab.flow_pane.id = id + 1000;
        let opening = app.runtime.tabs.plan_open();
        app.runtime.tabs.append(opening, tab);
    }
    let target = app.runtime.tabs.last().unwrap();
    let tab_id = app
        .runtime
        .tabs
        .id_at(app.runtime.tabs.len() - 1)
        .to_string();
    let pane_id = target.flow_pane.id.to_string();
    let visible = !target
        .flow_pane
        .layer_switched_on(ChartLayer::Crosshair, &app.runtime.style);
    let directory = gateway_test_directory("layers-omitted-readback");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let original_connection = client.handshake().connection_id.clone();
    let captured = snapshot(&mut app, &mut client);
    assert_eq!(captured["panes"].as_array().unwrap().len(), 64);
    assert_ne!(captured["omitted_panes"], "0");
    assert!(
        !captured["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|pane| pane["tab_id"] == tab_id)
    );
    let (start, _) = unkeyed_call(
        &mut app,
        &mut client,
        "events.read",
        json!({ "start": "latest" }),
    );
    let cursor = success_result(&start)["next_cursor"].clone();
    let payload = json!({ "tab_id": tab_id, "pane_id": pane_id, "layer_id": "crosshair", "visible": visible });
    let (lost, first) = keyed_call(
        &mut app,
        &mut client,
        "first",
        "layers.visibility.set",
        payload.clone(),
        "layer-key",
    );
    let (retry, second) = keyed_call(
        &mut app,
        &mut client,
        "retry",
        "layers.visibility.set",
        payload.clone(),
        "layer-key",
    );
    assert_eq!(success_result(&lost)["changed"], true);
    assert_eq!(retry.outcome, lost.outcome);
    assert!(first.iter().any(|entry| entry.began));
    assert!(second.is_empty());
    drop(client);
    let mut reconciler = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let (page, _) = unkeyed_call(
        &mut app,
        &mut reconciler,
        "events.read",
        json!({ "cursor": cursor }),
    );
    let events: Vec<_> = success_result(&page)["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "layers.visibility.set")
        .cloned()
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["request_id"], "first");
    assert_eq!(
        events[0]["payload"]["connection_id"],
        json!(original_connection)
    );
    assert_eq!(events[0]["payload"]["result"]["tab_id"], tab_id);
    assert_eq!(events[0]["payload"]["result"]["pane_id"], pane_id);
    assert_eq!(
        events[0]["payload"]["result"]["layer"]["requested"],
        visible
    );
    assert_eq!(events[0]["actor"]["client_name"], CLIENT_NAME);
    let (noop, _) = keyed_call(
        &mut app,
        &mut reconciler,
        "noop",
        "layers.visibility.set",
        payload,
        "another-key",
    );
    assert_eq!(success_result(&noop)["changed"], false);
    let (page, _) = unkeyed_call(
        &mut app,
        &mut reconciler,
        "events.read",
        json!({ "cursor": success_result(&page)["next_cursor"] }),
    );
    let events: Vec<_> = success_result(&page)["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "layers.visibility.set")
        .cloned()
        .collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["payload"]["request_id"], "noop");
    assert_eq!(events[0]["payload"]["result"]["changed"], false);
    disable_test_gateway(&mut app, &ctx);
}

/// Volume dots are a registered display switch, so an agent reaches them by
/// the same named call as every other one — no mouse, no second vocabulary —
/// and reads the answer back from the bubbles scope.
#[test]
fn volume_dots_are_switched_by_the_layer_call() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("volume-dots-switch");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let merging = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        flow.expect("the flow pane has an engine")
            .cached_config()
            .volume_dots
            .enabled
    };
    assert!(!merging(&app), "off until someone asks");
    for on in [true, false] {
        let payload = layer_input(&app, "bubble_overlap_merge", on);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        let result = success_result(&response);
        assert_eq!(result["layer"]["requested"], on);
        assert_eq!(result["layer"]["persistence"], "orderflow_preset");
        assert_eq!(result["changed"], true);
        assert_eq!(merging(&app), on, "the call set the pane's own switch");
    }
    disable_test_gateway(&mut app, &ctx);
}

/// Tape only is a registered display switch too: one named call turns a
/// pane into the tape alone, the pane's own config carries it, and the
/// bubbles scope reads it back.
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
    let tape_only = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        flow.expect("the flow pane has an engine")
            .cached_config()
            .live_lane
            .tape_only
    };
    assert!(!tape_only(&app), "off until someone asks");
    for on in [true, false] {
        let payload = layer_input(&app, "tape_only", on);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        let result = success_result(&response);
        assert_eq!(result["layer"]["requested"], on);
        assert_eq!(result["layer"]["persistence"], "orderflow_preset");
        assert_eq!(result["changed"], true);
        assert_eq!(tape_only(&app), on, "the call set the pane's own switch");
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
    }
    disable_test_gateway(&mut app, &ctx);
}

/// The two grids are two named switches on the same control door, each read
/// back as its own layer, and the time grid becomes the trader's once set.
#[test]
fn price_and_time_grids_switch_apart_through_the_layer_door() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("grid-axes");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let requested = |snapshot: &Value, id: &str| {
        snapshot["panes"][0]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == id)
            .map(|layer| (layer["requested"].clone(), layer["persistence"].clone()))
            .unwrap()
    };
    let initial = snapshot(&mut app, &mut observer);
    assert_eq!(
        requested(&initial, "grid"),
        (json!(true), json!("workspace"))
    );
    assert_eq!(
        requested(&initial, "time_grid"),
        (json!(false), json!("workspace"))
    );
    for (id, visible) in [("time_grid", true), ("grid", false)] {
        let payload = layer_input(&app, id, visible);
        let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        assert_eq!(success_result(&response)["layer"]["requested"], visible);
        run_frame(&mut app, &ctx);
    }
    let pane = &app.active_tab().flow_pane;
    assert!(pane.layer_switched_on(ChartLayer::TimeGrid, &app.runtime.style));
    assert!(!pane.layer_switched_on(ChartLayer::Grid, &app.runtime.style));
    let authored = pane.presentation.candles.authored().unwrap();
    assert_eq!(
        authored.ownership.time_grid,
        quantick_appearance::Inheritance::Explicit,
        "a switched time grid no longer follows the theme"
    );
    let after = snapshot(&mut app, &mut observer);
    assert_eq!(requested(&after, "grid").0, json!(false));
    assert_eq!(requested(&after, "time_grid").0, json!(true));
    // A retry of the same switch is a no-op that reads back the same answer.
    let payload = layer_input(&app, "time_grid", true);
    let (again, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(success_result(&again)["layer"]["requested"], true);
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
    app.apply_menu_command(crate::app::menu_bar::MenuCommand::ToggleLayer(
        ChartLayer::TimeGrid,
    ));
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
