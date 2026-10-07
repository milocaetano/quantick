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
    set_pane(app, PaneSide::Time(0), interval_ms);
}

fn set_pane(app: &mut QuantickApp, side: PaneSide, interval_ms: i64) {
    let tab = app.active_tab_mut();
    tab.pane_mut(side)
        .spec
        .retain(crate::state::BarSpec::Time(interval_ms));
    tab.apply_spec_changes();
    tab.apply_spec_changes();
}

/// [`history_app`] whose capabilities the test can move, as a push feed's do.
fn push_feed_app(
    ctx: &egui::Context,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
    tokio::sync::watch::Sender<FeedCapabilities>,
) {
    push_feed_app_restoring(ctx, None)
}

/// The same, with the time pane restored from a saved workspace at
/// `restored_ms` rather than opened on the header default.
fn push_feed_app_restoring(
    ctx: &egui::Context,
    restored_ms: Option<i64>,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
    tokio::sync::watch::Sender<FeedCapabilities>,
) {
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (caps_tx, caps_rx) = tokio::sync::watch::channel(FeedCapabilities {
        book_capture: false,
        history_paging: true,
        traded_volume: true,
        deal_counter: false,
        ohlcv_history: true,
        ohlcv_generation: 1,
        ohlcv_daily_generation: 1,
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
    let trades: Vec<_> = (0..200).map(minute_trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    run_frame(&mut app, ctx);
    match restored_ms {
        Some(ms) => app.active_tab_mut().restore_canvas(
            CanvasLayout::TimeAndFlow,
            None,
            crate::tab::CanvasCollapseRestore {
                context: false,
                flow: false,
                heights: &[],
                collapsed_slots: &[],
            },
            None,
            &[ms],
            crate::tab::LegendFold::default(),
        ),
        None => app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow),
    }
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    (app, evt_tx, cmd_rx, caps_tx)
}

/// A workspace saved on 1mo opens its time pane there, and the very first
/// candle request asks for days — never a week of minutes thrown away a frame
/// later.
#[test]
fn a_restored_monthly_pane_asks_for_days_first() {
    let ctx = egui::Context::default();
    let (app, _events, mut commands, _caps) =
        push_feed_app_restoring(&ctx, Some(quantick_engine::time_bucket::CALENDAR_MONTH_MS));
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).state.spec(),
        &BarSpec::Time(quantick_engine::time_bucket::CALENDAR_MONTH_MS)
    );
    assert_eq!(
        drain_ohlcv_bases(&mut commands)
            .iter()
            .map(|asked| asked.0)
            .collect::<Vec<_>>(),
        vec![quantick_feed::OHLCV_DAILY_INTERVAL_MS]
    );
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

/// Back to an intraday interval, the daily base cannot fold to it — but the
/// minutes the chart held before, including what *load older* paged in, come
/// back as they were: nothing is asked again and nothing is lost.
#[test]
fn a_trip_to_a_daily_pane_and_back_keeps_the_paged_minutes() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_bases(&mut commands);
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(120),
    );
    app.drain_tabs();
    let tab_id = app.tabs.active_id();
    let capabilities = app.active_tab().capabilities(&app.config);
    assert!(
        app.active_tab_mut()
            .request_older_ohlcv_history(tab_id, capabilities),
        "load older goes out"
    );
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history_range(-240, -120),
    );
    app.drain_tabs();
    drain_ohlcv_bases(&mut commands);
    assert_eq!(app.active_tab().venue_candles_held(), 240);
    let five_minutes = 5 * quantick_feed::OHLCV_BASE_INTERVAL_MS;
    set_time_pane(&mut app, five_minutes);
    let seam_on_minutes = app.active_tab().pane(PaneSide::Time(0)).seam_slot();
    assert_eq!(seam_on_minutes, 48, "240 minutes are 48 five-minute bars");

    set_time_pane(&mut app, DAY_MS);
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_bases(&mut commands)
            .iter()
            .map(|asked| asked.0)
            .collect::<Vec<_>>(),
        vec![quantick_feed::OHLCV_DAILY_INTERVAL_MS],
        "the days are asked for once"
    );
    answer(
        &events,
        quantick_feed::OHLCV_DAILY_INTERVAL_MS,
        daily_history(10),
    );
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 10);

    set_time_pane(&mut app, five_minutes);
    app.drain_tabs();
    assert_eq!(
        app.active_tab().pane(PaneSide::Time(0)).seam_slot(),
        seam_on_minutes,
        "the paged minutes are back under the five-minute chart"
    );
    assert_eq!(app.active_tab().venue_candles_held(), 240);
    assert!(
        drain_ohlcv_bases(&mut commands).is_empty(),
        "nothing is fetched again"
    );

    // And the days are parked in turn: back to 1d asks nothing either.
    set_time_pane(&mut app, DAY_MS);
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 10);
    assert!(drain_ohlcv_bases(&mut commands).is_empty());
}

/// A 1d pane stacked over a 5m one: one base serves both, so the stack
/// folds from minutes and the 5m pane gets its history.
#[test]
fn a_daily_pane_over_an_intraday_one_still_folds_from_minutes() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands) = history_app(&ctx);
    drain_ohlcv_bases(&mut commands);
    app.active_tab_mut()
        .set_layout(CanvasLayout::TimeTimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let five_minutes = 5 * quantick_feed::OHLCV_BASE_INTERVAL_MS;
    set_pane(&mut app, PaneSide::Time(1), five_minutes);
    set_pane(&mut app, PaneSide::Time(0), DAY_MS);
    app.drain_tabs();
    assert!(
        drain_ohlcv_bases(&mut commands)
            .iter()
            .all(|asked| asked.0 == quantick_feed::OHLCV_BASE_INTERVAL_MS),
        "the 5m pane below keeps the stack on minutes"
    );
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(3 * 1_440),
    );
    app.drain_tabs();
    let tab = app.active_tab();
    assert_eq!(tab.pane(PaneSide::Time(0)).seam_slot(), 3, "three days");
    assert_eq!(
        tab.pane(PaneSide::Time(1)).seam_slot(),
        3 * 288,
        "and three days of five-minute bars"
    );
}

/// A daily block landing on a push feed moves only the daily generation: a
/// chart folding minutes keeps its base and asks for nothing.
#[test]
fn a_daily_generation_leaves_a_minute_chart_alone() {
    let ctx = egui::Context::default();
    let (mut app, events, mut commands, caps) = push_feed_app(&ctx);
    drain_ohlcv_bases(&mut commands);
    answer(
        &events,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(120),
    );
    app.drain_tabs();
    assert_eq!(app.active_tab().venue_candles_held(), 120);

    caps.send_modify(|caps| caps.ohlcv_daily_generation = 2);
    app.drain_tabs();
    assert_eq!(app.active_tab().venue_candles_held(), 120, "base kept");
    assert!(drain_ohlcv_bases(&mut commands).is_empty(), "nothing asked");

    caps.send_modify(|caps| caps.ohlcv_generation = 2);
    app.drain_tabs();
    assert_eq!(
        drain_ohlcv_bases(&mut commands)
            .iter()
            .map(|asked| asked.0)
            .collect::<Vec<_>>(),
        vec![quantick_feed::OHLCV_BASE_INTERVAL_MS],
        "the minutes' own generation still asks again"
    );
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
