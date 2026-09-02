use super::*;
use crate::app::*;

/// The same app, plus the notice sender its feed would hold. The other
/// ends come back so the caller keeps the channels open, exactly as a live
/// feed thread would.
#[allow(clippy::type_complexity)]
/// A library entry whose file is gone must still produce something the
/// user can see: the click used to log a warning and leave the chart
/// unchanged, while this function's doc promised an error slot.
#[test]
fn a_script_that_no_longer_reads_becomes_a_visible_error_slot() {
    let (mut app, _events, _commands, _book) = test_app();
    let dir = std::env::temp_dir().join(format!("quantick-app-script-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("vanishing.pine");
    std::fs::write(
        &path,
        "//@version=5
plot(close)
",
    )
    .expect("write");

    app.script_library = crate::indicators::library::ScriptLibrary::scan_dir(&dir);
    let index = app
        .script_library
        .entries()
        .iter()
        .position(|e| e.name == "vanishing.pine")
        .expect("the file was scanned");
    std::fs::remove_file(&path).expect("remove");

    let before = app.active_tab().flow_pane.indicators.all().len();
    let slot = app
        .add_script_indicator(index)
        .expect("a click on a known entry claims a slot");
    assert_eq!(
        app.active_tab().flow_pane.indicators.all().len(),
        before + 1,
        "a slot appeared"
    );
    let view = app
        .active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|v| v.slot == slot)
        .expect("the slot has a view");
    assert!(view.error.is_some(), "and it carries the read failure");
    assert_eq!(view.label(), "vanishing.pine");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An operator sees what the trader sees: the corner is named in the
/// scene, with the rectangle it was drawn at, and the popup's two controls
/// name the capabilities that operate them.
#[test]
fn the_scene_names_the_corner_and_what_operates_it() {
    let (mut app, _notices, _channels) = test_app_with_notices();
    let ctx = egui::Context::default();
    app.active_tab_mut().forced_stall = Some(crate::feed::stall::ForcedStall::Silent);
    run_frame(&mut app, &ctx);

    let scene = observer_scene(&app);
    let chip = scene["controls"]
        .as_array()
        .expect("the scene reports a control list")
        .iter()
        .find(|control| control["control_id"] == "feed_status.chip")
        .expect("the corner is on screen, so the scene names it")
        .clone();
    assert_eq!(chip["label"], crate::feed_notice::OFFLINE_LABEL);
    assert_eq!(chip["role"], "toggle");
    assert_eq!(chip["selected"], false, "the popup is shut");
    assert!(
        chip["bounds"].is_object(),
        "the one control with no capability behind it has to be reachable by its rectangle"
    );
    assert!(
        !scene_control_ids(&scene)
            .iter()
            .any(|id| id == "feed_status.reconnect"),
        "a control behind a click is not on screen"
    );

    let chip_rect = app.control_feed_chip_rect().expect("the corner is up");
    click_chart(&mut app, &ctx, chip_rect.center());
    let scene = observer_scene(&app);
    let calls: Vec<String> = scene["controls"]
        .as_array()
        .expect("the scene reports a control list")
        .iter()
        .filter(|control| {
            control["control_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("feed_status."))
        })
        .filter_map(|control| control["capability_id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(calls, ["feed.reconnect", "feed.reload"]);
}

/// The toolbar's heatmap lamp answers "is this switched on", not "is the
/// book capture up yet".
///
/// `depth_visible()` is `enabled && show_depth`, so a lamp lit from it
/// reads dark while capture is starting — and a trader who presses a dark
/// button switches *off* the layer they were reaching for. The button
/// states a source's inability separately, through `disabled_explanation`.
#[test]
fn the_heatmap_lamp_reads_the_switch_not_the_capture() {
    let (mut app, _events, _commands, _book) = test_app();
    {
        let tape = app.active_tab_mut().tape_mut();
        tape.set_depth_visible(true);
        // Capture not up: exactly the first seconds of every launch, and
        // permanently on a source with no book.
        tape.set_enabled(false, 0);
        assert!(
            !tape.depth_visible(),
            "the renderer is right to draw nothing; this test is about the lamp"
        );
        assert!(tape.depth_switched_on(), "the switch itself is on");
    }
    assert!(
        app.heatmap_lamp_on(),
        "the lamp reads the switch, so it stays lit while capture catches up"
    );
    // And it goes dark for the one reason it should: the switch itself.
    app.active_tab_mut().tape_mut().set_depth_visible(false);
    assert!(
        !app.heatmap_lamp_on(),
        "switched off is the only way it darkens"
    );
}

/// The scripted click lands on the canvas of the pane it names, and asks
/// for nothing that is not on screen: no draw yet means no geometry, and a
/// canvas with no tape has no tape menu to open.
#[test]
fn the_scripted_click_lands_on_the_pane_it_names() {
    let (mut app, _events, _commands, _book) = test_app();
    assert_eq!(
        app.scripted_context_menu_pos(ContextMenuPane::Tape),
        None,
        "nothing has drawn yet, so there is no geometry to click"
    );

    let rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 400.0));
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.last_chart_rect = Some(rect);
        pane.last_lane_divider_x = Some(700.0);
    }
    let tape = app
        .scripted_context_menu_pos(ContextMenuPane::Tape)
        .expect("a drawn tape can be clicked");
    assert!(tape.x > 700.0 && tape.x < 1000.0, "{tape:?}");
    let chart = app
        .scripted_context_menu_pos(ContextMenuPane::Chart)
        .expect("and so can the candles");
    assert!(chart.x > 0.0 && chart.x < 700.0, "{chart:?}");
    assert!(rect.contains(tape) && rect.contains(chart));

    // No lane: the candles still answer, the tape has nothing to open.
    app.active_tab_mut().flow_pane.last_lane_divider_x = None;
    assert_eq!(app.scripted_context_menu_pos(ContextMenuPane::Tape), None);
    assert!(
        app.scripted_context_menu_pos(ContextMenuPane::Chart)
            .is_some()
    );
}

/// With a recording playing and the round trips closed, the scripted
/// seek presses Restart once — and only once, whatever the next frame
/// finds.
#[test]
fn the_scripted_replay_restart_seeks_once_the_trades_are_in() {
    let (mut app, evt_tx, mut cmd_rx, _book_tx) = test_app();
    let dir = std::env::temp_dir().join(format!(
        "quantick-replay-restart-hook-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let journal = dir.join("journal");
    app.active_tab_mut().paper.redirect_history_dir(journal);
    app.active_tab_mut().replay = Some(feed::ReplayLink::for_test(recording_at(&dir)));
    while cmd_rx.try_recv().is_ok() {}
    app.pending_replay_restart = Some(1);

    // No round trip yet: the hook waits rather than seeking an empty
    // ledger, which would photograph nothing it exists to show.
    app.apply_replay_restart();
    assert_eq!(
        app.pending_replay_restart,
        Some(1),
        "the seek fired before a trade had closed"
    );

    // One round trip, then the seek.
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    app.apply_toolbar_action(ToolbarAction::PaperClose);
    evt_tx.try_send(FeedEvent::Live(trade(6))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(|| 0);
    assert_eq!(app.active_tab().paper.session_trades().len(), 1);

    app.apply_replay_restart();
    assert_eq!(app.pending_replay_restart, None, "the hook is consumed");
    assert!(
        matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::Replay(ReplayControl::Restart))
        ),
        "the transport was asked for its own Restart"
    );

    // A second frame asks for nothing: an env var is a request for this
    // run, not a standing rule.
    app.apply_replay_restart();
    assert!(cmd_rx.try_recv().is_err(), "the seek repeated itself");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The scripted seek needs a recording under it: on a live feed there is
/// no timeline to restart, so the hook waits instead of firing into one
/// — and the transport channel stays empty.
#[test]
fn the_scripted_replay_restart_waits_for_a_recording() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    // Whatever the startup already asked the feed for is not the
    // subject; only what the hook adds after it is.
    while cmd_rx.try_recv().is_ok() {}
    app.pending_replay_restart = Some(1);
    app.apply_replay_restart();
    assert_eq!(
        app.pending_replay_restart,
        Some(1),
        "a live feed has no timeline to seek"
    );
    assert!(
        cmd_rx.try_recv().is_err(),
        "and nothing was asked of the transport"
    );
}

/// The whole gesture, walked the way a trader describes it: click, move
/// and watch a straight line follow, click again to fix it, then move and
/// watch the corridor open, and click to place.
///
/// Written from a trader's own account of how this must behave, so each
/// step asserts what is *on screen* at that step and not merely the state
/// behind it — a placement gesture does all of its talking in the frames
/// between the clicks.
#[test]
fn the_channel_walks_the_gesture_a_trader_describes() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");

    let (first, second, width) = (
        egui::pos2(600.0, 400.0),
        egui::pos2(800.0, 340.0),
        // Well below the trend line: the corridor must open downward.
        egui::pos2(800.0, 460.0),
    );

    click_chart_with(&mut app, &ctx, first, egui::Modifiers::NONE);
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        1,
        "the first click anchors the trend line"
    );

    // Moving between the first and second click shows a straight line and
    // nothing else — there is no width yet to draw.
    let output = move_chart_with(&mut app, &ctx, second, egui::Modifiers::NONE);
    assert_eq!(
        drawing_strokes(&output),
        1,
        "only the line between the anchor and the pointer"
    );

    click_chart_with(&mut app, &ctx, second, egui::Modifiers::NONE);
    assert_eq!(
        app.active_tab().flow_pane.drawings.draft_len(),
        2,
        "the second click fixes the trend line"
    );

    // From here every movement opens the corridor.
    let output = move_chart_with(&mut app, &ctx, width, egui::Modifiers::NONE);
    assert!(
        drawing_strokes(&output) >= 2,
        "both rails follow the pointer now, painted {}",
        drawing_strokes(&output)
    );

    click_chart_with(&mut app, &ctx, width, egui::Modifiers::NONE);
    let channel = app
        .active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("the third click placed the channel")
        .clone();
    assert_eq!(channel.points.len(), 3);
    assert!(
        channel.points[2].price < trend_price_at(&channel.points, channel.points[2].bar),
        "the corridor opened downward, the way the pointer moved: {:?}",
        channel.points
    );
}

/// The remembered position is one window's, shared by every tool on the
/// rail: parked while an **anchored VWAP** is selected, it is where the
/// **volume profile** opens, and the other way round.
///
/// The rule was proved on horizontal lines, and a line is the easy case.
/// These two paint far past their anchors — the VWAP is a series across
/// the whole view, the profile covers the price axis — so they are the
/// objects automatic placement has the most to say about, and therefore
/// the ones where a hand overriding it has the most to lose.
#[test]
fn a_parked_popup_greets_the_avwap_and_the_profile_too() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(200);
    run_frame(&mut app, &ctx);
    let avwap = place_drawing(&mut app, &ctx, "anchored-vwap", &[egui::pos2(500.0, 300.0)]);
    let profile = place_drawing(
        &mut app,
        &ctx,
        "fixed-range-profile",
        &[egui::pos2(700.0, 300.0), egui::pos2(820.0, 420.0)],
    );

    // Park it with the VWAP selected, through the title-bar gesture.
    select_and_open_popup(&mut app, &ctx, avwap);
    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("open on the VWAP");
    let grip = egui::pos2(popup.left() + 60.0, popup.top() + 14.0);
    drag_chart(&mut app, &ctx, grip, grip + egui::vec2(-220.0, 150.0));
    run_frame(&mut app, &ctx);
    assert!(
        app.surfaces.drawing_chrome.inspector_moved(),
        "the drag records the manual move"
    );
    let parked = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("still open")
        .min;

    assert_eq!(
        select_and_open_popup(&mut app, &ctx, profile),
        parked,
        "the volume profile's settings open where the hand left the window"
    );
    assert_eq!(
        select_and_open_popup(&mut app, &ctx, avwap),
        parked,
        "and so do the VWAP's, coming back to it"
    );
}

#[test]
fn a_selected_drawing_exposes_its_edit_and_delete_controls() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
    drag_chart(
        &mut app,
        &ctx,
        egui::pos2(620.0, 300.0),
        egui::pos2(800.0, 450.0),
    );
    assert_eq!(app.active_tab().flow_pane.drawings.items().len(), 1);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));

    open_inspector(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().flow_pane.drawings.selected(), Some(0));
    let texts = painted_text(&output);
    for label in [
        "Rectangle settings",
        "Style",
        "line width (px)",
        "fill opacity",
        "Delete drawing",
    ] {
        assert!(
            texts.iter().any(|text| text.contains(label)),
            "selected drawing inspector omitted {label:?}; painted text: {texts:?}"
        );
    }
}

/// The scripted pan (`QUANTICK_PAN_PX`) is the gesture, not a teleport: it
/// re-applies every frame and settles exactly where the projection margin
/// holds it, which is how a screenshot reaches that state at all.
#[test]
fn the_scripted_pan_settles_on_the_projection_margin() {
    let (mut app, _cmd_rx) = app_with_history(400);
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    let slots = app.active_tab().flow_pane.slots();
    let newest = (slots - 1) as f32;

    app.scripted_pan_px = Some(-9_000.0);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let settled = app.active_tab().flow_pane.viewport.right_edge_bar(slots);
    assert!(!app.active_tab().flow_pane.viewport.follows_live());
    assert!(
        settled > newest + 1.0,
        "the chart is out in the empty canvas: {settled}"
    );

    // And it stays there. The margin is a wall, not a slope — a hook that
    // kept sliding would screenshot a different chart every frame.
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let again = app.active_tab().flow_pane.viewport.right_edge_bar(slots);
    assert!((again - settled).abs() < 0.001, "{again} vs {settled}");
}

#[test]
fn capture_starts_with_the_feed_and_commits_only_after_the_command_is_queued() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();

    // Construction already asked the feed to record: capture follows the
    // market, not the toolbar.
    assert_eq!(take_capture_start(&mut cmd_rx), BOOK_GENERATION_STRIDE);
    assert!(app.active_tab().tape().enabled());
    with_config(&mut app, |tab, config| tab.ensure_book_capture(config));
    assert!(
        cmd_rx.try_recv().is_err(),
        "a recorder already running needs no second command"
    );

    drop(cmd_rx);
    with_config(&mut app, |tab, config| {
        tab.request_book_capture(config, false)
    });
    assert!(
        app.active_tab().tape().enabled(),
        "closed command channel must preserve current capture state"
    );
}

#[test]
fn bubble_toggle_needs_no_feed_command_and_leaves_capture_alone() {
    let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
    take_capture_start(&mut cmd_rx);
    // The shipped config opens the layer, so the round trip is off and
    // back on — both directions have to leave the feed alone, not just
    // whichever one happens to be the opening state.
    assert!(app.active_tab().tape().bubbles_enabled());

    app.active_tab_mut().tape_mut().set_bubbles_enabled(false);
    assert!(!app.active_tab().tape().bubbles_enabled());
    assert!(
        cmd_rx.try_recv().is_err(),
        "hiding the bubbles is a display choice; no feed command is needed"
    );

    app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
    assert!(app.active_tab().tape().bubbles_enabled());
    assert!(
        cmd_rx.try_recv().is_err(),
        "aggregate trades already flow; no feed command is needed"
    );

    app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
    assert!(
        app.active_tab().tape().bubbles_enabled(),
        "hiding the book must not stop the bubbles"
    );
}

/// What the window is showing is what the file records — the arrangement
/// is read off the live state at save time, so nothing can be arranged
/// through a path that forgot to mark it.
#[test]
fn the_saved_workspace_describes_the_window_that_saved_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.ui_state_path = scratch_ui_state("capture");
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.active_tab_mut().split_fraction = 0.35;
    app.active_tab_mut().focus = PaneSide::Flow;
    app.tz = TzOffset::new(-180);
    app.dock.open_tab(DockTab::Trading);
    app.toolrail.set_dock(ToolboxDock::Bottom);
    app.show_perf = false;

    let workspace = app.capture_workspace();

    assert_eq!(workspace.tabs.len(), 1);
    let tab = &workspace.tabs[0];
    assert_eq!(tab.layout, crate::config::DeclaredLayout::TimeAndFlow);
    assert_eq!(tab.split_fraction, Some(0.35));
    assert_eq!(tab.focus, Some(ui_state::SavedFocus::Flow));
    assert_eq!(
        tab.flow_bars,
        app.active_tab().flow_pane.state.spec().to_config_string(),
        "the recorded rule is the one the pane is actually on"
    );
    assert!(
        tab.time_bars.is_some(),
        "a tab showing the split records the interval its time pane is on"
    );
    let chrome = workspace.chrome.expect("the chrome is part of a workspace");
    assert_eq!(chrome.timezone_minutes, -180);
    assert_eq!(chrome.dock_tab, Some(ui_state::SavedDockTab::Trading));
    assert_eq!(chrome.rail_dock, ui_state::SavedRailDock::Bottom);
    assert!(!chrome.perf_readings);
}

