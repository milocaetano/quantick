use super::*;
use quantick_feed::replay::test_support as replay_support;
use quantick_layers::OrderflowSwitch;

fn enable_tape_only(app: &mut QuantickApp) {
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        OrderflowSwitch::TapeOnly,
        true,
    );
}

#[test]
fn an_ordinary_lane_never_receives_the_new_clock() {
    let (mut app, _events, _commands, _book) = test_app();
    let print = trade(1);
    app.active_tab_mut().ingest_live_trade_at(&print, 10_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), None);
    app.active_tab_mut().update_tape_clock_at(20_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), None);
}

#[test]
fn a_tape_only_live_frame_keeps_moving_without_new_events_and_resets_on_source_change() {
    let (mut app, _events, _commands, _book) = test_app();
    enable_tape_only(&mut app);
    let print = trade(1);
    app.active_tab_mut().ingest_live_trade_at(&print, 10_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    assert_eq!(
        app.active_tab().tape().lane_now_ms(),
        Some(print.timestamp_ms)
    );
    app.active_tab_mut().update_tape_clock_at(10_016);
    assert_eq!(
        app.active_tab().tape().lane_now_ms(),
        Some(print.timestamp_ms + 16)
    );
    app.active_tab_mut().tape_mut().reset_for_symbol("WINV26");
    assert_eq!(app.active_tab().tape().lane_now_ms(), None);
}

#[test]
fn replay_clock_uses_applied_prints_to_bound_playhead_motion_and_pause() {
    let text = "# quantick-replay 1\n# symbol=WINV26\n# timezone=UTC\n# side_source=venue_flags\nDate,Time,Price,Bid,Ask,Volume,Side\n2026-09-22,09:00:00.000,187000,186995,187005,10,B\n2026-09-22,09:00:10.000,187005,187000,187010,20,S\n2026-09-22,09:00:20.000,187010,187005,187015,30,B\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINV26/2026-09-22.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("clock fixture is a valid recording");
    let first = session.trades[0].clone();
    let second = session.trades[1].clone();
    let start = first.timestamp_ms;
    let link = replay_support::detached_link(session);
    let status = std::sync::Arc::clone(&link.status);
    let (mut app, _events, _commands, _book) = test_app();
    enable_tape_only(&mut app);
    app.active_tab_mut().link.replay = Some(link);
    app.active_tab_mut().flow_pane.ingest_backfill(&[first]);

    replay_support::set_position_ms(&status, start + 1_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), Some(start + 1_000));
    // No playhead movement while paused; wall time must not move the tape.
    app.active_tab_mut().update_tape_clock_at(50_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), Some(start + 1_000));
    // The playhead includes its speed multiplier already.
    replay_support::set_position_ms(&status, start + 6_000);
    app.active_tab_mut().update_tape_clock_at(51_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), Some(start + 6_000));
    // A released batch has not reached the application yet.
    replay_support::set_position_ms(&status, start + 19_000);
    app.active_tab_mut().update_tape_clock_at(52_000);
    assert_eq!(app.active_tab().tape().lane_now_ms(), Some(start + 10_000));
    app.active_tab_mut().ingest_live_trade_at(&second, 52_000);
    app.active_tab_mut().update_tape_clock_at(52_016);
    assert_eq!(app.active_tab().tape().lane_now_ms(), Some(start + 19_000));
}

/// A quiet tape's window stops once the last print has walked 45 % of it, a minute
/// on as much as ten seconds; the clock under it runs on, so a link that delivers
/// again and a first print both land on the time it really is. A replay's link
/// never holds it, and what the worker hears is the same hold.
#[test]
fn a_tape_with_nothing_coming_holds_its_window_until_the_link_or_a_print_frees_it() {
    use quantick_chart::connection_state::Link;
    let (mut app, _events, _commands, _book) = test_app();
    enable_tape_only(&mut app);
    let print = trade(1);
    let at = print.timestamp_ms;
    let tab = app.active_tab_mut();
    tab.ingest_live_trade_at(&print, 10_000);
    tab.update_tape_clock_at(10_000);
    let window = tab.tape().live_lane_window_ms(tab.flow_pane.state.bars());
    let cap = at + window * 45 / 100;
    let hold = |tab: &mut crate::tab::Tab, link| {
        let pane = &mut tab.flow_pane;
        let view = pane.orderflow.as_mut().unwrap();
        view.hold_tape_window(Some(link), &pane.state);
    };

    tab.update_tape_clock_at(11_000);
    hold(tab, Link::Quiet);
    assert_eq!(tab.tape().lane_now_ms(), Some(at + 1_000), "it walks there");
    let far = 10_000 + u64::try_from(window).unwrap() * 4;
    tab.update_tape_clock_at(far);
    for link in [Link::Quiet, Link::Attempting, Link::Offline] {
        hold(tab, link);
        assert_eq!(tab.tape().lane_now_ms(), Some(cap), "{link:?}");
        assert_eq!(tab.tape().cached_live_end_ms(), Some(cap), "{link:?}");
    }
    let clock = at + window * 4;
    for link in [Link::Live, Link::NoTransport] {
        hold(tab, link);
        assert_eq!(tab.tape().lane_now_ms(), Some(clock), "{link:?}");
        hold(tab, Link::Quiet);
    }
    let next = quantick_engine::Trade {
        timestamp_ms: clock,
        ..trade(2)
    };
    tab.ingest_live_trade_at(&next, i64::try_from(far).unwrap());
    tab.update_tape_clock_at(far);
    hold(tab, Link::Quiet);
    assert_eq!(
        tab.tape().lane_now_ms(),
        Some(clock),
        "the first print frees it"
    );
}

