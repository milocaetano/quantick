//! Shift/Alt order entry over the tape. The flow pane's tape lane shares the
//! pane's price axis, so the aim and its press work there exactly as they do
//! over candles, in every tape mode. The tape chip keeps its own press.
use super::*;

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

/// A flow pane with 200 backfilled prints in `mode`, then one live print at
/// 100.05: it marks the paper venue and teaches it a cent tick, so a level
/// a quarter down the plot is a real price above the mark. Laid out for two
/// frames, so the divider is published.
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
    let live = quantick_engine::Trade {
        price: Decimal::new(10005, 2),
        ..trade(201)
    };
    app.active_tab_mut()
        .ingest_live_trade_at(&live, live.timestamp_ms);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    let mark = app.active_tab().paper.mark_price();
    assert_eq!(
        mark,
        Some(Decimal::new(10005, 2)),
        "{mode:?}: the live mark"
    );
    app
}

/// A quarter down the plot, at `x`: above the mark in every mode's fit.
fn at_x(app: &QuantickApp, x: f32) -> egui::Pos2 {
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the pane laid out");
    egui::pos2(x, chart.top() + chart.height() * 0.25)
}

/// The middle of the tape lane, a quarter down the plot, clear of the chip.
fn on_the_tape(app: &QuantickApp) -> egui::Pos2 {
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_area.expect("the pane laid out");
    let divider = pane
        .frame
        .lane_divider_x
        .expect("a tape lane on the flow pane");
    at_x(app, (divider + chart.right()) / 2.0)
}

/// The price a press at `at` should rest at: the pane's own axis, snapped
/// to the venue's tick.
fn pointer_price(app: &QuantickApp, at: egui::Pos2) -> Decimal {
    let raw = price_at_y(app, PaneSide::Flow, at.y);
    app.active_tab().paper.account().snap(raw)
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
        for (modifiers, side) in [
            (egui::Modifiers::SHIFT, quantick_engine::Side::Buy),
            (egui::Modifiers::ALT, quantick_engine::Side::Sell),
        ] {
            let ctx = egui::Context::default();
            let mut app = tape_app(&ctx, mode);
            let at = on_the_tape(&app);
            let price = pointer_price(&app, at);
            assert!(
                price > Decimal::new(10005, 2),
                "{mode:?}: {price} sits above the mark"
            );
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
    let at = at_x(&app, chart.left() + chart.width() * 0.1);
    let price = pointer_price(&app, at);
    click_chart_with(&mut app, &ctx, at, egui::Modifiers::SHIFT);
    assert_eq!(the_order(&app), (quantick_engine::Side::Buy, price));
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
    assert!(
        !layer_on(&app, ChartLayer::TapeChart),
        "the press put the tape away"
    );
}

/// The lane's divider keeps its drag with the buy key down: Shift on it
/// rests nothing, on either side of the hairline.
#[test]
fn the_lane_divider_keeps_its_press_with_shift_down() {
    for mode in [TapeMode::Chart, TapeMode::Native] {
        let ctx = egui::Context::default();
        let mut app = tape_app(&ctx, mode);
        let divider = app
            .active_tab()
            .flow_pane
            .frame
            .lane_divider_x
            .expect("a tape lane on the flow pane");
        for x in [divider - 2.0, divider + 2.0] {
            let at = at_x(&app, x);
            click_chart_with(&mut app, &ctx, at, egui::Modifiers::SHIFT);
            assert!(
                app.active_tab().paper.working_orders().is_empty(),
                "{mode:?}: the divider at {x} is chrome, not a price"
            );
        }
    }
}

/// Over the tape an order line is grabbed and dragged as over candles: the
/// aim stands down near it there, and the press must not fall into a dead
/// band between the two.
#[test]
fn an_order_line_drags_over_the_tape() {
    for mode in [TapeMode::Chart, TapeMode::Only] {
        let ctx = egui::Context::default();
        let mut app = tape_app(&ctx, mode);
        let at = on_the_tape(&app);
        click_chart_with(&mut app, &ctx, at, egui::Modifiers::SHIFT);
        let (_, placed) = the_order(&app);
        let line = egui::pos2(
            at.x,
            price_y(
                &app,
                PaneSide::Flow,
                rust_decimal::prelude::ToPrimitive::to_f64(&placed).expect("a price"),
            ),
        );
        let lower = line + egui::vec2(0.0, 30.0);
        drag_chart(&mut app, &ctx, line, lower);
        let (_, moved) = the_order(&app);
        assert_eq!(
            moved,
            pointer_price(&app, lower),
            "{mode:?}: the drag over the tape repriced the order"
        );
    }
}

/// Tape only leaves the candles no width: the order's tag anchors at the
/// plot's right edge, and its ✕ there cancels the order.
#[test]
fn tape_only_cancels_from_the_tag_inside_the_plot() {
    let ctx = egui::Context::default();
    let mut app = tape_app(&ctx, TapeMode::Only);
    let at = on_the_tape(&app);
    click_chart_with(&mut app, &ctx, at, egui::Modifiers::SHIFT);
    let (_, placed) = the_order(&app);
    let chart = app
        .active_tab()
        .flow_pane
        .frame
        .chart_area
        .expect("the pane laid out");
    let y = price_y(
        &app,
        PaneSide::Flow,
        rust_decimal::prelude::ToPrimitive::to_f64(&placed).expect("a price"),
    );
    let center = crate::paper_trading::clamp_tag_center(y, chart.top(), chart.bottom());
    let cross = crate::paper_trading::close_button_rect(chart.right(), center).center();
    assert!(chart.contains(cross), "the ✕ sits on the plot");
    move_chart_with(&mut app, &ctx, cross, egui::Modifiers::NONE);
    click_chart_with(&mut app, &ctx, cross, egui::Modifiers::NONE);
    assert!(
        app.active_tab().paper.working_orders().is_empty(),
        "the ✕ where it is painted cancels the order"
    );
}
