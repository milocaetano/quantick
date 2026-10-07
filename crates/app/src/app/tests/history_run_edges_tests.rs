//! The History run at its edges: a pane opened while pages are held, a press
//! and a reply while a reconnect's resume floor stands, Esc with a drawing tool armed,
//! and a press that retries a failed rebuild.
use super::*;
use crate::toolbar::ToolbarAction;
use quantick_feed::history_reach::{HistoryReach, SESSION_GAP_MS};
use quantick_feed::history_run::{Press, RunStatus};

/// Minutes from `before_minute` back past a close: a silence wider than the
/// session gap.
fn previous_close_minute(before_minute: i64) -> i64 {
    before_minute - (SESSION_GAP_MS / quantick_feed::OHLCV_BASE_INTERVAL_MS) - 1
}

/// The newest print the chart holds: where a reconnect's floor would sit.
fn live_edge_ms(app: &QuantickApp) -> i64 {
    let trades = app.active_tab().flow_pane.state.trades();
    trades
        .get(trades.len() - 1)
        .expect("a charted tape")
        .timestamp_ms
}

/// Drain until no pane has a rebuild waiting.
fn settle_rebuilds(app: &mut QuantickApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while app
        .active_tab()
        .panes()
        .any(|(pane, _)| pane.history_pending())
    {
        app.drain_tabs();
        assert!(std::time::Instant::now() < deadline, "the rebuild lands");
        std::thread::yield_now();
    }
}

/// A time pane opened while a run holds its pages gets every one of them:
/// the tape it shows at the end is the flow pane's, with no hole where the
/// pages held before it existed would have been.
#[test]
fn a_pane_opened_during_a_run_ends_with_the_whole_tape() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Sessions(1)));
    assert_eq!(drain_load_older(&mut commands).len(), 1);

    // Today's earlier prints land and are held, before the split exists.
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-120..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert_eq!(drain_load_older(&mut commands).len(), 1);

    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    {
        let QuantickApp {
            tabs,
            config,
            style,
            pane_ids,
            ..
        } = &mut app;
        for (tab_id, tab) in tabs.iter_with_ids_mut() {
            tab.apply_pending_layout(tab_id, config, style, pane_ids);
        }
    }
    assert!(app.active_tab().time_pane().is_some(), "the split opened");

    // Yesterday, then the close before it: the run ends and releases.
    let yesterday_close = previous_close_minute(-120);
    let yesterday_open = yesterday_close - 300;
    for page in [
        (yesterday_open..=yesterday_close)
            .map(minute_trade_at)
            .collect::<Vec<_>>(),
        (previous_close_minute(yesterday_open) - 30..=previous_close_minute(yesterday_open))
            .map(minute_trade_at)
            .collect(),
    ] {
        events.try_send(FeedEvent::HistoryPrepended(page)).unwrap();
        app.drain_tabs();
    }
    assert!(!app.active_tab().history_reach_running(), "the target met");
    settle_rebuilds(&mut app);

    let tab = app.active_tab();
    let flow: Vec<i64> = tab
        .flow_pane
        .state
        .trades()
        .iter()
        .map(|trade| trade.timestamp_ms)
        .collect();
    let time: Vec<i64> = tab
        .time_pane()
        .expect("split")
        .state
        .trades()
        .iter()
        .map(|trade| trade.timestamp_ms)
        .collect();
    assert_eq!(
        flow.len(),
        200 + 120 + 301 + 31,
        "the flow pane holds it all"
    );
    assert_eq!(time, flow, "the pane opened mid-run holds the same tape");
}

/// Where the run stands while it pages, or a failure naming what it is.
fn loading_progress(app: &QuantickApp) -> quantick_feed::history_reach::ReachProgress {
    match app.active_tab().history_status() {
        RunStatus::Loading(progress) => progress,
        other => panic!("the run is not loading: {other:?}"),
    }
}

