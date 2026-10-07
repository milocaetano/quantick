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
    app.active_tab_mut().replay = Some(link);
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
