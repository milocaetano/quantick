//! `home.show` and `home.leave` over the control plane.
use super::*;

#[test]
fn home_show_is_authorized_idempotent_readable() {
    const CAPABILITY: &str = "home.show";
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let identity = app.runtime.tabs.active_id();
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("home-show-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let (denied, _) = unkeyed_call(&mut app, &mut observer, CAPABILITY, json!({}));
    assert!(matches!(denied.outcome, ResponseOutcome::Failure { .. }));
    assert!(
        !app.chrome.title_bar.surface.is_home(),
        "an observer cannot move the window"
    );

    let (extra, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"tab_id": "1"}));
    assert!(matches!(extra.outcome, ResponseOutcome::Failure { .. }));

    for _ in 0..2 {
        let (shown, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({}));
        assert_eq!(success_result(&shown)["active_surface"], "home");
        assert!(app.chrome.title_bar.surface.is_home());
    }
    assert_eq!(app.runtime.tabs.active_id(), identity);
    assert_eq!(app.runtime.tabs.len(), 1);
    let scene = observer_scene(&app);
    assert_eq!(scene["active_surface"], "home");
    let home = scene_control(&scene, "tab_strip.home");
    assert_eq!(home["selected"], true);
    assert_eq!(home["capability_id"], CAPABILITY);
    assert!(!home["bounds"].is_null());
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn home_leave_brings_the_active_market_and_its_panes_back() {
    const CAPABILITY: &str = "home.leave";
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let identity = app.runtime.tabs.active_id();
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("home-leave-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );

    let (shown, _) = unkeyed_call(&mut app, &mut client, "home.show", json!({}));
    assert_eq!(success_result(&shown)["active_surface"], "home");
    run_frame(&mut app, &ctx);
    let canvas = format!("pane.{}.canvas", app.active_tab().flow_pane.id);
    let scene = observer_scene(&app);
    assert_eq!(scene["active_surface"], "home");
    assert!(!scene_control_ids(&scene).contains(&canvas));

    let (denied, _) = unkeyed_call(&mut app, &mut observer, CAPABILITY, json!({}));
    assert!(matches!(denied.outcome, ResponseOutcome::Failure { .. }));
    assert!(app.chrome.title_bar.surface.is_home(), "an observer cannot");
    let (extra, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({"tab_id": "1"}));
    assert!(matches!(extra.outcome, ResponseOutcome::Failure { .. }));

    for _ in 0..2 {
        let (left, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, json!({}));
        assert_eq!(success_result(&left)["active_surface"], "market");
        run_frame(&mut app, &ctx);
        let scene = observer_scene(&app);
        assert_eq!(scene["active_surface"], "market");
        assert!(
            scene_control_ids(&scene).contains(&canvas),
            "panes are back"
        );
        assert_eq!(scene_control(&scene, "tab_strip.home")["selected"], false);
    }
    assert_eq!(app.runtime.tabs.active_id(), identity);
    disable_test_gateway(&mut app, &ctx);
}
