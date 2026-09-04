use super::*;

#[test]
fn the_newest_notice_wins_and_clear_puts_the_chart_back() {
    let (mut app, notices, _feed_ends) = test_app_with_notices();
    assert_eq!(
        app.active_tab().notice,
        FeedNotice::Clear,
        "nothing to report at birth"
    );

    // A burst arriving between two frames must leave the latest state, not
    // a queue of cards to page through.
    notices
        .blocking_send(FeedNotice::working("starting the MetaTrader bridge"))
        .unwrap();
    notices
        .blocking_send(FeedNotice::attention(
            "MetaTrader 5 is not running",
            "Open the terminal and log in.",
        ))
        .unwrap();
    app.active_tab_mut().drain_notices();
    assert!(
        matches!(app.active_tab().notice, FeedNotice::Attention { .. }),
        "the newest notice is what the user sees, got {:?}",
        app.active_tab().notice
    );

    // And a feed that recovers takes its own instruction back down.
    notices.blocking_send(FeedNotice::Clear).unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(app.active_tab().notice, FeedNotice::Clear);
}

#[test]
fn only_explicit_connection_notices_drive_transport_state() {
    let (mut app, notices, _feed_ends) = test_app_with_notices();
    app.active_tab_mut().latest_trade_latency_ms = Some(42);
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Connecting
    );

    notices.blocking_send(FeedNotice::Connected).unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Connected
    );
    assert_eq!(app.active_tab().notice, FeedNotice::Clear);

    // The MetaTrader bridge supervisor and bridge server share this
    // channel. Progress or attention from either can arrive after the
    // server has reported Connected, so neither is a transport transition.
    notices
        .blocking_send(FeedNotice::working("late supervisor progress"))
        .unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Connected
    );
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Live
    );

    notices
        .blocking_send(FeedNotice::attention(
            "late supervisor warning",
            "No transport action.",
        ))
        .unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Connected
    );
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Live
    );

    notices
        .blocking_send(FeedNotice::reconnecting(
            "Hyperliquid disconnected — reconnecting",
        ))
        .unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Reconnecting
    );
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Reconnecting,
        "a previous latency observation must not keep a disconnected socket green"
    );

    notices.blocking_send(FeedNotice::Connected).unwrap();
    app.active_tab_mut().drain_notices();
    assert_eq!(
        app.active_tab().feed_connection,
        FeedConnectionState::Connected
    );
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Live
    );
}

#[test]
fn a_feed_with_nothing_to_report_leaves_the_chart_alone() {
    // Binance and replay hand over a closed channel; draining it must be a
    // no-op rather than an error the app has to special-case.
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().drain_notices();
    assert_eq!(app.active_tab().notice, FeedNotice::Clear);
}

#[test]
fn the_load_older_hook_waits_for_bars_then_presses_once_per_frame() {
    // The hook cannot fire at startup: paging asks for trades older than
    // the ones on screen, and at launch there are none — the feed would
    // refuse it as `nothing_charted_yet` and the capture would photograph
    // the refusal path rather than the feature.
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    // Whatever startup queued is not what this test is about.
    while cmd_rx.try_recv().is_ok() {}
    app.harness.arm_load_older(2, 3);
    app.apply_load_older();
    assert!(
        cmd_rx.try_recv().is_err(),
        "nothing is charted yet, so nothing may be asked for"
    );
    assert_eq!(
        app.harness.load_older_remaining(),
        Some((2, 2)),
        "it waits, spending one frame of its budget"
    );

    // Give up rather than hang a capture run on a bridge that never came:
    // the budget counts down one frame at a time and then the hook is done.
    for _ in 0..3 {
        app.apply_load_older();
    }
    assert_eq!(
        app.harness.load_older_remaining(),
        None,
        "the budget is finite"
    );
    assert!(
        cmd_rx.try_recv().is_err(),
        "and it gave up quietly rather than asking from nothing"
    );

    // With bars, it presses — through the button's own function, so the
    // loading indicator the trader sees is the one a hooked run drives.
    let (mut app, mut cmd_rx) = app_with_history(200);
    while cmd_rx.try_recv().is_ok() {}
    app.active_tab_mut().loading.end(LoadingTask::History);
    app.harness.arm_load_older(2, 10);
    app.apply_load_older();
    assert!(
        matches!(cmd_rx.try_recv(), Ok(FeedCommand::LoadOlder { .. })),
        "the first page is asked for"
    );
    assert_eq!(
        app.harness.load_older_remaining(),
        Some((1, 10)),
        "one still owed"
    );

    // One at a time: the feed serves one request per session, so firing
    // the second before the first is answered would have it refused and
    // answered empty.
    app.apply_load_older();
    assert!(
        cmd_rx.try_recv().is_err(),
        "a page is still in flight; the hook waits for it"
    );
    assert_eq!(app.harness.load_older_remaining(), Some((1, 10)));

    app.active_tab_mut().loading.end(LoadingTask::History);
    app.apply_load_older();
    assert!(matches!(
        cmd_rx.try_recv(),
        Ok(FeedCommand::LoadOlder { .. })
    ));
    assert_eq!(
        app.harness.load_older_remaining(),
        None,
        "both pages asked for"
    );
}

#[test]
fn loader_survives_until_every_pending_load_is_answered() {
    // Two "load older" clicks land while the initial backfill is still in
    // flight: three loads pending. The first reply must NOT hide the
    // indicator - only the last one may.
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    assert_eq!(
        app.active_tab().loading.count(LoadingTask::History),
        1,
        "backfill in flight at start"
    );

    with_config(&mut app, |tab, config| tab.request_older_history(config));
    with_config(&mut app, |tab, config| tab.request_older_history(config));
    assert_eq!(app.active_tab().loading.count(LoadingTask::History), 3);

    evt_tx.try_send(FeedEvent::Backfilled(Vec::new())).unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(
        app.active_tab().loading.count(LoadingTask::History),
        2,
        "older loads still pending"
    );

    evt_tx
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(
        app.active_tab().loading.count(LoadingTask::History),
        1,
        "one reply answers one load"
    );

    evt_tx
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(
        app.active_tab().loading.count(LoadingTask::History),
        0,
        "last reply hides the loader"
    );
}

#[test]
fn rejected_request_does_not_arm_the_loader() {
    // With the command channel closed the request never reaches the feed,
    // so no reply will ever come - the count must not grow.
    let (mut app, _evt_tx, cmd_rx, _book_tx) = test_app();
    drop(cmd_rx);
    with_config(&mut app, |tab, config| tab.request_older_history(config));
    assert_eq!(
        app.active_tab().loading.count(LoadingTask::History),
        1,
        "only the initial backfill"
    );
}

