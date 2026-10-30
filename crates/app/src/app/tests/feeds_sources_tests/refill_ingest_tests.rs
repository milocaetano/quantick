//! A reconnect refill charted through the real tab: the hole must read as
//! one continuous load of the same tape, never as a second backfill.

use super::*;

const NOW_MS: i64 = 90_000;

fn tape(ids: std::ops::RangeInclusive<u64>) -> Vec<quantick_engine::Trade> {
    ids.map(trade).collect()
}

fn drain(app: &mut QuantickApp) {
    let tab_id = app.tabs.active_id();
    app.active_tab_mut()
        .drain_feed_with_clock(tab_id, || NOW_MS);
}

/// How the flow pane splits its prints and bars into history and live.
fn labelling(app: &QuantickApp) -> (usize, Option<usize>) {
    let state = &app.active_tab().flow_pane.state;
    (state.backfill_trade_count(), state.backfill_boundary())
}

/// Finding (review round 2, B1): refilled prints went through the one-shot
/// backfill path, which labels every print on the chart history — a status
/// of 900 live prints read "live 0, backfilled all", the backfill divider
/// jumped to the right edge and the FLOW frame was dropped. The feed sends a
/// (possibly empty) refill batch on every reconnect, so a reconnect with no
/// hole did it too.
#[test]
fn a_refill_keeps_the_live_bars_live_with_a_hole_or_without_one() {
    for hole in [true, false] {
        let (mut app, _notices, (events, _book)) = test_app_with_notices();
        events
            .blocking_send(FeedEvent::Backfilled(tape(1..=100)))
            .unwrap();
        events
            .blocking_send(FeedEvent::LiveBatch(tape(101..=200)))
            .unwrap();
        drain(&mut app);
        let before = labelling(&app);
        assert_eq!(before, (100, Some(2)), "two history bars, then live");

        let (events, _commands) = reconnect_onto_fresh_feed(&mut app);
        let refilled = if hole { tape(201..=260) } else { Vec::new() };
        events.blocking_send(FeedEvent::Refilled(refilled)).unwrap();
        events.blocking_send(FeedEvent::Live(trade(261))).unwrap();
        drain(&mut app);

        assert_eq!(
            labelling(&app),
            before,
            "the live bars stay live after a refill (hole: {hole})"
        );
        let tab = app.active_tab();
        assert_eq!(
            tab.flow_pane.state.trades().len(),
            if hole { 261 } else { 201 }
        );
        assert_eq!(tab.live_trades, 101, "only the prints that arrived live");
    }
}

/// Finding (review round 2, B2): a refill left the tape's newest print
/// where the chart stopped, so with no live print yet the stall readout
/// called the feed silent for the length of the hole and offered Reload,
/// and a second Reconnect resumed from before the hole.
#[test]
fn a_refill_moves_the_tape_to_its_last_print() {
    let (mut app, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(tape(1..=100)))
        .unwrap();
    drain(&mut app);

    let (events, _commands) = reconnect_onto_fresh_feed(&mut app);
    events
        .blocking_send(FeedEvent::Refilled(tape(101..=200)))
        .unwrap();
    drain(&mut app);

    let last_ms = trade(200).timestamp_ms;
    let tab = app.active_tab();
    assert_eq!(tab.latest_trade_ms, Some(last_ms));
    assert_eq!(tab.tape_age_at(last_ms + 1_000), Some(1_000));

    let _feed = reconnect_onto_fresh_feed(&mut app);
    assert_eq!(
        app.active_tab().resume_floor_ms,
        Some(last_ms),
        "the next reconnect resumes after the refilled prints"
    );
}

/// Finding (review round 2, B3): refilled prints never reached the
/// order-flow layer, so FLOW was blank across a refilled hole while one
/// continuous load of the same tape drew it.
#[test]
fn the_flow_layer_across_a_refilled_hole_matches_a_continuous_load() {
    let whole = tape(1..=300);
    let flow = |app: &mut QuantickApp| {
        let tape = app.active_tab_mut().tape_mut();
        tape.flush_for_test();
        let health = tape.health();
        (health.aggression_count, health.tape_age)
    };
    let bars = |app: &QuantickApp| app.active_tab().flow_pane.state.bars().to_vec();

    let (mut continuous, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(whole.clone()))
        .unwrap();
    drain(&mut continuous);

    let (mut refilled, _notices, (events, _book)) = test_app_with_notices();
    events
        .blocking_send(FeedEvent::LiveBatch(whole[..100].to_vec()))
        .unwrap();
    drain(&mut refilled);
    let (events, _commands) = reconnect_onto_fresh_feed(&mut refilled);
    events
        .blocking_send(FeedEvent::Refilled(whole[100..250].to_vec()))
        .unwrap();
    events
        .blocking_send(FeedEvent::LiveBatch(whole[250..].to_vec()))
        .unwrap();
    drain(&mut refilled);

    let expected = flow(&mut continuous);
    assert!(expected.0 > 0, "the test chart draws the FLOW layer");
    assert_eq!(flow(&mut refilled), expected, "FLOW across the hole");
    assert_eq!(bars(&refilled), bars(&continuous), "the bars too");
}