/// A fine-tick market at a high price: BTCUSDT quotes in cents and trades
/// near $80k, so its own tick is a true fact about the instrument and a
/// useless row — a few hundred dollars of range asks for tens of thousands
/// of them, and the profile paints as a jagged wash. The chart has no L2
/// here (`book_capture: false`), which is exactly why this used to fail:
/// the sizing rule only ever saw a price when a depth snapshot handed it
/// one.
#[test]
fn a_bitcoin_magnitude_tape_groups_the_profile_by_the_dollar() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = tape_app(&ctx, Decimal::from(80_000), Decimal::new(1, 2));
    place_range_profile_with_the_layer_off(&mut app);
    // The bucket these tests name an exact number for is decided on the
    // book worker's own thread. Without this barrier they race it and read
    // the fine default on a loaded runner.
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.active_tab()
            .pane(PaneSide::Flow)
            .state
            .footprint_group(),
        Decimal::from(1),
        "an $80k market stayed on the cent-wide rows its tick names",
    );
    assert_eq!(
        folded_profile_group(&app),
        Decimal::from(1),
        "the profile folded at a different width than the ladders under it",
    );
}

/// (g) MetaTrader narrows into serving candles after the bridge says
/// hello, so a pane that asked early was told there was nothing held. The
/// rising edge asks again, and the empty answer strands no spinner.
#[test]
fn a_capability_that_rises_later_is_asked_again() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
        book_capture: false,
        history_paging: true,
        traded_volume: true,
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
    let trades: Vec<_> = (0..50).map(minute_trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    run_frame(&mut app, &ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);

    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        0,
        "a feed that says it serves no candles is not asked"
    );

    // The bridge says hello and the session turns out to hold rates.
    caps_tx.send_modify(|caps| caps.ohlcv_history = true);
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        1,
        "the rising edge asks once"
    );

    // ...and the venue holds nothing after all.
    evt_tx
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: Vec::new(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "an empty reply is a complete answer, and ends the wait"
    );
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        0,
        "with no prefix rather than a fabricated one"
    );
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        0,
        "and the tab stops asking"
    );
}

/// A push feed re-answers: an empty block, then a real one on the next
/// reconnect. The capability flag rose once and stays, so only the
/// generation can say the answer changed.
#[test]
fn a_new_candle_generation_is_asked_for_again_and_installed() {
    let ctx = egui::Context::default();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
        book_capture: false,
        history_paging: true,
        traded_volume: true,
        ohlcv_history: true,
        ohlcv_generation: 1,
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
    let trades: Vec<_> = (0..50).map(minute_trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    run_frame(&mut app, &ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        1,
        "generation 1 is asked"
    );

    // A cold terminal: the block it had was empty.
    evt_tx
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: Vec::new(),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 0);
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        0,
        "an answered request is not repeated on its own"
    );

    // The reconnect stores a real block, and says so by moving the count.
    caps_tx.send_modify(|caps| caps.ohlcv_generation = 2);
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_requests(&mut cmd_rx),
        1,
        "a new generation is a new answer, and is asked for"
    );

    evt_tx
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(30),
            slice: crate::feed::OhlcvSlice::Last { complete: false },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        30,
        "and the block it carried is installed through the usual path"
    );
    assert!(
        !app.active_tab()
            .loading
            .is_active(LoadingTask::VenueHistory),
        "a short answer is still an answer, and ends the wait"
    );
    let texts = painted_text(&run_frame(&mut app, &ctx));
    assert!(has_price_axis(&texts), "the pane draws it: {texts:?}");
}

/// A block already held is replaced too: a reconnect can carry a longer or
/// corrected one, and holding the first would pin the chart to it.
#[test]
fn a_new_generation_replaces_a_block_already_held() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(30),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    drain_ohlcv_requests(&mut commands);
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 30);

    // The feed's capabilities are fixed in this fixture, so move the tab's
    // own record of what it has acted on — the same thing a bumped
    // generation does when it arrives.
    app.active_tab_mut().forget_ohlcv_generation_for_test();
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_requests(&mut commands),
        1,
        "the tab asks again rather than keeping the block it has"
    );

    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: crate::feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(90),
            slice: crate::feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        90,
        "and the longer block replaces the shorter one"
    );
}

