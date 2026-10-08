//! The control plane reads the native tape beside tape only.
use super::*;

/// The WIN source declaration, as a feed switch applies it.
fn native_split(app: &mut QuantickApp) {
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions")
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

fn settled_flow_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let output = settled_frame(app, ctx);
        let owner = app.active_tab().tape();
        if owner.flow_execution_frame().is_some() && !owner.flow_execution_progress().pending {
            return output;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "FLOW must finish the requested source"
        );
        std::thread::yield_now();
    }
}

fn flow_snapshot(app: &mut QuantickApp, observer: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let (read, _) = unkeyed_call(
        app,
        observer,
        "snapshot.read",
        json!({"scopes":["orderflow.bubbles"]}),
    );
    success_result(&read)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap())
        .find(|pane| pane["pane_id"] == pane_id)
        .unwrap()["flow_execution"]
        .clone()
}

fn assert_complete_flow_fixture(app: &mut QuantickApp, observer: &mut LocalClient) -> Value {
    let snapshot = flow_snapshot(app, observer);
    // app_with_history(200): one contract each, alternating sides, prices 100..101.9.
    assert_eq!(snapshot["buy_quantity"], "100");
    assert_eq!(snapshot["sell_quantity"], "100");
    assert_eq!(snapshot["trade_count"], "200");
    assert_eq!(snapshot["worker"]["loaded_executions"], "200");
    assert_eq!(snapshot["worker"]["pending"], false);
    assert_eq!(snapshot["omitted_executions"], "0");
    assert_eq!(snapshot["off_axis_executions"], "0");
    assert_eq!(snapshot["offscreen_executions"], "0");
    let owner = app.active_tab().tape();
    let painted = quantick_control_schema::orderflow::FlowExecutionSnapshot::from((
        owner.flow_execution_frame().unwrap(),
        owner.flow_execution_progress(),
    ));
    assert_eq!(snapshot, serde_json::to_value(painted).unwrap());
    snapshot
}

fn fit_flow_fixture(app: &mut QuantickApp) {
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.model.viewport.set_px_per_bar(1.0);
    assert!(pane.price_view.set_manual_range(99.0, 103.0));
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

fn regional_colours(app: &QuantickApp, context: bool) -> [egui::Color32; 2] {
    regional_colours_at_opacity(app, if context { 0.12 } else { 0.65 })
}

fn regional_colours_at_opacity(app: &QuantickApp, opacity: f32) -> [egui::Color32; 2] {
    [
        egui::Color32::from_rgb(112, 185, 244),
        egui::Color32::from_rgb(232, 175, 99),
    ]
    .map(|color| {
        let source = color.gamma_multiply(opacity);
        // Independently model the old source-over-opaque-canvas GPU result.
        let rgb = std::array::from_fn(|channel| {
            (f64::from(source[channel])
                + f64::from(app.style.canvas.background[channel])
                    * (1.0 - f64::from(source.a()) / 255.0))
                .round() as u8
        });
        let [r, g, b] = rgb;
        egui::Color32::from_rgb(r, g, b)
    })
}

/// The packed regional sector mesh in history, excluding candle outlines and Tape.
fn regional_circles(app: &QuantickApp, output: &egui::FullOutput) -> Vec<egui::Mesh> {
    let flow = app.active_tab().tape().flow_execution_frame().unwrap();
    let colours: Vec<_> = flow
        .dots
        .iter()
        .flat_map(|dot| regional_colours_at_opacity(app, flow.ordinary_region_opacity(dot)))
        .collect();
    regional_circles_with_colours(app, output, &colours)
}

fn regional_circles_with_colours(
    app: &QuantickApp,
    output: &egui::FullOutput,
    colours: &[egui::Color32],
) -> Vec<egui::Mesh> {
    let frame = &app.active_tab().flow_pane.frame;
    let rect = frame.chart_rect.unwrap();
    let history = rect.with_max_x(frame.lane_divider_x.unwrap_or(rect.right()));
    output
        .shapes
        .iter()
        .filter_map(|clipped| {
            let egui::Shape::Mesh(mesh) = &clipped.shape else {
                return None;
            };
            (!mesh.vertices.is_empty()
                && mesh
                    .vertices
                    .iter()
                    .all(|vertex| colours.contains(&vertex.color))
                && mesh
                    .calc_bounds()
                    .intersect(clipped.clip_rect)
                    .intersect(history)
                    .is_positive())
            .then(|| mesh.clone())
        })
        .collect()
}

fn regional_labels(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };
            let text = &text.galley.job.text;
            (text.starts_with("Buy ") || text.starts_with("Sell ")).then(|| text.clone())
        })
        .collect()
}

#[derive(Debug)]
struct RegionalFan {
    center: egui::Pos2,
    radius: f32,
    area: f32,
    color: egui::Color32,
}

