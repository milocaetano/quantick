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
    let tab_id = app.tabs.active_id().to_string();
    let pane = pane_id.to_string();
    let call = |enabled: serde_json::Value| json!({"tab_id": tab_id, "pane_id": pane, "drawing_id": id, "enabled": enabled});
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
        call(json!(true)),
    );
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    for invalid in [
        call(json!("yes")),
        json!({"tab_id": tab_id, "pane_id": pane, "enabled": true}),
        json!({"tab_id": tab_id, "pane_id": pane, "drawing_id": id}),
        json!({"pane_id": pane, "drawing_id": id, "enabled": true}),
        json!({"tab_id": tab_id, "drawing_id": id, "enabled": true}),
        json!({"tab_id": tab_id, "pane_id": pane, "drawing_id": id, "enabled": true, "x": 1}),
        json!({"tab_id": tab_id, "pane_id": pane, "drawing_id": "999999", "enabled": true}),
        json!({"tab_id": tab_id, "pane_id": "999999", "drawing_id": id, "enabled": true}),
        json!({"tab_id": "999999", "pane_id": pane, "drawing_id": id, "enabled": true}),
    ] {
        let (refused, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, invalid);
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
        assert!(!own(&app), "a refused call changes nothing");
    }

    let answer = |enabled: bool, changed: bool| {
        json!({
            "drawing_id": id,
            "tab_id": tab_id,
            "pane_id": pane,
            "enabled": enabled,
            "changed": changed,
        })
    };
    let (on, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, call(json!(true)));
    assert_eq!(success_result(&on), answer(true, true));
    assert!(own(&app));
    assert!(
        !app.toolrail.magnet(),
        "the rail's magnet is another switch"
    );
    assert_eq!(read(&mut app, &mut client), json!(true));

    let (again, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, call(json!(true)));
    assert_eq!(success_result(&again), answer(true, false));

    let (off, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, call(json!(false)));
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

/// A drawing id is unique only within its pane: with the same id on both
/// panes of a split tab, the call moves exactly the one on the pane it
/// names, whichever pane that is.
#[test]
fn the_object_magnet_moves_only_the_named_panes_drawing_when_ids_collide() {
    let ctx = egui::Context::default();
    let (mut app, _commands, _) = split_with_a_shared_line(&ctx);
    let flow_id = app.active_tab().flow_pane.drawings.items()[0].id;
    let time = app
        .active_tab_mut()
        .time_pane_mut()
        .expect("the split built it");
    time.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        drawings::ChartPoint::at(5.5, 100.0),
        |tool| drawings::NewDrawing {
            style: drawings::DrawingStyle::default(),
            payload: tool.default_payload(),
        },
    );
    let last = time.drawings.items().len() - 1;
    time.drawings.items_mut()[last].id = flow_id;
    run_frame(&mut app, &ctx);
    let tab_id = app.tabs.active_id().to_string();
    let flow_pane = app.active_tab().flow_pane.id.to_string();
    let time_pane = app.active_tab().time_pane().unwrap().id.to_string();
    assert_ne!(flow_pane, time_pane);
    let own = |app: &QuantickApp| {
        let tab = app.active_tab();
        let of = |drawings: &drawings::Drawings| {
            drawings.items()[drawings.index_of(flow_id).unwrap()].magnet
        };
        (
            of(&tab.flow_pane.drawings),
            of(&tab.time_pane().unwrap().drawings),
        )
    };

    let directory = gateway_test_directory("drawing-object-magnet-collide");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let id = flow_id.0.to_string();
    for (pane, expected) in [(&time_pane, (false, true)), (&flow_pane, (true, true))] {
        let (answer, _) = unkeyed_call(
            &mut app,
            &mut client,
            OBJECT_CAPABILITY,
            json!({"tab_id": tab_id, "pane_id": pane, "drawing_id": id, "enabled": true}),
        );
        assert_eq!(success_result(&answer)["pane_id"], json!(pane));
        assert_eq!(own(&app), expected, "only the named pane's drawing moved");
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A mark that snaps by its own rule shows no magnet slot, and the call
/// refuses it the same way, saying why in a field a client can branch on.
#[test]
fn a_tool_that_snaps_by_its_own_rule_refuses_the_object_magnet() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let index = place_drawing(&mut app, &ctx, "arrow-mark-up", &[egui::pos2(700.0, 300.0)]);
    let pane = app.active_tab().drawing_pane();
    let input = json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": pane.id.to_string(),
        "drawing_id": pane.drawings.items()[index].id.0.to_string(),
        "enabled": true,
    });
    let directory = gateway_test_directory("drawing-object-magnet-mark");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let (refused, _) = unkeyed_call(&mut app, &mut client, OBJECT_CAPABILITY, input);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    assert_eq!(
        response_error(&refused).context.details,
        Some(json!({"reason": "tool_snaps_by_its_own_rule", "tool_id": "arrow-mark-up"}))
    );
    assert!(!app.active_tab().drawing_pane().drawings.items()[index].magnet);
    assert!(
        object_magnet_events(&app).is_empty(),
        "a refusal journals nothing"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A hand on the context bar's magnet slot leaves the same journal event a
/// script does, and the flip stays one undo step.
#[test]
fn a_hand_flip_of_a_drawings_own_magnet_journals_like_the_call() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    let directory = gateway_test_directory("drawing-object-magnet-hand");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    run_frame(&mut app, &ctx);
    let index = place_drawing(
        &mut app,
        &ctx,
        "horizontal-line",
        &[egui::pos2(700.0, 300.0)],
    );
    run_frame(&mut app, &ctx);
    let slot = app
        .drawings
        .chrome
        .context_bar()
        .magnet_rect()
        .expect("a pointer-anchored drawing renders the magnet slot")
        .center();
    click_chart(&mut app, &ctx, slot);
    run_frame(&mut app, &ctx);
    let own = |app: &QuantickApp| app.active_tab().drawing_pane().drawings.items()[index].magnet;
    assert!(own(&app), "the click turned it on");
    assert!(!app.toolrail.magnet(), "the rail's magnet never moves");
    assert_eq!(object_magnet_events(&app), [json!(true)]);
    assert!(
        app.drawing_pane_mut().drawings.undo(),
        "the flip is one undo step"
    );
    assert!(!own(&app), "undo takes it back");
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
