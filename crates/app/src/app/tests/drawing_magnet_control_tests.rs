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
}
