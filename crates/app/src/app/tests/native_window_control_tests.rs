//! Native presentation has explicit authority and keyed delivery.
use super::*;

#[test]
fn native_window_requests_validate_authority_and_retry_without_repeating_the_effect() {
    const CAPABILITY: &str = "layout.window.state.set";
    let ctx = egui::Context::default();
    let (mut app, _, _, _) = test_app();
    let identity = app.runtime.tabs.active_id();
    let directory = gateway_test_directory("native-window-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let payload = json!({"state":"maximized"});
    let (denied, _) = unkeyed_call(&mut app, &mut observer, CAPABILITY, payload.clone());
    assert!(matches!(denied.outcome, ResponseOutcome::Failure { .. }));
    let (missing_key, _) = unkeyed_call(&mut app, &mut client, CAPABILITY, payload.clone());
    assert!(matches!(
        missing_key.outcome,
        ResponseOutcome::Failure { .. }
    ));
    let (first, began) = keyed_call(
        &mut app,
        &mut client,
        "window-first",
        CAPABILITY,
        payload.clone(),
        "window-key",
    );
    assert!(began.iter().any(|request| request.began));
    assert_eq!(success_result(&first)["requested"], "maximized");
    let (retry, repeated) = keyed_call(
        &mut app,
        &mut client,
        "window-retry",
        CAPABILITY,
        payload,
        "window-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert!(repeated.iter().all(|request| !request.began));
    let (closed, _) = keyed_call(
        &mut app,
        &mut client,
        "window-close",
        CAPABILITY,
        json!({"state":"closed"}),
        "close-key",
    );
    assert!(matches!(closed.outcome, ResponseOutcome::Failure { .. }));
    let mut input = egui::RawInput::default();
    input
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .maximized = Some(true);
    let _ = ctx.run(input, |ctx| app.draw_frame(ctx, frame_instant(ctx)));
    let scene = observer_scene(&app);
    assert_eq!(
        scene_control(&scene, "tab_strip.maximize")["selected"],
        true
    );
    assert!(scene_control(&scene, "tab_strip.close")["capability_id"].is_null());
    assert_eq!(app.runtime.tabs.active_id(), identity);
    disable_test_gateway(&mut app, &ctx);
}
