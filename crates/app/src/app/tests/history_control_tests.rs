//! `feed.history.load` and `feed.history.cancel` through the real local
//! gateway: permission-checked, validated with structured refusals,
//! retry-safe, and read back in `feed.status` as `tabs[].history_load`.
use super::*;

/// The active tab's `history_load` block in `feed.status`.
fn history_read(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let tab_id = app.runtime.tabs.active_id().to_string();
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
    let note = app
        .active_tab()
        .session
        .history_note()
        .expect("never silent");
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

/// What `workspace.summary` says about the History button.
fn summary_reach(app: &mut QuantickApp, client: &mut LocalClient) -> (Value, Value, Value) {
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["workspace.summary"] }),
    );
    let summary = success_result(&response)["scopes"]["workspace.summary"]["value"].clone();
    (
        summary["history_reach"].clone(),
        summary["history_reach_span_minutes"].clone(),
        summary["history_target"].clone(),
    )
}

/// A press by mouse and the same press by `feed.history.load` leave the
/// window in one state: the main click's default, the summary and the saved
/// workspace all name the target pressed. The frozen v1 field keeps the
/// tokens it documents; the target itself reads in `history_target`.
#[test]
fn a_mouse_press_and_a_control_press_leave_the_same_state() {
    use quantick_feed::history_reach::HistoryReach;
    let ctx = egui::Context::default();
    for (reach, legacy) in [
        (HistoryReach::Hours(4), "span"),
        (HistoryReach::Sessions(1), "previous-session"),
        (HistoryReach::Sessions(3), ""),
    ] {
        let mut states = Vec::new();
        for by_control in [false, true] {
            let (mut app, _commands) = app_with_history(4);
            run_frame(&mut app, &ctx);
            let directory = gateway_test_directory("history-agree");
            grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
            enable_test_gateway(&mut app, &ctx, &directory, 4);
            let mut observer = connect(&directory, &options("observer", &[]));
            let mut cockpit = connect(
                &directory,
                &options("cockpit", &["cockpit", "cockpit.layout"]),
            );
            if by_control {
                let (pressed, _) = unkeyed_call(
                    &mut app,
                    &mut cockpit,
                    "feed.history.load",
                    json!({ "reach": reach.token() }),
                );
                success_result(&pressed);
            } else {
                app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadHistory(reach));
            }
            let summary = summary_reach(&mut app, &mut observer);
            assert_eq!(
                summary.0, legacy,
                "{reach:?}: the v1 field keeps its tokens"
            );
            // An hours target is what a v1 `span` of its minutes meant.
            let span_minutes = match reach {
                HistoryReach::Hours(hours) => hours * 60,
                HistoryReach::Sessions(_) => app.runtime.history.history_reach_span_minutes,
            };
            assert_eq!(summary.1, span_minutes.to_string(), "{reach:?}");
            assert_eq!(summary.2, reach.token().as_str(), "{reach:?}");
            states.push((
                app.runtime.history.history_reach,
                app.active_tab().session.main_history_reach(),
                summary,
            ));
            disable_test_gateway(&mut app, &ctx);
            std::fs::remove_dir_all(directory).ok();
        }
        assert_eq!(states[0], states[1], "{reach:?}: mouse and control agree");
        assert_eq!(states[0].0, reach, "the main click's default follows");
    }
}

