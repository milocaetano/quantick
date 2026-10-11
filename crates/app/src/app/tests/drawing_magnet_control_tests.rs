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

#[test]
fn the_magnet_is_set_by_a_named_call_and_read_back_in_the_scene() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let control = magnet_control(&app);
    assert!(!control.selected, "the magnet opens off");
    assert_eq!(control.capability_id.as_deref(), Some(CAPABILITY));

    let directory = gateway_test_directory("drawing-magnet-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );

    let (denied, _) = unkeyed_call(
        &mut app,
        &mut observer,
        CAPABILITY,
        json!({"enabled": true}),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    for invalid in [
        json!({"enabled": "yes"}),
        json!({}),
        json!({"enabled": true, "x": 1}),
    ] {
        let (refused, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, invalid);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        assert!(!app.toolrail.magnet(), "a refused call changes nothing");
    }

    let (on, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"enabled": true}));
    assert_eq!(
        success_result(&on),
        json!({"enabled": true, "changed": true})
    );
    assert!(app.toolrail.magnet());
    run_frame(&mut app, &ctx);
    assert!(magnet_control(&app).selected, "the scene reads it on");

    let (again, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"enabled": true}));
    assert_eq!(
        success_result(&again),
        json!({"enabled": true, "changed": false})
    );

    let (off, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"enabled": false}));
    assert_eq!(
        success_result(&off),
        json!({"enabled": false, "changed": true})
    );
    assert!(!app.toolrail.magnet());
    run_frame(&mut app, &ctx);
    assert!(!magnet_control(&app).selected);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The snapshot carries the magnet whether or not the rail is on screen:
/// the setter works on a hidden rail, so the read must too.
#[test]
fn the_magnet_reads_back_in_the_snapshot_with_the_rail_hidden() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    app.toolrail.set_visible(false);
    run_frame(&mut app, &ctx);
    assert!(
        crate::control::scene_snapshot(&app)
            .controls
            .iter()
            .all(|control| control.control_id != "tool_rail.toggle.magnet"),
        "a hidden rail lists no magnet control"
    );
    let directory = gateway_test_directory("drawing-magnet-snapshot");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let read = |app: &mut QuantickApp, client: &mut LocalClient| {
        let (response, _) = unkeyed_call(
            app,
            client,
            "snapshot.read",
            json!({"scopes":["analysis.drawings"]}),
        );
        success_result(&response)["scopes"]["analysis.drawings"]["value"]["magnet"].clone()
    };
    assert_eq!(read(&mut app, &mut client), json!(false));
    let (on, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"enabled": true}));
    success_result(&on);
    assert_eq!(read(&mut app, &mut client), json!(true));
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
        .toolrail
        .magnet_button_rect()
        .expect("the magnet button rendered")
        .center();
    click_chart(&mut app, &ctx, button);
    run_frame(&mut app, &ctx);
    assert!(app.toolrail.magnet(), "the click turned it on");
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

const OBJECT_CAPABILITY: &str = "annotate.drawing_magnet.set";

/// One drawing's own magnet through the control plane: a named call sets it
/// by the object's id, the drawings scope reads it back, a repeat changes
/// nothing, and an unknown id is refused without touching anything.
#[test]
fn a_drawings_own_magnet_is_set_by_a_named_call_and_read_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 300.0)],
    );
    let drawing_id = app.active_tab().drawing_pane().drawings.items()[index].id;
    let pane_id = app.active_tab().drawing_pane().id;
    let own = |app: &QuantickApp| {
        let drawings = &app.active_tab().drawing_pane().drawings;
        drawings.items()[drawings.index_of(drawing_id).unwrap()].magnet
    };
    assert!(!own(&app), "an object's magnet opens off");

    let directory = gateway_test_directory("drawing-object-magnet-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let id = drawing_id.0.to_string();
    let read = |app: &mut QuantickApp, client: &mut LocalClient| {
        let (response, _) = unkeyed_call(
            app,
            client,
            "snapshot.read",
            json!({"scopes":["analysis.drawings"]}),
        );
        let value = success_result(&response)["scopes"]["analysis.drawings"]["value"].clone();
        value["tabs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|tab| tab["panes"].as_array().unwrap().clone())
            .flat_map(|pane| pane["drawings"].as_array().unwrap().clone())
            .find(|drawing| drawing["drawing_id"] == json!(id.clone()))
            .expect("the drawings scope lists the object")["magnet"]
            .clone()
    };
    assert_eq!(read(&mut app, &mut client), json!(false));

    let (denied, _) = unkeyed_call(
        &mut app,
        &mut observer,
        OBJECT_CAPABILITY,
        json!({"drawing_id": id, "enabled": true}),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    for invalid in [
        json!({"drawing_id": id, "enabled": "yes"}),
        json!({"enabled": true}),
        json!({"drawing_id": id}),
        json!({"drawing_id": id, "enabled": true, "x": 1}),
        json!({"drawing_id": "999999", "enabled": true}),
    ] {
        let (refused, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, invalid);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        assert!(!own(&app), "a refused call changes nothing");
    }

    let tab_id = app.tabs.active_id().to_string();
    let answer = |enabled: bool, changed: bool| {
        json!({
            "drawing_id": id,
            "tab_id": tab_id,
            "pane_id": pane_id.to_string(),
            "enabled": enabled,
            "changed": changed,
        })
    };
    let (on, _) = unkeyed_call(
        &mut app,
        &mut client,
        OBJECT_CAPABILITY,
        json!({"drawing_id": id, "enabled": true}),
    );
    assert_eq!(success_result(&on), answer(true, true));
    assert!(own(&app));
    assert!(!app.toolrail.magnet(), "the rail's magnet is another switch");
    assert_eq!(read(&mut app, &mut client), json!(true));

    let (again, _) = unkeyed_call(
        &mut app,
        &mut client,
        OBJECT_CAPABILITY,
        json!({"drawing_id": id, "enabled": true}),
    );
    assert_eq!(success_result(&again), answer(true, false));

    let (off, _) = unkeyed_call(
        &mut app,
        &mut client,
        OBJECT_CAPABILITY,
        json!({"drawing_id": id, "enabled": false}),
    );
    assert_eq!(success_result(&off), answer(false, true));
    assert!(!own(&app));
    assert_eq!(read(&mut app, &mut client), json!(false));

    let events = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 20)
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "annotate.drawing_magnet.changed")
        .map(|event| event.payload["drawing_magnet"]["enabled"].clone())
        .collect::<Vec<_>>();
    assert_eq!(events, [json!(true), json!(true), json!(false)]);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