#[test]
fn a_source_reset_restarts_the_history_wait() {
    // Loads queued before a reset will never be answered; the refill after
    // the reset is the one load left in flight.
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    with_config(&mut app, |tab, config| tab.request_older_history(config));
    with_config(&mut app, |tab, config| tab.request_older_history(config));
    assert_eq!(app.active_tab().loading.count(LoadingTask::History), 3);
    app.active_tab_mut()
        .flow_pane
        .drawings
        .place(drawing_tool("horizontal-line"), ChartPoint::at(1.0, 100.0));

    evt_tx.try_send(FeedEvent::Reset).unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().loading.count(LoadingTask::History), 1);
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "a rewind rebuilds the bars, not the trader's marks (§D7b)"
    );
}

/// The popup is chrome over a chart that may be full of bars, so it takes
/// the pointer rather than letting it through to the candles underneath.
///
/// Before this the body was painted, not allocated: a click on the
/// headline reached the pane's own `click_and_drag` sense and panned the
/// chart, or dropped a drawing anchor on it, while the dismissal logic
/// counted the same click as landing inside the popup and left it open.
#[test]
fn a_click_on_the_popup_never_reaches_the_chart() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    let ctx = egui::Context::default();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1), trade(2), trade(3)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(app.control_feed_popup_open());

    // Where the popup landed, derived rather than assumed: the corner is
    // measured against the canvas, and on a flow-only layout the pane *is*
    // the canvas — which the chip's own rectangle proves before the popup
    // is measured from it.
    let stall = app
        .active_tab()
        .stall_at(&app.config, metrics::wall_clock_ms());
    let report = feed_notice::report(&app.active_tab().notice, stall.as_ref())
        .expect("a stalled feed speaks");
    let area = app
        .active_tab()
        .flow_pane
        .last_area
        .expect("the pane painted");
    let mut popup = egui::Rect::NOTHING;
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        assert_eq!(
            feed_notice::chip_rect(&painter, area),
            chip,
            "the pane is the canvas on this layout"
        );
        popup = feed_notice::popup_rect(&painter, area, chip, &report);
    });

    // The headline's own row: painted text, which registers no widget.
    let on_the_sentence = egui::pos2(popup.center().x, popup.top() + 12.0);
    let held = app.active_tab().flow_pane.state.trades().len();
    let before = {
        let viewport = &app.active_tab().flow_pane.viewport;
        (viewport.px_per_bar(), viewport.right_edge_bar(held))
    };
    click_chart(&mut app, &ctx, on_the_sentence);

    assert!(
        app.control_feed_popup_open(),
        "a click on the popup is not a click somewhere else"
    );
    let after = {
        let viewport = &app.active_tab().flow_pane.viewport;
        (viewport.px_per_bar(), viewport.right_edge_bar(held))
    };
    assert_eq!(
        after, before,
        "and it must not have moved the chart under it"
    );
}

/// A feed that recovered while the trader was reading about it takes the
/// explanation away with it, rather than leaving a stale one on the chart.
#[test]
fn a_recovered_feed_puts_the_popup_away() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);
    let chip = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip.center());
    assert!(app.control_feed_popup_open());

    app.active_tab_mut().forced_stall = None;
    run_frame(&mut app, &ctx);
    assert!(!app.control_feed_popup_open());
    assert!(app.control_feed_chip_rect().is_none());
}

/// The floor lives for one event. Left standing it swallowed the next
/// *load older* answer whole, because every older print is below it.
#[test]
fn a_resume_floor_never_outlives_the_event_it_filtered() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(5)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    let floor = app.active_tab().latest_trade_ms.expect("a print landed");
    app.active_tab_mut().resume_floor_ms = Some(floor);

    // A session that replays a window containing nothing new — a venue
    // that replays nothing at all sends exactly this.
    events
        .blocking_send(FeedEvent::Backfilled(Vec::new()))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(
        app.active_tab().resume_floor_ms,
        None,
        "the window is replayed once, at the start"
    );
    assert!(
        app.active_tab().feed_gaps.is_empty(),
        "nothing was missed, so nothing is marked"
    );

    // And the page that follows is prepended rather than dropped.
    let older = vec![
        quantick_engine::Trade {
            agg_id: 1,
            timestamp_ms: floor - 10_000,
            ..trade(1)
        },
        quantick_engine::Trade {
            agg_id: 2,
            timestamp_ms: floor - 5_000,
            ..trade(2)
        },
    ];
    let held = app.active_tab().flow_pane.state.trades().len();
    events
        .blocking_send(FeedEvent::HistoryPrepended(older))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(
        app.active_tab().flow_pane.state.trades().len(),
        held + 2,
        "load older must not be swallowed by a floor from a past reconnect"
    );
}

/// A floor and a seam both belong to the timeline that made them. Carried
/// into another market they filter its prints against the old clock and
/// paint a gap on a chart that never reconnected.
#[test]
fn a_market_switch_leaves_no_floor_and_no_seam_behind() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(vec![trade(1)]))
        .unwrap();
    app.active_tab_mut().drain_feed();
    app.active_tab_mut().resume_floor_ms = app.active_tab().latest_trade_ms;
    app.active_tab_mut().feed_gaps.push(crate::feed::FeedGap {
        from_ms: 1,
        to_ms: 1_000_000,
    });

    app.active_tab_mut().reset_market_state();
    assert_eq!(app.active_tab().resume_floor_ms, None);
    assert!(app.active_tab().feed_gaps.is_empty());
}

/// The budget is measured from the transport's own transition, so a
/// supervisor alternating two lines cannot keep resetting it — the failure
/// the escalation exists to end.
#[test]
fn an_alternating_supervisor_cannot_hold_the_reconnect_budget_open() {
    use crate::feed::stall::RECONNECT_BUDGET_MS;
    let (mut app, notices, _feed_ends) = test_app_with_notices();
    notices
        .blocking_send(FeedNotice::reconnecting("bridge lost — reconnecting"))
        .unwrap();
    app.active_tab_mut().drain_notices_at(0);

    // Two lines, alternating, while the transport stays exactly as broken.
    for step in 1..=8 {
        let notice = if step % 2 == 0 {
            FeedNotice::reconnecting("bridge lost — reconnecting")
        } else {
            FeedNotice::working("waiting for the bridge")
        };
        notices.blocking_send(notice).unwrap();
        app.active_tab_mut().drain_notices_at(step * 3_000);
    }

    let config = app.control_config().clone();
    assert!(
        app.active_tab()
            .stall_at(&config, RECONNECT_BUDGET_MS)
            .is_some(),
        "the budget runs from the transition, not from the newest sentence"
    );
}

#[test]
fn quiet_market_keeps_the_observed_arrival_latency_live() {
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    app.active_tab_mut().feed_connection = FeedConnectionState::Connected;
    let trade = trade(1);
    let received_at_ms = trade.timestamp_ms + 42;

    app.active_tab_mut()
        .ingest_live_trade_at(&trade, received_at_ms);

    assert_eq!(app.active_tab().trade_arrival_ms(), Some(42));
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Live
    );
    assert_eq!(
        app.active_tab().trade_arrival_ms(),
        Some(42),
        "reading the status again without another print must not age latency"
    );
}

