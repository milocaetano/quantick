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
    assert_eq!(full["native_tape"], true, "tape only is the native tape: {full}");
    assert_eq!(full["tape_only"], true, "{full}");
    disable_test_gateway(&mut app, &ctx);
}