/// Recover each actual flat fan from its indexed centre, independent of mesh count.
fn regional_fans(meshes: &[egui::Mesh]) -> Vec<RegionalFan> {
    let mut fans: Vec<RegionalFan> = Vec::new();
    for mesh in meshes {
        assert!(mesh.is_valid());
        assert_eq!(mesh.texture_id, egui::TextureId::default());
        assert!(
            mesh.vertices
                .iter()
                .all(|vertex| vertex.uv == egui::epaint::WHITE_UV)
        );
        assert_eq!(mesh.indices.len() % 3, 0);
        let mut center_index = None;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            assert_eq!(a.color, b.color);
            assert_eq!(a.color, c.color);
            if center_index != Some(triangle[0]) {
                fans.push(RegionalFan {
                    center: a.pos,
                    radius: 0.0,
                    area: 0.0,
                    color: a.color,
                });
                center_index = Some(triangle[0]);
            }
            let fan = fans.last_mut().unwrap();
            fan.radius = fan
                .radius
                .max(a.pos.distance(b.pos))
                .max(a.pos.distance(c.pos));
            let b = b.pos - a.pos;
            let c = c.pos - a.pos;
            fan.area += (b.x * c.y - b.y * c.x).abs() * 0.5;
        }
    }
    fans
}

fn assert_regional_circles(
    app: &QuantickApp,
    output: &egui::FullOutput,
    snapshot: &Value,
) -> Vec<egui::Mesh> {
    let circles = regional_circles(app, output);
    assert!(!circles.is_empty(), "regional volume must be painted");
    let fans = regional_fans(&circles);
    let mut painted = fans.iter();
    assert_eq!(snapshot["marks_truncated"], false);
    let marks = snapshot["marks"].as_array().unwrap();
    let number = |mark: &Value, field: &str| mark[field].as_str().unwrap().parse::<f64>().unwrap();
    assert!(marks.iter().all(|mark| mark["opening_oversized"] == false));
    let pane = &app.active_tab().flow_pane;
    let rect = pane.frame.chart_rect.unwrap();
    let right = pane.frame.lane_divider_x.unwrap_or(rect.right());
    let (low, high) = pane.price_view.resolve(pane.frame.auto_range.unwrap());
    let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
    let history = rect.with_max_x(right);
    let geometry = quantick_chart::flow_execution::FlowExecutionGeometry::new(
        pane.model.viewport,
        total,
        0,
        (low, high),
        [
            history.left(),
            history.top(),
            history.right(),
            history.bottom(),
        ],
        false,
    )
    .unwrap();
    let presentation = quantick_chart::flow_execution::FlowPresentation::new(
        app.active_tab().tape().flow_execution_frame().unwrap(),
        [history.min.into(), history.max.into()],
        |dot| geometry.point(dot).map(|(x, y)| [x, y]),
    );
    assert_eq!(presentation.regions.len(), marks.len());
    for region in &presentation.regions {
        let mark = &marks[region.dot_index];
        let radius = number(mark, "radius_px") as f32;
        let x = pane.model.viewport.x_at_bar_position(
            number(mark, "candle_position") as f32 - 0.5,
            right,
            total,
        );
        let y = rect.top() + rect.height() * ((high - number(mark, "price")) / (high - low)) as f32;
        let source = egui::Pos2::from(region.source_disc.center);
        assert!(source.distance(egui::pos2(x - 18.0, y - 18.0)) < 0.0001);
        let center = egui::Pos2::from(region.disc.center);
        assert!(center.distance(source) <= 24.0001);
        assert!(center.y <= source.y + 0.0001);
        assert_eq!(region.disc.radius, region.source_disc.radius);
        let buy = number(mark, "buy_quantity") as f32;
        let sell = number(mark, "sell_quantity") as f32;
        let flow = app.active_tab().tape().flow_execution_frame().unwrap();
        let colours = regional_colours_at_opacity(
            app,
            flow.ordinary_region_opacity(&flow.dots[region.dot_index]),
        );
        let mut areas = [0.0; 2];
        for (side, quantity) in [buy, sell].into_iter().enumerate() {
            if quantity <= 0.0 {
                continue;
            }
            let fan = painted
                .next()
                .expect("every positive side has one painted fan");
            assert!(fan.center.distance(center) < 0.0001);
            assert!(
                (fan.radius - radius).abs() < 0.0001,
                "painted {}, published {radius}",
                fan.radius
            );
            assert_eq!(fan.color, colours[side]);
            areas[side] = fan.area;
        }
        assert_eq!(areas[0] > 0.0, buy > 0.0);
        assert_eq!(areas[1] > 0.0, sell > 0.0);
        let total_area = areas.iter().sum::<f32>();
        assert!((areas[0] / total_area - buy / (buy + sell)).abs() < 0.005);
        assert!((total_area / (std::f32::consts::PI * radius.powi(2)) - 1.0).abs() < 0.05);
    }
    assert!(painted.next().is_none(), "no regional side is duplicated");
    circles
}

