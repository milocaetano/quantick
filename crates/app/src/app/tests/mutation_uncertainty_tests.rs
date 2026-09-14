//! Real gateway failures after an action begins, with observable readback.

use super::*;

#[test]
fn an_unkeyed_post_start_timeout_is_unknown_and_not_retryable() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("unkeyed-post-start");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway_with_limits(
        &mut app,
        &ctx,
        &directory,
        4,
        Duration::from_millis(1500),
        4,
    );
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = readback(&mut app, &ctx, &mut client, "layout.tab.create").len();
    client.send("layout.tab.create", json!({})).unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    let held = with_access(&mut app, |access, app| {
        access.serve_one_withholding_answer_for_test(app)
    })
    .unwrap();
    assert!(held.served.began);
    let response = client.read().unwrap();
    let error = response_error(&response);
    assert_eq!(error.code.as_str(), codes::TIMEOUT);
    assert!(!error.retryable);
    assert_eq!(
        error.context.details.as_ref().unwrap()["outcome"],
        "unknown"
    );
    assert!(
        error
            .context
            .next_steps
            .iter()
            .any(|step| step.contains("Read the state back"))
    );
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "layout.tab.create").len(),
        before + 1
    );
    assert!(!held.deliver());
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn post_execution_socket_loss_is_unknown_for_keyed_and_unkeyed_mutations() {
    for keyed in [false, true] {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(4);
        run_frame(&mut app, &ctx);
        let directory = gateway_test_directory(if keyed {
            "post-execution-keyed"
        } else {
            "post-execution-unkeyed"
        });
        grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
        enable_test_gateway_with_limits(&mut app, &ctx, &directory, 4, Duration::from_secs(5), 4);
        let opt = options("cockpit", &["cockpit", "cockpit.layout"]);
        let (mut client, connection) = connect_listed(&mut app, &ctx, &directory, &opt);
        let before = readback(&mut app, &ctx, &mut client, "layout.tab.create").len();
        client
            .send_versioned_with_key(
                "layout.tab.create",
                1,
                json!({}),
                keyed.then(|| IdempotencyKey::new("lost-key").unwrap()),
            )
            .unwrap();
        wait_for_queued_gateway_requests(&app, 1);
        let held = with_access(&mut app, |access, app| {
            access.serve_one_withholding_answer_for_test(app)
        })
        .unwrap();
        assert!(held.served.began, "the side effect precedes socket loss");
        // Revocation closes the real socket after the action, before its reply.
        with_access(&mut app, |access, _| access.revoke(connection));
        let error = client
            .read()
            .expect_err("the response never reached the client");
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert!(!error.retryable);
        assert_eq!(
            error.context.details.as_ref().unwrap()["outcome"],
            "unknown"
        );
        let mut read_client = connect(&directory, &opt);
        assert_eq!(
            readback(&mut app, &ctx, &mut read_client, "layout.tab.create").len(),
            before + 1
        );
        drop(held);
        disable_test_gateway(&mut app, &ctx);
        std::fs::remove_dir_all(directory).ok();
    }
}

#[test]
fn a_gateway_deadline_cancels_queued_mutation_before_any_later_dispatch() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("cancel-before-dispatch");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway_with_limits(
        &mut app,
        &ctx,
        &directory,
        4,
        Duration::from_millis(1500),
        4,
    );
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = readback(&mut app, &ctx, &mut client, "layout.tab.create").len();
    client.send("layout.tab.create", json!({})).unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    let response = client.read().unwrap();
    let error = response_error(&response);
    assert!(error.retryable);
    assert_eq!(
        error.context.details.as_ref().unwrap()["outcome"],
        "not_started"
    );
    let served = serve(&mut app);
    assert_eq!(served.len(), 1);
    assert!(!served[0].began);
    assert_eq!(
        readback(&mut app, &ctx, &mut client, "layout.tab.create").len(),
        before
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