#[test]
fn backfill_does_not_claim_a_live_transport_latency() {
    let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(1)]))
        .unwrap();

    app.active_tab_mut().drain_feed();

    assert_eq!(app.active_tab().trade_arrival_ms(), None);
    assert_eq!(
        statusbar::feed_state(false, app.active_tab().feed_connection),
        statusbar::FeedState::Connecting
    );
}

/// The dark chart: with the view panned into history, a coarser spec cuts
/// the same trades into far fewer bars, and the old right-edge index falls
/// off the end of the series — leaving the window over empty space, where
/// nothing is drawn at all.
#[test]
fn a_rebuild_keeps_the_view_on_the_market_time_it_was_showing() {
    let (mut app, _cmd_rx) = app_with_history(400);
    // Pan back to bar 200 of 400 and remember what the edge was showing.
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .viewport
        .pan_pixels(200.0 * 8.0, slots);
    assert!(!app.active_tab().flow_pane.viewport.follows_live());
    let was_showing = app
        .active_tab()
        .flow_pane
        .right_edge_time()
        .expect("a bar under the edge");

    // Coarsen: 400 trades become 10 bars, so index 200 no longer exists.
    app.active_tab_mut().flow_pane.tick_n = 40;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 10);

    let slots = app.active_tab().flow_pane.slots();
    let (start, end) = app
        .active_tab()
        .flow_pane
        .viewport
        .visible_range(800.0, slots);
    assert!(
        start < end,
        "the window must still hold bars, got {start}..{end} of {slots}"
    );
    let now_showing = app
        .active_tab()
        .flow_pane
        .right_edge_time()
        .expect("still on a bar");
    let bar = &app.active_tab().flow_pane.state.bars()
        [app.active_tab().flow_pane.viewport.right_edge_bar(slots) as usize];
    assert!(
        bar.open_time <= was_showing && was_showing <= bar.close_time,
        "the edge bar ({}..{}) must span the time it was showing ({was_showing})",
        bar.open_time,
        bar.close_time
    );
    assert!(now_showing <= was_showing, "never jumps into the future");
}

/// Finer, not coarser: the series grows and the same market time moves to
/// a much higher index. Following that is what keeps the user's place.
#[test]
fn a_finer_spec_follows_the_same_market_time_forward() {
    let (mut app, _cmd_rx) = app_with_history(400);
    app.active_tab_mut().flow_pane.tick_n = 40;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let slots = app.active_tab().flow_pane.slots();
    app.active_tab_mut()
        .flow_pane
        .viewport
        .pan_pixels(5.0 * 8.0, slots); // back to bar 4 of 10
    let was_showing = app
        .active_tab()
        .flow_pane
        .right_edge_time()
        .expect("a bar under the edge");

    app.active_tab_mut().flow_pane.tick_n = 1;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 400);
    let edge = app
        .active_tab()
        .flow_pane
        .viewport
        .right_edge_bar(app.active_tab().flow_pane.slots());
    assert_eq!(
        edge, 160.0,
        "bar 4 of tick(40) opens on trade 161 — bar 160 of tick(1)"
    );
    assert_eq!(
        app.active_tab().flow_pane.right_edge_time(),
        Some(was_showing)
    );
}

#[test]
fn changing_feed_falls_back_to_a_valid_symbol() {
    // Two feeds with disjoint symbol lists: switching to a feed that does
    // not offer the current symbol must snap to that feed's first symbol.
    let (_evt_tx, evt_rx) = mpsc::channel(8);
    let (_book_tx, book_rx) = mpsc::channel(8);
    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    let config = AppConfig {
        default_feed: "a".to_string(),
        default_symbol: "AAA".to_string(),
        feeds: vec![
            FeedConfig {
                id: "a".to_string(),
                name: "A".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["AAA".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
            },
            FeedConfig {
                id: "b".to_string(),
                name: "B".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["BBB".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
            },
        ],
        metatrader: Default::default(),
        paper: Default::default(),
        deals: Default::default(),
        history: Default::default(),
    };
    let mut app = QuantickApp::new(
        config,
        "a",
        "AAA",
        BarSpec::Tick(10),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );

    app.active_tab_mut().feed_id = "b".to_string();
    with_config(&mut app, |tab, config| tab.ensure_symbol_valid(config));
    assert_eq!(
        app.active_tab().symbol,
        "BBB",
        "symbol snaps to feed b's first symbol"
    );

    // A symbol already valid for the feed is left untouched.
    app.active_tab_mut().symbol = "BBB".to_string();
    with_config(&mut app, |tab, config| tab.ensure_symbol_valid(config));
    assert_eq!(app.active_tab().symbol, "BBB");
}

#[test]
fn a_symbol_hop_inside_one_feed_keeps_the_panel_look() {
    let mut config = test_config();
    config.feeds[0].bubble_preset = Some("live lane pie".to_string());
    let mut app = app_on(config, "binance", "TESTUSDT");
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );

    // The user picks a different look by hand mid-session...
    assert!(app.active_tab_mut().tape_mut().apply_preset("dense tape"));
    // ...then hops symbols inside the same feed: the hand-picked look
    // survives — the declared preset belongs to the feed, not the symbol.
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "binance", "OTHERUSDT")
    });
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "dense tape"
    );

    // Arriving from another feed is what re-applies the declared look.
    with_config(&mut app, |tab, config| {
        tab.apply_feed_bubble_preset_after_switch(config, "other-feed", "TESTUSDT")
    });
    assert_eq!(
        app.active_tab().tape().active_preset_for_test(),
        "live lane pie"
    );
}

#[test]
fn replay_keeps_the_recorded_symbol_out_of_the_live_feed_snap() {
    // A recorded instrument no configured live feed offers must survive
    // the toolbar frame untouched — snapping it away would relabel the
    // whole session on the status bar and in the logs. The live path,
    // drawn through the very same frame, must keep snapping an invalid
    // selection back to the feed's list.
    let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
    let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINJ26_2026-03-16.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("fixture session parses");
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            crate::feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: crate::feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    assert_eq!(app.active_tab().symbol, "WINJ26");

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
    assert_eq!(
        app.active_tab().symbol,
        "WINJ26",
        "a toolbar frame during replay must not relabel the session"
    );

    // The same frame path with the replay closed: validation still works.
    app.active_tab_mut().replay = None;
    app.active_tab_mut().symbol = "NOT-A-SYMBOL".to_owned();
    let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
    assert_eq!(
        app.active_tab().symbol,
        "TESTUSDT",
        "live selections keep snapping to the feed's symbol list"
    );
}