/// After a reconnect on a quiet market (after the close, a weekend) no print
/// comes to spend the resume floor, so a press must not wait for one: it
/// begins at once, and its reply is judged on what it brought.
#[test]
fn a_press_after_a_reconnect_on_a_quiet_market_begins_at_once() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.active_tab_mut().resume_floor_ms = Some(live_edge_ms(&app));

    let config = app.config.clone();
    let press = app
        .active_tab_mut()
        .load_history(&config, HistoryReach::Hours(2));
    assert_eq!(press, Press::Start, "nothing is left to wait for");
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "the press went out"
    );

    events
        .try_send(FeedEvent::HistoryPrepended(
            (-30..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    let progress = loading_progress(&app);
    assert_eq!(
        progress.prints_pulled, 30,
        "the reply was judged on its prints"
    );
    assert_eq!(progress.oldest_ms, minute_trade_at(-30).timestamp_ms);
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "and the run asked again"
    );
    assert!(
        app.active_tab().resume_floor_ms.is_some(),
        "an older page is not the session's recovery window"
    );
}

/// A reply landing while the resume floor stands is the run's, judged with
/// the prints it carried — not swallowed by the floor, and not faked empty.
#[test]
fn a_reply_during_the_resume_floor_is_judged_on_its_own_prints() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Hours(2)));
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    app.active_tab_mut().resume_floor_ms = Some(live_edge_ms(&app));

    events
        .try_send(FeedEvent::HistoryPrepended(
            (-30..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    let progress = loading_progress(&app);
    assert_eq!(progress.prints_pulled, 30);
    assert_eq!(progress.oldest_ms, minute_trade_at(-30).timestamp_ms);
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "the reply cleared the request and the run asked again"
    );
}

/// The new session's recovery window, landing while a request is out, goes
/// through the floor and is not taken for the run's reply; the reply that
/// follows is.
#[test]
fn the_recovery_window_is_not_taken_for_a_runs_reply() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.active_tab_mut().resume_floor_ms = Some(live_edge_ms(&app));
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Hours(2)));
    assert_eq!(drain_load_older(&mut commands).len(), 1);

    events
        .try_send(FeedEvent::HistoryPrepended(
            (190..210).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().resume_floor_ms,
        None,
        "the floor was spent"
    );
    assert_eq!(loading_progress(&app).prints_pulled, 0, "not judged");
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "the request is still out"
    );

    events
        .try_send(FeedEvent::HistoryPrepended(
            (-30..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert_eq!(loading_progress(&app).prints_pulled, 30);
    assert_eq!(drain_load_older(&mut commands).len(), 1);
}

/// Esc with a drawing tool armed puts the tool down; only an Esc nothing
/// else wants cancels the run.
#[test]
fn esc_disarms_a_drawing_tool_before_it_cancels_a_run() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Hours(2)));
    assert!(app.active_tab().history_reach_running());

    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert_eq!(app.toolrail.tool(), Tool::Pointer, "the tool went down");
    assert!(
        app.active_tab().history_reach_running(),
        "the run kept loading"
    );

    run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
    assert!(!app.active_tab().history_reach_running(), "now it cancels");
    assert!(
        app.active_tab()
            .history_note()
            .is_some_and(|note| note.contains("cancelled"))
    );
}

/// A press on a tab whose rebuild failed retries the rebuild **and** takes
/// the press: it is queued behind the retried rebuild and begins after it.
#[test]
fn a_press_after_a_failed_rebuild_retries_and_queues_the_target() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Sessions(1)));
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-120..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.active_tab_mut().flow_pane.fail_history_publication();
    app.drain_tabs();
    assert!(!app.active_tab().history_reach_running(), "the run gave up");
    assert!(
        app.active_tab()
            .history_note()
            .is_some_and(|note| note.contains("could not be built"))
    );
    drain_load_older(&mut commands);

    let config = app.config.clone();
    let press = app
        .active_tab_mut()
        .load_history(&config, HistoryReach::Hours(4));
    assert_eq!(press, Press::Queued, "the press is taken, not swallowed");
    assert_eq!(
        app.active_tab().history_status(),
        RunStatus::Queued(HistoryReach::Hours(4))
    );
    // The reply the failed run still had out lands; then the rebuild.
    events
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    settle_rebuilds(&mut app);
    app.drain_tabs();
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "the queued target begins once the retried rebuild lands"
    );
    assert!(app.active_tab().history_reach_running());
    assert_eq!(
        app.active_tab().flow_pane.state.trades().len(),
        320,
        "the retried rebuild kept the page"
    );
}

/// A target pressed while a recut was pending is queued behind it; when that
/// recut fails, the target stays queued — on screen and on the control plane
/// — and the press that retries the rebuild runs it.
#[test]
fn a_target_queued_behind_a_failed_recut_survives_for_the_retry() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Sessions(1)));
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-120..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    // Cancelled: the held pages are released into one recut.
    app.active_tab_mut().cancel_history();

    let config = app.config.clone();
    let press = app
        .active_tab_mut()
        .load_history(&config, HistoryReach::Hours(4));
    assert_eq!(press, Press::Queued);
    app.active_tab_mut().flow_pane.fail_history_publication();
    app.drain_tabs();
    assert!(
        app.active_tab()
            .history_note()
            .is_some_and(|note| note.contains("could not be built"))
    );
    assert_eq!(
        app.active_tab().history_status(),
        RunStatus::Queued(HistoryReach::Hours(4)),
        "the failed recut did not drop the target"
    );

    // The reply still owed lands; a failed chart does not begin the target.
    events
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.drain_tabs();
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "nothing on a failed chart"
    );
    assert_eq!(
        app.active_tab().history_status(),
        RunStatus::Queued(HistoryReach::Hours(4))
    );

    // The main click retries the rebuild and runs the queued target.
    let press = app.active_tab_mut().request_older_history(&config);
    assert_eq!(press, Press::Queued);
    settle_rebuilds(&mut app);
    app.drain_tabs();
    assert_eq!(drain_load_older(&mut commands).len(), 1, "the target began");
    assert_eq!(loading_progress(&app).reach, HistoryReach::Hours(4));
}
