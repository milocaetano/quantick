use super::*;

#[test]
fn the_load_older_hook_keeps_its_action_while_opening_history_is_publishing() {
    let (mut app, events, mut commands, _book) = test_app();
    while commands.try_recv().is_ok() {}
    let tab_id = app.runtime.tabs.active_id();
    for mut pane in app.active_tab_mut().panes_mut() {
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
    assert!(app.active_tab().flow().slots() > 0);
    assert!(app.active_tab().flow().history_pending());
    assert!(
        app.active_tab()
            .loading
            .is_active(LoadingTask::HistoryRebuild)
    );
    assert!(!app.active_tab().loading.is_active(LoadingTask::History));
    app.chrome.harness.arm_load_older(1, 10);
    app.chrome
        .harness
        .apply_load_older(&mut app.runtime.tabs, &app.config);
    assert!(commands.try_recv().is_err());
    assert_eq!(
        app.chrome.harness.load_older_remaining(),
        Some((1, 10)),
        "early live bars must not consume an action refused during publication"
    );

    for mut pane in app.active_tab_mut().panes_mut() {
        pane.hold_history_publication(false);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while app.active_tab().flow().history_pending() {
        app.active_tab_mut().drain_feed(tab_id);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    app.chrome
        .harness
        .apply_load_older(&mut app.runtime.tabs, &app.config);
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
            let mut serial_parts = crate::pane::PaneParts::time(1, 60_000);
            let mut serial = serial_parts.at();
            serial.ingest_backfill(&recent);
            let start = std::time::Instant::now();
            serial.prepend_history(&older);
            serial_times.push(start.elapsed().as_secs_f64() * 1000.0);
            let mut asynchronous_parts = crate::pane::PaneParts::time(2, 60_000);
            let mut asynchronous = asynchronous_parts.at();
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
            assert_eq!(
                asynchronous.series_state().bars(),
                serial.series_state().bars()
            );
            assert_eq!(
                asynchronous.series_state().backfill_boundary(),
                serial.series_state().backfill_boundary()
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
    let tab_id = app.runtime.tabs.active_id();
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
        .push_time_pane(crate::pane::ChartPane::time(999, 60_000));
    let tab_id = app.runtime.tabs.active_id();
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
        let flow = tab.flow_series().state().trades().len();
        let time = tab.pane_at(1).unwrap().series.state().trades().len();
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
    assert_eq!(
        tab.flow_series().state().trades()[0].agg_id,
        trade(0).agg_id
    );
    assert_eq!(
        tab.flow_series().state().trades().last().unwrap().agg_id,
        trade(100_000).agg_id
    );
}

#[test]
fn mixed_size_panes_share_admission_and_settle_the_history_reply_once() {
    let (mut app, events, _commands, _book) = test_app();
    let tab = app.active_tab_mut();
    tab.time_panes.clear();
    tab.push_time_pane(crate::pane::ChartPane::time(999, 60_000));
    tab.flow_mut()
        .ingest_backfill(&(100..105).map(trade).collect::<Vec<_>>());
    tab.pane_at_mut(1)
        .unwrap()
        .ingest_backfill(&(100..60_100).map(trade).collect::<Vec<_>>());
    tab.loading.set_active(LoadingTask::History, true);
    let tab_id = app.runtime.tabs.active_id();
    events
        .try_send(FeedEvent::HistoryPrepended((0..100).map(trade).collect()))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        app.active_tab_mut().drain_feed(tab_id);
        let tab = app.active_tab();
        let flow_added = tab.flow_series().state().trades().len() - 5;
        let time_added = tab.pane_at(1).unwrap().series.state().trades().len() - 60_000;
        assert_eq!(
            flow_added, time_added,
            "the small pane waits for the large pane"
        );
        // The reply ends the request's wait at once; the panes publish
        // together when the shared recut lands.
        if !tab.loading.is_active(LoadingTask::History)
            && !tab.flow().history_pending()
            && !tab.pane_at(1).unwrap().history_pending()
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

/// The history worker recuts a pane whose spec changed beside the frame; what
/// it rebuilds lands in the tab session under that pane's id, and the pane
/// draws from there.
#[test]
fn history_recut_lands_in_the_tab_session() {
    let (mut app, events, _commands, _book) = test_app();
    let tab_id = app.runtime.tabs.active_id();
    let tab = app.active_tab_mut();
    tab.push_time_pane(crate::pane::ChartPane::time(999, 60_000));
    tab.loading.set_active(LoadingTask::History, true);
    let recent: Vec<_> = (50_000..100_000).map(trade).collect();
    tab.pane_at_mut(1).unwrap().ingest_backfill(&recent);
    tab.set_pane_bar_spec(1, crate::state::BarSpec::Time(300_000))
        .expect("a time interval is always available");
    events
        .try_send(FeedEvent::HistoryPrepended(
            (0..50_000).map(trade).collect(),
        ))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        app.active_tab_mut().drain_feed(tab_id);
        let pane = app.active_tab().pane_at(1).unwrap();
        if !pane.history_pending() && pane.series.state().trades().len() == 100_000 {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the recut completed");
        std::thread::yield_now();
    }
    let tab = app.active_tab();
    let pane = tab.pane_at(1).unwrap();
    let series = tab
        .session
        .series(999)
        .expect("the session holds the pane's series");
    assert!(
        std::ptr::eq(series, pane.series),
        "the pane reads the session's series"
    );
    assert_eq!(
        series.state().spec(),
        &crate::state::BarConfiguration::from(crate::state::BarSpec::Time(300_000))
    );
    assert_eq!(series.state().trades()[0].agg_id, trade(0).agg_id);
    let mut alone = crate::state::ChartState::new(crate::state::BarSpec::Time(300_000));
    alone.ingest_backfill(&(0..100_000).map(trade).collect::<Vec<_>>());
    assert_eq!(
        series.state().bars(),
        alone.bars(),
        "the recut is the whole tape"
    );
    assert_eq!(
        pane.slots(),
        series.closed_slots() + usize::from(alone.partial().is_some())
    );
}