/// The one layout that has no flow pane to focus still focuses something.
#[test]
fn a_window_that_opens_on_the_timeframe_alone_focuses_it() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, _cmd_rx) = mpsc::channel(16);
    let mut config = test_config();
    config.feeds[0].default_layout = Some(crate::config::DeclaredLayout::Time);
    let mut app = QuantickApp::new(
        config,
        "binance",
        "TESTUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let _ends = (evt_tx, book_tx);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));
}

/// A chart opens on one week, not a quarter — and the quarter is still
/// reachable, a week at a time. "+ older candles" asks for the same span
/// again with its right-hand edge moved to just before the oldest bucket
/// held, so the two windows meet without overlapping.
#[test]
fn asking_for_older_candles_reaches_back_past_the_oldest_held() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    assert_eq!(
        drain_ohlcv_before_requests(&mut commands),
        vec![None],
        "the opening request reaches back from the live edge"
    );
    assert!(
        !app.active_tab()
            .can_load_older_candles(app.active_tab().capabilities(&app.config)),
        "with nothing held there is nothing to reach back from"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-120, -20),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    let oldest = -120 * crate::feed::OHLCV_BASE_INTERVAL_MS;
    assert_eq!(app.active_tab().venue_candles_held(), 100);
    assert!(
        app.active_tab()
            .can_load_older_candles(app.active_tab().capabilities(&app.config)),
        "now there is"
    );

    let slots_before = app.active_tab().pane(PaneSide::Time(0)).slots();
    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    assert_eq!(
        drain_ohlcv_before_requests(&mut commands),
        vec![Some(oldest - 1)],
        "one millisecond before the oldest bucket held, so nothing is fetched twice"
    );
    assert!(
        app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and the chart says it is waiting again"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-200, -120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    assert_eq!(
        app.active_tab().venue_candles_held(),
        180,
        "the older span went in front of what was already held"
    );
    assert!(
        app.active_tab().pane(PaneSide::Time(0)).slots() > slots_before,
        "and the chart grew leftwards by it"
    );
    assert!(
        app.active_tab()
            .can_load_older_candles(app.active_tab().capabilities(&app.config)),
        "a span that brought something older leaves the door open"
    );
}

/// A short answer teaches nothing about where the record starts. A venue
/// that stopped answering, or a socket that failed, brings back nothing
/// older for a reason that has nothing to do with the venue's depth —
/// latching on it would retire the control for the session and tell the
/// trader their history begins here, which is a lie the data-honesty rule
/// exists to prevent.
#[test]
fn a_short_answer_never_retires_the_candle_reach() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_fetches(&mut commands);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-120, -20),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    assert_eq!(drain_ohlcv_fetches(&mut commands).len(), 1);
    // The shape of a failed fetch: nothing, and known to be short.
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: Vec::new(),
            slice: crate::feed::OhlcvSlice::Last { complete: false },
        })
        .unwrap();
    app.drain_tabs();

    let capabilities = app.active_tab().capabilities(&app.config);
    assert_eq!(
        app.active_tab().older_candles(capabilities),
        crate::tab::OlderCandles::Available,
        "a short answer is a reason to try again, not to stop offering"
    );
    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    assert_eq!(
        drain_ohlcv_fetches(&mut commands).len(),
        1,
        "and pressing it again really asks"
    );
}

/// A reach-back reply at an interval the pane cannot fold from is refused,
/// not obeyed. Recording an empty base is right for the *opening* answer —
/// it is how "this venue serves nothing this pane can fold" is remembered
/// — but doing it here would throw away every span the trader had already
/// waited for, over one unusable slice.
#[test]
fn a_reach_back_at_a_bad_interval_keeps_the_history_already_paged_in() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_fetches(&mut commands);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-120, -20),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    let held = app.active_tab().venue_candles_held();
    assert_eq!(held, 100);

    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    drain_ohlcv_fetches(&mut commands);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: 5 * crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history_range(-200, -120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    assert_eq!(
        app.active_tab().venue_candles_held(),
        held,
        "the week the trader already has survives a slice it cannot fold"
    );
    let capabilities = app.active_tab().capabilities(&app.config);
    assert_eq!(
        app.active_tab().older_candles(capabilities),
        crate::tab::OlderCandles::Available,
        "and the reach is still offered"
    );
}

/// The venue's record starts somewhere, and the only way to find that out
/// is to ask. A provider that answers a *load older* by re-sending what is
/// already held is not answering empty — so the test is whether the oldest
/// bucket moved, and a run that did not move it stops offering the button.
#[test]
fn a_load_older_that_brings_nothing_older_stops_offering() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_before_requests(&mut commands);
    let held = venue_history_range(-120, -20);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: held.clone(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    assert_eq!(drain_ohlcv_before_requests(&mut commands).len(), 1);
    // The same candles again — an honest answer from a provider serving
    // from a block it already holds, and not an empty one.
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: held,
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    assert!(
        !app.active_tab()
            .can_load_older_candles(app.active_tab().capabilities(&app.config)),
        "nothing older came back, so there is nothing older to offer"
    );
    app.apply_toolbar_action(ToolbarAction::LoadOlderCandles);
    assert!(
        drain_ohlcv_before_requests(&mut commands).is_empty(),
        "and pressing it again asks the venue nothing"
    );
}

/// The progressive answer: each slice paints when it lands, the chart
/// grows leftwards, and the wait ends on the closing slice — not the
/// first one.
#[test]
fn progressive_slices_paint_as_they_arrive_and_the_wait_ends_on_the_last() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    assert_eq!(
        drain_ohlcv_slice_requests(&mut commands),
        vec![Some(crate::feed::OHLCV_SLICE_SPAN_MS)],
        "the default asks for slices"
    );

    // A trader reading back through history rather than pinned to live:
    // the only state in which a prepend can move the chart under them. A
    // following pane is immune by construction — its right edge *is* the
    // newest bar — so this is where the guarantee has to be proven.
    {
        let pane = app.active_tab_mut().pane_mut(PaneSide::Time(0));
        let total = pane.slots();
        pane.viewport.pan_pixels(200.0, total);
        assert!(
            pane.viewport.right_edge_bar(total) < total.saturating_sub(1) as f32,
            "the pane must really be panned back or this proves nothing"
        );
    }
    let anchored_bar = {
        let pane = app.active_tab().pane(PaneSide::Time(0));
        pane.viewport.right_edge_bar(pane.slots())
    };

    // Newest week first, then older ones behind it: what a provider
    // walking `ohlcv_plan::plan` sends.
    let slices = [
        (venue_history_range(-20, 0), 20),
        (venue_history_range(-40, -20), 40),
        (venue_history_range(-60, -40), 60),
    ];
    for (bars, expected_seam) in &slices[..2] {
        events
            .try_send(FeedEvent::OhlcvHistory {
                interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
                bars: bars.clone(),
                slice: crate::feed::OhlcvSlice::More,
            })
            .unwrap();
        app.drain_tabs();
        assert_eq!(
            app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
            *expected_seam,
            "the prefix grew by the slice that just landed"
        );
        assert!(
            app.active_tab()
                .loading
                .is_active(LoadingTask::VenueHistory),
            "and the chart still says more is coming"
        );
        // It paints mid-run: a partial prefix is a chart, not a stall.
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(has_price_axis(&texts), "the pane draws mid-run: {texts:?}");
        // And the bar the trader was looking at is still under the same
        // edge: the right edge moved by exactly the bars that appeared to
        // its left, so nothing shifted on screen.
        let pane = app.active_tab().pane(PaneSide::Time(0));
        let expected = anchored_bar + *expected_seam as f32;
        assert!(
            (pane.viewport.right_edge_bar(pane.slots()) - expected).abs() < 0.001,
            "the viewport jumped: expected {expected}, got {}",
            pane.viewport.right_edge_bar(pane.slots())
        );
    }

    let (bars, expected_seam) = &slices[2];
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: bars.clone(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        *expected_seam,
        "the closing slice is merged in front, not written over the rest"
    );
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and only now is the wait over"
    );
    assert_eq!(
        drain_ohlcv_requests(&mut commands),
        0,
        "a run in progress is never asked again"
    );

    // The composed series is still searchable: `open_time` never decreases
    // across the merge, which is what the seam contract rests on.
    let pane = app.active_tab().pane(PaneSide::Time(0));
    let opens: Vec<i64> = pane
        .history_prefix
        .iter()
        .map(|bar| bar.open_time)
        .collect();
    assert!(
        opens.windows(2).all(|pair| pair[0] < pair[1]),
        "the merged prefix is strictly ascending: {opens:?}"
    );
}

