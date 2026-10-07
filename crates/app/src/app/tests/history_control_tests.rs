//! `feed.history.load` and `feed.history.cancel` through the real local
//! gateway: permission-checked, validated with structured refusals,
//! retry-safe, and read back in `feed.status` as `tabs[].history_load`.
use super::*;

/// The active tab's `history_load` block in `feed.status`.
fn history_read(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let tab_id = app.tabs.active_id().to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["feed.status"] }),
    );
    success_result(&response)["scopes"]["feed.status"]["value"]["tabs"]
        .as_array()
        .expect("tabs")
        .iter()
        .find(|tab| tab["tab_id"] == tab_id)
        .expect("the active tab is readable")["history_load"]
        .clone()
}

#[test]
fn a_history_load_runs_reads_back_and_cancels_through_the_control_plane() {
    let ctx = egui::Context::default();
    let (mut app, mut commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    while commands.try_recv().is_ok() {}
    let directory = gateway_test_directory("history-load");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );

    let idle = history_read(&mut app, &mut observer);
    assert_eq!(idle["state"], "idle");
    assert_eq!(idle["main_reach"], "sessions:1", "yesterday until pressed");

    // The observer may read and may not press.
    let load = "feed.history.load";
    let (denied, _) = unkeyed_call(&mut app, &mut observer, load, json!({ "reach": "hours:2" }));
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));

    // A target outside the grammar is refused with the field and the grammar.
    for bad in ["weeks:2", "hours:0", "sessions:11", "hours:two", ""] {
        let (refused, served) = unkeyed_call(&mut app, &mut cockpit, load, json!({ "reach": bad }));
        assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST), "{bad}");
        let ResponseOutcome::Failure { error } = &refused.outcome else {
            panic!("refused");
        };
        assert_eq!(
            error.context.details.as_ref().unwrap()["field"],
            "reach",
            "{bad}"
        );
        assert!(
            served.iter().all(|request| request.began),
            "validated in the handler"
        );
    }
    assert_eq!(history_read(&mut app, &mut observer)["state"], "idle");

    // A real press, retried under its key: one request, one run.
    let payload = json!({ "reach": "page" });
    let (first, _) = keyed_call(&mut app, &mut cockpit, "load-1", load, payload.clone(), "k");
    let (retry, served) = keyed_call(&mut app, &mut cockpit, "load-2", load, payload, "k");
    assert!(served.is_empty(), "the retry never reached the application");
    assert_eq!(first.outcome, retry.outcome);
    let result = success_result(&first);
    assert_eq!(
        result["reach"], "hours:2",
        "an old token loads as its target"
    );
    assert_eq!(result["press"], "started");
    assert_eq!(result["status"]["state"], "loading");
    let asks = std::iter::from_fn(|| commands.try_recv().ok())
        .filter(|command| matches!(command, FeedCommand::LoadOlder { .. }))
        .count();
    assert_eq!(asks, 1, "one press, one request out");
    let loading = history_read(&mut app, &mut observer);
    assert_eq!(loading["state"], "loading");
    assert_eq!(loading["reach"], "hours:2");
    assert_eq!(loading["pages"], 1);

    // A second press while it runs starts nothing new.
    let (again, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        load,
        json!({ "reach": "sessions:5" }),
    );
    assert_eq!(success_result(&again)["press"], "already_running");

    let cancel = "feed.history.cancel";
    let (stopped, _) = unkeyed_call(&mut app, &mut cockpit, cancel, json!({}));
    assert_eq!(success_result(&stopped)["cancelled"], "run");
    assert_eq!(history_read(&mut app, &mut observer)["state"], "idle");
    let note = app.active_tab().history_note().expect("never silent");
    assert!(note.contains("cancelled"), "{note}");
    let (nothing, _) = unkeyed_call(&mut app, &mut cockpit, cancel, json!({}));
    assert_eq!(
        success_result(&nothing)["cancelled"],
        "nothing",
        "stopping twice stops once"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}
