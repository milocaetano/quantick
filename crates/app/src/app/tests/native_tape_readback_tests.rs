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
    read_layer(app, observer, "native_tape")
}

/// The flow pane's layer `id` as the layers scope reads it back.
fn read_layer(app: &mut QuantickApp, observer: &mut LocalClient, id: &str) -> Value {
    let scopes = json!({ "scopes": ["layers.visibility"] });
    let (read, _) = unkeyed_call(app, observer, "snapshot.read", scopes);
    let value = success_result(&read)["scopes"]["layers.visibility"]["value"].clone();
    value["panes"][0]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == id)
        .unwrap_or_else(|| panic!("layer {id} is read back"))
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

/// A frame after the order-flow worker has answered the one before it.
fn settled_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame(app, ctx);
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(app, ctx)
}

/// The per-candle summary's marks a frame painted: its one translucent colour
/// per side, as a disc or as a pie.
fn summary_marks(output: &egui::FullOutput) -> Vec<egui::Shape> {
    let colours = [crate::theme::BUY, crate::theme::SELL].map(|colour| colour.gamma_multiply(0.35));
    output
        .shapes
        .iter()
        .map(|shape| &shape.shape)
        .filter(|shape| match shape {
            egui::Shape::Circle(disc) => colours.contains(&disc.fill),
            egui::Shape::Mesh(mesh) => {
                !mesh.vertices.is_empty()
                    && mesh
                        .vertices
                        .iter()
                        .all(|vertex| colours.contains(&vertex.color))
            }
            _ => false,
        })
        .cloned()
        .collect()
}

/// Every other disc a frame painted left of `x`: the candles' own per-print
/// bubbles, and whatever the frame draws there with them switched off.
fn other_discs_left_of(output: &egui::FullOutput, x: f32) -> usize {
    let summary = summary_marks(output);
    output
        .shapes
        .iter()
        .filter(|shape| matches!(&shape.shape, egui::Shape::Circle(disc) if disc.center.x < x))
        .filter(|shape| !summary.contains(&shape.shape))
        .count()
}

/// The trader's one candle switch beside the native tape. The aggression
/// bubbles stay available there and draw the per-candle summary on the tick
/// candles, once, whether or not candle aggression asks too; the layer call,
/// the readback, the settings switch and the paint agree. Off, the candles
/// carry nothing; with the tape off, their own per-print bubbles return.
#[test]
fn beside_the_native_tape_the_bubbles_switch_draws_the_candle_summary() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    let directory = gateway_test_directory("native-tape-candle-bubbles");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    native_split(&mut app);
    let switched_on = |app: &QuantickApp| app.active_tab().tape().cached_config().show_aggressions;
    assert!(switched_on(&app), "the trader's candle switch is on");

    // Beside the native tape: available, effective, and drawn as the summary.
    let bubbles = read_layer(&mut app, &mut observer, "bubbles");
    assert_eq!(bubbles["requested"], true, "{bubbles}");
    assert_eq!(bubbles["effective"], true, "{bubbles}");
    assert_eq!(bubbles["blocked_reason"], Value::Null, "{bubbles}");
    let candle_aggression = read_layer(&mut app, &mut observer, "candle_aggression");
    assert_eq!(candle_aggression["requested"], false, "{candle_aggression}");
    let frame = settled_frame(&mut app, &ctx);
    let summary = summary_marks(&frame);
    assert!(
        !summary.is_empty(),
        "the bubbles switch draws the per-candle summary on the candles"
    );
    let divider = app
        .active_tab()
        .flow_pane
        .frame
        .lane_divider_x
        .expect("the tape beside the candles");
    let beside = other_discs_left_of(&frame, divider);

    // Candle aggression asked for too: the same marks, drawn once.
    let layer = set_layer(&mut app, &mut cockpit, "candle_aggression", true);
    assert_eq!(layer["effective"], true, "{layer}");
    assert_eq!(
        summary_marks(&settled_frame(&mut app, &ctx)),
        summary,
        "one summary whichever switch asks for it, never two"
    );
    set_layer(&mut app, &mut cockpit, "candle_aggression", false);

    // Off: the call moves the settings switch, and the candles carry nothing.
    let layer = set_layer(&mut app, &mut cockpit, "bubbles", false);
    assert_eq!(layer["requested"], false, "{layer}");
    assert_eq!(layer["effective"], false, "{layer}");
    assert!(!switched_on(&app), "the settings switch is the layer's own");
    let frame = settled_frame(&mut app, &ctx);
    assert!(
        summary_marks(&frame).is_empty(),
        "bubbles off draws nothing"
    );
    assert_eq!(
        other_discs_left_of(&frame, divider),
        beside,
        "beside the native tape the candles never carry per-print bubbles"
    );

    // The tape off: the original tick chart, with its own per-print bubbles.
    set_layer(&mut app, &mut cockpit, "tape_chart", false);
    let right = app.active_tab().flow_pane.frame.chart_rect.unwrap().right();
    let without = other_discs_left_of(&settled_frame(&mut app, &ctx), right);
    let layer = set_layer(&mut app, &mut cockpit, "bubbles", true);
    assert_eq!(layer["effective"], true, "{layer}");
    let frame = settled_frame(&mut app, &ctx);
    assert!(
        summary_marks(&frame).is_empty(),
        "without the native tape the switch draws main's bubbles, not the summary"
    );
    assert!(
        other_discs_left_of(&frame, right) > without,
        "the candles' per-print bubbles return with the tape off"
    );
    disable_test_gateway(&mut app, &ctx);
}