/// A press refused because a run is already loading changes nothing: not
/// the main click's default, not the summary, not the saved workspace — by
/// mouse or by `feed.history.load` alike.
#[test]
fn a_press_refused_while_a_run_loads_leaves_the_default_alone() {
    use quantick_feed::history_reach::HistoryReach;
    let ctx = egui::Context::default();
    for by_control in [false, true] {
        let (mut app, _commands) = app_with_history(4);
        run_frame(&mut app, &ctx);
        let directory = gateway_test_directory("history-refused");
        grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
        enable_test_gateway(&mut app, &ctx, &directory, 4);
        let mut observer = connect(&directory, &options("observer", &[]));
        let mut cockpit = connect(
            &directory,
            &options("cockpit", &["cockpit", "cockpit.layout"]),
        );
        app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadHistory(
            HistoryReach::Hours(2),
        ));
        assert!(app.active_tab().session.history_reach_running());

        if by_control {
            let (pressed, _) = unkeyed_call(
                &mut app,
                &mut cockpit,
                "feed.history.load",
                json!({ "reach": "sessions:5" }),
            );
            assert_eq!(success_result(&pressed)["press"], "already_running");
        } else {
            app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadHistory(
                HistoryReach::Sessions(5),
            ));
        }
        assert_eq!(
            app.runtime.history.history_reach,
            HistoryReach::Hours(2),
            "by control: {by_control}; the refused target is not the default"
        );
        assert_eq!(
            app.active_tab().session.main_history_reach(),
            HistoryReach::Hours(2)
        );
        assert_eq!(summary_reach(&mut app, &mut observer).2, "hours:2");
        disable_test_gateway(&mut app, &ctx);
        std::fs::remove_dir_all(directory).ok();
    }
}

/// One tape, the same presses: through `feed.history.load` and
/// `feed.history.cancel`, and through the toolbar, the session ends in one
/// state after every press and tells one story. The two entries are one
/// dispatch, so they cannot drift into two accounts of the same press.
#[test]
fn a_control_press_and_a_toolbar_press_reach_the_session_as_one() {
    use crate::toolbar::ToolbarAction;
    use quantick_feed::history_reach::HistoryReach;
    use quantick_feed::history_run::{Cancelled, Press};
    use quantick_session::SessionEvent;
    let ctx = egui::Context::default();
    // A press, one refused while it runs, a cancel, a cancel of nothing, and
    // a fresh press after it.
    let presses = [
        Some(HistoryReach::Hours(2)),
        Some(HistoryReach::Sessions(5)),
        None,
        None,
        Some(HistoryReach::Sessions(1)),
    ];
    let mut runs = Vec::new();
    for by_control in [false, true] {
        let (mut app, _commands) = app_with_history(4);
        run_frame(&mut app, &ctx);
        let directory = gateway_test_directory("history-bus");
        grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
        enable_test_gateway(&mut app, &ctx, &directory, 4);
        let mut cockpit = connect(
            &directory,
            &options("cockpit", &["cockpit", "cockpit.layout"]),
        );
        app.active_tab_mut().session.drain_events();
        let mut snapshots = Vec::new();
        for press in presses {
            match (press, by_control) {
                (Some(reach), true) => {
                    let payload = json!({ "reach": reach.token() });
                    let (pressed, _) =
                        unkeyed_call(&mut app, &mut cockpit, "feed.history.load", payload);
                    success_result(&pressed);
                }
                (Some(reach), false) => app.apply_toolbar_action(ToolbarAction::LoadHistory(reach)),
                (None, true) => {
                    let cancel = "feed.history.cancel";
                    let (stopped, _) = unkeyed_call(&mut app, &mut cockpit, cancel, json!({}));
                    success_result(&stopped);
                }
                (None, false) => app.apply_toolbar_action(ToolbarAction::CancelHistory),
            }
            snapshots.push(app.active_tab().session.snapshot());
        }
        let trail = app.active_tab_mut().session.drain_events();
        disable_test_gateway(&mut app, &ctx);
        std::fs::remove_dir_all(directory).ok();
        runs.push((snapshots, trail));
    }
    assert_eq!(runs[0], runs[1], "the toolbar and the control plane agree");
    let (snapshots, trail) = &runs[0];
    assert_eq!(snapshots[1].last_press, Some(Press::AlreadyRunning));
    assert_eq!(snapshots[3].last_cancel, Some(Cancelled::Nothing));
    assert!(
        matches!(
            trail.first(),
            Some(SessionEvent::Pressed {
                press: Press::Start,
                ..
            })
        ),
        "{trail:?}"
    );
    // The cancelled run's page is still owed a reply, so the fresh press
    // queues behind it rather than asking twice.
    assert_eq!(snapshots[4].last_press, Some(Press::Queued));
    let asked = trail
        .iter()
        .filter(|event| matches!(event, SessionEvent::Asked(_)))
        .count();
    assert_eq!(asked, 1, "one page out: {trail:?}");
}
