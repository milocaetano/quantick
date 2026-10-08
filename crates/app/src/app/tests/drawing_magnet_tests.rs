// The toolrail magnet through the app: placement clicks, the live
// placement preview and handle re-drags of the three tools a trader
// marks a swing with — rectangle, horizontal line and trend line.

use super::*;
use rust_decimal::prelude::ToPrimitive;

/// Bars of `TRADES_PER_BAR` prints: open, high, low, close, in that order.
const TRADES_PER_BAR: u64 = 4;
const CLOSED_BARS: u64 = 120;

/// The four prints of bar `bar`: a candle spanning `low ..= high` with a
/// body from `open` to `close`, stepped so neighbours differ.
fn bar_prints(bar: u64) -> [Decimal; 4] {
    let base = Decimal::from(100) + Decimal::from(bar % 4);
    [
        base,
        base + Decimal::from(3),
        base - Decimal::from(3),
        base + Decimal::from(1),
    ]
}

fn ohlc(bar: u64) -> [f64; 4] {
    bar_prints(bar).map(|price| price.to_f64().unwrap())
}

/// An app holding `CLOSED_BARS` four-print candles and a forming bar that
/// has printed its open, high and low so far.
fn app_with_candles() -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, evt_tx, cmd_rx, _book_tx) = test_app_with_launch(AppLaunch::default());
    app.active_tab_mut().flow_pane.set_layer_visible(
        ChartLayer::LiveStrip,
        false,
        &mut Default::default(),
    );
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(TRADES_PER_BAR));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let mut trades = Vec::new();
    for bar in 0..=CLOSED_BARS {
        let prints = bar_prints(bar);
        // The forming bar stops after its low: three prints of four.
        let count = if bar == CLOSED_BARS { 3 } else { 4 };
        for price in &prints[..count] {
            let agg_id = trades.len() as u64 + 1;
            trades.push(quantick_engine::Trade {
                agg_id,
                timestamp_ms: 1_000 + agg_id as i64 * 100,
                price: *price,
                quantity: Decimal::ONE,
                side: quantick_engine::Side::Buy,
            });
        }
    }
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    assert_eq!(
        app.active_tab().flow_pane.state.bars().len() as u64,
        CLOSED_BARS
    );
    (app, cmd_rx)
}

/// The screen x of fractional bar `bar` on the flow pane.
fn bar_x(app: &QuantickApp, bar: f32) -> f32 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the pane reported its rect");
    let history_right = pane.frame.lane_divider_x.unwrap_or(chart.right());
    pane.viewport
        .x_at_bar_position(bar, history_right, pane.slots())
}

fn magnet_app() -> (QuantickApp, mpsc::Receiver<FeedCommand>, egui::Context) {
    let (mut app, commands) = app_with_candles();
    let ctx = egui::Context::default();
    app.toolrail.set_magnet(true);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    (app, commands, ctx)
}

fn last_points(app: &QuantickApp) -> Vec<ChartPoint> {
    app.active_tab()
        .flow_pane
        .drawings
        .items()
        .last()
        .expect("a drawing was placed")
        .points
        .to_vec()
}

/// Pixels per price unit on the flow pane — the fixture must leave the
/// candle body taller than the old 12 px reach for these proofs to bite.
fn px_per_unit(app: &QuantickApp) -> f32 {
    (price_y(app, PaneSide::Flow, 100.0) - price_y(app, PaneSide::Flow, 101.0)).abs()
}

/// A click inside a candle's body, farther than 12 px from every one of
/// its prices, still lands on the nearest of them: the pointer is *on* the
/// candle, which is the nearest a pointer gets.
#[test]
fn a_horizontal_line_clicked_inside_a_candle_takes_its_nearest_print() {
    let (mut app, _commands, ctx) = magnet_app();
    assert!(px_per_unit(&app) > 30.0, "{}", px_per_unit(&app));
    let [_, _, _, close] = ohlc(60);
    // 0.4 of a unit above the close: well clear of 12 px, nearest the close.
    let y = price_y(&app, PaneSide::Flow, close + 0.4);
    let at = egui::pos2(bar_x(&app, 60.0), y);
    place_drawing(&mut app, &ctx, "horizontal-line", &[at]);
    assert_eq!(last_points(&app)[0].price, close);
}