#[test]
fn gateway_client_reads_the_running_application_and_wrong_tokens_fail_closed() {
    use quantick_control::{
        error::codes,
        handshake::{BearerToken, CURRENT_PROTOCOL_VERSION, ProtocolVersionRange},
        id::{InstanceId, ProcessNonce},
    };

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    let directory = gateway_test_directory("read");
    let _descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 8);
    let discovery =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options()).unwrap();
    assert!(discovery.issues.is_empty());
    let mut client = discovery.select(None).unwrap();
    assert!(
        client
            .effective_scopes()
            .contains(&quantick_control::id::PermissionId::new("observe.chart").unwrap())
    );

    let descriptor = client.descriptor().clone();
    let mut wrong_token = descriptor.clone();
    wrong_token.bearer_token = BearerToken::from_bytes([0xEE; 32]);
    let error =
        quantick_control_local::client::LocalClient::connect(wrong_token, &gateway_test_options())
            .unwrap_err();
    assert_eq!(error.code.as_str(), codes::AUTH_FAILED);

    let mut wrong_instance = descriptor.clone();
    wrong_instance.instance_id = InstanceId::from_bytes([0xDD; 16]);
    let error = quantick_control_local::client::LocalClient::connect(
        wrong_instance,
        &gateway_test_options(),
    )
    .unwrap_err();
    assert_eq!(error.code.as_str(), codes::AUTH_FAILED);

    let mut wrong_nonce = descriptor.clone();
    wrong_nonce.process_nonce = ProcessNonce::from_bytes([0xCC; 16]);
    let error =
        quantick_control_local::client::LocalClient::connect(wrong_nonce, &gateway_test_options())
            .unwrap_err();
    assert_eq!(error.code.as_str(), codes::AUTH_FAILED);

    let mut non_overlapping_version = descriptor;
    non_overlapping_version.protocol_versions =
        ProtocolVersionRange::new(CURRENT_PROTOCOL_VERSION + 1, CURRENT_PROTOCOL_VERSION + 1)
            .unwrap();
    let error = quantick_control_local::client::LocalClient::connect(
        non_overlapping_version,
        &gateway_test_options(),
    )
    .unwrap_err();
    assert_eq!(error.code.as_str(), codes::VERSION_UNSUPPORTED);

    let request_id = client
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({
                "scopes": ["system.info", "workspace.summary", "chart.summary"]
            }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test(),
        0,
        "the application frame must drain the queued gateway request"
    );
    let response = client.read().unwrap();
    assert_eq!(response.request_id, request_id);
    let first_revisions = response.module_revisions.clone();
    let result = match response.outcome {
        quantick_control::wire::ResponseOutcome::Success { result } => result,
        quantick_control::wire::ResponseOutcome::Failure { error } => {
            panic!("running-app read failed: {error:?}")
        }
    };
    assert_eq!(
        result["scopes"]["chart.summary"]["value"]["panes"][0]["closed_bar_count"],
        "12"
    );
    assert_eq!(
        result["scopes"]["workspace.summary"]["value"]["tabs"][0]["symbol"],
        "TESTUSDT"
    );

    // An observer read changes nothing: the same scopes read again report
    // the same module revisions (threat model O-08).
    let again = client
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({
                "scopes": ["system.info", "workspace.summary", "chart.summary"]
            }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    run_frame(&mut app, &ctx);
    let repeated = client.read().unwrap();
    assert_eq!(repeated.request_id, again);
    assert_eq!(
        repeated.module_revisions, first_revisions,
        "observer reads leave every module revision where it was"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_full_queue_returns_backpressure_without_draining_on_the_socket_thread() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("backpressure");
    enable_test_gateway(&mut app, &ctx, &directory, 1);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let first = client
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    let second = client
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    let rejected = client.read().unwrap();
    assert_eq!(rejected.request_id, second);
    assert_eq!(response_error(&rejected).code.as_str(), codes::BACKPRESSURE);
    assert!(response_error(&rejected).retryable);

    run_frame(&mut app, &ctx);
    let completed = client.read().unwrap();
    assert_eq!(completed.request_id, first);
    assert!(matches!(
        completed.outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_request_timeout_is_structured_and_late_ui_work_is_discarded() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("timeout");
    enable_test_gateway_with_limits(
        &mut app,
        &ctx,
        &directory,
        4,
        std::time::Duration::from_millis(50),
        quantick_control::limits::CONTROL_MAX_CONNECTIONS,
    );
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    client
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    let response = client.read().unwrap();
    assert_eq!(response_error(&response).code.as_str(), codes::TIMEOUT);
    assert!(response_error(&response).retryable);

    run_frame(&mut app, &ctx);
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test(),
        0
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_connection_and_rate_limits_return_stable_backpressure() {
    use quantick_control::{error::codes, limits::CONTROL_CLIENT_BURST};

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("limits");
    enable_test_gateway_with_limits(
        &mut app,
        &ctx,
        &directory,
        4,
        std::time::Duration::from_millis(quantick_control::limits::CONTROL_REQUEST_TIMEOUT_MS),
        1,
    );
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let descriptor = client.descriptor().clone();
    let connection_error =
        quantick_control_local::client::LocalClient::connect(descriptor, &gateway_test_options())
            .unwrap_err();
    assert_eq!(connection_error.code.as_str(), codes::BACKPRESSURE);

    let request_count = usize::try_from(CONTROL_CLIENT_BURST).unwrap() * 2;
    for _ in 0..request_count {
        client.send("unknown.read", serde_json::json!({})).unwrap();
    }
    let mut rate_limited = 0usize;
    for _ in 0..request_count {
        let response = client.read().unwrap();
        if let quantick_control::wire::ResponseOutcome::Failure { error } = response.outcome
            && error.code.as_str() == codes::BACKPRESSURE
            && error.message.contains("rate limit")
        {
            rate_limited += 1;
        }
    }
    assert!(rate_limited > 0);

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_shutdown_unblocks_a_half_open_handshake() {
    use std::io::Read as _;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("half-open");
    let descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 4);
    let descriptor: quantick_control::descriptor::InstanceDescriptor =
        serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();
    let mut socket =
        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, descriptor.port)).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    disable_test_gateway(&mut app, &ctx);
    assert!(!descriptor_path.exists());
    let mut byte = [0u8; 1];
    match socket.read(&mut byte) {
        Ok(0) => {}
        Err(error)
            if !matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        result => panic!("half-open client was not closed during shutdown: {result:?}"),
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_ui_budget_defers_work_beyond_one_frame() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("frame-budget");
    enable_test_gateway(&mut app, &ctx, &directory, 16);
    let mut first =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut second =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    for _ in 0..4 {
        first
            .send(
                crate::control::SNAPSHOT_CAPABILITY_ID,
                serde_json::json!({ "scopes": ["system.info"] }),
            )
            .unwrap();
        second
            .send(
                crate::control::SNAPSHOT_CAPABILITY_ID,
                serde_json::json!({ "scopes": ["system.info"] }),
            )
            .unwrap();
    }
    wait_for_queued_gateway_requests(&app, 8);

    run_frame(&mut app, &ctx);
    let remaining = app
        .control_access
        .as_ref()
        .expect("control access is installed")
        .queued_requests_for_test();
    assert!((4..=8).contains(&remaining));
    for _ in 0..10 {
        if app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test()
            == 0
        {
            break;
        }
        run_frame(&mut app, &ctx);
    }
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test(),
        0
    );
    for client in [&mut first, &mut second] {
        for _ in 0..4 {
            assert!(matches!(
                client.read().unwrap().outcome,
                quantick_control::wire::ResponseOutcome::Success { .. }
            ));
        }
    }

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_shutdown_removes_discovery_and_stale_descriptors_are_stable_errors() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("shutdown");
    let descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 4);
    let discovery =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options()).unwrap();
    let stale_descriptor = discovery.clients[0].descriptor().clone();
    let mut client = discovery.select(None).unwrap();

    disable_test_gateway(&mut app, &ctx);
    assert!(!descriptor_path.exists());
    let stale = quantick_control_local::client::LocalClient::connect(
        stale_descriptor,
        &gateway_test_options(),
    )
    .unwrap_err();
    assert_eq!(stale.code.as_str(), codes::INSTANCE_GONE);
    let closed = client
        .invoke(
            crate::control::DESCRIBE_CAPABILITY_ID,
            serde_json::json!({}),
        )
        .unwrap_err();
    assert_eq!(closed.code.as_str(), codes::INSTANCE_GONE);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_discovery_requires_explicit_selection_for_multiple_live_instances() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut first_app, _first_commands) = app_with_history(1);
    let (mut second_app, _second_commands) = app_with_history(1);
    let directory = gateway_test_directory("multiple");
    enable_test_gateway(&mut first_app, &ctx, &directory, 4);
    enable_test_gateway(&mut second_app, &ctx, &directory, 4);

    let discovery =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options()).unwrap();
    assert_eq!(discovery.clients.len(), 2);
    assert!(discovery.issues.is_empty());
    let first_id = discovery.clients[0].descriptor().instance_id.clone();
    let ambiguous = discovery.select(None).unwrap_err();
    assert_eq!(ambiguous.code.as_str(), codes::INSTANCE_AMBIGUOUS);
    assert_eq!(
        ambiguous.context.details.as_ref().unwrap()["instance_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let selected = quantick_control_local::client::discover_in(&directory, &gateway_test_options())
        .unwrap()
        .select(Some(&first_id))
        .unwrap();
    assert_eq!(selected.descriptor().instance_id, first_id);

    disable_test_gateway(&mut first_app, &ctx);
    disable_test_gateway(&mut second_app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn observer_modules_project_headless_state_that_matches_their_schemas() {
    let (app, _commands) = app_with_history(12);
    let mut registry = crate::control::standard_registry().unwrap();
    let descriptors = registry
        .descriptors()
        .map(|descriptor| (descriptor.scope_id.clone(), descriptor.schema.clone()))
        .collect::<Vec<_>>();
    assert_eq!(descriptors.len(), 17, "every registered scope is projected");
    let scopes = descriptors
        .iter()
        .map(|(scope_id, _)| scope_id.clone())
        .collect::<Vec<_>>();

    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    assert!(capture.omitted_scopes.is_empty());
    for (scope_id, schema) in descriptors {
        let value = &capture
            .scopes
            .get(&scope_id)
            .unwrap_or_else(|| panic!("{scope_id} was projected"))
            .value;
        quantick_control::schema::validate_instance(&schema, value)
            .unwrap_or_else(|error| panic!("{scope_id} instance is invalid: {error}"));
    }

    let chart = &capture.scopes[&observer_scope("chart.summary")].value;
    assert_eq!(chart["panes"][0]["closed_bar_count"], "12");
    assert_eq!(chart["panes"][0]["symbol"], "TESTUSDT");
    assert_eq!(chart["panes"][0]["viewport"]["geometry_available"], false);
    assert_eq!(chart["panes"][0]["viewport"]["visible_start_slot"], "0");
    assert_eq!(
        chart["panes"][0]["viewport"]["visible_end_slot_exclusive"],
        "0"
    );
    let feed = &capture.scopes[&observer_scope("feed.status")].value;
    assert_eq!(feed["tabs"][0]["history_trade_count"], "12");
    assert_eq!(
        feed["tabs"][0]["provenance"]["price"],
        "venue_or_broker_trade"
    );
}

/// Criterion 2 of roadmap 5.1, for the nine new scopes: a split chart
/// captures both panes, keeps them apart, and keeps the focus and the
/// provenance the trader can see on screen.
#[test]
fn observer_new_scopes_preserve_two_pane_focus_and_provenance() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 80);
    let time_point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, time_point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));

    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("analysis.indicators"),
        observer_scope("analysis.drawings"),
        observer_scope("orderflow.footprint"),
        observer_scope("session.paper"),
    ];
    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();

    // The three pane-addressed scopes see both panes, and tell them apart
    // by id and by side rather than by position.
    let flow_id = app.active_tab().pane(PaneSide::Flow).id.to_string();
    let time_id = app.active_tab().pane(PaneSide::Time(0)).id.to_string();
    for scope in [&scopes[0], &scopes[1], &scopes[2]] {
        let panes = capture.scopes[scope].value["tabs"][0]["panes"]
            .as_array()
            .unwrap_or_else(|| panic!("{scope} publishes a pane list"));
        assert_eq!(panes.len(), 2, "{scope} captures both panes of a split");
        let sides = panes
            .iter()
            .map(|pane| pane["side"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(
            sides.contains(&"flow") && sides.contains(&"time"),
            "{scope} names each pane's side; got {sides:?}"
        );
        let ids = panes
            .iter()
            .map(|pane| pane["pane_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert!(
            ids.contains(&flow_id) && ids.contains(&time_id),
            "{scope} addresses panes by the ids the rest of the capture uses"
        );
    }

    // The focus itself stays where the interaction scope reports it: the
    // new scopes address panes, they do not re-answer which one is focused,
    // so the two can never disagree.
    let focus = registry
        .capture(
            &app,
            &observer_instance(),
            std::slice::from_ref(&observer_scope("interaction.selection")),
        )
        .unwrap()
        .into_serialized()
        .unwrap();
    let selection = &focus.scopes[&observer_scope("interaction.selection")].value;
    assert_eq!(selection["focused_pane_side"], "time");
    assert_eq!(selection["focused_pane_id"], time_id);

    // Provenance survives the split: the paper ledger names its source once
    // per tab, not once per pane, because that is where it lives.
    let paper = &capture.scopes[&scopes[3]].value["tabs"][0];
    assert_eq!(paper["provenance"], "paper_trading_session_ledger");
    assert_eq!(paper["symbol"], "TESTUSDT");
}

#[test]
fn observer_journals_indicator_and_drawing_changes_without_the_trader_text() {
    const CANARY: &str = "CANARY_JOURNAL_secret note";

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(120);
    let directory = gateway_test_directory("analysis-journal");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    // One quiet frame establishes the baseline: the journal starts when the
    // door opens and records changes, not the state it found.
    run_frame(&mut app, &ctx);

    let read = |app: &QuantickApp| {
        app.control_access
            .as_ref()
            .unwrap()
            .journal()
            .read(1, 256, 1 << 20)
            .events
            .iter()
            .map(|event| event.kind.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    let baseline = read(&app);
    assert!(
        !baseline.iter().any(|kind| kind.starts_with("analysis.")),
        "a quiet frame journals nothing about analysis; got {baseline:?}"
    );

    // Attach an indicator the way the worker delivers one.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let slot = pane.indicators.allocate_slot("native.cvd");
        let descriptor = quantick_indicators::IndicatorDescriptor {
            title: "CVD".to_owned(),
            short_title: None,
            overlay: false,
            plots: Vec::new(),
            fills: Vec::new(),
            inputs: Vec::new(),
        };
        pane.indicators
            .apply(crate::indicator_worker::IndicatorEvent::rebuilt(
                slot,
                descriptor,
                Vec::new(),
            ));
    }
    run_frame(&mut app, &ctx);
    assert!(
        read(&app).contains(&"analysis.indicator.attached".to_owned()),
        "attaching an indicator reaches the journal"
    );

    // Place a drawing, name it with the canary, then lock it.
    let (time, price) = {
        let pane = &app.active_tab().flow_pane;
        let bar = pane.closed_bar(60).expect("fixture bar");
        (
            pane.slot_open_time(60).expect("fixture market time"),
            rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap(),
        )
    };
    {
        let flow = &mut app.active_tab_mut().flow_pane;
        assert!(flow.drawings.place_with(
            drawing_tool("horizontal-line"),
            &drawings::DrawingBand::Price,
            ChartPoint::at_time(60.5, price, Some(time)),
            |tool| drawings::NewDrawing {
                style: drawings::DrawingStyle::default(),
                payload: tool.default_payload(),
            },
        ));
        let selected = flow.drawings.selected().expect("placement selects");
        flow.drawings.rename_at(selected, CANARY);
    }
    run_frame(&mut app, &ctx);
    assert!(
        read(&app).contains(&"analysis.drawing.created".to_owned()),
        "placing a drawing reaches the journal"
    );

    {
        let flow = &mut app.active_tab_mut().flow_pane;
        let selected = flow.drawings.selected().expect("still selected");
        flow.drawings
            .items_mut()
            .get_mut(selected)
            .expect("the drawing is there")
            .locked = true;
    }
    run_frame(&mut app, &ctx);
    assert!(
        read(&app).contains(&"analysis.drawing.edited".to_owned()),
        "locking a drawing reaches the journal"
    );

    // The whole journal is held to the wire's rule: presence, never text.
    let encoded = serde_json::to_string(
        &app.control_access
            .as_ref()
            .unwrap()
            .journal()
            .read(1, 256, 1 << 20)
            .events,
    )
    .unwrap();
    assert!(
        !encoded.contains(CANARY),
        "the journal redacts the trader's own drawing name"
    );
    assert!(
        encoded.contains("\"user_label_present\":true"),
        "it reports that a name exists without saying what it is"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn observer_projects_order_flow_layers_and_states_the_absent_engine() {
    let (app, _commands) = app_with_history(8);
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("orderflow.tape"),
        observer_scope("orderflow.footprint"),
        observer_scope("orderflow.bubbles"),
        observer_scope("orderflow.heatmap"),
        observer_scope("orderflow.l2"),
    ];
    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();

    // The footprint scope answers for every pane, engine or not: the layer
    // and its setup are chart state, not book state.
    let footprint = &capture.scopes[&scopes[1]].value["tabs"][0]["panes"][0];
    // Compared against the pane rather than a literal: which layers a
    // fresh chart opens with is a product decision that lives in
    // `config/chart-layers.toml`, and a test that pins it here fails the
    // day someone edits the file. What this asserts is that the scope
    // reports what the pane actually holds.
    assert_eq!(
        footprint["visible"],
        app.active_tab().flow_pane.footprint_visible,
        "the scope reports the pane's own layer state"
    );
    assert_eq!(
        footprint["overridden"], false,
        "a fresh pane follows the window's setup"
    );
    assert!(
        footprint["setup"]["imbalance_ratio"].is_string(),
        "the ratio crosses as an exact decimal string"
    );
    // The style name is the control plane's own vocabulary, spelled the
    // way the schema declares it. A `{:?}` rendering of the enum would say
    // "split" here too and "bidask" for the one variant whose name has two
    // words, which is the spelling no client was ever told about.
    assert_eq!(
        footprint["setup"]["style"], "split",
        "the default style crosses under its wire name"
    );
    let mut styled = app;
    for style in [
        crate::footprint_config::FootprintStyle::Split,
        crate::footprint_config::FootprintStyle::Ladder,
        crate::footprint_config::FootprintStyle::BidAsk,
        crate::footprint_config::FootprintStyle::Cluster,
        crate::footprint_config::FootprintStyle::Auto,
    ] {
        styled.footprint_config.style = style;
        let capture = registry
            .capture(
                &styled,
                &observer_instance(),
                std::slice::from_ref(&scopes[1]),
            )
            .unwrap()
            .into_serialized()
            .unwrap();
        let name = capture.scopes[&scopes[1]].value["tabs"][0]["panes"][0]["setup"]["style"]
            .as_str()
            .expect("every style has a wire name")
            .to_owned();
        assert!(
            ["split", "ladder", "bid_ask", "cluster", "auto"].contains(&name.as_str()),
            "{style:?} crossed as `{name}`, which the schema does not declare"
        );
    }

    // The tape's age is measured against the capture's own clock. Handing
    // the newest print in as "now" compares it with itself and answers
    // zero for every tape, on every capture, forever.
    let (mut aged, _aged_commands) = app_with_history(8);
    let a_minute_ago = metrics::wall_clock_ms() - 60_000;
    aged.active_tab_mut().latest_trade_ms = Some(a_minute_ago);
    let late = registry
        .capture(
            &aged,
            &observer_instance(),
            std::slice::from_ref(&scopes[0]),
        )
        .unwrap()
        .into_serialized()
        .unwrap();
    let age = late.scopes[&scopes[0]].value["tabs"][0]["panes"][0]["tape"]["age_ms"]
        .as_i64()
        .expect("a tab with a print behind it has an age");
    assert!(
        (55_000..70_000).contains(&age),
        "the tape is a minute behind its market and reports {age} ms"
    );

    // The four engine-backed scopes state the absence rather than
    // reporting an empty book, which would read as a silent venue.
    for scope in [&scopes[0], &scopes[2], &scopes[3], &scopes[4]] {
        let pane = &capture.scopes[scope].value["tabs"][0]["panes"][0];
        let available = pane["engine"]["available"].as_bool().expect("declared");
        if available {
            continue;
        }
        assert_eq!(
            pane["engine"]["reason"], "order_flow_engine_not_attached_to_this_pane",
            "{scope} names why it cannot answer"
        );
        for payload in ["tape", "bubbles", "heatmap", "book"] {
            if let Some(value) = pane.get(payload) {
                assert!(
                    value.is_null(),
                    "{scope} publishes no {payload} without an engine"
                );
            }
        }
    }
}

#[test]
fn observer_projects_each_pane_indicator_with_its_inputs_and_latest_reading() {
    let (mut app, _commands) = app_with_history(12);

    // Deliver an indicator the way the worker does: allocate the slot the
    // UI owns, then apply the rebuild delta the worker sends back.
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        let slot = pane.indicators.allocate_slot("native.ema");
        let descriptor = quantick_indicators::IndicatorDescriptor {
            title: "EMA".to_owned(),
            short_title: Some("ema".to_owned()),
            overlay: true,
            plots: vec![quantick_indicators::PlotSpec {
                id: quantick_indicators::PlotId::new(0),
                title: "EMA".to_owned(),
                style: quantick_indicators::PlotStyle::Line,
                base_color: quantick_indicators::Rgba8::opaque(1, 2, 3),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            fills: Vec::new(),
            inputs: vec![quantick_indicators::InputSpec::Int {
                name: "len".to_owned(),
                title: "Length".to_owned(),
                default: 9,
                min: Some(1),
                max: Some(500),
                step: Some(1),
                options: Vec::new(),
            }],
        };
        pane.indicators
            .apply(crate::indicator_worker::IndicatorEvent::rebuilt(
                slot,
                descriptor,
                vec![vec![1.5, 2.25, 3.125]],
            ));
    }

    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("analysis.indicators");
    let capture = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let pane = &capture.scopes[&scope].value["tabs"][0]["panes"][0];
    assert_eq!(pane["indicator_count"], "1");
    assert_eq!(pane["indicators_truncated"], false);

    let indicator = &pane["indicators"][0];
    assert_eq!(indicator["kind"], "native.ema");
    assert_eq!(
        indicator["source_kind"], "native",
        "a native kernel's diagnostics are ours, not the trader's"
    );
    assert_eq!(indicator["title"], "EMA");
    assert_eq!(indicator["short_title"], "ema");
    assert_eq!(indicator["overlay"], true);
    assert_eq!(indicator["committed_bar_count"], "3");
    assert!(
        indicator["failure"].is_null(),
        "a clean evaluation reports no failure"
    );

    let plot = &indicator["plots"][0];
    assert_eq!(plot["style"], "line");
    assert_eq!(plot["base_color"], "#010203ff");
    assert_eq!(
        plot["latest_value"], "3.125",
        "the newest committed reading crosses as an exact decimal string"
    );

    let input = &indicator["inputs"][0];
    assert_eq!(input["name"], "len");
    assert_eq!(input["title"], "Length");
    assert_eq!(input["kind"], "int");
    assert_eq!(input["default"], "9");
    assert_eq!(
        input["text_present"], false,
        "an int input holds no free text to withhold"
    );
}

#[test]
fn observer_projects_the_replay_playhead_and_its_trace_sidecar() {
    let dir = std::env::temp_dir().join(format!("quantick-observer-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut app, _commands) = app_with_history(4);

    // A live tab is not replaying, and says so rather than reporting a
    // playhead parked at zero.
    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("session.replay");
    let live = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(live.scopes[&scope].value["tabs"][0]["replaying"], false);
    assert!(live.scopes[&scope].value["tabs"][0]["session"].is_null());

    // The same tab, now playing a recording written to disk.
    app.active_tab_mut().replay = Some(feed::ReplayLink::for_test(recording_at(&dir)));
    let playing = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let session = &playing.scopes[&scope].value["tabs"][0]["session"];
    assert_eq!(playing.scopes[&scope].value["tabs"][0]["replaying"], true);
    assert_eq!(session["symbol"], "TESTUSDT");
    assert_eq!(session["date"], "2026-03-16");
    assert_eq!(
        session["file_name"], "20260316.csv",
        "the file name identifies the recording without leaking the folder"
    );
    assert_eq!(session["total_trades"], "1");
    assert_eq!(
        session["trace"]["state"]["available"], false,
        "a capture does no file I/O, so it answers nothing about the trace"
    );
    assert_eq!(
        session["trace"]["state"]["reason"],
        "trace_state_is_served_by_the_gateway_not_by_a_capture"
    );
    assert_eq!(
        session["trace"]["file_name"], "20260316.csv.control-trace.jsonl",
        "it still names the file the gateway should be asked about"
    );

    // A mark writes the sidecar; the next capture sees it present.
    let ctx = egui::Context::default();
    hover_bar(&mut app, &ctx, 2);
    app.take_mark(Some("traced".to_owned()));
    let traced = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    // The sidecar now exists on disk, and the capture still declines to
    // look: the answer costs a `stat` this thread must not spend.
    assert!(
        crate::control::replay_trace_path_for(&recording_at(&dir).path).is_file(),
        "the mark wrote the sidecar"
    );
    assert_eq!(
        traced.scopes[&scope].value["tabs"][0]["session"]["trace"]["state"]["available"],
        false
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn observer_projects_the_paper_ledger_with_its_provenance() {
    let (mut app, _commands) = app_with_history(4);
    let print = |agg_id: u64, price: i64| quantick_engine::Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small ids") * 1000,
        price: rust_decimal::Decimal::from(price),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    };

    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("session.paper");
    let flat = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let before = &flat.scopes[&scope].value["tabs"][0];
    assert_eq!(before["flat"], true);
    assert!(before["position"].is_null());
    assert_eq!(before["provenance"], "paper_trading_session_ledger");

    // Open a position through the simulator's own path.
    {
        let paper = &mut app.active_tab_mut().paper;
        paper.seed(&print(0, 100));
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(1, 100));
    }
    let open = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let after = &open.scopes[&scope].value["tabs"][0];
    assert_eq!(after["flat"], false);
    assert_eq!(after["position"]["side"], "buy");
    assert_eq!(
        after["position"]["quantity"], "1",
        "quantity crosses as an exact decimal string, never an f64"
    );
    assert_eq!(after["position"]["average_entry_price"], "100");
    assert_eq!(after["closed_trade_count"], "0");
    assert_eq!(after["working_orders_truncated"], false);
}

#[test]
fn observer_capture_revisions_are_coherent_and_monotonic() {
    let (mut app, _commands) = app_with_history(4);
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("feed.status"),
        observer_scope("chart.summary"),
    ];
    let instance = observer_instance();
    // Derived, not a literal: a capture names every scope it did not
    // project, so registering a new module must not send anyone editing
    // an arithmetic constant in a test about revisions.
    let registered = registry.descriptors().count();

    let first = registry
        .capture(&app, &instance, &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(first.omitted_scopes.len(), registered - scopes.len());
    let second = registry
        .capture(&app, &instance, &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(
        first.capture_revision.get() + 1,
        second.capture_revision.get()
    );
    assert_eq!(first.module_revisions, second.module_revisions);

    let next = trade(5);
    app.active_tab_mut()
        .ingest_live_trade_at(&next, next.timestamp_ms);
    let third = registry
        .capture(&app, &instance, &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    assert_eq!(
        second.capture_revision.get() + 1,
        third.capture_revision.get()
    );
    for before in &second.module_revisions {
        let after = third
            .module_revisions
            .iter()
            .find(|revision| revision.module_id == before.module_id)
            .expect("the same requested modules are present");
        assert!(
            after.revision > before.revision,
            "{} advances after one normal live ingest",
            before.module_id
        );
    }
}

/// Where a capture's time actually goes, scope by scope.
///
/// The whole-capture guard says whether the total fits; it cannot say what
/// to shrink when it does not. This times each registered scope on its own
/// over the same loaded workspace, so a budget decision is made against
/// measurements instead of a guess about which projection is expensive.
///
/// Reading only, so it is `#[ignore]`d: it asserts nothing, and the numbers
/// it prints are worth having on the record when a scope is added.
#[test]
#[ignore]
fn observer_per_scope_capture_cost() {
    const MEASURED: usize = 200;

    let (app, _commands) = loaded_observer_workspace(2_000);
    let mut registry = crate::control::standard_registry().unwrap();
    let scope_ids = registry
        .descriptors()
        .map(|descriptor| descriptor.scope_id.clone())
        .collect::<Vec<_>>();
    let instance = observer_instance();
    for scope in scope_ids {
        let one = [scope.clone()];
        for _ in 0..25 {
            drop(registry.capture(&app, &instance, &one).unwrap());
        }
        let mut elapsed = Vec::with_capacity(MEASURED);
        for _ in 0..MEASURED {
            drop(registry.capture(&app, &instance, &one).unwrap());
            elapsed.push(registry.performance().last_capture_us);
        }
        elapsed.sort_unstable();
        let median = elapsed[elapsed.len() / 2];
        let worst = *elapsed.last().unwrap();
        println!(
            "CONTROL_SCOPE_COST {{\"scope\":\"{scope}\",\"median_us\":{median},\"worst_us\":{worst}}}"
        );
    }
}

#[test]
fn observer_core_capture_stays_within_the_ui_budget() {
    // The always-on guard judges the median of the best batch: a typical
    // coherent capture of every scope must fit the budget, and that
    // reading survives a loaded test runner. The tail is measured by the
    // ignored sibling below, on a quiet machine, and recorded in
    // `docs/control-plane/pr2-performance.md`.
    let (median_us, p99_us, worst_us) = measure_core_capture_us();
    assert!(
        median_us <= quantick_control::limits::CONTROL_UI_BUDGET_US,
        "core capture median {median_us} us (p99 {p99_us} us, worst {worst_us} us) exceeds the {} us UI budget",
        quantick_control::limits::CONTROL_UI_BUDGET_US
    );
}

/// The strict tail reading: `cargo test -p quantick-app
/// observer_core_capture_p99 -- --ignored --nocapture` on a quiet machine.
/// Ignored in the ordinary suite because a p99 measured beside a thousand
/// other tests reports the runner's load, not the capture's cost.
#[test]
#[ignore]
fn observer_core_capture_p99_stays_within_the_ui_budget() {
    let (median_us, p99_us, worst_us) = measure_core_capture_us();
    assert!(
        p99_us <= quantick_control::limits::CONTROL_UI_BUDGET_US,
        "core capture p99 {p99_us} us (median {median_us} us, worst {worst_us} us) exceeds the {} us UI budget",
        quantick_control::limits::CONTROL_UI_BUDGET_US
    );
}

#[test]
fn gateway_refuses_a_request_before_the_handshake_and_closes() {
    use std::io::{Read as _, Write as _};

    use quantick_control::{
        codec::{BoundedCodec, FrameRole},
        error::codes,
        id::{CapabilityId, RequestId},
        wire::RequestEnvelope,
    };

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("first-frame");
    let descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 4);
    let descriptor: quantick_control::descriptor::InstanceDescriptor =
        serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();
    // ADR 0001 §2: literal IPv4 loopback on an OS-assigned port, and the
    // descriptor says exactly that.
    assert_eq!(descriptor.host, "127.0.0.1");
    assert_ne!(descriptor.port, 0);

    let mut socket =
        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, descriptor.port)).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let request = RequestEnvelope {
        protocol_version: quantick_control::handshake::CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("before-handshake").unwrap(),
        instance_id: descriptor.instance_id.clone(),
        capability_id: CapabilityId::new(crate::control::DESCRIBE_CAPABILITY_ID).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload: serde_json::json!({}),
    };
    let frame = BoundedCodec::default()
        .encode(FrameRole::Request, &request)
        .unwrap();
    socket.write_all(&frame).unwrap();
    let reply = BoundedCodec::handshake()
        .read_handshake_reply(&mut socket)
        .unwrap();
    let error = reply.into_accepted().unwrap_err();
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    let mut byte = [0u8; 1];
    assert!(
        matches!(socket.read(&mut byte), Ok(0) | Err(_)),
        "the connection must close after a rejected first frame"
    );
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test(),
        0,
        "no capability ran"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_exit_shutdown_removes_discovery() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("exit");
    let descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 4);
    let descriptor: quantick_control::descriptor::InstanceDescriptor =
        serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();

    app.control_access
        .as_mut()
        .expect("control access is installed")
        .shutdown_for_exit();
    assert!(!descriptor_path.exists(), "exit removes discovery");
    assert!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .is_disabled_for_test()
    );
    let error =
        quantick_control_local::client::LocalClient::connect(descriptor, &gateway_test_options())
            .unwrap_err();
    assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_revoking_one_client_closes_it_and_keeps_serving_others() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("revoke");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut ids = Vec::new();
    for _ in 0..400 {
        run_frame(&mut app, &ctx);
        ids = app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .connection_ids_for_test();
        if !ids.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(ids.len(), 1, "the connected client is listed");

    app.control_access
        .as_mut()
        .expect("control access is installed")
        .revoke(ids[0].clone());
    run_frame(&mut app, &ctx);
    assert!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .connection_ids_for_test()
            .is_empty()
    );
    // The accept loop closes the socket off the UI thread; a revoked
    // client cannot complete another read.
    let mut revoked = false;
    for _ in 0..100 {
        if client
            .invoke(
                crate::control::DESCRIBE_CAPABILITY_ID,
                serde_json::json!({}),
            )
            .is_err()
        {
            revoked = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(revoked, "a revoked client loses its connection");

    let mut fresh =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    assert!(matches!(
        fresh
            .invoke(
                crate::control::DESCRIBE_CAPABILITY_ID,
                serde_json::json!({})
            )
            .unwrap()
            .outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_a_client_that_never_reads_does_not_stall_another() {
    use quantick_control::limits::CONTROL_MAX_IN_FLIGHT_PER_CONNECTION;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("stalled-reader");
    enable_test_gateway(&mut app, &ctx, &directory, 16);
    let mut stalled =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut live = quantick_control_local::client::discover_in(&directory, &gateway_test_options())
        .unwrap()
        .select(None)
        .unwrap();
    for _ in 0..CONTROL_MAX_IN_FLIGHT_PER_CONNECTION {
        stalled
            .send(
                crate::control::SNAPSHOT_CAPABILITY_ID,
                serde_json::json!({ "scopes": ["system.info"] }),
            )
            .unwrap();
    }
    // A worker-side read is answered without the frame loop and without
    // the stalled client's replies ever being read.
    assert!(matches!(
        live.invoke(
            crate::control::DESCRIBE_CAPABILITY_ID,
            serde_json::json!({})
        )
        .unwrap()
        .outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    // A UI-side read completes while the stalled client's replies sit
    // unread in its socket.
    let request_id = live
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    for iteration in 0..400 {
        run_frame(&mut app, &ctx);
        let queued = app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test();
        if iteration >= 10 && queued == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let response = live.read().unwrap();
    assert_eq!(response.request_id, request_id);
    assert!(matches!(
        response.outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_a_half_written_frame_does_not_hold_the_connection() {
    use std::io::{Read as _, Write as _};

    use quantick_control::{
        codec::{BoundedCodec, FrameRole},
        handshake::HandshakeRequest,
        id::ProfileId,
    };

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("half-frame");
    let descriptor_path = enable_test_gateway_with_limits(
        &mut app,
        &ctx,
        &directory,
        4,
        std::time::Duration::from_millis(50),
        quantick_control::limits::CONTROL_MAX_CONNECTIONS,
    );
    let descriptor: quantick_control::descriptor::InstanceDescriptor =
        serde_json::from_slice(&std::fs::read(&descriptor_path).unwrap()).unwrap();
    let mut socket =
        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, descriptor.port)).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let codec = BoundedCodec::handshake();
    let request = HandshakeRequest {
        protocol_versions: descriptor.protocol_versions,
        instance_id: descriptor.instance_id.clone(),
        client_name: "half writer".to_owned(),
        client_version: "0".to_owned(),
        bearer_token: descriptor.bearer_token.clone(),
        requested_profile: ProfileId::new("observer").unwrap(),
        requested_scopes: gateway_test_scopes(),
    };
    socket
        .write_all(&codec.encode(FrameRole::Request, &request).unwrap())
        .unwrap();
    codec
        .read_handshake_reply(&mut socket)
        .unwrap()
        .into_accepted()
        .unwrap();

    // A header promising 64 bytes, then silence.
    socket.write_all(&64u32.to_be_bytes()).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_millis(100)))
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let mut closed = false;
    let mut byte = [0u8; 1];
    while std::time::Instant::now() < deadline {
        match socket.read(&mut byte) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => {
                closed = true;
                break;
            }
        }
    }
    assert!(
        closed,
        "a frame that never completes must not hold the connection open"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_rejects_a_duplicate_request_id_while_the_first_is_in_flight() {
    use quantick_control::{error::codes, id::RequestId};

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(1);
    let directory = gateway_test_directory("duplicate-id");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let first = client
        .send_with_request_id(
            RequestId::new("twice").unwrap(),
            crate::control::SNAPSHOT_CAPABILITY_ID,
            1,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    client
        .send_with_request_id(
            RequestId::new("twice").unwrap(),
            crate::control::SNAPSHOT_CAPABILITY_ID,
            1,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    // The duplicate is refused at once, before the first has been served.
    let rejected = client.read().unwrap();
    assert_eq!(rejected.request_id, first);
    assert_eq!(
        response_error(&rejected).code.as_str(),
        codes::INVALID_REQUEST
    );
    run_frame(&mut app, &ctx);
    let served = client.read().unwrap();
    assert_eq!(served.request_id, first);
    assert!(matches!(
        served.outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_wait_for_change_sees_a_human_mark_and_does_not_delay_a_concurrent_read() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    let directory = gateway_test_directory("wait-mark");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    run_frame(&mut app, &ctx);
    let mut waiter =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let mut reader =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    // The waiter parks on the gateway side, from the journal's current end.
    let wait_id = waiter
        .send(
            "events.wait",
            serde_json::json!({ "start": "latest", "timeout_ms": 5000 }),
        )
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test(),
        0,
        "a parked wait holds no UI request slot"
    );

    // A concurrent read on another connection is answered at once —
    // the worker-side describe without the frame loop, the UI-side
    // snapshot within one frame.
    assert!(matches!(
        reader
            .invoke(
                crate::control::DESCRIBE_CAPABILITY_ID,
                serde_json::json!({})
            )
            .unwrap()
            .outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    let snapshot_id = reader
        .send(
            crate::control::SNAPSHOT_CAPABILITY_ID,
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    // Frames until the answer is on the socket, not exactly one. What this
    // asserts is that a *parked wait blocks nothing* — the read is served
    // while one is registered, and the assertion above already proves the
    // wait holds no UI request slot. How many frames the gateway needs to
    // drain its queue is scheduling, not contract, and pinning it to one
    // made the test fail under load with `control.timeout`: the reply had
    // not been written yet, so `read()` blocked until the request's own
    // five-second deadline turned a slow frame into a wrong answer. Same
    // bounded loop the parked waiter below already uses.
    let mut served = false;
    for _ in 0..400 {
        run_frame(&mut app, &ctx);
        if reader.reply_pending(std::time::Duration::from_millis(5)) {
            served = true;
            break;
        }
    }
    assert!(served, "the concurrent read was never answered");
    let snapshot = reader.read().unwrap();
    assert_eq!(snapshot.request_id, snapshot_id);
    assert!(matches!(
        snapshot.outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));

    // The human points at bar 20 and takes a mark with a note. The waiter
    // wakes and its page names the bar, the note and the human.
    hover_bar(&mut app, &ctx, 20);
    app.take_mark(Some("this absorption is what I mean".to_owned()));
    let mut reply = None;
    for _ in 0..400 {
        // The woken waiter enqueues its bounded read; frames serve it.
        run_frame(&mut app, &ctx);
        if waiter.reply_pending(std::time::Duration::from_millis(5)) {
            reply = Some(waiter.read().unwrap());
            break;
        }
    }
    let page = reply.expect("the parked wait was answered");
    assert_eq!(page.request_id, wait_id);
    let result = success_result(&page);
    assert_eq!(result["timed_out"], false);
    let events = result["events"].as_array().expect("a page of events");
    let mark = events
        .iter()
        .find(|event| event["kind"] == "attention.mark.created")
        .expect("the mark is in the page");
    assert_eq!(mark["actor"]["kind"], "human_ui");
    assert_eq!(mark["payload"]["note"], "this absorption is what I mean");
    assert_eq!(mark["payload"]["target_source"], "pointer");
    assert_eq!(mark["payload"]["target"]["pointer"]["bar"]["slot"], "20");
    assert_eq!(mark["payload"]["target"]["pointer"]["symbol"], "TESTUSDT");

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_wait_for_change_times_out_cleanly_and_parked_slots_are_bounded() {
    use quantick_control::{
        error::codes,
        limits::{CONTROL_MAX_PARKED_WAITERS, CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION},
    };

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("wait-timeout");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let wait_id = client
        .send(
            "events.wait",
            serde_json::json!({ "start": "latest", "timeout_ms": 100 }),
        )
        .unwrap();
    // Nothing happens; the waiter times out, then its bounded read enters
    // the queue and a frame serves it.
    let mut reply = None;
    for _ in 0..400 {
        run_frame(&mut app, &ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            reply = Some(client.read().unwrap());
            break;
        }
    }
    let reply = reply.expect("the timed-out wait was answered");
    assert_eq!(reply.request_id, wait_id);
    let page = success_result(&reply);
    assert_eq!(page["timed_out"], true);
    assert!(page["events"].as_array().unwrap().is_empty());
    assert!(page["next_cursor"]["next_sequence"].is_string());

    // One connection holds at most its share of the parked slots: the
    // overflow is refused with backpressure, at once, while the others
    // stay parked.
    let mut ids = Vec::new();
    for _ in 0..(CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION + 1) {
        ids.push(
            client
                .send(
                    "events.wait",
                    serde_json::json!({ "start": "latest", "timeout_ms": 30000 }),
                )
                .unwrap(),
        );
    }
    let refused = client.read().unwrap();
    assert_eq!(
        refused.request_id,
        ids[CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION]
    );
    assert_eq!(response_error(&refused).code.as_str(), codes::BACKPRESSURE);
    assert!(response_error(&refused).retryable);

    // Other connections fill the rest of the global slots; the first wait
    // past them is refused too, whoever sends it.
    let mut others = Vec::new();
    for _ in 1..(CONTROL_MAX_PARKED_WAITERS / CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION) {
        let mut other =
            quantick_control_local::client::discover_in(&directory, &gateway_test_options())
                .unwrap()
                .select(None)
                .unwrap();
        for _ in 0..CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION {
            other
                .send(
                    "events.wait",
                    serde_json::json!({ "start": "latest", "timeout_ms": 30000 }),
                )
                .unwrap();
        }
        // Its own overflow is refused at once by the per-connection cap,
        // which also proves the four before it are parked before the
        // next connection sends — the reader handles a connection's
        // requests in order.
        let overflow = other
            .send(
                "events.wait",
                serde_json::json!({ "start": "latest", "timeout_ms": 30000 }),
            )
            .unwrap();
        let refused = other.read().unwrap();
        assert_eq!(refused.request_id, overflow);
        assert_eq!(response_error(&refused).code.as_str(), codes::BACKPRESSURE);
        others.push(other);
    }
    let mut late = quantick_control_local::client::discover_in(&directory, &gateway_test_options())
        .unwrap()
        .select(None)
        .unwrap();
    late.send(
        "events.wait",
        serde_json::json!({ "start": "latest", "timeout_ms": 30000 }),
    )
    .unwrap();
    let refused = late.read().unwrap();
    assert_eq!(response_error(&refused).code.as_str(), codes::BACKPRESSURE);

    // A client that goes away releases its parked slots at the manager's
    // next pass instead of holding them to the deadline: the late client's
    // wait now parks, and times out into a page.
    drop(client);
    let mut outcome = None;
    for _ in 0..40 {
        let late_id = late
            .send(
                "events.wait",
                serde_json::json!({ "start": "latest", "timeout_ms": 100 }),
            )
            .unwrap();
        let mut reply = None;
        for _ in 0..400 {
            run_frame(&mut app, &ctx);
            if late.reply_pending(std::time::Duration::from_millis(5)) {
                reply = Some(late.read().unwrap());
                break;
            }
        }
        let reply = reply.expect("the late wait was answered one way or the other");
        assert_eq!(reply.request_id, late_id);
        match &reply.outcome {
            quantick_control::wire::ResponseOutcome::Success { .. } => {
                outcome = Some(reply);
                break;
            }
            quantick_control::wire::ResponseOutcome::Failure { .. } => {
                assert_eq!(response_error(&reply).code.as_str(), codes::BACKPRESSURE);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
    let page = success_result(&outcome.expect("the released slots admitted the late wait"));
    assert_eq!(page["timed_out"], true);

    drop(others);
    drop(late);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_rejects_a_duplicate_request_id_while_a_wait_is_parked() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("wait-duplicate-id");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let request_id = quantick_control::id::RequestId::new("parked-1").unwrap();
    client
        .send_with_request_id(
            request_id.clone(),
            "events.wait",
            1,
            serde_json::json!({ "start": "latest", "timeout_ms": 300 }),
        )
        .unwrap();
    // The same ID while the wait is parked: refused, never answered twice.
    client
        .send_with_request_id(
            request_id.clone(),
            crate::control::DESCRIBE_CAPABILITY_ID,
            1,
            serde_json::json!({}),
        )
        .unwrap();
    let refused = client.read().unwrap();
    assert_eq!(refused.request_id, request_id);
    assert_eq!(
        response_error(&refused).code.as_str(),
        codes::INVALID_REQUEST
    );
    // The wait itself completes as usual, and the ID is free again.
    let mut reply = None;
    for _ in 0..400 {
        run_frame(&mut app, &ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            reply = Some(client.read().unwrap());
            break;
        }
    }
    let page = reply.expect("the parked wait was answered");
    assert_eq!(page.request_id, request_id);
    assert_eq!(success_result(&page)["timed_out"], true);
    client
        .send_with_request_id(
            request_id.clone(),
            crate::control::DESCRIBE_CAPABILITY_ID,
            1,
            serde_json::json!({}),
        )
        .unwrap();
    assert!(matches!(
        client.read().unwrap().outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// The launch hooks reach the tier the way a client would: an agent's
/// object arrives attributed, and an agent's interruption arrives on the
/// channel it named. A hook that a screenshot cannot reach is a surface
/// that ships unvalidated (`ui-harness`).
#[test]
fn an_assistants_object_and_interruption_arrive_from_a_launch() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    app.pending_control_annotation = Some("this absorption".to_owned());
    app.pending_control_notification = Some("popup:look at 108k".to_owned());
    run_frame(&mut app, &ctx);

    let items = app.active_tab().drawing_pane().drawings.items();
    assert_eq!(items.len(), 1, "the hook placed one object");
    let author = items[0]
        .author
        .as_ref()
        .expect("an object a hook placed is an assistant's, never the trader's");
    assert_eq!(author.actor_kind, "agent");
    assert!(
        author.label().contains("agent"),
        "the label a panel shows names what acted: {}",
        author.label()
    );
    let popup = app
        .surfaces
        .agent_popup
        .pending()
        .expect("the popup is on screen");
    assert_eq!(popup.message, "look at 108k");
    assert!(popup.author.contains("agent"));
}

/// Criterion 2: what an assistant placed is visibly its own, and the
/// trader takes every one of them back in a single gesture that leaves
/// their own drawings exactly where they were.
#[test]
fn the_trader_takes_back_every_object_an_assistant_placed_in_one_action() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    app.surfaces.drawing_chrome.set_pending_text_note(true);
    run_frame(&mut app, &ctx);
    let mine = app.active_tab().drawing_pane().drawings.items()[0].id;

    let anchor = newest_anchor(&app);
    let placed = ["first", "second"]
        .into_iter()
        .map(|text| {
            let result = app
                .control_action(
                    "annotate.label.create",
                    1,
                    crate::control::ActionOrigin::Human,
                    serde_json::json!({ "anchors": [anchor.clone()], "text": text }),
                )
                .expect("the local path places an annotation");
            result["annotation_id"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
        })
        .collect::<Vec<_>>();
    // A local action is the trader's own hand, so nothing is authored yet.
    assert_eq!(
        app.active_tab().drawing_pane().drawings.authored_count(),
        0,
        "an object the trader placed is never labelled as an assistant's"
    );

    // Now the same action with an agent's actor, as the gateway calls it.
    let removed = {
        let pane = app.active_tab_mut().drawing_pane_mut();
        pane.drawings.remove_authored()
    };
    assert_eq!(removed, 0, "there is nothing of an assistant's to remove");

    stamp_agent_author(&mut app, &placed);
    assert_eq!(
        app.active_tab().drawing_pane().drawings.authored_count(),
        2,
        "two objects are now an assistant's"
    );
    let removed = app
        .active_tab_mut()
        .drawing_pane_mut()
        .drawings
        .remove_authored();
    assert_eq!(removed, 2, "one gesture takes back both");
    let items = app.active_tab().drawing_pane().drawings.items();
    assert_eq!(items.len(), 1, "the trader's own object stays");
    assert_eq!(items[0].id, mine);
}

/// Criterion 5, the tier's floor: an operator cannot reach an object the
/// trader drew, whatever id it names. This is what keeps the annotate
/// tier below the cockpit (plan §2.6).
#[test]
fn an_operator_cannot_remove_an_object_the_trader_drew() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    app.surfaces.drawing_chrome.set_pending_text_note(true);
    run_frame(&mut app, &ctx);
    let mine = app.active_tab().drawing_pane().drawings.items()[0].id.0;

    let refused = app
        .control_action(
            "annotate.remove",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "annotation_id": mine.to_string() }),
        )
        .expect_err("the trader's own object is not an annotation");
    assert_eq!(refused.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(
        app.active_tab().drawing_pane().drawings.items().len(),
        1,
        "and it is still there"
    );

    // An object an operator placed is removable, and reports that it went.
    app.control_action(
        "annotate.label.create",
        1,
        crate::control::ActionOrigin::Human,
        serde_json::json!({ "anchors": [newest_anchor(&app)], "text": "theirs" }),
    )
    .unwrap();
    let theirs = app
        .active_tab()
        .drawing_pane()
        .drawings
        .items()
        .last()
        .unwrap()
        .id
        .0;
    stamp_agent_author(&mut app, &[theirs]);
    let id = app
        .active_tab()
        .drawing_pane()
        .drawings
        .items()
        .iter()
        .find(|drawing| drawing.author.is_some())
        .unwrap()
        .id
        .0;
    let result = app
        .control_action(
            "annotate.remove",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "annotation_id": id.to_string() }),
        )
        .unwrap();
    assert_eq!(result["removed"], true);
}

/// The tier's floor, again, on the surface the review found open: an
/// operator detaches what an operator attached, and the trader's own
/// indicator stays on the chart whatever slot id is named.
#[test]
fn an_operator_cannot_detach_the_traders_own_indicator() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    // The trader's own, through the library's door.
    let (_, _, mine) = app.attach_script_indicator(
        "the trader's".to_owned(),
        "//@version=5
indicator(\"mine\")
plot(close)
"
        .to_owned(),
        false,
    );
    for _ in 0..200 {
        run_frame(&mut app, &ctx);
        if !indicator_kinds(&app).is_empty() {
            break;
        }
    }
    let refused = app
        .control_action(
            "indicator.script.detach",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "slot_id": mine.0.to_string() }),
        )
        .expect_err("the trader's own indicator is not this tier's to remove");
    assert_eq!(refused.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(
        indicator_kinds(&app).len(),
        1,
        "and it is still on the pane"
    );
}

/// The regression the slot-identity fix exists for: slot numbers are
/// allocated per pane, so the trader's chart and an operator's second
/// chart both hold a slot 0. Keyed by the number alone, the operator's
/// detach walked `slot_kinds` and took whichever slot 0 was registered
/// first — the trader's.
#[test]
fn an_operator_detaching_its_own_slot_leaves_the_traders_slot_of_the_same_number() {
    use quantick_control::error::codes;

    const SCRIPT: &str = "//@version=5
indicator(\"probe\")
plot(close)
";

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);

    // A second chart on a layout of its own, so the trader's script —
    // a layout edit, mirrored onto every pane of its layout — does not
    // take that chart's slot 0 before the operator gets there.
    app.open_tab("binance".to_owned(), "ETHUSDT".to_owned(), None);
    run_frame(&mut app, &ctx);
    app.create_layout(Some("agent"))
        .expect("a layout for the second chart");
    app.cycle_tab(-1);

    // The trader's own, on the first tab.
    let (traders_tab, _, traders_slot) =
        app.attach_script_indicator("the trader's".to_owned(), SCRIPT.to_owned(), false);
    for _ in 0..200 {
        run_frame(&mut app, &ctx);
        if !indicator_kinds(&app).is_empty() {
            break;
        }
    }

    // The second chart, whose slot numbering starts over from zero.
    app.cycle_tab(1);
    run_frame(&mut app, &ctx);
    let (operators_tab, _, operators_slot) =
        app.attach_script_indicator("an assistant's".to_owned(), SCRIPT.to_owned(), true);
    for _ in 0..200 {
        run_frame(&mut app, &ctx);
        if !indicator_kinds(&app).is_empty() {
            break;
        }
    }
    assert_ne!(traders_tab, operators_tab, "two charts, not one");
    assert_eq!(
        traders_slot.0, operators_slot.0,
        "the numbering starts over per pane, which is what made the bug reachable"
    );

    // The operator takes back its own. The trader's, wearing the same
    // number on another chart, must still be there afterwards.
    assert!(
        app.control_action(
            "indicator.script.detach",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "slot_id": operators_slot.0.to_string() }),
        )
        .is_ok(),
        "an operator may take back what it attached"
    );
    run_frame(&mut app, &ctx);
    assert!(
        indicator_kinds(&app).is_empty(),
        "the assistant's is off its own chart"
    );

    let traders_index = app
        .control_tabs()
        .iter()
        .position(|tab| tab.id == traders_tab)
        .expect("the trader's chart is still open");
    assert_eq!(
        app.control_tabs()[traders_index]
            .focused_pane()
            .indicators
            .all()
            .len(),
        1,
        "and the trader's, sharing only a number with it, was never touched"
    );

    // With its own claim spent, the same number now names only the
    // trader's slot, and the tier refuses it rather than reaching across.
    app.active_tab = traders_index;
    run_frame(&mut app, &ctx);
    let refused = app
        .control_action(
            "indicator.script.detach",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "slot_id": traders_slot.0.to_string() }),
        )
        .expect_err("the trader's own indicator is not this tier's to remove");
    assert_eq!(refused.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(
        indicator_kinds(&app).len(),
        1,
        "and it is still on the pane"
    );
}

/// An annotation never lands inside a drawing the trader is still making:
/// `place_with` would have pushed the call's anchor onto their draft.
#[test]
fn an_annotation_refuses_to_land_in_a_drawing_the_trader_is_still_making() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    // The trader drops the first corner of a rectangle and stops there.
    let anchor = newest_anchor(&app);
    let point = {
        let pane = app.active_tab().drawing_pane();
        let slot = pane.slots().saturating_sub(1);
        drawings::ChartPoint::at_time(slot as f32 + 0.5, 1.0, pane.slot_open_time(slot))
    };
    let rectangle = drawings::DrawingTool::by_id("rectangle").unwrap();
    let fresh = app.control_new_drawing(rectangle);
    app.active_tab_mut().drawing_pane_mut().drawings.place_with(
        rectangle,
        &drawings::DrawingBand::Price,
        point,
        |_| fresh,
    );
    assert!(
        app.active_tab().drawing_pane().drawings.draft().is_some(),
        "the trader is mid-gesture"
    );

    let refused = app
        .control_action(
            "annotate.zone.create",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "anchors": [anchor.clone(), anchor] }),
        )
        .expect_err("an annotation waits for the hand to finish");
    assert_eq!(refused.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(refused.retryable, "the trader will finish; try again");
    let pane = app.active_tab().drawing_pane();
    assert_eq!(
        pane.drawings.draft().map(|draft| draft.points.len()),
        Some(1),
        "their unfinished object is untouched"
    );
    assert_eq!(pane.drawings.items().len(), 0, "and nothing was committed");
}

/// Criterion 3: a script that does not compile comes back as spans and
/// codes, never as a rendered paragraph an agent has to parse.
#[test]
fn a_script_that_does_not_compile_answers_with_spans_and_codes() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let error = app
        .control_action(
            "indicator.script.attach",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({
                "name": "broken",
                "source": "//@version=5\nindicator(\"broken\")\nplot(\n",
            }),
        )
        .expect_err("a broken script never becomes a slot");
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    let details = error.context.details.expect("diagnostics travel as data");
    let diagnostics = details["diagnostics"].as_array().expect("a list");
    assert!(!diagnostics.is_empty());
    let first = &diagnostics[0];
    assert!(
        first["code"].as_str().is_some_and(|code| !code.is_empty()),
        "every diagnostic names its stable code"
    );
    assert!(first["line"].as_u64().unwrap() >= 1);
    assert!(first["column"].as_u64().unwrap() >= 1);
    assert!(first["end"].as_u64().unwrap() >= first["start"].as_u64().unwrap());
    assert!(first["message"].as_str().is_some());
    assert_eq!(indicator_kinds(&app).len(), 0, "nothing was attached");
}

/// Criterion 4: a script that compiles is attached through the same door
/// the library's click uses, and detaching restores the pane exactly.
#[test]
fn attaching_a_script_and_detaching_it_leaves_the_pane_as_it_was() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let before = indicator_kinds(&app);

    let attached = app
        .run_agent_action(
            "indicator.script.attach",
            serde_json::json!({
                "name": "agent ema",
                "source": "//@version=5\nindicator(\"agent ema\")\nplot(close)\n",
            }),
        )
        .expect("a script that compiles is attached");
    let slot_id = attached["slot_id"].as_str().unwrap().to_owned();
    // The worker builds off the application thread; frames apply what it
    // produced, exactly as the library's own click path is served.
    for _ in 0..200 {
        run_frame(&mut app, &ctx);
        if indicator_kinds(&app).len() > before.len() {
            break;
        }
    }
    assert_eq!(
        indicator_kinds(&app).len(),
        before.len() + 1,
        "the script is on the pane"
    );

    let detached = app
        .run_agent_action(
            "indicator.script.detach",
            serde_json::json!({ "slot_id": slot_id }),
        )
        .expect("what an operator attached, an operator detaches");
    assert_eq!(detached["detached"], true);
    for _ in 0..200 {
        run_frame(&mut app, &ctx);
        if indicator_kinds(&app).len() == before.len() {
            break;
        }
    }
    assert_eq!(
        indicator_kinds(&app),
        before,
        "the pane is exactly what it was before the attach"
    );
}

/// Criterion 6: a client that floods the trader is refused before the
/// third interruption, and the refusal says when to come back.
#[test]
fn a_notification_flood_is_refused_before_the_trader_is_buried() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("notify-flood");
    grant_annotate_for_test(&mut app, "all-reads,annotate-tier");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &annotator_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let mut outcomes = Vec::new();
    for index in 0..3 {
        outcomes.push(remote_call(
            &mut app,
            &ctx,
            &mut client,
            "notify.toast",
            serde_json::json!({ "message": format!("look {index}") }),
        ));
    }
    assert_eq!(
        success_result(&outcomes[0])["raised"],
        true,
        "the first interruption lands"
    );
    assert_eq!(success_result(&outcomes[1])["raised"], true);
    let refused = response_error(&outcomes[2]);
    assert_eq!(
        refused.code.as_str(),
        codes::BACKPRESSURE,
        "the burst is spent and the third is refused"
    );
    assert!(refused.retryable);
    assert!(
        refused
            .context
            .next_steps
            .iter()
            .any(|step| step.contains("retry in")),
        "the refusal says when to come back: {:?}",
        refused.context.next_steps
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// Criterion 6, second half: sound has a scope of its own, and a client
/// the trader did not give it to cannot make a noise — however loudly it
/// asks for the scope at the handshake.
#[test]
fn a_client_without_the_sound_scope_cannot_make_a_sound() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("notify-sound");
    // Everything of the annotate tier except the sound.
    grant_annotate_for_test(
        &mut app,
        "all-reads,annotate,annotate.attention,annotate.chart,annotate.notification,annotate.script",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut options = annotator_test_options();
    options
        .requested_scopes
        .insert(quantick_control::id::PermissionId::new("annotate.sound").unwrap());
    let mut client = quantick_control_local::client::discover_in(&directory, &options)
        .unwrap()
        .select(None)
        .unwrap();
    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "notify.sound",
        serde_json::json!({ "message": "listen" }),
    );
    assert_eq!(
        response_error(&response).code.as_str(),
        codes::PERMISSION_DENIED,
        "asking for a scope is not being granted it"
    );
    // The toast, which the trader did grant, still works.
    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "notify.toast",
        serde_json::json!({ "message": "read this" }),
    );
    assert_eq!(success_result(&response)["raised"], true);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// Criterion 8: the observer profile reaches no action of this tier —
/// every one of them, not only the mark.
#[test]
fn an_observer_reaches_no_action_of_the_annotate_tier() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("annotate-denied");
    // The default grant: reads only, which is what a fresh window offers.
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &annotator_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let anchor = newest_anchor(&app);
    for (capability, payload) in [
        (
            "annotate.label.create",
            serde_json::json!({ "anchors": [anchor.clone()], "text": "no" }),
        ),
        (
            "annotate.arrow.create",
            serde_json::json!({ "anchors": [anchor.clone(), anchor.clone()] }),
        ),
        (
            "annotate.zone.create",
            serde_json::json!({ "anchors": [anchor.clone(), anchor.clone()] }),
        ),
        (
            "annotate.remove",
            serde_json::json!({ "annotation_id": "1" }),
        ),
        ("notify.popup", serde_json::json!({ "message": "hello" })),
        ("notify.toast", serde_json::json!({ "message": "hello" })),
        ("notify.sound", serde_json::json!({ "message": "hello" })),
        (
            "indicator.script.attach",
            serde_json::json!({ "name": "x", "source": "//@version=5\nindicator(\"x\")\nplot(close)\n" }),
        ),
        (
            "indicator.script.detach",
            serde_json::json!({ "slot_id": "1" }),
        ),
    ] {
        let response = remote_call(&mut app, &ctx, &mut client, capability, payload);
        assert_eq!(
            response_error(&response).code.as_str(),
            codes::PERMISSION_DENIED,
            "{capability} is denied to a connection the trader granted reads to"
        );
    }
    assert_eq!(
        app.active_tab().drawing_pane().drawings.items().len(),
        0,
        "and nothing reached the chart"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// Criterion 7 and the gap #223 left: the trace records the *resolved*
/// input, so a rerun marks the bar that was marked rather than wherever
/// the pointer happens to be during the rerun.
#[test]
fn the_trace_records_what_was_resolved_not_what_was_asked() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    hover_bar(&mut app, &ctx, 20);
    let result = app
        .control_action(
            crate::control::MARK_CAPABILITY_ID,
            crate::control::MARK_CAPABILITY_VERSION,
            crate::control::ActionOrigin::Human,
            // No target at all: the caller says "here", and the port is
            // what turns that into a bar.
            serde_json::json!({}),
        )
        .expect("the mark resolves the pointer");
    assert_eq!(
        result["target_source"], "pointer",
        "the port resolved it, and says so"
    );
    assert_eq!(result["target"]["pointer"]["bar"]["slot"], "20");
}

#[test]
fn gateway_observer_cannot_create_a_mark_remotely_but_reads_the_registered_action() {
    use quantick_control::error::codes;

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(2);
    let directory = gateway_test_directory("remote-mark");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let refused = client
        .invoke(crate::control::MARK_CAPABILITY_ID, serde_json::json!({}))
        .unwrap();
    assert_eq!(
        response_error(&refused).code.as_str(),
        codes::PERMISSION_DENIED
    );

    let described = success_result(
        &client
            .invoke(
                crate::control::DESCRIBE_CAPABILITY_ID,
                serde_json::json!({}),
            )
            .unwrap(),
    );
    let capabilities = described["capabilities"].as_array().unwrap();
    let mark = capabilities
        .iter()
        .find(|capability| capability["id"] == crate::control::MARK_CAPABILITY_ID)
        .expect("the action is discoverable");
    assert_eq!(mark["effect"], "annotate");
    assert_eq!(mark["read_only"], false);
    assert!(
        capabilities
            .iter()
            .any(|capability| capability["id"] == "events.read")
    );
    assert!(
        capabilities
            .iter()
            .any(|capability| capability["id"] == "events.wait")
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn gateway_journals_a_focus_change_the_human_made() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 40);
    let directory = gateway_test_directory("focus-event");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    // Baseline frame, then the human focuses the other pane.
    run_frame(&mut app, &ctx);
    let before = app.active_tab().focused_side();
    let other = match before {
        PaneSide::Time(_) => PaneSide::Flow,
        PaneSide::Flow => PaneSide::Time(0),
    };
    let other_point = pane_point(&app, other);
    click_chart(&mut app, &ctx, other_point);
    assert_eq!(app.active_tab().focused_side(), other);
    run_frame(&mut app, &ctx);

    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let read_id = client
        .send("events.read", serde_json::json!({ "start": "oldest" }))
        .unwrap();
    wait_for_queued_gateway_requests(&app, 1);
    run_frame(&mut app, &ctx);
    let page = client.read().unwrap();
    assert_eq!(page.request_id, read_id);
    let result = success_result(&page);
    let kinds = result["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| event["kind"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(
        kinds.iter().any(|kind| kind == "workspace.focus.changed"),
        "the focus change is in the journal: {kinds:?}"
    );
    assert!(
        kinds
            .iter()
            .any(|kind| kind == "interaction.selection.changed"),
        "the selection scope changed with the focus: {kinds:?}"
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn two_tabs_on_the_same_recording_share_one_trace_walk() {
    let ctx = egui::Context::default();
    let dir = std::env::temp_dir().join(format!(
        "quantick-control-trace-two-tabs-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut app, _commands) = app_with_history(12);
    app.active_tab_mut().replay = Some(feed::ReplayLink::for_test(recording_at(&dir)));
    hover_bar(&mut app, &ctx, 6);
    app.take_mark(Some("once".to_owned()));
    drop(app);

    let (mut app, _commands) = app_with_history(12);
    app.active_tab_mut().replay = Some(feed::ReplayLink::for_test(recording_at(&dir)));
    let _second = open_second_tab(&mut app, &ctx, "ETHUSDT");
    app.tabs[1].replay = Some(feed::ReplayLink::for_test(recording_at(&dir)));
    app.active_tab = 0;
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let replayed = app
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 64, 1 << 20)
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "attention.mark.created")
        .count();
    assert_eq!(replayed, 1, "one walk per recording, not one per tab");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn observer_max_chart_window_capture_stays_within_the_ui_budget() {
    // The always-on guard judges the median of the best batch: a typical
    // capture of the largest allowed page must fit the budget, and that
    // reading survives a loaded test runner. The tail is measured by the
    // ignored sibling below, on a quiet machine, and recorded in the
    // evidence document.
    let (median_us, p99_us, worst_us) = measure_max_chart_window_capture_us();
    assert!(
        median_us <= quantick_control::limits::CONTROL_UI_BUDGET_US,
        "maximum chart-window capture median {median_us} us (p99 {p99_us} us, worst {worst_us} us) exceeds the {} us UI budget",
        quantick_control::limits::CONTROL_UI_BUDGET_US
    );
}

/// The strict tail reading: `cargo test -p quantick-app
/// observer_max_chart_window_capture_p99 -- --ignored --nocapture` on a
/// quiet machine. Ignored in the ordinary suite because a p99 measured
/// beside a thousand other tests reports the runner's load, not the
/// capture's cost.
#[test]
#[ignore]
fn observer_max_chart_window_capture_p99_stays_within_the_ui_budget() {
    let (median_us, p99_us, worst_us) = measure_max_chart_window_capture_us();
    assert!(
        p99_us <= quantick_control::limits::CONTROL_UI_BUDGET_US,
        "maximum chart-window capture p99 {p99_us} us (median {median_us} us, worst {worst_us} us) exceeds the {} us UI budget",
        quantick_control::limits::CONTROL_UI_BUDGET_US
    );
}

#[test]
fn observer_preserves_split_pane_focus_and_market_provenance() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 80);
    let time_point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, time_point);
    assert_eq!(app.active_tab().focused_side(), PaneSide::Time(0));

    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("workspace.summary"),
        observer_scope("chart.summary"),
    ];
    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    let workspace = &capture.scopes[&scopes[0]].value;
    assert_eq!(workspace["tabs"][0]["layout"], "time_and_flow");
    assert_eq!(workspace["tabs"][0]["focused_pane"], "time");
    let visible = workspace["tabs"][0]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|pane| pane["visible"] == true)
        .collect::<Vec<_>>();
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .iter()
            .filter(|pane| pane["focused"] == true)
            .count(),
        1
    );

    let chart = &capture.scopes[&scopes[1]].value;
    let panes = chart["panes"].as_array().unwrap();
    assert_eq!(panes.len(), 2);
    assert!(panes.iter().all(|pane| pane["feed_id"] == "binance"));
    assert!(panes.iter().all(|pane| pane["symbol"] == "TESTUSDT"));
    assert_eq!(
        panes.iter().filter(|pane| pane["focused"] == true).count(),
        1
    );
}

#[test]
fn observer_cursor_resolves_the_exact_bar_under_the_pointer() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    let expected_slot = 20usize;
    let position = {
        let pane = &app.active_tab().flow_pane;
        let chart = pane.last_chart_area.expect("the pane reported its rect");
        let right = pane.last_lane_divider_x.unwrap_or_else(|| chart.right());
        egui::pos2(
            pane.viewport.x_center(expected_slot, right, pane.slots()),
            chart.center().y,
        )
    };
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(position)]);

    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("interaction.cursor");
    let capture = registry
        .capture(&app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    let cursor = &capture.scopes[&scope].value;
    assert_eq!(cursor["pointer_availability"]["available"], true);
    assert_eq!(cursor["pointer"]["slot"], expected_slot.to_string());
    assert_eq!(cursor["pointer"]["bar"]["slot"], expected_slot.to_string());
    assert_eq!(cursor["pointer"]["bar"]["state"], "closed");
    assert_eq!(cursor["pointer"]["symbol"], "TESTUSDT");
}

#[test]
fn the_scene_names_the_same_controls_two_frames_running() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let first = observer_scene(&app);
    run_frame(&mut app, &ctx);
    let second = observer_scene(&app);
    assert_eq!(
        first, second,
        "a frame passing may not rename anything on screen"
    );

    // And the identifiers are not positions in disguise. Switching the
    // dock adds or removes a strip of tabs ahead of the chart canvases,
    // moving every control after it; one that answered to its index would
    // be renamed by that alone, and an assistant told to press it a moment
    // later would press whatever had slid into its place.
    let before = scene_control_ids(&first);
    let dock_was_open = app.dock.visible();
    app.dock.toggle_visible();
    run_frame(&mut app, &ctx);
    let after = scene_control_ids(&observer_scene(&app));
    let dock_now_listed = after.iter().any(|id| id.starts_with("dock."));
    assert_ne!(
        dock_now_listed, dock_was_open,
        "switching the dock changes what is on screen"
    );
    assert_ne!(
        after.len(),
        before.len(),
        "so the controls after it have all moved"
    );
    let survivors: Vec<&String> = before
        .iter()
        .filter(|id| id.starts_with("tool_rail.") || id.starts_with("toolbar."))
        .collect();
    assert!(
        !survivors.is_empty(),
        "the rail and the layer toggles are on screen in this fixture"
    );
    for id in survivors {
        assert!(
            after.contains(id),
            "{id} kept its place on screen and must keep its name"
        );
    }
}

#[test]
fn the_control_the_cursor_resolves_to_is_one_the_scene_names() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(40);
    run_frame(&mut app, &ctx);
    let position = {
        let pane = &app.active_tab().flow_pane;
        let chart = pane.last_chart_area.expect("the pane reported its rect");
        chart.center()
    };
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(position)]);

    // One capture, so the two scopes cannot describe different moments.
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("interaction.cursor"),
        observer_scope("scene.controls"),
    ];
    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    let cursor = &capture.scopes[&scopes[0]].value;
    let scene = &capture.scopes[&scopes[1]].value;

    assert_eq!(cursor["semantic_scene"]["available"], true);
    assert_eq!(
        cursor["pointer"]["control_id_availability"]["available"],
        true
    );
    let under_pointer = cursor["pointer"]["control_id"]
        .as_str()
        .expect("the pointer resolves to a control");
    assert!(
        scene_control_ids(scene).contains(&under_pointer.to_owned()),
        "the cursor answered {under_pointer}, which the scene does not name"
    );

    // And it is the canvas of the pane the cursor reports, not merely some
    // control that happens to exist.
    let control = scene_control(scene, under_pointer);
    assert_eq!(control["role"], "canvas");
    assert_eq!(control["owner"]["kind"], "tab");
    assert_eq!(
        control["control_id"],
        format!(
            "pane.{}.canvas",
            cursor["pointer"]["pane_id"].as_str().unwrap()
        )
    );

    // A canvas is the one control the frame already measured, so it is the
    // one that answers with a rectangle instead of saying it has none.
    assert_eq!(control["bounds_availability"]["available"], true);
    assert!(control["bounds"]["width_pt"].is_string());
}

#[test]
fn the_scene_names_the_rails_buttons_and_not_the_tools_behind_them() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let rail: Vec<String> = scene_control_ids(&observer_scene(&app))
        .into_iter()
        .filter(|id| id.starts_with("tool_rail."))
        .collect();
    assert!(rail.len() < 2 + drawings::DRAWING_TOOLS.len(), "{rail:?}");
    // A family folds into one slot with a flyout, so its members have no
    // button of their own and the scene must not name them. `ray` shares
    // the lines family with `trend-line`; listing it would send an
    // assistant looking for a button that is not painted.
    assert!(
        rail.iter().any(|id| id == "tool_rail.tool.pointer"),
        "{rail:?}"
    );
    assert!(
        !rail.iter().any(|id| id == "tool_rail.tool.ray"),
        "a folded family member has no button of its own: {rail:?}"
    );
}

#[test]
fn the_scene_says_which_regions_it_looked_at_rather_than_claiming_the_screen() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let scene = observer_scene(&app);
    // Whole regions of the window are still unnamed — the SOURCE and BARS
    // groups, the menus, every dialog. A capture that answered "complete"
    // would tell a client those controls do not exist.
    let regions = scene["covered_regions"].as_array().unwrap();
    assert!(regions.iter().any(|region| region == "toolbar"));
    assert!(regions.iter().any(|region| region == "tool_rail"));
    for control in scene["controls"].as_array().unwrap() {
        assert!(
            regions.contains(&control["owner"]["kind"]),
            "{} belongs to a region the capture did not declare",
            control["control_id"]
        );
    }

    // And a *declared* region is not a finished one either. The toolbar is
    // walked as far as its LAYERS group and no further, so a client that
    // read `coverage` as "this is the toolbar" would conclude the PANELS
    // button and the whole SOURCE half do not exist. Nothing is truncated
    // in this fixture and the answer is still not "complete".
    assert!(
        !scene["controls"].as_array().unwrap().is_empty(),
        "the fixture has controls, so this is not vacuously partial"
    );
    assert_eq!(scene["coverage"]["available"], false);
    assert_eq!(
        scene["coverage"]["reason"], "only_the_named_group_of_each_covered_region_is_enumerated",
        "and it says which of the two cuts applies"
    );
    // The toolbar paints these beside the four the scene names; they are
    // the proof the region is a walk and not an inventory.
    let named: Vec<String> = scene_control_ids(&scene);
    assert!(
        !named.iter().any(|id| id.starts_with("toolbar.source")),
        "the SOURCE group is unnamed, which is what `coverage` admits"
    );
}

