//! Daily, weekly and monthly time panes: the candle base they ask for, and
//! what they fold from it.
use super::*;
use quantick_engine::time_bucket::{DAY_MS, WEEK_MS};

/// Every queued candle request as `(interval_ms, span_ms, slice_ms)`.
fn drain_ohlcv_bases(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<(i64, i64, Option<i64>)> {
    let mut asked = Vec::new();
    while let Ok(command) = commands.try_recv() {
        if let FeedCommand::FetchOhlcv {
            interval_ms,
            span_ms,
            slice_ms,
            ..
        } = command
        {
            asked.push((interval_ms, span_ms, slice_ms));
        }
    }
    asked
}

/// Daily venue candles for the `count` days before the fixture's trades,
/// which start on 1970-01-01.
fn daily_history(count: i64) -> Vec<quantick_engine::Bar> {
    (-count..0)
        .map(|day| {
            let open_time = day * DAY_MS;
            quantick_engine::Bar {
                open_time,
                close_time: open_time + DAY_MS - 1,
                ..venue_candle(0, day.rem_euclid(5))
            }
        })
        .collect()
}

fn set_time_pane(app: &mut QuantickApp, interval_ms: i64) {
    let tab = app.active_tab_mut();
    tab.pane_mut(PaneSide::Time(0))
        .spec
        .retain(crate::state::BarSpec::Time(interval_ms));
    tab.apply_spec_changes();
    tab.apply_spec_changes();
}

fn answer(events: &mpsc::Sender<FeedEvent>, interval_ms: i64, bars: Vec<quantick_engine::Bar>) {
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms,
            bars,
            slice: quantick_feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
}

/// A pane moved to 1w stops folding minutes: it asks the venue for years of
/// daily candles in one reply, and cuts them into weeks that open on Monday.
#[test]
fn a_weekly_pane_asks_for_daily_candles_and_folds_them_into_monday_weeks() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    let opening = drain_ohlcv_bases(&mut commands);
    assert!(
        opening
            .iter()
            .all(|(interval, ..)| *interval == quantick_feed::OHLCV_BASE_INTERVAL_MS),
        "a minute pane asks for minutes: {opening:?}"
    );
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(120),
    );
    app.drain_tabs();

    set_time_pane(&mut app, WEEK_MS);
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_bases(&mut commands),
        vec![(
            quantick_feed::OHLCV_DAILY_INTERVAL_MS,
            quantick_feed::DAILY_HISTORY_SPAN_MS,
            None
        )],
        "one unsliced request for five years of days"
    );

    // 35 days back from Thursday 1970-01-01: weeks open on the Mondays
    // 1969-11-24 (four of its days), then five whole weeks to 1969-12-29 —
    // the week the fixture's own trades open, which the trades own.
    answer(
        &events,
        quantick_feed::OHLCV_DAILY_INTERVAL_MS,
        daily_history(35),
    );
    app.drain_tabs();
    let pane = app.active_tab().pane(PaneSide::Time(0));
    assert_eq!(pane.seam_slot(), 5, "five venue weeks before the seam week");
    assert_eq!(app.active_tab().venue_candles_held(), 35);
}

/// Back to an intraday interval, the daily base cannot fold to it: it goes,
/// and the minutes are asked for again.
#[test]
fn leaving_a_daily_pane_for_minutes_fetches_minutes_again() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_bases(&mut commands);
    answer(&events, quantick_feed::OHLCV_BASE_INTERVAL_MS, Vec::new());
    app.drain_tabs();
    set_time_pane(&mut app, DAY_MS);
    app.drain_tabs();
    drain_ohlcv_bases(&mut commands);
    answer(
        &events,
        quantick_feed::OHLCV_DAILY_INTERVAL_MS,
        daily_history(10),
    );
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 10);

    set_time_pane(&mut app, 5 * quantick_feed::OHLCV_BASE_INTERVAL_MS);
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        0,
        "no daily candle stands under a five-minute chart"
    );
    app.drain_tabs();
    let asked = drain_ohlcv_bases(&mut commands);
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].0, quantick_feed::OHLCV_BASE_INTERVAL_MS);
    assert_eq!(asked[0].1, quantick_feed::TIME_HISTORY_SPAN_MS);
}

/// A provider without daily candles — a bridge that pushes only M1 —
/// answers the daily request in minutes, tagged as minutes. The day pane
/// folds them, and the tab does not keep asking for what it cannot get.
#[test]
fn minutes_served_for_a_daily_request_still_fold_into_days() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_bases(&mut commands);
    answer(&events, quantick_feed::OHLCV_BASE_INTERVAL_MS, Vec::new());
    app.drain_tabs();
    set_time_pane(&mut app, DAY_MS);
    app.drain_tabs();
    drain_ohlcv_bases(&mut commands);

    // Three days of minutes before the epoch.
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(3 * 1_440),
    );
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 3);
    app.drain_tabs();
    assert!(
        drain_ohlcv_bases(&mut commands).is_empty(),
        "the minute answer stands; nothing is asked again"
    );
}

/// The named calls reach the same calendar intervals the chips and the
/// quick switch do, and refuse one no control could produce.
#[test]
fn the_control_plane_sets_and_refuses_calendar_intervals() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = history_app(&ctx);
    let month = quantick_engine::time_bucket::CALENDAR_MONTH_MS;
    let changed = app
        .control_action(
            "layout.pane.set_bar_spec",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "pane": "1", "spec": "time:1mo" }),
        )
        .unwrap();
    assert_eq!(changed["changed"], true);
    app.active_tab_mut().apply_spec_changes();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).state.spec(),
        &BarSpec::Time(month)
    );
    let weekly = app
        .control_action(
            "layout.pane.set_interval",
            2,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "pane": "1", "interval_ms": WEEK_MS }),
        )
        .unwrap();
    assert_eq!(weekly["changed"], true, "{weekly}");
    let refused = app
        .control_action(
            "layout.pane.set_bar_spec",
            1,
            crate::control::ActionOrigin::Human,
            serde_json::json!({ "pane": "1", "spec": "time:13mo" }),
        )
        .expect_err("thirteen months is no chart");
    assert!(refused.message.contains("13mo"), "{}", refused.message);
}
