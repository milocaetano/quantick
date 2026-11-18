//! The toolrail magnet through the control plane: a named call sets it, the
//! scene reads it back, and both go through the rail's own state.
use super::*;

const CAPABILITY: &str = "annotate.magnet.set";

fn magnet_control(app: &QuantickApp) -> quantick_control_schema::scene::SceneControlSnapshot {
    crate::control::scene_snapshot(app)
        .controls
        .into_iter()
        .find(|control| control.control_id == "tool_rail.toggle.magnet")
        .expect("the scene lists the magnet beside the rail")
}

const OBJECT_CAPABILITY: &str = "annotate.drawing_magnet.set";
const SCOPE_CAPABILITY: &str = "annotate.drawing_scope.set";

/// The `analysis.drawings` scope as a client reads it.
fn read_drawings(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let read = json!({"scopes":["analysis.drawings"]});
    let (response, _) = unkeyed_call(app, client, "snapshot.read", read);
    success_result(&response)["scopes"]["analysis.drawings"]["value"].clone()
}

/// The magnet and scope calls through the window's adapter reach their
/// owners: the rail's switch, which the scene and the drawings scope read
/// back with the rail shown or hidden; and one drawing on the addressed pane,
/// its own magnet, its scope shared on the layout that pane shows, and its
/// scope pinned to the bars that pane is cut to. What
/// each call decides is proven on the fake window in
/// `quantick-control-handlers`.
#[test]
fn the_magnet_and_scope_calls_reach_the_rail_and_the_addressed_drawing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let control = magnet_control(&app);
    assert!(!control.selected, "the magnet opens off");
    assert_eq!(control.capability_id.as_deref(), Some(CAPABILITY));
    let index = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 300.0)],
    );
    let pane = app.active_tab().drawing_pane();
    let (drawing_id, pane_id) = (pane.drawings.items()[index].id, pane.id);
    let drawn_at = *pane.series.state.spec();
    let directory = gateway_test_directory("drawing-magnet-wiring");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );

    let (on, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"enabled": true}));
    success_result(&on);
    assert!(app.surfaces.toolrail.magnet());
    run_frame(&mut app, &ctx);
    assert!(magnet_control(&app).selected, "the scene reads it on");
    assert_eq!(read_drawings(&mut app, &mut client)["magnet"], json!(true));
    app.surfaces.toolrail.set_visible(false);
    run_frame(&mut app, &ctx);
    assert!(
        crate::control::scene_snapshot(&app)
            .controls
            .iter()
            .all(|control| control.control_id != "tool_rail.toggle.magnet"),
        "a hidden rail lists no magnet control"
    );
    let off = json!({"enabled": false});
    let (off, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, off);
    success_result(&off);
    assert!(
        !app.surfaces.toolrail.magnet(),
        "a hidden rail still switches"
    );
    assert_eq!(read_drawings(&mut app, &mut client)["magnet"], json!(false));

    let address = json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "pane_id": pane_id.to_string(),
        "drawing_id": drawing_id.0.to_string(),
    });
    let mut own = address.clone();
    own["enabled"] = json!(true);
    let (own, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, own);
    success_result(&own);
    let drawing = |app: &QuantickApp| {
        let drawings = &app.active_tab().drawing_pane().drawings;
        drawings.items()[drawings.index_of(drawing_id).unwrap()].clone()
    };
    assert!(drawing(&app).magnet);
    assert!(
        !app.surfaces.toolrail.magnet(),
        "the rail's is another switch"
    );
    let listed = read_drawings(&mut app, &mut client)["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|tab| tab["panes"].as_array().unwrap().clone())
        .flat_map(|pane| pane["drawings"].as_array().unwrap().clone())
        .find(|row| row["drawing_id"] == address["drawing_id"])
        .expect("the drawings scope lists the object");
    assert_eq!(listed["magnet"], json!(true));

    let mut wide = address.clone();
    wide["scope"] = json!("all_charts");
    wide["all_windows"] = json!(true);
    let (shared, _) = unkeyed_call(&mut app, &mut client, SCOPE_CAPABILITY, wide);
    let shared = success_result(&shared);
    assert_eq!(shared["all_windows"], json!(true));
    let layout = app.runtime.layout_state().focused_pane_layout();
    assert_eq!(
        shared["layout_id"],
        json!(layout.0.to_string()),
        "the pane's own layout"
    );

    let mut scope = address;
    scope["scope"] = json!("this_chart");
    let (scoped, _) = unkeyed_call(&mut app, &mut client, SCOPE_CAPABILITY, scope);
    assert_eq!(
        success_result(&scoped)["pinned_bars"],
        json!(drawn_at.to_config_string()),
        "pinned to the bars the pane is cut to"
    );
    assert_eq!(
        drawing(&app).scope,
        quantick_chart_interaction::drawing_model::DrawingScope::ThisChart
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A hand on the rail leaves the same journal event a script does.
#[test]
fn a_hand_flip_of_the_magnet_journals_like_the_call() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let directory = gateway_test_directory("drawing-magnet-hand");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    run_frame(&mut app, &ctx);
    let button = app
        .surfaces
        .toolrail
        .magnet_button_rect()
        .expect("the magnet button rendered")
        .center();
    click_chart(&mut app, &ctx, button);
    run_frame(&mut app, &ctx);
    assert!(app.surfaces.toolrail.magnet(), "the click turned it on");
    let events = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "annotate.magnet.changed")
        .map(|event| event.payload["drawing_magnet"]["enabled"].clone())
        .collect::<Vec<_>>();
    assert_eq!(events, [json!(true)]);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The object magnet's events, in order, as `enabled` values.
fn object_magnet_events(app: &QuantickApp) -> Vec<serde_json::Value> {
    app.control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "annotate.drawing_magnet.changed")
        .map(|event| event.payload["drawing_magnet"]["enabled"].clone())
        .collect()
}

