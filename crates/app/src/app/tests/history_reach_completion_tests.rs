use super::*;
use quantick_feed::history_reach::HistoryReach;

const SESSION_PRINTS: u64 = 1_000_000;
const SESSION_START_MS: i64 = 1_790_000_000_000;

/// A print every 32 ms for a million prints a day: about nine traded hours,
/// then fifteen hours of nothing until the next session's open.
fn dense_trade(id: u64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        timestamp_ms: SESSION_START_MS
            + (id / SESSION_PRINTS) as i64 * 86_400_000
            + (id % SESSION_PRINTS) as i64 * 32,
        ..trade(id)
    }
}

/// One press of Yesterday on a million-print-a-day tape: the run pages back
/// through today and yesterday to the close that proves yesterday's open,
/// the chart holds still while it does, and it rebuilds once at the end.
#[test]
fn one_press_of_yesterday_reaches_the_previous_open_on_a_million_print_tape() {
    let (mut app, events, mut commands, _book) = test_app();
    let tab_id = app.tabs.active_id();
    let mut config = app.tab_reads().config().clone();
    config.feeds[0].provider = ProviderKind::MetaTrader;
    let tab = app.active_tab_mut();
    tab.flow_pane.spec.retain(BarSpec::Tick(1000));
    tab.apply_spec_changes();
    // Today is session 2; the live edge sits at its last print.
    let edge = 3 * SESSION_PRINTS;
    let mut cursor = edge - 50_000;
    events
        .try_send(FeedEvent::Backfilled(
            (cursor..edge).map(dense_trade).collect(),
        ))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    while commands.try_recv().is_ok() {}
    let held = app.active_tab().flow_pane.state.trades().len();
    app.active_tab_mut()
        .load_history(&config, HistoryReach::Sessions(1));
    let start = std::time::Instant::now();
    let deadline = start + std::time::Duration::from_secs(120);
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
        assert_eq!(
            app.active_tab().flow_pane.state.trades().len(),
            held,
            "the chart holds still while the run pages"
        );
        assert!(std::time::Instant::now() < deadline, "the run completes");
        std::thread::yield_now();
    }
    let fetched = edge - 50_000 - cursor;
    println!(
        "yesterday pages={pages} fetched={fetched} complete_ms={:.3} drain_peak_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.0,
        frame_peak.as_secs_f64() * 1000.0
    );
    assert!(
        cursor < SESSION_PRINTS,
        "the run crossed into the day before yesterday to prove the open"
    );
    let note = app.active_tab().history_note().expect("the run says where");
    assert!(note.ends_with("(1 session)"), "{note}");
    // One rebuild publishes everything at once.
    let mut lengths = std::collections::BTreeSet::new();
    while app.active_tab().flow_pane.history_pending() {
        app.active_tab_mut().drain_feed(tab_id);
        lengths.insert(app.active_tab().flow_pane.state.trades().len());
        assert!(std::time::Instant::now() < deadline, "the rebuild lands");
        std::thread::yield_now();
    }
    lengths.insert(app.active_tab().flow_pane.state.trades().len());
    assert!(
        lengths.len() <= 2,
        "the chart moved once, not once a page: {lengths:?}"
    );
    let tab = app.active_tab();
    assert!(!tab.loading.is_active(LoadingTask::History));
    for (pane, _) in tab.panes() {
        assert_eq!(pane.state.trades().len(), (edge - cursor) as usize);
        assert_eq!(pane.state.trades()[0].agg_id, cursor);
        assert_eq!(pane.state.trades().last().unwrap().agg_id, edge - 1);
    }
}

/// A MetaTrader run asks in the bridge's bounded campaign pages, whatever
/// the tab's page size says.
#[test]
fn metatrader_runs_ask_in_bounded_campaign_pages() {
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
    app.active_tab_mut().request_older_history(&config);
    assert!(matches!(
        commands.try_recv(),
        Ok(FeedCommand::LoadOlder {
            count: quantick_feed::history_reach::CAMPAIGN_PAGE_PRINTS
        })
    ));
}