/// A few pixels past the old reach above the high, with the pointer in
/// the gap at the bar's right edge, still takes that bar's high.
#[test]
fn a_rectangle_snaps_both_corners_just_beyond_the_old_reach() {
    let (mut app, _commands, ctx) = magnet_app();
    let [_, high_a, _, _] = ohlc(40);
    let [_, _, low_b, _] = ohlc(70);
    let first = egui::pos2(
        bar_x(&app, 40.45),
        price_y(&app, PaneSide::Flow, high_a) - 18.0,
    );
    let second = egui::pos2(
        bar_x(&app, 69.55),
        price_y(&app, PaneSide::Flow, low_b) + 18.0,
    );
    place_drawing(&mut app, &ctx, "rectangle", &[first, second]);
    let points = last_points(&app);
    assert_eq!(points[0].price, high_a, "{points:?}");
    assert_eq!(points[1].price, low_b, "{points:?}");
    assert_eq!(slot_of(points[0].bar), 40, "x stays on the bar pointed at");
    assert_eq!(slot_of(points[1].bar), 70, "x stays on the bar pointed at");
}

/// Far from any candle the magnet lets go — a free diagonal stays free.
#[test]
fn far_from_the_candle_the_magnet_lets_go() {
    let (mut app, _commands, ctx) = magnet_app();
    // Bar 52 is the lowest step, so two units over its high is on screen.
    let [_, high, _, _] = ohlc(52);
    let y = price_y(&app, PaneSide::Flow, high + 2.0);
    let at = egui::pos2(bar_x(&app, 52.0), y);
    place_drawing(&mut app, &ctx, "horizontal-line", &[at]);
    assert!((last_points(&app)[0].price - high).abs() > 1.0);
}

/// The trend line's preview shows the snapped anchor before the click,
/// and both clicks — one on the forming bar — commit snapped anchors.
#[test]
fn a_trend_line_previews_and_commits_snapped_anchors_on_the_forming_bar() {
    let (mut app, _commands, ctx) = magnet_app();
    let forming = CLOSED_BARS as usize;
    assert_eq!(app.active_tab().flow_pane.slots(), forming + 1);
    let [_, high_a, _, _] = ohlc(30);
    let [_, _, low_forming, _] = ohlc(CLOSED_BARS);
    arm_drawing_from_toolbox(&mut app, &ctx, "trend-line");
    let first = egui::pos2(
        bar_x(&app, 30.0),
        price_y(&app, PaneSide::Flow, high_a - 0.4),
    );
    move_chart_with(&mut app, &ctx, first, egui::Modifiers::NONE);
    let preview = app
        .active_tab()
        .flow_pane
        .gestures
        .hover
        .expect("a preview");
    assert_eq!(preview.price, high_a, "the preview shows the snap");
    click_chart(&mut app, &ctx, first);
    let second = egui::pos2(
        bar_x(&app, forming as f32),
        price_y(&app, PaneSide::Flow, low_forming) + 20.0,
    );
    move_chart_with(&mut app, &ctx, second, egui::Modifiers::NONE);
    let preview = app
        .active_tab()
        .flow_pane
        .gestures
        .hover
        .expect("a preview");
    assert_eq!(preview.price, low_forming, "the far end previews snapped");
    click_chart(&mut app, &ctx, second);
    run_frame(&mut app, &ctx);
    let points = last_points(&app);
    assert_eq!(points[0].price, high_a, "{points:?}");
    assert_eq!(points[1].price, low_forming, "{points:?}");
    assert_eq!(slot_of(points[1].bar), forming);
}