/// A starred tool is a real button in the rail's pinned section, and one
/// the trader put there on purpose.
///
/// It is painted beside the folded run, so it needs a name of its own:
/// naming it after the run slot it was pinned from would give one
/// identifier two rectangles. The rail listed neither, and an assistant
/// asked to press the button the trader had starred was told it was not
/// on screen.
#[test]
fn a_starred_tool_is_a_button_the_scene_names_in_its_own_right() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let unpinned = scene_control_ids(&observer_scene(&app));
    assert!(
        !unpinned
            .iter()
            .any(|id| id.starts_with("tool_rail.favorite.")),
        "nothing is starred yet: {unpinned:?}"
    );

    let starred = drawings::DRAWING_TOOLS[0];
    app.toolrail.toggle_favorite(starred);
    run_frame(&mut app, &ctx);
    let pinned = scene_control_ids(&observer_scene(&app));
    let expected = format!("tool_rail.favorite.{}", starred.id());
    assert!(
        pinned.contains(&expected),
        "the star paints a button, so the scene names it: {pinned:?}"
    );
    // And it is a *second* name, not a rename: the run keeps its slot.
    assert_eq!(
        pinned.iter().filter(|id| **id == expected).count(),
        1,
        "one pinned button, one name"
    );
    assert!(
        pinned.len() > unpinned.len(),
        "starring adds a button rather than moving one"
    );
}