/// What a time pane's price tag reads with the tab's market clock, as the
/// canvas hands it to every pane.
fn time_left(app: &QuantickApp) -> Option<i64> {
    let tab = app.active_tab();
    let clock_ms = tab.tape().market_now_ms();
    match tab.flow_pane.state.bar_progress_at(clock_ms)? {
        quantick_chart::bar_progress::BarProgressReading::Remaining { remaining_ms, .. } => {
            Some(remaining_ms)
        }
        other => panic!("a time pane counts down, not {other:?}"),
    }
}

/// A time bar counts down on the market clock with no tape layer on: live
/// it runs on the host's time between prints, replayed it stands on the
/// playhead and stops while the replay is paused.
#[test]
fn the_time_countdown_reads_the_market_clock_live_and_replayed() {
    let (mut app, _events, _commands, _book) = test_app();
    app.active_tab_mut()
        .flow_pane
        .set_spec(quantick_engine::BarSpec::Time(60_000));
    let print = trade(1);
    app.active_tab_mut().ingest_live_trade_at(&print, 10_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    assert_eq!(time_left(&app), Some(60_000 - print.timestamp_ms));
    app.active_tab_mut().update_tape_clock_at(17_000);
    assert_eq!(
        time_left(&app),
        Some(60_000 - print.timestamp_ms - 7_000),
        "no print came"
    );

    let text = "# quantick-replay 1\n# symbol=WINV26\n# timezone=UTC\n# side_source=venue_flags\nDate,Time,Price,Bid,Ask,Volume,Side\n2026-09-22,09:00:05.000,187000,186995,187005,10,B\n2026-09-22,09:00:50.000,187005,187000,187010,20,S\n";
    let session = quantick_replay::Session::from_text(
        std::path::Path::new("WINV26/2026-09-22.csv"),
        text,
        quantick_replay::ParseOptions::default(),
    )
    .expect("clock fixture is a valid recording");
    let first = session.trades[0].clone();
    let link = replay_support::detached_link(session);
    let status = std::sync::Arc::clone(&link.status);
    let (mut app, _events, _commands, _book) = test_app();
    app.active_tab_mut()
        .flow_pane
        .set_spec(quantick_engine::BarSpec::Time(60_000));
    app.active_tab_mut().link.replay = Some(link);
    app.active_tab_mut()
        .flow_pane
        .ingest_backfill(std::slice::from_ref(&first));
    replay_support::set_position_ms(&status, first.timestamp_ms + 20_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    assert_eq!(time_left(&app), Some(35_000), "09:00:25 of a 09:00 bar");
    app.active_tab_mut().update_tape_clock_at(40_000);
    assert_eq!(
        time_left(&app),
        Some(35_000),
        "a paused replay's countdown stands"
    );
}

/// The market clock `chart.summary` reads a pane's time rule on, per tab.
fn summary_clocks(app: &QuantickApp) -> Vec<(u64, Option<i64>)> {
    use quantick_control_handlers::chart::ChartPort as _;
    let port: &dyn crate::app::control_host::ControlPort = app;
    let mut clocks = Vec::new();
    port.visit_chart_panes(&mut |entry, _| clocks.push((entry.tab_id, entry.market_clock_ms)));
    clocks
}

/// Only the tab on screen has its market clock advanced each frame, so a
/// tab behind it reads its time rule on its trades' own time rather than on
/// a clock stopped the frame it was last drawn.
#[test]
fn a_background_tab_summary_reads_its_trades_time_not_a_stopped_clock() {
    let (mut app, _events, _commands, _book) = test_app();
    let first = app.runtime.tabs.active_id();
    app.active_tab_mut()
        .flow_pane
        .set_spec(quantick_engine::BarSpec::Time(60_000));
    app.active_tab_mut().ingest_live_trade_at(&trade(1), 10_000);
    app.active_tab_mut().update_tape_clock_at(10_000);
    app.active_tab_mut().update_tape_clock_at(17_000);
    let on_screen = summary_clocks(&app);
    assert!(
        on_screen
            .iter()
            .any(|&(tab, clock)| tab == first && clock.is_some()),
        "{on_screen:?}"
    );

    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "SECOND".to_owned(), None);
    assert_ne!(
        app.runtime.tabs.active_id(),
        first,
        "the new tab is on screen"
    );
    let behind = summary_clocks(&app);
    let first_clocks: Vec<_> = behind.iter().filter(|(tab, _)| *tab == first).collect();
    assert!(!first_clocks.is_empty());
    assert!(
        first_clocks.iter().all(|(_, clock)| clock.is_none()),
        "{behind:?}"
    );

    use quantick_control_handlers::chart::ChartPort as _;
    let port: &dyn crate::app::control_host::ControlPort = &app;
    let pane = app.runtime.tabs.by_id(first).unwrap().flow_pane.id;
    let mut clock = Some(0);
    port.visit_chart_pane(first, pane, &mut |entry| clock = entry.market_clock_ms)
        .unwrap();
    assert_eq!(clock, None, "the one-pane read agrees");
}