/// The switch turned off restores exactly the old shape of the exchange:
/// one request that asks for no slicing, one reply that ends the wait.
#[test]
fn turning_the_switch_off_asks_for_the_whole_span_in_one_reply() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_slice_requests(&mut commands);
    // Close the request the pane made on being built, so the tab is idle
    // and free to ask again.
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: Vec::new(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    // Forget the answer and ask again with the switch off.
    app.progressive_history = false;
    app.active_tab_mut().forget_ohlcv_generation_for_test();
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_slice_requests(&mut commands),
        vec![None],
        "no slicing is asked for"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    let pane = app.active_tab().pane(PaneSide::Time(0));
    assert_eq!(pane.seam_slot(), 120, "the whole prefix stands at once");
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and the single reply ends the wait"
    );
}

/// (b) Changing the timeframe refolds what is already held. A chip click
/// must never reach the venue.
#[test]
fn a_timeframe_change_refolds_locally_without_asking_again() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_requests(&mut commands);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 120);

    // 1m → 5m: the same history, folded five ways.
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .time_interval_ms = 5 * crate::feed::OHLCV_BASE_INTERVAL_MS;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        24,
        "120 minutes are 24 five-minute bars"
    );
    assert_eq!(
        drain_ohlcv_requests(&mut commands),
        0,
        "and the venue was not asked again"
    );
}

/// §11's own clause, and one drag away in the UI: an interval that is not
/// a whole number of minutes gets no prefix rather than an approximated
/// one, and the pane keeps drawing.
#[test]
fn an_unfoldable_interval_drops_the_prefix_and_still_draws() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 120);

    // 90 seconds: a minute and a half, which no whole number of venue
    // candles adds up to.
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .time_interval_ms = 90_000;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        0,
        "no prefix rather than buckets built from fractions of a candle"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        has_price_axis(&texts),
        "and the pane keeps drawing what it does have: {texts:?}"
    );

    // Back to a foldable one, and the history returns from the same base.
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .time_interval_ms = 5 * crate::feed::OHLCV_BASE_INTERVAL_MS;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 24);
}

/// A feed switch is a different market: the candles that described the old
/// one go with it, and nothing is left waiting on a reply that can never
/// arrive down a dropped channel.
#[test]
fn switching_the_feed_drops_the_prefix_and_clears_the_wait() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 120);

    // A fresh feed arrives, as a symbol switch installs one.
    let (_evt_tx, evt_rx) = mpsc::channel(8);
    let (_book_tx, book_rx) = mpsc::channel(8);
    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    app.active_tab_mut().attach_for_test(FeedHandle {
        events: evt_rx,
        book_events: book_rx,
        notices: feed::silent_notices(),
        capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
        latency: feed::unsplit_latency(),
        commands: cmd_tx,
        replay: None,
    });

    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        0,
        "the old market's candles do not describe the new one"
    );
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "and nothing waits on a reply that went with the old channel"
    );
    run_frame(&mut app, &ctx);
}

/// (f) The prefix arrives under a chart the user is already reading: the
/// right edge must not move.
#[test]
fn installing_the_prefix_keeps_the_view_where_it_was() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    // Pan off the live edge, so there is a position to preserve.
    let slots = app.active_tab().pane(PaneSide::Time(0)).slots();
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .viewport
        .pan_pixels(40.0, slots);
    let edge_time = app.active_tab().pane(PaneSide::Time(0)).right_edge_time();
    let edge_bar = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .viewport
        .right_edge_bar(slots);
    assert!(edge_time.is_some(), "the view is off the live edge");

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    let pane = app.active_tab().pane(PaneSide::Time(0));
    assert_eq!(
        pane.viewport.right_edge_bar(pane.slots()),
        edge_bar + 120.0,
        "the right edge moved with the bars inserted in front of it"
    );
    assert_eq!(
        pane.right_edge_time(),
        edge_time,
        "so the user is still looking at the same market time"
    );
}