#[test]
fn observer_resolves_mirrored_drawings_without_leaking_user_text() {
    const CANARY: &str = "CANARY_PRIVATE_PATH_C:\\Users\\Trader\\secret";

    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    let slot = 100;
    let (time, price) = {
        let pane = &app.active_tab().flow_pane;
        let bar = pane.closed_bar(slot).expect("fixture bar");
        (
            pane.slot_open_time(slot).expect("fixture market time"),
            rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap(),
        )
    };
    let flow = &mut app.active_tab_mut().flow_pane;
    assert!(flow.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        ChartPoint::at_time(slot as f32 + 0.5, price, Some(time)),
        |tool| drawings::NewDrawing {
            style: drawings::DrawingStyle::default(),
            payload: tool.default_payload(),
        },
    ));
    let selected = flow.drawings.selected().expect("placement selects");
    flow.drawings.rename_at(selected, CANARY);
    flow.drawings
        .selected_mut()
        .expect("selected drawing")
        .scope = drawings::DrawingScope::AllCharts;
    app.active_tab_mut().notice = FeedNotice::attention(CANARY, CANARY);
    run_frame(&mut app, &ctx);

    let time_chart = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .last_chart_area
        .expect("time pane reported its rect");
    let position = egui::pos2(
        time_chart.center().x,
        price_y(&app, PaneSide::Time(0), price),
    );
    run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(position)]);

    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [
        observer_scope("feed.status"),
        observer_scope("interaction.cursor"),
        observer_scope("interaction.selection"),
        // The enumerating scope is held to the same rule as the pointer
        // ones: it lists every drawing, so it is the likeliest place for a
        // trader's own name to escape.
        observer_scope("analysis.drawings"),
    ];
    let capture = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    let encoded = serde_json::to_string(&capture).unwrap();
    assert!(
        !encoded.contains(CANARY),
        "observer output redacts user text"
    );

    let notice = &capture.scopes[&scopes[0]].value["tabs"][0]["notice"];
    assert_eq!(notice["kind"], "attention");
    assert_eq!(notice["headline_present"], true);
    assert_eq!(notice["next_step_present"], true);
    assert_eq!(
        notice["text_availability"],
        "redacted_pending_attention_scope"
    );
    let drawing = &capture.scopes[&scopes[1]].value["pointer"]["drawing"];
    assert_eq!(drawing["mirrored"], true);
    assert_eq!(drawing["owner_pane_side"], "flow");
    assert_eq!(drawing["user_label_present"], true);
    let selection = &capture.scopes[&scopes[2]].value["drawing"];
    assert_eq!(selection["user_label_present"], true);

    let listed = &capture.scopes[&scopes[3]].value["tabs"][0]["panes"][0]["drawings"][0];
    assert_eq!(listed["tool_id"], "horizontal-line");
    assert_eq!(listed["scope"], "all_charts");
    assert_eq!(listed["band"], "price");
    assert_eq!(
        listed["user_label_present"], true,
        "the trader named it, and the wire says so without saying what"
    );
    assert!(
        listed["author"].is_null(),
        "the trader placed it by hand, so it carries no other author"
    );

    // "Hide all" is a switch of its own, and it decides what is on screen
    // whatever each object's own eye says. Reporting only the per-object
    // eye tells an agent every mark is visible on a pane showing none.
    let pane = &capture.scopes[&scopes[3]].value["tabs"][0]["panes"][0];
    assert_eq!(pane["layer_hidden"], false, "the layer starts drawn");
    assert_eq!(listed["hidden"], false, "and so does this object");
    app.active_tab_mut().flow_pane.drawings.set_all_hidden(true);
    let hidden = registry
        .capture(&app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    let pane = &hidden.scopes[&scopes[3]].value["tabs"][0]["panes"][0];
    assert_eq!(
        pane["layer_hidden"], true,
        "the pane says its whole drawing layer is off"
    );
    assert_eq!(
        pane["drawings"][0]["hidden"], false,
        "without rewriting each object's own eye, which `show all` restores"
    );
}

