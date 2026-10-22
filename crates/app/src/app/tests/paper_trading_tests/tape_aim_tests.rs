//! Shift/Alt order entry over the tape. The flow pane's tape lane shares the
//! pane's price axis, so the aim and its press work there exactly as they do
//! over candles, in every tape mode. The tape chip keeps its own press.
use super::*;
use rust_decimal::prelude::ToPrimitive;

/// The tape modes a flow pane offers, and how a test enters each.
#[derive(Debug, Clone, Copy)]
enum TapeMode {
    /// The tape lane beside the candles, the shipped default.
    Chart,
    /// The tape alone across the whole plot.
    Only,
    /// The WIN source's native tape beside its tick candles.
    Native,
}

/// A flow pane with 200 backfilled prints (mark 100.0, range 100.0..101.9)
/// in `mode`, laid out for two frames so the divider is published.
fn tape_app(ctx: &egui::Context, mode: TapeMode) -> QuantickApp {
    let (mut app, _commands) = app_with_history(200);
    match mode {
        TapeMode::Chart => switch_layer(&mut app, ChartLayer::TapeChart, true),
        TapeMode::Only => switch_layer(&mut app, ChartLayer::TapeOnly, true),
        TapeMode::Native => assert!(
            app.active_tab_mut()
                .tape_mut()
                .apply_preset("mini index regions", false)
        ),
    }
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    app
}

/// A point inside the tape lane at `price`, clear of the tape chip.
fn on_the_tape(app: &QuantickApp, price: f64) -> egui::Pos2 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the pane laid out");
    let divider = pane
        .frame
        .lane_divider_x
        .expect("a tape lane on the flow pane");
    egui::pos2(
        (divider + chart.right()) / 2.0,
        price_y(app, PaneSide::Flow, price),
    )
}

/// The one working order, as (side, price).
fn the_order(app: &QuantickApp) -> (quantick_engine::Side, Decimal) {
    let orders = app.active_tab().paper.working_orders();
    assert_eq!(orders.len(), 1, "one press, one order: {orders:?}");
    (orders[0].side, orders[0].price.expect("a resting order"))
}

/// Shift over the tape rests a buy, and Alt a sell, at the price under the
/// pointer, in every tape mode.
#[test]
fn shift_and_alt_rest_an_order_at_the_pointer_price_over_the_tape() {
    for mode in [TapeMode::Chart, TapeMode::Only, TapeMode::Native] {
        for (modifiers, side, price) in [
            (
                egui::Modifiers::SHIFT,
                quantick_engine::Side::Buy,
                Decimal::new(1005, 1),
            ),
            (
                egui::Modifiers::ALT,
                quantick_engine::Side::Sell,
                Decimal::new(1010, 1),
            ),
        ] {
            let ctx = egui::Context::default();
            let mut app = tape_app(&ctx, mode);
            let at = on_the_tape(&app, price.to_f64().unwrap());
            move_chart_with(&mut app, &ctx, at, modifiers);
            assert!(
                app.active_tab().paper.aiming(),
                "{mode:?}: the held key aims over the tape"
            );
            click_chart_with(&mut app, &ctx, at, modifiers);
            assert_eq!(
                the_order(&app),
                (side, price),
                "{mode:?}: {side:?} rests at the pointer's price"
            );
        }
    }
}

/// Tape only lays the lane over the whole plot: the divider sits on the
/// plot's left edge, and the aim still lives anywhere across it.
#[test]
fn tape_only_aims_across_the_whole_plot() {
    let ctx = egui::Context::default();
    let mut app = tape_app(&ctx, TapeMode::Only);
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the pane laid out");
    let divider = pane.frame.lane_divider_x.expect("the tape takes the plot");
    assert!((divider - chart.left()).abs() < 1.0, "no candles beside it");
    let at = egui::pos2(
        chart.left() + chart.width() * 0.25,
        price_y(&app, PaneSide::Flow, 100.5),
    );
    click_chart_with(&mut app, &ctx, at, egui::Modifiers::SHIFT);
    assert_eq!(
        the_order(&app),
        (quantick_engine::Side::Buy, Decimal::new(1005, 1))
    );
}

/// The tape chip keeps its press with the buy key down: Shift on it puts the
/// tape away rather than resting an order under it.
#[test]
fn the_tape_chip_keeps_its_press_with_shift_down() {
    let ctx = egui::Context::default();
    let mut app = tape_app(&ctx, TapeMode::Chart);
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the pane laid out");
    let chip = crate::pane::tape_switch_rect(chart, false);
    click_chart_with(&mut app, &ctx, chip.center(), egui::Modifiers::SHIFT);
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "the chip is chrome, not a price"
    );
}
