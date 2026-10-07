use super::*;

#[test]
fn the_load_older_hook_keeps_its_action_while_opening_history_is_publishing() {
    let (mut app, events, mut commands, _book) = test_app();
    while commands.try_recv().is_ok() {}
    let tab_id = app.tabs.active_id();
    for pane in app.active_tab_mut().panes_mut() {
        pane.hold_history_publication(true);
    }
    // MT5 opens the feed with an empty reply, then translates its initial
    // bridge backfill into HistoryPrepended while an early live print lands.
    events.try_send(FeedEvent::Backfilled(Vec::new())).unwrap();
    events
        .try_send(FeedEvent::HistoryPrepended(
            (0..60_000).map(trade).collect(),
        ))
        .unwrap();
    events.try_send(FeedEvent::Live(trade(60_000))).unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    assert!(app.active_tab().flow_pane.slots() > 0);
    assert!(app.active_tab().flow_pane.history_pending());
    assert!(
        app.active_tab()
            .loading
            .is_active(LoadingTask::HistoryRebuild)
    );
    assert!(!app.active_tab().loading.is_active(LoadingTask::History));
    app.chrome.harness.arm_load_older(1, 10);
    app.chrome
        .harness
        .apply_load_older(&mut app.tabs, &app.config);
    assert!(commands.try_recv().is_err());
    assert_eq!(
        app.chrome.harness.load_older_remaining(),
        Some((1, 10)),
        "early live bars must not consume an action refused during publication"
    );

    for pane in app.active_tab_mut().panes_mut() {
        pane.hold_history_publication(false);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while app.active_tab().flow_pane.history_pending() {
        app.active_tab_mut().drain_feed(tab_id);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    app.chrome
        .harness
        .apply_load_older(&mut app.tabs, &app.config);
    assert!(matches!(
        commands.try_recv(),
        Ok(FeedCommand::LoadOlder { .. })
    ));
    assert_eq!(app.chrome.harness.load_older_remaining(), None);
}

#[test]
#[ignore = "manual comparable history recut and frame-work benchmark"]
fn benchmark_history_frame_work() {
    for (held, page) in [(500_000, 200_000), (1_000_000, 2_000), (1_000_000, 200_000)] {
        let mut serial_times = Vec::new();
        let mut queued_times = Vec::new();
        let mut completed_times = Vec::new();
        let mut frame_peaks = Vec::new();
        for _ in 0..5 {
            let recent: Vec<_> = (page..page + held).map(trade).collect();
            let older: Vec<_> = (0..page).map(trade).collect();
            let mut serial = crate::pane::ChartPane::time(1, 60_000);
            serial.ingest_backfill(&recent);
            let start = std::time::Instant::now();
            serial.prepend_history(&older);
            serial_times.push(start.elapsed().as_secs_f64() * 1000.0);
            let mut asynchronous = crate::pane::ChartPane::time(2, 60_000);
            asynchronous.ingest_backfill(&recent);
            let start = std::time::Instant::now();
            asynchronous.receive_history(std::sync::Arc::new(older), true);
            let frame = std::time::Instant::now();
            let mut ready = asynchronous.prepare_history();
            let mut peak = frame.elapsed().as_secs_f64() * 1000.0;
            queued_times.push(start.elapsed().as_secs_f64() * 1000.0);
            while !ready {
                std::thread::sleep(std::time::Duration::from_millis(1));
                let frame = std::time::Instant::now();
                ready = asynchronous.prepare_history();
                peak = peak.max(frame.elapsed().as_secs_f64() * 1000.0);
            }
            let frame = std::time::Instant::now();
            assert!(asynchronous.install_history());
            peak = peak.max(frame.elapsed().as_secs_f64() * 1000.0);
            completed_times.push(start.elapsed().as_secs_f64() * 1000.0);
            frame_peaks.push(peak);
            assert_eq!(asynchronous.state.bars(), serial.state.bars());
            assert_eq!(
                asynchronous.state.backfill_boundary(),
                serial.state.backfill_boundary()
            );
        }
        for samples in [
            &mut serial_times,
            &mut queued_times,
            &mut completed_times,
            &mut frame_peaks,
        ] {
            samples.sort_by(f64::total_cmp);
        }
        println!(
            "history held={held} page={page} runs=5 serial_blocked_median_ms={:.3} queue_median_ms={:.3} complete_median_ms={:.3} frame_peak_median_ms={:.3}",
            serial_times[2], queued_times[2], completed_times[2], frame_peaks[2]
        );
    }
}

#[test]
fn repeated_history_requests_are_coalesced_while_one_reply_is_outstanding() {
    let (mut app, events, mut commands, _book) = test_app();
    let config = app.tab_reads().config().clone();
    let tab_id = app.tabs.active_id();
    events
        .try_send(FeedEvent::Backfilled((1..=10).map(trade).collect()))
        .unwrap();
    app.active_tab_mut().drain_feed(tab_id);
    for _ in 0..10 {
        app.active_tab_mut().request_older_history(&config);
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

#[test]
fn mixed_size_panes_share_admission_and_settle_the_history_reply_once() {
    let (mut app, events, _commands, _book) = test_app();
    let tab = app.active_tab_mut();
    tab.time_panes.clear();
    tab.time_panes
        .push(crate::pane::ChartPane::time(999, 60_000));
    tab.flow_pane
        .ingest_backfill(&(100..105).map(trade).collect::<Vec<_>>());
    tab.time_panes[0].ingest_backfill(&(100..60_100).map(trade).collect::<Vec<_>>());
    tab.loading.set_active(LoadingTask::History, true);
    let tab_id = app.tabs.active_id();
    events
        .try_send(FeedEvent::HistoryPrepended((0..100).map(trade).collect()))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        app.active_tab_mut().drain_feed(tab_id);
        let tab = app.active_tab();
        let flow_added = tab.flow_pane.state.trades().len() - 5;
        let time_added = tab.time_panes[0].state.trades().len() - 60_000;
        assert_eq!(
            flow_added, time_added,
            "the small pane waits for the large pane"
        );
        // The reply ends the request's wait at once; the panes publish
        // together when the shared recut lands.
        if !tab.loading.is_active(LoadingTask::History)
            && !tab.flow_pane.history_pending()
            && !tab.time_panes[0].history_pending()
        {
            assert_eq!(flow_added, 100);
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the flow-owned reply must settle"
        );
        std::thread::yield_now();
    }
}