/// Dragging a trend line handle near another candle re-snaps it there.
#[test]
fn a_trend_line_handle_resnaps_when_dragged() {
    let (mut app, _commands, ctx) = magnet_app();
    let [open_a, _, _, _] = ohlc(30);
    let [_, _, _, close_b] = ohlc(50);
    let start = egui::pos2(bar_x(&app, 30.0), price_y(&app, PaneSide::Flow, open_a));
    let end = egui::pos2(bar_x(&app, 50.0), price_y(&app, PaneSide::Flow, close_b));
    place_drawing(&mut app, &ctx, "trend-line", &[start, end]);
    let [_, high_c, _, _] = ohlc(80);
    let to = egui::pos2(
        bar_x(&app, 80.4),
        price_y(&app, PaneSide::Flow, high_c) - 18.0,
    );
    drag_chart_with(&mut app, &ctx, end, to, egui::Modifiers::NONE);
    let points = last_points(&app);
    assert_eq!(points[0].price, open_a, "{points:?}");
    assert_eq!(points[1].price, high_c, "{points:?}");
    assert_eq!(slot_of(points[1].bar), 80);
}

/// A horizontal line dragged into another candle's body takes its print.
#[test]
fn a_horizontal_line_resnaps_when_dragged() {
    let (mut app, _commands, ctx) = magnet_app();
    let [open_a, _, _, _] = ohlc(60);
    let at = egui::pos2(bar_x(&app, 60.0), price_y(&app, PaneSide::Flow, open_a));
    place_drawing(&mut app, &ctx, "horizontal-line", &[at]);
    let [_, _, low_b, _] = ohlc(61);
    let to = egui::pos2(
        bar_x(&app, 61.0),
        price_y(&app, PaneSide::Flow, low_b + 0.4),
    );
    drag_chart_with(&mut app, &ctx, at, to, egui::Modifiers::NONE);
    assert_eq!(last_points(&app)[0].price, low_b);
}

/// A rectangle corner dragged lands on the print exactly, and the corner
/// it did not move keeps its own price to the bit.
#[test]
fn a_rectangle_corner_resnaps_when_dragged_and_the_other_keeps_its_price() {
    let (mut app, _commands, ctx) = magnet_app();
    let [_, high_a, _, _] = ohlc(40);
    let [_, _, low_b, _] = ohlc(70);
    let first = egui::pos2(bar_x(&app, 40.0), price_y(&app, PaneSide::Flow, high_a));
    let second = egui::pos2(bar_x(&app, 70.0), price_y(&app, PaneSide::Flow, low_b));
    place_drawing(&mut app, &ctx, "rectangle", &[first, second]);
    let [_, _, _, close_c] = ohlc(90);
    let to = egui::pos2(
        bar_x(&app, 90.45),
        price_y(&app, PaneSide::Flow, close_c + 0.4),
    );
    drag_chart_with(&mut app, &ctx, second, to, egui::Modifiers::NONE);
    let points = last_points(&app);
    assert_eq!(points[0].price, high_a, "{points:?}");
    assert_eq!(points[1].price, close_c, "{points:?}");
    assert_eq!(slot_of(points[1].bar), 90);
}

/// Magnet off: the same click inside the body keeps the pointer's price.
#[test]
fn with_the_magnet_off_a_click_inside_a_candle_stays_free() {
    let (mut app, _commands) = app_with_candles();
    let ctx = egui::Context::default();
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    let [_, _, _, close] = ohlc(60);
    let y = price_y(&app, PaneSide::Flow, close + 0.4);
    let at = egui::pos2(bar_x(&app, 60.0), y);
    place_drawing(&mut app, &ctx, "horizontal-line", &[at]);
    assert!((last_points(&app)[0].price - (close + 0.4)).abs() < 0.05);
}
