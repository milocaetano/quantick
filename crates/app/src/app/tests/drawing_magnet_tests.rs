// The magnets reach their owner through the app: the toolrail's on a
// placement click and on a body drag, the object's own through its context
// bar and through a mirror on another pane. Where an anchor snaps is
// `quantick-drawings`' to test (`gesture/projection_tests.rs`,
// `gesture/placing_tests.rs`, `gesture/pointer.rs`).

use super::*;
use rust_decimal::prelude::ToPrimitive;

/// Bars of `TRADES_PER_BAR` prints: open, high, low, close, in that order.
const TRADES_PER_BAR: u64 = 4;
const CLOSED_BARS: u64 = 120;
/// The fixture's first trade id, leaving ids and time below it for older
/// trades a test prepends.
const FIRST_AGG_ID: u64 = 1_000;

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
            let agg_id = FIRST_AGG_ID + trades.len() as u64;
            trades.push(quantick_engine::Trade {
                agg_id,
                timestamp_ms: 1_000_000 + agg_id as i64 * 100,
                price: *price,
                quantity: Decimal::ONE,
                side: quantick_engine::Side::Buy,
            });
        }
    }
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    let tab_id = app.runtime.tabs.active_id();
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
    let history_right = pane.frame.candles_right.unwrap_or(chart.right());
    pane.model
        .viewport
        .x_at_bar_position(bar, history_right, pane.slots())
}

fn magnet_app() -> (QuantickApp, mpsc::Receiver<FeedCommand>, egui::Context) {
    let (mut app, commands) = app_with_candles();
    let ctx = egui::Context::default();
    app.surfaces.toolrail.set_magnet(true);
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

/// A press at `from`, the pointer carried through every one of `path`, and a
/// release at the last — one frame per stop, the way a hand drags.
fn drag_through(app: &mut QuantickApp, ctx: &egui::Context, from: egui::Pos2, path: &[egui::Pos2]) {
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
    );
    for stop in path {
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(*stop)]);
    }
    let end = *path.last().expect("a drag goes somewhere");
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );
}

/// Place `id` through `anchors` with the magnet off, so the object starts
/// on free prices, then switch the magnet on for the move under test.
fn place_free(app: &mut QuantickApp, ctx: &egui::Context, id: &str, anchors: &[egui::Pos2]) {
    app.surfaces.toolrail.set_magnet(false);
    place_drawing(app, ctx, id, anchors);
    app.surfaces.toolrail.set_magnet(true);
    run_frame(app, ctx);
}

/// A horizontal line grabbed by its body, far from its handle, and
/// carried into a candle lands on that candle's nearest print exactly.
#[test]
fn a_horizontal_line_dragged_by_its_body_lands_on_a_print() {
    let (mut app, _commands, ctx) = magnet_app();
    let at = egui::pos2(bar_x(&app, 52.0), price_y(&app, PaneSide::Flow, 104.3));
    place_free(&mut app, &ctx, "horizontal-line", &[at]);
    let grab = egui::pos2(bar_x(&app, 35.0), at.y);
    let [_, _, low_b, _] = ohlc(61);
    let to = egui::pos2(
        bar_x(&app, 61.0),
        price_y(&app, PaneSide::Flow, low_b + 0.4),
    );
    drag_through(&mut app, &ctx, grab, &[to]);
    assert_eq!(last_points(&app)[0].price, low_b);
}

/// The magnet slot on the selected object's context bar flips that
/// object's own magnet, and nothing else: the rail's stays where it was.
#[test]
fn the_context_bar_flips_the_selected_drawings_own_magnet() {
    let (mut app, _commands, ctx) = magnet_app();
    app.surfaces.toolrail.set_magnet(false);
    let at = egui::pos2(bar_x(&app, 52.0), price_y(&app, PaneSide::Flow, 104.3));
    let index = place_drawing(&mut app, &ctx, "horizontal-line", &[at]);
    run_frame(&mut app, &ctx);
    for expected in [true, false] {
        let slot = app
            .surfaces
            .drawing_chrome
            .context_bar()
            .magnet_rect()
            .expect("a pointer-anchored drawing renders the magnet slot");
        click_chart(&mut app, &ctx, slot.center());
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().flow_pane.drawings.items()[index].magnet,
            expected
        );
        assert!(
            !app.surfaces.toolrail.magnet(),
            "the rail's magnet never moves"
        );
    }
}

/// Where a shared line ends after its mirror on the time pane is carried
/// onto a time-pane candle's high, with the rail's and the object's own
/// magnet as given.
fn shared_mirror_drag(rail: bool, own: bool) -> f64 {
    let ctx = egui::Context::default();
    let (mut app, _commands, on_the_time_pane) = split_with_a_shared_line(&ctx);
    app.surfaces.toolrail.set_magnet(rail);
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_magnet_at(0, own);
    run_frame(&mut app, &ctx);
    let pane = app.active_tab().time_pane().expect("the split built it");
    let slot = pane.slots() - 3;
    let high = pane.state.bars()[slot].high.to_f64().unwrap();
    let chart = pane.frame.chart_area.expect("the time pane drew");
    let history_right = pane.frame.candles_right.unwrap_or(chart.right());
    let x = pane
        .model
        .viewport
        .x_at_bar_position(slot as f32, history_right, pane.slots());
    let (_, scale) = time_pane_projection(&app);
    let to = egui::pos2(x, scale.y(high) - 4.0);
    drag_chart(&mut app, &ctx, on_the_time_pane, to);
    run_frame(&mut app, &ctx);
    app.active_tab().flow_pane.drawings.items()[0].points[0].price
}

/// A mirror on another pane honours the object's own magnet exactly as it
/// honours the rail's: the same drag lands on the same price.
#[test]
fn a_shared_mirror_drag_honours_the_drawings_own_magnet() {
    let free = shared_mirror_drag(false, false);
    let rail = shared_mirror_drag(true, false);
    let own = shared_mirror_drag(false, true);
    assert_ne!(rail, free, "the fixture's drag must reach a print");
    assert_eq!(own, rail);
}
