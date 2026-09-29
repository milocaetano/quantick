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

/// The native tape is switched by the layer call exactly like tape only, and
/// read back twice: the layer readback carries the request, the bubbles scope
/// the tape the pane actually builds. Asked for without volume dots, or with
/// the tape off, the pane builds the ordinary lane and says so.
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
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let mut set = |app: &mut QuantickApp, layer: &str, visible: bool| {
        let payload = json!({
            "tab_id": app.tabs.active_id().to_string(),
            "pane_id": pane_id,
            "layer_id": layer,
            "visible": visible,
        });
        let (response, _) = unkeyed_call(app, &mut cockpit, "layers.visibility.set", payload);
        success_result(&response)["layer"].clone()
    };
    let mut built = |app: &mut QuantickApp| {
        let (read, _) = unkeyed_call(
            app,
            &mut observer,
            "snapshot.read",
            json!({ "scopes": ["orderflow.bubbles"] }),
        );
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
    };
    let requested = |app: &QuantickApp| {
        let flow = app.active_tab().flow_pane.orderflow.as_ref();
        flow.expect("the flow pane has an engine")
            .cached_config()
            .live_lane
            .native_tape
    };
    set(&mut app, "bubble_overlap_merge", true);
    for on in [true, false] {
        let layer = set(&mut app, "native_tape", on);
        assert_eq!(layer["requested"], on, "{layer}");
        assert_eq!(layer["effective"], on, "{layer}");
        assert_eq!(layer["persistence"], "orderflow_preset", "{layer}");
        assert_eq!(requested(&app), on, "the call set the pane's own switch");
        assert_eq!(built(&mut app), on);
    }

    set(&mut app, "native_tape", true);
    let layer = set(&mut app, "bubble_overlap_merge", false);
    assert_eq!(layer["requested"], false);
    let layer = set(&mut app, "native_tape", true);
    assert_eq!(layer["requested"], true, "the request is kept: {layer}");
    assert_eq!(layer["effective"], false, "no execution tape: {layer}");
    assert_eq!(built(&mut app), false, "the ordinary lane is what is built");

    set(&mut app, "bubble_overlap_merge", true);
    assert_eq!(built(&mut app), true);
    set(&mut app, "tape_chart", false);
    assert_eq!(
        built(&mut app),
        false,
        "with the tape off nothing is native"
    );
    assert!(requested(&app), "and the request waits for the tape");
    disable_test_gateway(&mut app, &ctx);
}
