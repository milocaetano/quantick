//! The control plane reads the native tape beside tape only.
use super::*;

/// The WIN source declaration, as a feed switch applies it.
fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_source_preset(Some("mini index regions"))
    );
}

/// The control plane reads both switches: the native tape the WIN source
/// opened, and tape only when a caller asks for the whole canvas.
#[test]
fn the_bubbles_scope_reads_back_the_native_tape_and_tape_only() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("native-tape-readback");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    native_split(&mut app);
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let mut read = |app: &mut QuantickApp| {
        let (read, _) = unkeyed_call(
            app,
            &mut observer,
            "snapshot.read",
            json!({ "scopes": ["orderflow.bubbles"] }),
        );
        let value = success_result(&read)["scopes"]["orderflow.bubbles"]["value"].clone();
        value["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tab| tab["panes"].as_array().unwrap().iter())
            .find(|pane| pane["pane_id"] == pane_id.as_str())
            .expect("the flow pane is in the bubbles scope")["bubbles"]
            .clone()
    };
    let beside = read(&mut app);
    assert_eq!(beside["native_tape"], true, "{beside}");
    assert_eq!(beside["tape_only"], false, "{beside}");
    let payload = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": pane_id,
        "layer_id": "tape_only",
        "visible": true,
    });
    let (response, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(success_result(&response)["changed"], true);
    let full = read(&mut app);
    assert_eq!(
        full["native_tape"], true,
        "tape only is the native tape: {full}"
    );
    assert_eq!(full["tape_only"], true, "{full}");
    disable_test_gateway(&mut app, &ctx);
}

fn set_layer(app: &mut QuantickApp, cockpit: &mut LocalClient, layer: &str, on: bool) -> Value {
    let payload = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "layer_id": layer,
        "visible": on,
    });
    let (response, _) = unkeyed_call(app, cockpit, "layers.visibility.set", payload);
    success_result(&response)["layer"].clone()
}

/// The native tape the bubbles scope says the pane builds.
fn built(app: &mut QuantickApp, observer: &mut LocalClient) -> Value {
    let scopes = json!({ "scopes": ["orderflow.bubbles"] });
    let (read, _) = unkeyed_call(app, observer, "snapshot.read", scopes);
    let value = success_result(&read)["scopes"]["orderflow.bubbles"]["value"].clone();
    let pane_id = app.active_tab().flow_pane.id.to_string();
    value["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap().iter())
        .find(|pane| pane["pane_id"] == pane_id.as_str())
        .expect("the flow pane is in the bubbles scope")["bubbles"]["native_tape"]
        .clone()
}

/// The native tape layer as the layers scope reads it back.
fn native_layer(app: &mut QuantickApp, observer: &mut LocalClient) -> Value {
    let scopes = json!({ "scopes": ["layers.visibility"] });
    let (read, _) = unkeyed_call(app, observer, "snapshot.read", scopes);
    let value = success_result(&read)["scopes"]["layers.visibility"]["value"].clone();
    value["panes"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == "native_tape")
        .expect("the native tape layer is read back")
        .clone()
}

/// The native tape is switched by the layer call exactly like tape only, and
/// read back twice: the layer readback carries the request, the bubbles scope
/// the tape the pane actually builds. Asked for without volume dots, or with
/// the tape off, the pane builds the ordinary lane and says why.
#[test]
fn the_native_tape_is_switched_by_the_layer_call_and_read_back_as_built() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("native-tape-switch");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let requested = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        flow.expect("the flow pane has an engine")
            .cached_config()
            .live_lane
            .native_tape
    };
    set_layer(&mut app, &mut cockpit, "bubble_overlap_merge", true);
    for on in [true, false] {
        let layer = set_layer(&mut app, &mut cockpit, "native_tape", on);
        assert_eq!(layer["requested"], on, "{layer}");
        assert_eq!(layer["effective"], on, "{layer}");
        assert_eq!(layer["persistence"], "orderflow_preset", "{layer}");
        assert_eq!(requested(&app), on, "the call set the pane's own switch");
        assert_eq!(built(&mut app, &mut observer), on);
    }

    set_layer(&mut app, &mut cockpit, "native_tape", true);
    set_layer(&mut app, &mut cockpit, "bubble_overlap_merge", false);
    let layer = native_layer(&mut app, &mut observer);
    assert_eq!(layer["requested"], true, "the request is kept: {layer}");
    assert_eq!(layer["effective"], false, "no execution tape: {layer}");
    assert_eq!(layer["blocked_reason"], "native_tape_needs_volume_dots");
    assert_eq!(
        built(&mut app, &mut observer),
        false,
        "the ordinary lane is what is built"
    );
    let payload = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "layer_id": "native_tape",
        "visible": false,
    });
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
    assert_eq!(
        error_code(&refused),
        Some(codes::CAPABILITY_UNAVAILABLE),
        "a blocked switch names why instead of moving"
    );

    set_layer(&mut app, &mut cockpit, "bubble_overlap_merge", true);
    assert_eq!(built(&mut app, &mut observer), true);
    set_layer(&mut app, &mut cockpit, "tape_chart", false);
    assert_eq!(
        built(&mut app, &mut observer),
        false,
        "with the tape off nothing is native"
    );
    assert!(requested(&app), "and the request waits for the tape");
    disable_test_gateway(&mut app, &ctx);
}

/// Under tape only the surfaces tell one story: the pane builds the native
/// tape (bubbles scope), and the switch has nothing to change there, so the
/// layer is blocked and names why (layers scope), the call is refused in
/// either direction with that reason, and the request is left as it was.
/// The settings box is locked with the same reason
/// (`a_blocked_native_tape_layer_locks_its_box_with_the_layers_reason`).
#[test]
fn under_tape_only_the_native_tape_layer_is_blocked_and_says_why() {
    const REASON: &str = "tape_only_always_draws_the_native_tape";
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("native-tape-under-tape-only");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let requested = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        flow.expect("the flow pane has an engine")
            .cached_config()
            .live_lane
            .native_tape
    };
    native_split(&mut app);
    set_layer(&mut app, &mut cockpit, "tape_only", true);
    assert_eq!(built(&mut app, &mut observer), true, "tape only is native");
    let layer = native_layer(&mut app, &mut observer);
    assert_eq!(layer["blocked_reason"], REASON, "{layer}");
    assert_eq!(layer["requested"], true, "the preset's request: {layer}");
    assert_eq!(layer["effective"], false, "{layer}");
    for visible in [false, true] {
        let payload = json!({
            "tab_id": app.tabs.active_id().to_string(),
            "pane_id": app.active_tab().flow_pane.id.to_string(),
            "layer_id": "native_tape",
            "visible": visible,
        });
        let (refused, _) = unkeyed_call(&mut app, &mut cockpit, "layers.visibility.set", payload);
        assert_eq!(error_code(&refused), Some(codes::CAPABILITY_UNAVAILABLE));
        let ResponseOutcome::Failure { error } = refused.outcome else {
            panic!("a blocked switch is refused");
        };
        assert_eq!(error.context.details.unwrap()["reason"], REASON);
        assert!(requested(&app), "the request is not moved");
    }
    assert_eq!(built(&mut app, &mut observer), true);
    disable_test_gateway(&mut app, &ctx);
}