/// The run-up a recording was downloaded with lands when the session
/// opens, with nobody pressing anything.
///
/// The context file exists precisely so a replay does not start at a
/// wall — and until now the tab refused to ask for it while replaying, so
/// the only way to see it was to press *load older* and hope. A recording
/// with no context still asks for nothing: the capability decides, and
/// `feed::replay` publishes it as `context.is_some()`.
#[test]
fn a_replay_installs_its_downloaded_context_without_a_press() {
    let ctx = egui::Context::default();
    let (mut app, _events, mut commands) = history_app(&ctx);
    drain_ohlcv_requests(&mut commands);

    let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
    let mut session = quantick_replay::Session::from_text(
        std::path::Path::new("WINJ26_2026-03-16.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("fixture session parses");
    // The previous day's close, downloaded beside the tape.
    let context = "# quantick-context 1\n# symbol=WINJ26\n# timezone=-03:00\n\
                       # interval_ms=60000\n# complete=true\n\
                       Date,Time,Open,High,Low,Close,Volume,Trades\n\
                       2026-03-13,17:57:00.000,181900,182000,181850,181950,120,0\n\
                       2026-03-13,17:58:00.000,181950,182050,181900,182000,130,0\n\
                       2026-03-13,17:59:00.000,182000,182100,181950,182050,140,0\n";
    session.context =
        Some(quantick_replay::parse_context(context).expect("context fixture parses"));
    with_config(&mut app, |tab, config| {
        tab.open_replay(
            config,
            crate::feed::ReplayRequest {
                session: std::sync::Arc::new(session),
                options: crate::feed::ReplayOptions {
                    autoplay: false,
                    ..Default::default()
                },
            },
        )
    });
    // The replay feed runs on its own thread and, paused, drains its
    // commands on a 33 ms timer — so this waits on a wall clock, not on a
    // worker that answers as fast as it is fed. A deadline with a sleep in
    // it, rather than a frame count: two hundred egui frames can finish
    // inside one of those ticks on an idle machine, and the test would
    // fail for having been fast.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        run_frame(&mut app, &ctx);
        app.drain_tabs();
        if app.active_tab().venue_candles_held() > 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        app.active_tab().venue_candles_held(),
        3,
        "the run-up downloaded beside the recording is on the chart, unpressed"
    );
    // And with the lead-in on, it reaches the chart cut by trades too —
    // the whole point of a run-up during a replay, where there are no
    // older *trades* to page: the tape in the file is the whole day.
    app.venue_lead_in = true;
    app.drain_tabs();
    assert_eq!(
        app.active_tab().flow_pane.seam_slot(),
        3,
        "the previous session sits in front of the tick chart's own bars"
    );
}

/// A reach of one page is one request and no more, however the answer
/// looks — the behaviour every release before this one had.
#[test]
fn a_reach_of_one_page_asks_once_and_stops() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "one press, one request"
    );
    // Answer it with prints still deep inside the same session.
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-60..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "and the answer asks for nothing further"
    );
    assert!(
        !app.active_tab().history_reach_running(),
        "a single page is never a run"
    );
}

/// A press with the longer reach keeps asking, page after page, until the
/// tape reaches past the market's last close and the lead beyond it.
#[test]
fn the_previous_session_reach_pages_until_the_lead_past_the_close_lands() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(drain_load_older(&mut commands).len(), 1, "the press");
    assert!(
        app.active_tab().history_reach_running(),
        "and the run is on"
    );

    // A page still inside today's session: no break has been crossed.
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-120..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        drain_load_older(&mut commands).len(),
        1,
        "so the run asks for another page by itself"
    );

    // The next page crosses the overnight break and lands the lead. The
    // previous session's last print sits a minute further back than the
    // gap threshold, so the stretch between the two sessions is wider than
    // a quiet market ever is; in front of it, exactly the lead.
    const MINUTE_MS: i64 = crate::feed::OHLCV_BASE_INTERVAL_MS;
    let close_minute = -120 - (crate::history_reach::SESSION_GAP_MS / MINUTE_MS) - 1;
    let lead_minutes = crate::history_reach::PREVIOUS_SESSION_LEAD_MS / MINUTE_MS;
    events
        .try_send(FeedEvent::HistoryPrepended(
            (close_minute - lead_minutes..=close_minute)
                .map(minute_trade_at)
                .collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert!(
        drain_load_older(&mut commands).is_empty(),
        "the previous session is on screen with its lead; the run is done"
    );
    assert!(
        !app.active_tab().history_reach_running(),
        "and nothing is left waiting on a reply"
    );
}

/// A venue that answers empty without ever saying it has run out stops the
/// run in a handful of requests, not sixty-four.
///
/// This is the case `can_page` cannot catch: only the MetaTrader bridge
/// withdraws `history_paging`, while Binance's is a compile-time `true`
/// that answers a rate-limited fetch with the same empty block it answers
/// "nothing older" with. Without the idle count, one press here would be
/// sixty-four back-to-back REST calls — how a 429 becomes an IP ban.
#[test]
fn a_venue_answering_empty_without_saying_so_stops_the_run_early() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    let mut asked = drain_load_older(&mut commands).len();
    assert_eq!(asked, 1, "the press");
    // Answer every request with nothing, as a refusing venue does. The
    // capability stays true throughout — that is the whole point.
    for _ in 0..crate::history_reach::MAX_CAMPAIGN_PAGES {
        if !app.active_tab().history_reach_running() {
            break;
        }
        events
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_tabs();
        asked += drain_load_older(&mut commands).len();
    }
    assert!(
        !app.active_tab().history_reach_running(),
        "the run gave up rather than spending its whole budget"
    );
    assert!(
        asked <= crate::history_reach::MAX_IDLE_PAGES as usize,
        "one press cost {asked} requests; the idle budget is \
             {}",
        crate::history_reach::MAX_IDLE_PAGES
    );
    assert!(
        !app.active_tab().loading.is_active(LoadingTask::History),
        "and nothing is left waiting on a reply"
    );
}

/// The sentence has to reach the glass, not just the field.
///
/// Every other test here proves the tab *holds* the right words. This one
/// proves a trader can read them, which is the entire complaint: the
/// outcome existed in a log line and nowhere a person looks.
#[test]
fn the_settled_reach_paints_its_sentence_over_the_chart() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let quiet = painted_text(&run_frame(&mut app, &ctx));
    let sentence = crate::history_reach::CampaignEnd::NothingComingBack
        .notice()
        .expect("the ending's own sentence");
    assert!(
        !quiet.iter().any(|text| text == sentence),
        "a chart nobody pressed anything on says nothing; painted: {quiet:?}"
    );

    app.active_tab_mut().raise_history_note(sentence);
    let spoken = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        spoken.iter().any(|text| text == sentence),
        "the outcome of the press has to be on screen; painted: {spoken:?}"
    );

    // And it leaves on its own, with nothing to dismiss.
    app.active_tab_mut()
        .expire_history_note(std::time::Instant::now() + crate::tab::HISTORY_NOTE_LINGER);
    let after = painted_text(&run_frame(&mut app, &ctx));
    assert!(
        !after.iter().any(|text| text == sentence),
        "and it is gone again without a click; painted: {after:?}"
    );
}

/// The reported bug, as a test: a reach that lands nothing must not end in
/// silence.
///
/// A run answered with empty page after empty page gives up after
/// `MAX_IDLE_PAGES` — correctly — and before this change said so only in a
/// log line. On screen the press was indistinguishable from a button that
/// does nothing, which is how "previous session" shipped looking like a
/// facade.
#[test]
fn a_run_that_reaches_nothing_says_so_where_the_trader_is_looking() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    drain_load_older(&mut commands);
    assert!(
        app.active_tab().history_note().is_none(),
        "a run under way has nothing to report yet"
    );
    for _ in 0..crate::history_reach::MAX_CAMPAIGN_PAGES {
        if !app.active_tab().history_reach_running() {
            break;
        }
        events
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_tabs();
        drain_load_older(&mut commands);
    }
    assert!(
        !app.active_tab().history_reach_running(),
        "the run gave up, as it should"
    );
    assert_eq!(
        app.active_tab().history_note(),
        crate::history_reach::CampaignEnd::NothingComingBack.notice(),
        "and it says the reason the campaign actually stopped for"
    );
}

