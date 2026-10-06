use super::*;

#[test]
fn repeated_history_requests_are_coalesced_while_one_reply_is_outstanding() {
    let (mut app, _events, mut commands, _book) = test_app();
    let config = app.tab_reads().config().clone();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut()
        .loading
        .set_active(LoadingTask::History, false);
    for _ in 0..10 {
        app.active_tab_mut().request_older_history(tab_id, &config);
    }
    let mut requests = 0;
    while let Ok(command) = commands.try_recv() {
        if matches!(command, FeedCommand::LoadOlder { .. }) {
            requests += 1;
        }
    }
    assert_eq!(requests, 1);
    assert_eq!(app.active_tab().loading.count(LoadingTask::History), 1);
}

#[test]
fn large_history_publishes_coherent_panes_and_preserves_the_live_tail() {
    let (mut app, events, _commands, _book) = test_app();
    app.active_tab_mut()
        .time_panes
        .push(crate::pane::ChartPane::time(999, 60_000));
    let tab_id = app.tabs.active_id();
    events
        .try_send(FeedEvent::Backfilled(
            (50_000..100_000).map(trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    events
        .try_send(FeedEvent::HistoryPrepended(
            (0..50_000).map(trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    events.try_send(FeedEvent::Live(trade(100_000))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        app.active_tab_mut().drain_feed(tab_id);
        let tab = app.active_tab();
        let flow = tab.flow_pane.state.trades().len();
        let time = tab.time_panes[0].state.trades().len();
        assert_eq!(flow, time, "panes publish the same source in one drain");
        if flow == 100_001 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the bounded recut completed"
        );
        std::thread::yield_now();
    }
    let tab = app.active_tab();
    assert!(!tab.loading.is_active(LoadingTask::HistoryRebuild));
    assert!(!tab.loading.is_active(LoadingTask::History));
    assert_eq!(tab.flow_pane.state.trades()[0].agg_id, trade(0).agg_id);
    assert_eq!(
        tab.flow_pane.state.trades().last().unwrap().agg_id,
        trade(100_000).agg_id
    );
}