#[test]
fn observer_chart_pagination_allows_append_but_rejects_prefix_changes() {
    use crate::control::chart::{ChartWindowQuery, ChartWindowRange, chart_window};
    use quantick_control::{error::codes, wire::WireU64};

    let (mut app, _commands) = app_with_history(8);
    let tab_id = app.active_tab().id;
    let pane_id = app.active_tab().flow_pane.id;
    let query = ChartWindowQuery {
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane_id),
        range: ChartWindowRange::Slots {
            start_slot: WireU64::new(0),
            end_slot_exclusive: WireU64::new(8),
        },
        page_size: 2,
    };
    let instance = observer_instance();
    let visible_query = ChartWindowQuery::visible(tab_id, pane_id);
    // Before the first paint the visible range is a well-formed question
    // the pane cannot answer yet: unavailable and retryable, with a next
    // step, never "malformed".
    let unavailable = chart_window(&app, &instance, &visible_query, None).unwrap_err();
    assert_eq!(
        unavailable.code.as_str(),
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE
    );
    assert!(unavailable.retryable);
    assert!(!unavailable.context.next_steps.is_empty());
    let first = chart_window(&app, &instance, &query, None).unwrap();
    assert_eq!(
        first
            .omitted_modules
            .iter()
            .map(|module| module.as_str())
            .collect::<Vec<_>>(),
        vec!["drawings", "indicators", "orderflow"]
    );
    let cursor = first.bars.next_cursor.expect("more closed bars remain");

    let appended = trade(9);
    app.active_tab_mut()
        .ingest_live_trade_at(&appended, appended.timestamp_ms);
    let second = chart_window(&app, &instance, &query, Some(&cursor)).unwrap();
    assert_eq!(
        second
            .bars
            .items
            .iter()
            .map(|bar| bar.slot.get())
            .collect::<Vec<_>>(),
        vec![2, 3]
    );

    app.active_tab_mut()
        .flow_pane
        .prepend_history(std::slice::from_ref(&trade(0)));
    let error = chart_window(&app, &instance, &query, Some(&cursor)).unwrap_err();
    assert_eq!(error.code.as_str(), codes::PAGE_STALE);
    assert!(error.retryable);
}