/// The other half of the same rule: a press that worked says nothing. The
/// chart is a better answer than a sentence about the chart, and a message
/// after every successful press is noise a trader learns to stop reading.
#[test]
fn a_run_that_meets_its_reach_says_nothing() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    drain_load_older(&mut commands);
    // One page inside today's session, then one that crosses the close and
    // lands the lead — the shape `the_previous_session_reach_pages_until…`
    // proves in full.
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-120..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    drain_load_older(&mut commands);

    const MINUTE_MS: i64 = crate::feed::OHLCV_BASE_INTERVAL_MS;
    let close_minute = -120 - (crate::history_reach::SESSION_GAP_MS / MINUTE_MS) - 1;
    let lead_minutes = crate::history_reach::PREVIOUS_SESSION_LEAD_MS / MINUTE_MS;
    events
        .try_send(FeedEvent::HistoryPrepended(
            (close_minute - lead_minutes..=close_minute)
                .map(minute_trade_at)
                .collect(),
        ))
        .unwrap();
    app.drain_tabs();

    assert!(
        !app.active_tab().history_reach_running(),
        "the reach was met"
    );
    assert_eq!(
        app.active_tab().history_note(),
        None,
        "and yesterday being on screen is the whole of the report"
    );
}

/// The default reach is held to the same honesty. It runs no campaign, so
/// nothing settles it — and one press answered with an empty block was as
/// silent as a whole run of them.
#[test]
fn a_single_page_press_that_brings_nothing_back_says_so() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);
    assert_eq!(
        app.active_tab().history_reach,
        crate::history_reach::HistoryReach::Page,
        "the default reach, unchanged"
    );

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(drain_load_older(&mut commands).len(), 1);
    events
        .try_send(FeedEvent::HistoryPrepended(Vec::new()))
        .unwrap();
    app.drain_tabs();

    assert!(
        app.active_tab().history_note().is_some(),
        "an empty answer to a press is a fact the trader owns"
    );
}

/// And a single page that *did* bring prints back stays quiet, for the
/// same reason a met reach does.
#[test]
fn a_single_page_press_that_lands_prints_says_nothing() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_load_older(&mut commands);

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    drain_load_older(&mut commands);
    events
        .try_send(FeedEvent::HistoryPrepended(
            (-60..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();

    assert_eq!(
        app.active_tab().history_note(),
        None,
        "sixty prints appeared on the chart; that is the acknowledgement"
    );
}

/// A venue that reports its record exhausted is not asked once more.
///
/// The feed withdraws `history_paging` on that report, and a run that read
/// its own page count instead would spend the whole budget against a wall
/// with the button already greyed out beside it.
#[test]
fn a_run_stops_the_moment_the_venue_says_its_record_ends() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
        book_capture: false,
        history_paging: true,
        traded_volume: true,
        deal_counter: false,
        ohlcv_history: false,
        ohlcv_generation: 0,
    });
    let mut app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(1),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: caps_rx,
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    evt_tx
        .try_send(FeedEvent::Backfilled((0..50).map(minute_trade).collect()))
        .unwrap();
    app.drain_tabs();
    run_frame(&mut app, &ctx);
    app.history_reach = crate::history_reach::HistoryReach::PreviousSession;
    app.drain_tabs();
    drain_load_older(&mut cmd_rx);

    app.apply_toolbar_action(crate::toolbar::ToolbarAction::LoadOlder);
    assert_eq!(drain_load_older(&mut cmd_rx).len(), 1);
    // The terminal reached its own oldest tick and the feed withdrew the
    // capability, then answered the outstanding request.
    caps_tx.send_modify(|caps| caps.history_paging = false);
    evt_tx
        .try_send(FeedEvent::HistoryPrepended(
            (-10..0).map(minute_trade_at).collect(),
        ))
        .unwrap();
    app.drain_tabs();
    assert!(
        drain_load_older(&mut cmd_rx).is_empty(),
        "nothing is asked of a venue that has said it has no more"
    );
    assert!(!app.active_tab().history_reach_running());
}

/// (i) The status bar names all three sources, in the order the chart puts
/// them in.
#[test]
fn the_status_bar_counts_venue_bars_separately() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    assert_eq!(
        app.status_model().venue_bars,
        0,
        "nothing to disclose before the reply"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    let status = app.status_model();
    assert_eq!(status.venue_bars, 120);
    assert!(
        status.backfilled_bars > 0,
        "and the trade-derived counts are still their own"
    );
    // The flow pane beside it has no prefix and says nothing about one.
    let point = pane_point(&app, PaneSide::Flow);
    click_chart(&mut app, &ctx, point);
    assert_eq!(app.status_model().venue_bars, 0);
}

/// (j) A venue bucket covering the same window as the first engine bar
/// would sit after it in time and before it in slot order. It is dropped,
/// which is what keeps the composed series searchable.
#[test]
fn the_seam_drops_a_venue_bucket_that_overlaps_the_first_engine_bar() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    let first_engine_open = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .state
        .bars()
        .first()
        .map(|bar| bar.open_time)
        .or_else(|| {
            app.active_tab()
                .pane(PaneSide::Time(0))
                .state
                .partial()
                .map(|bar| bar.open_time)
        })
        .expect("the pane holds the fixture trades");

    // Two candles before the engine's first bar and one covering it.
    let mut bars = venue_history(2);
    bars.push(venue_candle(
        first_engine_open / crate::feed::OHLCV_BASE_INTERVAL_MS,
        0,
    ));
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars,
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();

    let pane = app.active_tab().pane(PaneSide::Time(0));
    assert_eq!(
        pane.seam_slot(),
        2,
        "the overlapping bucket is dropped, which keeps the slot search sound"
    );
    // Dropping it is right for ordering and lossy for volume: the engine
    // bar inheriting that slot opened on its first print, part-way into
    // the interval the venue candle covered whole. The pane owns up to it
    // so a profile folding the slot can say so — the alternative is a
    // total that silently omits everything traded before the app
    // connected (36% of a minute, 94% of an hour, measured live).
    let interval = crate::feed::OHLCV_BASE_INTERVAL_MS;
    let first_open = pane
        .state
        .bars()
        .first()
        .or_else(|| pane.state.partial())
        .map(|bar| bar.open_time)
        .expect("the pane holds the fixture trades");
    assert_ne!(
        crate::resample::bucket_start(first_open, interval),
        first_open,
        "the fixture's first engine bar opens inside its bucket"
    );
    assert_eq!(
        pane.partial_bucket_slot(),
        Some(2),
        "the tape's first bar is named as partly covered"
    );
    // The composed series is non-decreasing in open_time, which is what
    // the slot search depends on.
    let opens: Vec<i64> = (0..pane.closed_slots())
        .filter_map(|slot| pane.slot_open_time(slot))
        .collect();
    assert!(
        opens.windows(2).all(|pair| pair[0] <= pair[1]),
        "open_time never decreases across the seam"
    );
}