/// The layer call, published execution facts and one regional paint agree.
/// Hiding Tape leaves FLOW active; disabling native mode restores ordinary bubbles.
#[test]
fn the_bubbles_switch_publishes_regional_flow_independently_of_tape_visibility() {
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
    // Initialize the camera before fitting this test's complete retained history.
    settled_frame(&mut app, &ctx);
    fit_flow_fixture(&mut app);
    let switched_on = |app: &QuantickApp| app.active_tab().tape().cached_config().show_aggressions;
    assert!(switched_on(&app), "the trader's candle switch is on");

    // Beside the native tape: available, effective, and projected from all 200 executions.
    let bubbles = read_layer(&mut app, &mut observer, "bubbles");
    assert_eq!(bubbles["requested"], true, "{bubbles}");
    assert_eq!(bubbles["effective"], true, "{bubbles}");
    assert_eq!(bubbles["blocked_reason"], Value::Null, "{bubbles}");
    let candle_aggression = read_layer(&mut app, &mut observer, "candle_aggression");
    assert_eq!(candle_aggression["requested"], false, "{candle_aggression}");
    let frame = settled_flow_frame(&mut app, &ctx);
    let regional = assert_complete_flow_fixture(&mut app, &mut observer);
    assert!(
        summary_marks(&frame).is_empty(),
        "the legacy summary is suppressed"
    );
    app.active_tab()
        .flow_pane
        .frame
        .lane_divider_x
        .expect("the tape beside the candles");
    let beside = assert_regional_circles(&app, &frame, &regional);
    // Retain actual painted colours before disabling the source: the negative
    // assertion must detect stale paint even after its frame has been cleared.
    let regional_colours: Vec<_> = beside
        .iter()
        .flat_map(|mesh| mesh.vertices.iter().map(|vertex| vertex.color))
        .collect();
    assert!(
        regional_labels(&frame).is_empty(),
        "regional quantities are passive hover detail"
    );

    // Asking for the baseline layer too cannot duplicate the regional paint.
    let layer = set_layer(&mut app, &mut cockpit, "candle_aggression", true);
    assert_eq!(layer["effective"], true, "{layer}");
    let duplicate = settled_flow_frame(&mut app, &ctx);
    assert_eq!(
        assert_complete_flow_fixture(&mut app, &mut observer)["marks"],
        regional["marks"]
    );
    assert!(summary_marks(&duplicate).is_empty());
    assert_eq!(
        assert_regional_circles(&app, &duplicate, &regional),
        beside,
        "one regional paint, never two"
    );
    set_layer(&mut app, &mut cockpit, "candle_aggression", false);

    // Off: the call moves the settings switch, and the candles carry nothing.
    let layer = set_layer(&mut app, &mut cockpit, "bubbles", false);
    assert_eq!(layer["requested"], false, "{layer}");
    assert_eq!(layer["effective"], false, "{layer}");
    assert!(!switched_on(&app), "the settings switch is the layer's own");
    let frame = settled_frame(&mut app, &ctx);
    assert_eq!(flow_snapshot(&mut app, &mut observer), Value::Null);
    assert!(
        summary_marks(&frame).is_empty(),
        "bubbles off draws nothing"
    );
    assert!(
        regional_circles_with_colours(&app, &frame, &regional_colours).is_empty(),
        "bubbles off removes regional paint"
    );
    assert!(
        regional_labels(&frame).is_empty(),
        "Bubbles switch removes annotations too"
    );

    // Tape visibility is independent: its hidden lane still leaves regional FLOW.
    set_layer(&mut app, &mut cockpit, "tape_chart", false);
    fit_flow_fixture(&mut app);
    let hidden_off = settled_frame(&mut app, &ctx);
    assert!(regional_circles_with_colours(&app, &hidden_off, &regional_colours).is_empty());
    let layer = set_layer(&mut app, &mut cockpit, "bubbles", true);
    assert_eq!(layer["effective"], true, "{layer}");
    let frame = settled_flow_frame(&mut app, &ctx);
    let hidden = assert_complete_flow_fixture(&mut app, &mut observer);
    assert!(summary_marks(&frame).is_empty());
    assert_regional_circles(&app, &frame, &hidden);
    assert!(
        regional_labels(&frame).is_empty(),
        "hidden Tape does not add automatic quantity boxes"
    );
    set_layer(&mut app, &mut cockpit, "bubbles", false);
    // Native-mode controls remain gated by the Tape lane's own visibility.
    set_layer(&mut app, &mut cockpit, "tape_chart", true);
    set_layer(&mut app, &mut cockpit, "native_tape", false);
    set_layer(&mut app, &mut cockpit, "tape_chart", false);
    app.active_tab_mut()
        .flow_pane
        .model
        .viewport
        .set_px_per_bar(8.0);
    let ordinary_off = settled_frame(&mut app, &ctx);
    let right = app.active_tab().flow_pane.frame.chart_rect.unwrap().right();
    let ordinary_off = other_discs_left_of(&ordinary_off, right);
    set_layer(&mut app, &mut cockpit, "bubbles", true);
    let ordinary = settled_frame(&mut app, &ctx);
    assert_eq!(flow_snapshot(&mut app, &mut observer), Value::Null);
    assert!(regional_circles_with_colours(&app, &ordinary, &regional_colours).is_empty());
    assert!(
        other_discs_left_of(&ordinary, right) > ordinary_off,
        "explicitly disabling native mode restores ordinary bubbles"
    );
    disable_test_gateway(&mut app, &ctx);
}

#[path = "flow_candle_paint_tests.rs"]
mod flow_candle_paint_tests;
