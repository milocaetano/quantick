//! The History run at its edges: a pane opened while pages are held, a press
//! while a reconnect's resume floor stands, Esc with a drawing tool armed,
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

/// While a reconnect's resume floor stands, the next history-shaped event
/// may be the session's recovery window: a press waits until the floor has
/// been spent instead of sending a request whose reply the floor would eat.
#[test]
fn a_press_while_the_resume_floor_stands_waits_for_it() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    let floor = live_edge_ms(&app);
    app.active_tab_mut().resume_floor_ms = Some(floor);

    app.apply_toolbar_action(ToolbarAction::LoadHistory(HistoryReach::Hours(2)));
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "nothing out yet"
    );
    assert_eq!(
        app.active_tab().history_status(),
        RunStatus::Queued(HistoryReach::Hours(2))
    );

    // The new session's first print spends the floor; the press begins.
    events
        .try_send(FeedEvent::Live(minute_trade_at(400)))
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().resume_floor_ms, None);
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "the press went out"
    );
    assert!(app.active_tab().history_reach_running());
}

/// A reply the resume floor consumes still answers the run's request: the
/// run is never left waiting on a reply that already came.
#[test]
fn a_reply_eaten_by_the_resume_floor_still_answers_the_run() {
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
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "the run took it as an empty reply and asked again"
    );
    assert!(matches!(
        app.active_tab().history_status(),
        RunStatus::Loading(_)
    ));
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