/// (a) Typing a contract the catalog does not have opens it and records
/// it. The real driver: B3 rotates the mini index every two months, and
/// the broker serves WINQ26 rather than the WIN$N alias.
#[test]
fn adding_a_symbol_opens_it_and_writes_it_to_the_sidecar() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let path = symbols_scratch("added");
    let _ = std::fs::remove_file(&path);
    app.workspace.set_symbols_path(path.clone());
    let tabs_before = app.tabs.len();

    // The real dialog: open it, type the contract, press Add.
    app.apply_tab_action(TabAction::New);
    run_frame(&mut app, &ctx);
    app.surfaces
        .source_picker
        .picker_mut()
        .expect("the + opened the picker")
        .set_draft_symbol("  WINQ26 ");
    run_frame(&mut app, &ctx);
    let add = app
        .surfaces
        .source_picker
        .picker()
        .expect("still open")
        .add_button_rect()
        .expect("the Add button was laid out");
    click_chart(&mut app, &ctx, add.center());
    run_frame(&mut app, &ctx);

    assert!(
        !app.surfaces.source_picker.is_open(),
        "adding closes the dialog, because it opened the market"
    );
    assert_eq!(app.tabs.len(), tabs_before + 1);
    assert_eq!(app.active_tab().symbol, "WINQ26", "and it is what opened");
    // Both surfaces that list symbols see it, because both read the
    // catalog the app is running on.
    assert!(
        app.config
            .feed("binance")
            .expect("the feed")
            .symbols
            .iter()
            .any(|symbol| symbol == "WINQ26")
    );
    assert!(app.added_symbols.contains("binance", "WINQ26"));

    let written = std::fs::read_to_string(&path).expect("the sidecar was written");
    assert!(
        written.contains("WINQ26"),
        "it records the symbol: {written}"
    );
    assert!(
        written.contains("quantick.toml is never modified"),
        "and says the config file is left alone: {written}"
    );
    let _ = std::fs::remove_file(&path);
}

/// An addition that the *whole* config would reject is refused where it
/// was typed, and nothing is written.
///
/// Typing `US500` into the B3 feed made two MetaTrader feeds offer one
/// mapped symbol — a configuration the app refuses to load. It used to be
/// accepted, persisted, and then kill the next launch with an error naming
/// the config file, which was not the file that broke.
#[test]
fn an_addition_the_config_would_reject_is_refused_and_not_written() {
    let ctx = egui::Context::default();
    let (_evt_tx, evt_rx) = mpsc::channel(8);
    let (_book_tx, book_rx) = mpsc::channel(8);
    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    let mut app = QuantickApp::new(
        two_metatrader_feeds(),
        "b3",
        "WIN$N",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::MetaTrader.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let path = symbols_scratch("refused");
    let _ = std::fs::remove_file(&path);
    app.workspace.set_symbols_path(path.clone());
    let tabs_before = app.tabs.len();

    // The real dialog, on the B3 feed, typing the Tickmill instrument.
    app.apply_tab_action(TabAction::New);
    run_frame(&mut app, &ctx);
    {
        let picker = app
            .surfaces
            .source_picker
            .picker_mut()
            .expect("the picker is open");
        picker.feed_id = "b3".to_string();
        picker.set_draft_symbol("US500");
    }
    run_frame(&mut app, &ctx);
    let add = app
        .surfaces
        .source_picker
        .picker()
        .expect("still open")
        .add_button_rect()
        .expect("the Add button was laid out");
    click_chart(&mut app, &ctx, add.center());
    run_frame(&mut app, &ctx);

    let picker = app
        .surfaces
        .source_picker
        .picker()
        .expect("the dialog stays open on a refusal");
    assert!(
        picker
            .refusal()
            .is_some_and(|reason| reason.contains("US500")),
        "the reason is shown where the symbol was typed: {:?}",
        picker.refusal()
    );
    assert_eq!(app.tabs.len(), tabs_before, "and no market was opened");
    assert!(
        !app.config
            .feed("b3")
            .expect("the feed")
            .symbols
            .iter()
            .any(|symbol| symbol == "US500"),
        "the catalog is untouched"
    );
    assert!(
        !path.exists(),
        "and nothing was persisted — the next launch is unharmed"
    );
    // The same symbol on the feed that *does* own it is still fine.
    assert!(app.add_symbol("tickmill", "WINQ26").is_ok());
    let _ = std::fs::remove_file(&path);
}

/// (b) The point of writing it: the next launch has it. Proven without
/// restarting, by loading a config through the same path `load` uses.
#[test]
fn a_recorded_symbol_is_in_the_catalog_on_the_next_load() {
    let path = symbols_scratch("reload");
    let _ = std::fs::remove_file(&path);
    let mut added = crate::symbols_file::AddedSymbols::default();
    added.add("binance", "WINQ26");
    crate::symbols_file::save(&path, &added).expect("the scratch file is writable");

    let reloaded = crate::symbols_file::load(&path);
    let mut config = test_config();
    config.merge_added_symbols(&reloaded);

    assert!(
        config
            .feed("binance")
            .expect("the feed")
            .symbols
            .iter()
            .any(|symbol| symbol == "WINQ26"),
        "a restart finds the contract the user added"
    );
    assert!(config.validate().is_ok(), "and the merged catalog is valid");
    let _ = std::fs::remove_file(&path);
}

/// Two tabs on one market are allowed — two views of one book is a
/// legitimate thing to want — and each still gets its own everything.
#[test]
fn the_same_market_can_be_open_twice() {
    let ctx = egui::Context::default();
    let (mut app, _cmd_rx) = app_with_history(50);
    let _ends = open_second_tab(&mut app, &ctx, "TESTUSDT");

    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.tabs[0].symbol, app.tabs[1].symbol);
    assert_ne!(app.tabs[0].id, app.tabs[1].id);
    assert_ne!(app.tabs[0].flow_pane.id, app.tabs[1].flow_pane.id);
    // Separate engines: what one holds says nothing about the other.
    assert!(!app.tabs[0].flow_pane.state.bars().is_empty());
    assert!(app.tabs[1].flow_pane.state.bars().is_empty());
}

/// A market order takes no price, and saying otherwise is refused before
/// anything reaches the venue — the input is wrong, not the market.
#[test]
fn a_market_action_with_a_price_is_refused_as_a_bad_request() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);

    let error = app
        .control_action(
            crate::control::trade::PLACE_CAPABILITY_ID,
            crate::control::trade::CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "side": "buy",
                "kind": "market",
                "quantity": "1",
                "price": "100",
            }),
        )
        .expect_err("a market order has no price of its own");
    assert!(
        format!("{error:?}").contains("market order has no price"),
        "{error:?}"
    );
}