/// Criteria 1 and 2 of roadmap 5.4: an agent explains the running session
/// from a bundle alone, without a picture of the window — and the events
/// it carries continue through the very cursor `events.read` takes.
///
/// Everything here goes over the loopback socket into the running
/// instance: capture, manifest, paged read, reassembly, digest.
#[test]
fn an_evidence_bundle_explains_the_session_without_an_image_and_its_events_keep_reading() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(12);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-explains");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "evidence.capture",
        serde_json::json!({ "scopes": EVIDENCE_TEST_SCOPES }),
    );
    let manifest = success_result(&response).clone();
    assert!(
        manifest["screenshot"].is_null(),
        "nothing was rasterised and the manifest does not pretend otherwise"
    );

    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    // The digest the manifest attests is the digest of what came back.
    assert_eq!(
        quantick_control::canonical::raw_digest(&bytes),
        manifest["content_digest"].as_str().unwrap(),
        "the chunks are byte runs of the canonical text the manifest hashed"
    );

    // Who and what: instance, session, build, host, one capture revision.
    assert_eq!(document["instance_id"], manifest["instance_id"]);
    assert_eq!(document["session_id"], manifest["session_id"]);
    assert_eq!(document["capture_revision"], manifest["capture_revision"]);
    let system = &document["environment"]["system"];
    assert_eq!(system["application"], "quantick");
    assert_eq!(system["application_version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(system["target_os"], std::env::consts::OS);
    assert!(system["control_protocol_version"].as_u64().unwrap() >= 1);
    assert_eq!(document["environment"]["graphics_backend"], "glow");
    assert!(
        document["environment"]["process_uptime_ms"]
            .as_i64()
            .unwrap()
            >= 0
    );

    // The session itself: the workspace, the market, the chart, the frame
    // cost and what is on screen, all at one revision.
    let scopes = &document["snapshot"]["scopes"];
    for scope in EVIDENCE_TEST_SCOPES {
        assert!(
            scopes[scope].is_object(),
            "{scope} is missing from the bundle"
        );
    }
    assert_eq!(
        scopes["feed.status"]["value"]["tabs"][0]["active_symbol"],
        "TESTUSDT"
    );
    assert!(
        scopes["chart.summary"]["value"]["panes"][0]["closed_bar_count"]
            .as_str()
            .is_some_and(|count| count.parse::<u64>().is_ok_and(|count| count > 0)),
        "the chart says how many bars it holds"
    );
    assert!(
        !scopes["scene.controls"]["value"]["controls"]
            .as_array()
            .unwrap()
            .is_empty(),
        "the scene names what is on screen"
    );

    // Criterion 2: the events came with it, and the cursor keeps going.
    let cursor = document["events"]["next_cursor"].clone();
    assert!(cursor.is_object(), "the bundle carries a usable cursor");
    let continued = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "events.read",
        serde_json::json!({ "cursor": cursor }),
    );
    let page = success_result(&continued);
    assert!(
        page["next_cursor"]["next_sequence"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= document["events"]["next_cursor"]["next_sequence"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap(),
        "reading from the bundle's cursor continues the same journal"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 3: the bundle says what it left out, and says it in codes a
/// client can branch on rather than sentences it would have to read.
#[test]
fn an_evidence_bundle_names_what_it_omitted_and_why_as_codes_not_prose() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(6);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-coverage");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "evidence.capture",
        serde_json::json!({ "scopes": ["system.info"] }),
    );
    let manifest = success_result(&response);
    let coverage = &manifest["coverage"];
    assert_eq!(
        coverage["complete"], false,
        "a bundle is never the whole session and never claims to be"
    );

    let omitted = coverage["omitted_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|scope| scope.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        omitted.contains(&"scene.controls") && omitted.contains(&"health.summary"),
        "every registered scope the caller did not name is listed: {omitted:?}"
    );

    let gaps = coverage["not_captured"].as_array().unwrap();
    let reasons = gaps
        .iter()
        .map(|gap| {
            (
                gap["subject"].as_str().unwrap(),
                gap["reason"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    for expected in [
        ("diagnostic_logs", "not_captured_in_this_tier"),
        // Two separate claims, because they have two separate answers: the
        // journal is stripped by key whatever the grant, while one
        // projection is allowed to publish the trader's own words and this
        // capture did not ask for it.
        ("user_authored_text_in_events", "redacted_by_payload_key"),
        (
            "user_authored_text_in_projections",
            "redacted_by_projection_policy",
        ),
        ("configuration_paths", "redacted_path_values"),
        ("disk_export", "cockpit_tier_capability"),
        ("screenshot", "not_requested"),
    ] {
        assert!(
            reasons.contains(&expected),
            "{expected:?} is missing from {reasons:?}"
        );
    }
    // Codes, not sentences: no gap reason contains a space or a capital.
    for (subject, reason) in &reasons {
        assert!(
            reason
                .chars()
                .all(|character| character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || character == '_'),
            "`{subject}` reports a rendered sentence rather than a code: `{reason}`"
        );
    }

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 4: a bundle with a screenshot stamps the image with the same
/// capture revision as the scene beside it, and every control the scene
/// gave bounds for resolves to a rectangle inside that image.
///
/// The revision is the whole mechanism: without it a client would be
/// pairing a list of names with a picture of some other frame.
#[test]
fn a_bundle_with_a_screenshot_maps_every_named_control_to_a_region_of_the_image() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(10);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-screenshot");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let width = TEST_WINDOW.x as u32;
    let height = TEST_WINDOW.y as u32;
    let response = capture_with_screenshot(
        &mut app,
        &ctx,
        &mut client,
        serde_json::json!({ "scopes": EVIDENCE_TEST_SCOPES, "screenshot": true }),
        test_screenshot(width, height),
    );
    assert!(
        app.surfaces.toast.message().is_some(),
        "the trader is told when a picture of their window is taken"
    );
    let manifest = success_result(&response).clone();
    let screenshot = &manifest["screenshot"];
    assert_eq!(
        screenshot["capture_revision"], manifest["capture_revision"],
        "the image is stamped with the capture the scene was taken in"
    );
    assert_eq!(screenshot["format"], "png");
    assert_eq!(screenshot["width_px"], width);
    assert_eq!(screenshot["height_px"], height);

    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let image = &document["screenshot"];
    assert_eq!(
        image["descriptor"]["capture_revision"], document["capture_revision"],
        "and so is the copy travelling with the pixels"
    );

    // The pixels really are a PNG, and really are the ones hashed.
    let png = decode_wire_base64(image["image_base64"].as_str().unwrap());
    assert_eq!(
        &png[..8],
        b"\x89PNG\r\n\x1a\n",
        "the image is a PNG a human can open"
    );
    assert_eq!(
        quantick_control::canonical::raw_digest(&png),
        image["descriptor"]["image_digest"].as_str().unwrap()
    );

    // Every control the scene placed has a region, and every region is
    // inside the image.
    let controls = document["snapshot"]["scopes"]["scene.controls"]["value"]["controls"]
        .as_array()
        .unwrap();
    let placed = controls
        .iter()
        .filter(|control| control["bounds"].is_object())
        .map(|control| control["control_id"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        !placed.is_empty(),
        "the chart canvases record their rectangle, so some control is placed"
    );
    let regions = screenshot["control_regions"].as_array().unwrap();
    for control_id in &placed {
        let region = regions
            .iter()
            .find(|region| region["control_id"] == *control_id)
            .unwrap_or_else(|| panic!("{control_id} has no region in the image"));
        assert_eq!(
            region["within_image"], true,
            "{control_id} is reported outside the picture it was taken from"
        );
    }
    // And every control without one is named with the reason, so nothing
    // is silently missing.
    let unplaced = controls.len() - placed.len();
    assert_eq!(
        screenshot["controls_without_region"]
            .as_array()
            .unwrap()
            .len(),
        unplaced,
        "a control with no bounds is listed with its reason rather than dropped"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// A bundle big enough to need several chunks comes back over the socket.
///
/// The whole point of a retained resource is the bundle that does not fit
/// one response, and a chunk sized against the wrong ceiling makes every
/// such bundle permanently unreadable while every small-bundle test still
/// passes. The image here is deliberately incompressible, so the pages are
/// real pages rather than one chunk that happened to deflate.
#[test]
fn a_bundle_too_large_for_one_chunk_still_pages_back_over_the_socket() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(6);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-multi-chunk");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 8);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let response = capture_with_screenshot(
        &mut app,
        &ctx,
        &mut client,
        serde_json::json!({ "scopes": ["system.info"], "screenshot": true }),
        incompressible_screenshot(512, 512),
    );
    let manifest = success_result(&response).clone();
    assert!(
        manifest["chunk_count"].as_u64().unwrap() > 1,
        "the fixture is meant to need more than one chunk: {} bytes",
        manifest["encoded_bytes"]
    );

    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    assert_eq!(
        quantick_control::canonical::raw_digest(&bytes),
        manifest["content_digest"].as_str().unwrap(),
        "every page came back and the chunks reassemble to what was hashed"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 6: the hunt. A token, a home path, the trader's own drawing
/// text and a redacted configuration value are planted where each could
/// plausibly leak, and none of them is anywhere in the bundle or its
/// manifest.
#[test]
fn no_token_user_path_user_text_or_redacted_config_key_reaches_an_evidence_bundle() {
    const NOTE_CANARY: &str = "CANARYNOTEqzx";
    const PATH_CANARY: &str = "C:/Users/CANARYUSERqzx/Documents/quantick-trades";
    const COMMAND_CANARY: &str = "C:/Users/CANARYUSERqzx/bridge/quantick_bridge.py";
    /// The other place the trader's words live: the journal, which the
    /// bundle embeds a page of. The drawing note above is stripped by the
    /// projections; this one is only stripped by the bundle itself.
    const MARK_CANARY: &str = "CANARYMARKqzx";

    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    // A path the trader configured, and a command that names their home.
    app.config.paper.trades_dir = Some(PATH_CANARY.to_owned());
    app.config.metatrader.bridge_command = vec!["python".to_owned(), COMMAND_CANARY.to_owned()];
    app.config.metatrader.listen_addr = "192.168.7.31:9100".to_owned();
    run_frame(&mut app, &ctx);
    // The trader's own words on the chart.
    app.surfaces.drawing_chrome.set_pending_text_note(true);
    run_frame(&mut app, &ctx);
    {
        let tool = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
            .expect("the note tool holds text");
        let pane = app.active_tab_mut().drawing_pane_mut();
        assert_eq!(
            pane.drawings.items().len(),
            1,
            "the note hook placed the trader's object"
        );
        let drawing = pane
            .drawings
            .selected_mut()
            .expect("the note hook selects what it placed");
        // The same call the inline editor makes when the trader types.
        tool.set_inline_text(drawing.payload.as_mut(), NOTE_CANARY.to_owned());
    }
    run_frame(&mut app, &ctx);
    // And the trader's own words in the *journal*, through the hotkey's
    // own action — the page a bundle embeds carries these verbatim, so
    // this is the leak the drawing canary above cannot find.
    app.pending_control_mark = Some(MARK_CANARY.to_owned());
    run_frame(&mut app, &ctx);

    let directory = gateway_test_directory("evidence-redaction");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    let descriptor_path = enable_test_gateway(&mut app, &ctx, &directory, 8);
    // The one real secret this process holds: the bearer token the
    // descriptor publishes for this connection.
    let token = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(&descriptor_path).unwrap(),
    )
    .unwrap()["bearer_token"]
        .as_str()
        .expect("the descriptor carries the connection token")
        .to_owned();
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    // The drawings scope is where the trader's own words would leak if
    // anything did; the cursor scope is the one that resolves a target.
    let mut scopes = EVIDENCE_TEST_SCOPES.to_vec();
    scopes.push("analysis.drawings");
    scopes.push("interaction.cursor");
    let response = capture_with_screenshot(
        &mut app,
        &ctx,
        &mut client,
        serde_json::json!({ "scopes": scopes, "screenshot": true }),
        test_screenshot(64, 48),
    );
    let manifest = success_result(&response).clone();
    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);

    let haystacks = [
        ("the bundle", String::from_utf8(bytes.clone()).unwrap()),
        ("the manifest", manifest.to_string()),
    ];
    for (where_it_is, haystack) in &haystacks {
        for (what, canary) in [
            ("the trader's note", NOTE_CANARY),
            ("the trader's mark note", MARK_CANARY),
            ("a configured path", PATH_CANARY),
            ("a bridge command", COMMAND_CANARY),
            ("the user name in a path", "CANARYUSERqzx"),
            ("the bind address", "192.168.7.31"),
            ("the connection's token", token.as_str()),
        ] {
            assert!(!haystack.contains(canary), "{what} reached {where_it_is}");
        }
    }

    // What it does carry instead: the fact that the settings exist.
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let configuration = &document["configuration"];
    assert_eq!(configuration["paper"]["trades_dir_configured"], true);
    assert_eq!(
        configuration["metatrader"]["bridge_command_configured"],
        true
    );
    assert_eq!(configuration["metatrader"]["listen_port"], 9100);
    assert_eq!(
        configuration["metatrader"]["listen_host_is_loopback"],
        false
    );
    let redacted = configuration["redacted_keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|key| key.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(redacted.contains(&"paper.trades_dir"));
    assert!(redacted.contains(&"metatrader.bridge_command"));
    assert!(redacted.contains(&"metatrader.listen_addr.host"));

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 7's other half, and the tier's own boundary: the scopes a
/// bundle aggregates are the scopes it needed, so a connection without the
/// evidence scope cannot capture and a bundle cannot launder a scope the
/// grant refuses one call earlier.
#[test]
fn evidence_capture_is_refused_without_its_own_scope_and_cannot_launder_another() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-scopes");
    // The safe default grant: everything an observer reads, and neither
    // of the two the evidence tier adds.
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &gateway_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let refused = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "evidence.capture",
        serde_json::json!({ "scopes": ["system.info"] }),
    );
    assert_eq!(
        response_error(&refused).code.as_str(),
        quantick_control::error::codes::PERMISSION_DENIED,
        "the evidence scope is off by default and the capability is out of reach"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 5's fixture: the `QUANTICK_CONTROL_EVIDENCE` hook captures a
/// bundle from a launch through the very read a connected client calls,
/// and the bundle it retains is readable back through the control plane.
///
/// This is what lets a validation skill assert against live structured
/// state before the cockpit tier gives it an action to set the fixture up
/// with. The scopes come from the registry, so a module registered
/// tomorrow is in the capture tomorrow.
#[test]
fn the_evidence_launch_hook_captures_through_the_same_read_a_client_calls() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence");
    app.pending_control_evidence = Some("all".to_owned());
    run_frame(&mut app, &ctx);

    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .retained_evidence_for_test(),
        1,
        "the hook captured one bundle without a client on the socket"
    );
    assert!(
        app.pending_control_evidence.is_none(),
        "and it fires once, not on every frame"
    );

    // Readable back the same way: the store is one store, and the read is
    // the same registered capability a client would invoke.
    let mut access = app
        .control_access
        .take()
        .expect("control access is installed");
    let scopes = access.readable_scopes();
    assert!(
        scopes
            .iter()
            .any(|scope| scope.as_str() == "scene.controls"),
        "`all` means every scope the grant reaches, taken from the registry"
    );
    let captured = access
        .invoke_local_read(
            &app,
            "evidence.capture",
            serde_json::json!({ "scopes": ["system.info"] }),
        )
        .expect("the read is registered and granted");
    let page = access
        .invoke_local_read(
            &app,
            "evidence.read",
            serde_json::json!({ "evidence_id": captured["evidence_id"] }),
        )
        .expect("what was captured is readable");
    app.control_access = Some(access);
    assert_eq!(page["content_digest"], captured["content_digest"]);
    assert_eq!(page["page"]["has_more"], false);
}

/// A capture that asks for a picture waits one frame for it rather than
/// answering without one, and the image it finally gets belongs to the
/// scene captured beside it.
#[test]
fn a_capture_that_wants_an_image_waits_for_the_frame_instead_of_answering_blind() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(6);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-await-image");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    let request_id = client
        .send(
            "evidence.capture",
            serde_json::json!({ "scopes": ["scene.controls"], "screenshot": true }),
        )
        .expect("the request is sent");
    // Frames pass and the capture does not answer: it is waiting for the
    // window, which a headless context never rasterises on its own.
    let mut waited = 0;
    for _ in 0..PARK_WAIT_FRAMES {
        run_frame(&mut app, &ctx);
        waited = app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .awaiting_screenshot_for_test();
        if waited > 0 {
            break;
        }
    }
    assert_eq!(waited, 1, "the capture parked instead of answering blind");

    let mut access = app
        .control_access
        .take()
        .expect("control access is installed");
    access.publish_screenshot_for_test(&mut app, test_screenshot(320, 200));
    app.control_access = Some(access);
    for _ in 0..REPLY_WAIT_FRAMES {
        run_frame(&mut app, &ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, request_id);
    let manifest = success_result(&response);
    assert_eq!(
        manifest["screenshot"]["capture_revision"], manifest["capture_revision"],
        "the image that arrived belongs to the capture that waited for it"
    );
    assert_eq!(
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .awaiting_screenshot_for_test(),
        0
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// A bundle always carries a page of the journal and the effective
/// configuration, so it always requires the scopes those belong to —
/// whatever scopes were named.
///
/// This is the aggregation hole the tier exists not to have: without it, a
/// connection refused `observe.events` reads the journal by asking for a
/// bundle of `system.info`, and because the manifest would not record the
/// scope either, the read-time recheck could never notice.
#[test]
fn a_bundle_requires_the_scopes_it_always_carries_however_few_were_named() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-always-carried");
    // Everything the evidence tier needs *except* the journal.
    grant_annotate_for_test(
        &mut app,
        "observe.system,observe.workspace,observe.market,observe.chart,observe.evidence",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut scopes = gateway_test_scopes();
    scopes.remove(&quantick_control::id::PermissionId::new("observe.events").unwrap());
    scopes.insert(quantick_control::id::PermissionId::new("observe.evidence").unwrap());
    let options = quantick_control_local::client::ConnectOptions::observer(
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    );
    let mut client = quantick_control_local::client::discover_in(&directory, &options)
        .unwrap()
        .select(None)
        .unwrap();

    let refused = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "evidence.capture",
        serde_json::json!({ "scopes": ["system.info"] }),
    );
    let error = response_error(&refused);
    assert_eq!(
        error.code.as_str(),
        quantick_control::error::codes::SCOPE_DENIED,
        "a bundle carrying the journal needs the journal's own scope"
    );
    assert!(
        error.context.details.as_ref().unwrap()["missing_permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "observe.events"),
        "and the refusal names it: {:?}",
        error.context.details
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// Criterion 2, sharpened: the events a bundle carries are the ones around
/// the capture, not the oldest the journal still holds.
///
/// A session that has run for a while has thousands of events; handing
/// back the first two hundred and fifty-six would be the application
/// starting up, and the moment the bundle was taken to explain would be
/// pages away.
#[test]
fn a_bundle_carries_the_events_around_the_capture_not_the_oldest_it_holds() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-recent-events");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &evidence_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    // More events than one page holds, through the journal the gateway
    // owns — the same door the frame emitter writes through.
    let limit = 4_usize;
    {
        let access = app
            .control_access
            .as_mut()
            .expect("control access is installed");
        for index in 0..(limit * 8) {
            access.journal_mut().record(
                crate::control::journal_test_event(index),
                i64::try_from(index).unwrap(),
            );
        }
    }
    let newest = app
        .control_access
        .as_ref()
        .expect("control access is installed")
        .journal()
        .bounds()
        .next_sequence
        .get();

    let response = remote_call(
        &mut app,
        &ctx,
        &mut client,
        "evidence.capture",
        serde_json::json!({ "scopes": ["system.info"], "event_limit": limit }),
    );
    let manifest = success_result(&response).clone();
    let bytes = read_evidence_bundle(&mut app, &ctx, &mut client, &manifest);
    let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    let events = document["events"]["events"].as_array().unwrap();
    assert_eq!(events.len(), limit);
    let last = events.last().unwrap()["sequence"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(
        last,
        newest - 1,
        "the page ends at the newest event the journal held, not at its oldest"
    );
    assert_eq!(
        document["events"]["next_cursor"]["next_sequence"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        newest,
        "and the cursor carries on from the capture instant"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

/// The tier's rate class, proved rather than asserted: with nothing asked
/// for, the evidence path costs the frame nothing at all — no store is
/// touched, no image is requested, no capture is built.
#[test]
fn evidence_costs_the_frame_nothing_until_a_client_asks_for_it() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(8);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("evidence-idle");
    grant_annotate_for_test(&mut app, "all-reads,observe.evidence,observe.screenshot");
    enable_test_gateway(&mut app, &ctx, &directory, 4);

    for _ in 0..30 {
        run_frame(&mut app, &ctx);
    }
    let access = app
        .control_access
        .as_ref()
        .expect("control access is installed");
    assert_eq!(
        access.awaiting_screenshot_for_test(),
        0,
        "no capture is waiting, so no frame was ever asked to rasterise"
    );

    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn observer_schemas_are_versioned_valid_and_ui_framework_free() {
    let documents = crate::control::schema_catalog::documents();
    // Every published wire type has a committed document, so a breaking
    // change shows up as a diff in review (contract §6). The count is
    // here to make an accidental *removal* visible too.
    assert_eq!(documents.len(), 46);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("schemas/control");
    let update =
        std::env::var_os("QUANTICK_UPDATE_CONTROL_SCHEMAS").is_some_and(|value| value == "1");
    if update {
        std::fs::create_dir_all(&root).unwrap();
    }
    for document in documents {
        quantick_control::schema::validate_schema(&document.schema)
            .unwrap_or_else(|error| panic!("{} is invalid: {error}", document.file_name));
        let json = format!(
            "{}\n",
            serde_json::to_string_pretty(&document.schema).unwrap()
        );
        assert!(
            !json.to_ascii_lowercase().contains("egui"),
            "{} leaks a UI implementation type",
            document.file_name
        );
        let path = root.join(document.file_name);
        if update {
            std::fs::write(&path, &json).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "read {} ({error}); regenerate observer schemas with QUANTICK_UPDATE_CONTROL_SCHEMAS=1",
                    path.display()
                )
            });
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::from_str::<serde_json::Value>(&committed).unwrap(),
            "regenerate and review {}",
            document.file_name
        );
    }
}

#[test]
fn observer_capability_catalog_is_registry_derived_and_versioned() {
    let catalog = crate::control::schema_catalog::capability_catalog();
    assert_eq!(catalog["catalog_version"], 1);
    assert_eq!(catalog["profile_id"], "observer");
    let capabilities = catalog["capabilities"].as_array().unwrap();
    // Nine observer reads plus the annotate tier's registered actions.
    assert_eq!(
        capabilities.len(),
        9 + crate::control::registered_action_count()
    );
    // Every observe-effect capability is read-only; everything else is an
    // action of a write tier — discoverable to any client, reachable only
    // under a profile the trader granted.
    //
    // The list of write tiers is named rather than open on purpose. A
    // capability arriving under an effect nobody added here is a capability
    // whose consent text nobody wrote, and the trader has no surface on
    // which to find it. Adding an effect is a deliberate act, and this is
    // where it is acknowledged.
    for capability in capabilities {
        if capability["effect"] == "observe" {
            assert_eq!(capability["read_only"], true, "{}", capability["id"]);
        } else {
            assert!(
                capability["effect"] == "annotate"
                        || capability["effect"] == "notify"
                        || capability["effect"] == "cockpit"
                        // Acknowledged deliberately, as the paragraph above
                        // asks: the one effect in this contract that permits
                        // destruction. Its consent text is the
                        // `cockpit.recover` permission descriptor, which says
                        // in the trader's words that it closes an open paper
                        // position and disarms every strategy, and which is
                        // marked sensitive so it is off until ticked.
                        || capability["effect"] == "cockpit.recover"
                        // The trade tier. Discoverable like the rest, and
                        // reachable by nothing a trader can currently grant:
                        // its permission's only ceiling is the `trader`
                        // profile, which the access panel does not offer.
                        || capability["effect"] == "trade",
                "{} has an unexpected effect {}",
                capability["id"],
                capability["effect"]
            );
            assert_eq!(capability["read_only"], false, "{}", capability["id"]);
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("schemas/control");
    let path = root.join("observer-capability-catalog-v1.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&catalog).unwrap());
    let update =
        std::env::var_os("QUANTICK_UPDATE_CONTROL_SCHEMAS").is_some_and(|value| value == "1");
    if update {
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, &json).unwrap();
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read {} ({error}); regenerate observer schemas with QUANTICK_UPDATE_CONTROL_SCHEMAS=1",
            path.display()
        )
    });
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::from_str::<serde_json::Value>(&committed).unwrap(),
        "regenerate and review {}",
        path.display()
    );
}

/// Reproducible application-frame comparison for control-plane PRs.
///
/// This is ignored because timing assertions do not belong in CI. Run the
/// exact test on `origin/main` and the candidate branch on the same host,
/// alternating order between samples. The normal frame never calls the
/// observer registry; this workload makes any accidental docking cost
/// visible while live batches are continuously drained and painted.
#[test]
#[ignore = "manual shared-host APP_HEALTH_SUMMARY comparison"]
fn control_idle_dense_replay_benchmark() {
    const WARMUP_FRAMES: u64 = 30;
    const MEASURED_FRAMES: u64 = 600;
    const TRADES_PER_FRAME: u64 = 64;

    let ctx = egui::Context::default();
    let (mut app, events, _commands, _book) = test_app();
    app.active_tab_mut().flow_pane.tick_n = 16;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    events
        .try_send(FeedEvent::Backfilled((1..=8_000).map(trade).collect()))
        .unwrap();
    app.active_tab_mut().drain_feed();

    let mut next_trade = 8_001;
    for _ in 0..WARMUP_FRAMES {
        let after = next_trade + TRADES_PER_FRAME;
        events
            .try_send(FeedEvent::LiveBatch(
                (next_trade..after).map(trade).collect(),
            ))
            .unwrap();
        next_trade = after;
        run_frame(&mut app, &ctx);
    }

    let started = Instant::now();
    let mut frame_cpu_ms = Vec::with_capacity(MEASURED_FRAMES as usize);
    for _ in 0..MEASURED_FRAMES {
        let after = next_trade + TRADES_PER_FRAME;
        events
            .try_send(FeedEvent::LiveBatch(
                (next_trade..after).map(trade).collect(),
            ))
            .unwrap();
        next_trade = after;
        let frame_started = Instant::now();
        run_frame(&mut app, &ctx);
        frame_cpu_ms.push(frame_started.elapsed().as_secs_f64() * 1_000.0);
    }
    let elapsed = started.elapsed().as_secs_f64();
    frame_cpu_ms.sort_by(f64::total_cmp);
    let average = frame_cpu_ms.iter().sum::<f64>() / frame_cpu_ms.len() as f64;
    let p99_index = (frame_cpu_ms.len() * 99).div_ceil(100).saturating_sub(1);
    let p99 = frame_cpu_ms[p99_index];
    let worst = *frame_cpu_ms.last().unwrap();
    let trades_per_second = MEASURED_FRAMES as f64 * TRADES_PER_FRAME as f64 / elapsed;
    println!(
        "CONTROL_IDLE_DENSE_REPLAY {{\"frame_cpu_ms\":{average:.6},\"frame_p99_ms\":{p99:.6},\"frame_worst_ms\":{worst:.6},\"feed_arrival_ms\":{:?},\"trades_per_s\":{trades_per_second:.3},\"frames\":{MEASURED_FRAMES},\"trades_per_frame\":{TRADES_PER_FRAME}}}",
        app.active_tab().trade_arrival_ms()
    );
}
