use super::*;

const SESSION_PRINTS: u64 = 1_000_000;
const SESSION_START_MS: i64 = 1_790_000_000_000;

fn dense_trade(id: u64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        timestamp_ms: SESSION_START_MS
            + (id / SESSION_PRINTS) as i64 * 86_400_000
            + (id % SESSION_PRINTS) as i64 * 32,
        ..trade(id)
    }
}

#[test]
fn one_action_reaches_the_previous_session_on_a_million_print_tape() {
    let (mut app, events, mut commands, _book) = test_app();
    let tab_id = app.tabs.active_id();
    let mut config = app.tab_reads().config().clone();
    config.feeds[0].provider = ProviderKind::MetaTrader;
    let tab = app.active_tab_mut();
    tab.flow_pane.spec.retain(BarSpec::Tick(1000));
    tab.apply_spec_changes();
    tab.history_reach = quantick_feed::history_reach::HistoryReach::PreviousSession;
    tab.history_step = 2000;
    let mut cursor = 1_950_000;
    events
        .try_send(FeedEvent::Backfilled(
            (cursor..2_000_000).map(dense_trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    while commands.try_recv().is_ok() {}
    app.active_tab_mut().request_older_history(tab_id, &config);
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_secs(90);
    let target = dense_trade(SESSION_PRINTS - 1).timestamp_ms
        - config.history.reach_bounds().previous_session_lead_ms;
    let mut pages = 0;
    let mut frame_peak = std::time::Duration::ZERO;
    while app.active_tab().history_reach_running() {
        if let Ok(FeedCommand::LoadOlder { count }) = commands.try_recv() {
            pages += 1;
            let first = cursor.saturating_sub(count as u64);
            events
                .try_send(FeedEvent::HistoryPrepended(
                    (first..cursor).map(dense_trade).collect(),
                ))
                .unwrap();
            cursor = first;
        }
        let frame = std::time::Instant::now();
        app.active_tab_mut().drain_feed(tab_id);
        frame_peak = frame_peak.max(frame.elapsed());
        assert!(
            std::time::Instant::now() < deadline,
            "the entire requested reach completes"
        );
        std::thread::yield_now();
    }
    let tab = app.active_tab();
    println!(
        "full history pages={pages} fetched={} complete_ms={:.3} drain_peak_ms={:.3}",
        1_950_000 - cursor,
        start.elapsed().as_secs_f64() * 1000.0,
        frame_peak.as_secs_f64() * 1000.0
    );
    assert!(
        tab.flow_pane.state.trades()[0].timestamp_ms <= target,
        "one action stopped before the advertised prior-close plus lead target: pages={pages}, fetched={}, note={:?}",
        1_950_000 - cursor,
        tab.history_note()
    );
    assert!(1_950_000 - cursor > 1_000_000);
    assert!(tab.history_note().is_none());
    assert!(!tab.loading.is_active(LoadingTask::History));
    for (pane, _) in tab.panes() {
        assert_eq!(pane.state.trades().len(), (2_000_000 - cursor) as usize);
        assert_eq!(pane.state.trades()[0].agg_id, cursor);
        assert_eq!(pane.state.trades().last().unwrap().agg_id, 1_999_999);
    }
}

#[test]
fn metatrader_single_page_keeps_the_traders_requested_count() {
    let (mut app, events, mut commands, _book) = test_app();
    let tab_id = app.tabs.active_id();
    let mut config = app.tab_reads().config().clone();
    config.feeds[0].provider = ProviderKind::MetaTrader;
    events
        .try_send(FeedEvent::Backfilled(vec![trade(1)]))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    while commands.try_recv().is_ok() {}
    app.active_tab_mut().history_step = 2000;
    app.active_tab_mut().request_older_history(tab_id, &config);
    assert!(matches!(
        commands.try_recv(),
        Ok(FeedCommand::LoadOlder { count: 2000 })
    ));
}