/// Click the context bar's magnet slot on a fresh horizontal line and
/// prove the flip landed as one undo step; answers the drawing's index.
fn hand_flip_own_magnet(app: &mut QuantickApp, ctx: &egui::Context) -> usize {
    run_frame(app, ctx);
    let index = place_drawing(app, ctx, "horizontal-line", &[egui::pos2(700.0, 300.0)]);
    run_frame(app, ctx);
    let slot = app
        .surfaces
        .drawing_chrome
        .context_bar()
        .magnet_rect()
        .expect("a pointer-anchored drawing renders the magnet slot")
        .center();
    click_chart(app, ctx, slot);
    run_frame(app, ctx);
    index
}

fn own_magnet_at(app: &QuantickApp, index: usize) -> bool {
    app.active_tab().drawing_pane().drawings.items()[index].magnet
}

/// A hand on the context bar's magnet slot leaves the same journal event a
/// script does, and the flip stays one undo step.
#[test]
fn a_hand_flip_of_a_drawings_own_magnet_journals_like_the_call() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let directory = gateway_test_directory("drawing-object-magnet-hand");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let index = hand_flip_own_magnet(&mut app, &ctx);
    assert!(own_magnet_at(&app, index), "the click turned it on");
    assert!(
        !app.surfaces.toolrail.magnet(),
        "the rail's magnet never moves"
    );
    assert_eq!(object_magnet_events(&app), [json!(true)]);
    let event = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .into_iter()
        .find(|event| event.kind.as_str() == "annotate.drawing_magnet.changed")
        .unwrap();
    assert_eq!(
        event.actor.map(|actor| actor.kind),
        Some(quantick_control::wire::ActorKind::HumanUi),
        "the trader's own hand"
    );
    assert!(
        app.drawing_pane_mut().drawings.undo(),
        "the flip is one undo step"
    );
    assert!(!own_magnet_at(&app, index), "undo takes it back");
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The default build has no control access: the hand's flip still lands,
/// as one undo step, with nothing to journal.
#[test]
fn a_hand_flip_of_a_drawings_own_magnet_lands_without_a_gateway() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    app.control.control_access = None;
    let index = hand_flip_own_magnet(&mut app, &ctx);
    assert!(own_magnet_at(&app, index), "the click turned it on");
    assert!(
        !app.surfaces.toolrail.magnet(),
        "the rail's magnet never moves"
    );
    assert!(
        app.drawing_pane_mut().drawings.undo(),
        "the flip is one undo step"
    );
    assert!(!own_magnet_at(&app, index), "undo takes it back");
}
