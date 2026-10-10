// Pane model integration preserves the existing chart behavior.
use super::*;

/// "Load older" moves the first engine bar backwards in time, and the
/// prefix was trimmed against where that bar used to be.
///
/// Three clicks reach this on Binance: split the canvas, let the venue
/// history land, pull older trades. The venue candles covering the newly
/// re-cut minutes then sat in front of engine bars covering the *same*
/// minutes — the window drawn twice, `open_time` going backwards across
/// the seam, and the precondition `slot_at_time` documents quietly false.
#[test]
fn pulling_older_trades_re_trims_the_venue_prefix() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = history_app(&ctx);
    events
        .try_send(FeedEvent::OhlcvHistory {
            interval_ms: quantick_feed::OHLCV_BASE_INTERVAL_MS,
            bars: venue_history(120),
            slice: quantick_feed::OhlcvSlice::Last { complete: true },
        })
        .unwrap();
    app.drain_tabs();
    assert_eq!(app.active_tab().pane(PaneSide::Time(0)).seam_slot(), 120);

    // Something anchored to a bar index, and a view off the live edge, so
    // the shift has something to preserve.
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let point = pane_point(&app, PaneSide::Time(0));
    click_chart(&mut app, &ctx, point);
    let slots = app.active_tab().pane(PaneSide::Time(0)).slots();
    app.active_tab_mut()
        .pane_mut(PaneSide::Time(0))
        .model
        .viewport
        .pan_pixels(40.0, slots);
    let edge_before = app.active_tab().pane(PaneSide::Time(0)).right_edge_time();
    let mark_before = app.active_tab().pane(PaneSide::Time(0)).drawings.items()[0].points[0];
    let mark_time_before = app
        .active_tab()
        .pane(PaneSide::Time(0))
        .slot_open_time(mark_before.bar as usize);
    assert!(edge_before.is_some(), "the view is off the live edge");

    // Five minutes of older trades, inside the window the prefix covers.
    let older: Vec<_> = (-5_i64..0).map(minute_trade_at).collect();
    events.try_send(FeedEvent::HistoryPrepended(older)).unwrap();
    app.drain_tabs();

    let pane = app.active_tab().pane(PaneSide::Time(0));
    let first_engine = pane
        .state
        .bars()
        .first()
        .or_else(|| pane.state.partial())
        .expect("the pane holds bars")
        .open_time;
    assert!(
        pane.history_prefix.iter().all(|bar| bar.open_time
            < crate::resample::bucket_start(first_engine, quantick_feed::OHLCV_BASE_INTERVAL_MS)),
        "no venue candle may cover a minute the engine has now re-cut"
    );
    assert_eq!(
        pane.seam_slot(),
        115,
        "the five overlapping buckets left the prefix"
    );
    let opens: Vec<i64> = (0..pane.closed_slots())
        .filter_map(|slot| pane.slot_open_time(slot))
        .collect();
    assert!(
        opens.windows(2).all(|pair| pair[0] <= pair[1]),
        "and open_time still never decreases across the seam"
    );

    // The user was reading a market moment; they still are, and their mark
    // is still on the bar they put it on.
    assert_eq!(
        pane.right_edge_time(),
        edge_before,
        "the view kept the market time it was showing"
    );
    assert_eq!(
        pane.slot_open_time(pane.drawings.items()[0].points[0].bar as usize),
        mark_time_before,
        "and the mark kept the bar it was drawn against"
    );
}
