//! Tape only frames its prices like the tape beside the candles. Trader
//! 2026-11-03, live BTCUSDT: a few cents of recent prints filled the whole
//! tape-only pane, and squeezing the price gutter turned it upside down
//! within a few drags, still narrower than the beside view opens.
use super::*;

/// Wide candles ten minutes ago, then a tape of prints a dime apart.
fn priced(agg_id: u64) -> quantick_engine::Trade {
    let mut trade = trade(agg_id);
    if agg_id > 380 {
        trade.timestamp_ms += 600_000;
        trade.price = Decimal::from(101) + Decimal::new((agg_id % 2) as i64, 1);
    } else {
        trade.price = Decimal::from(90 + (agg_id % 20) as i64);
    }
    trade
}

/// A native split on that tape, with its beside fit drawn.
fn narrow_tape_app(ctx: &egui::Context) -> QuantickApp {
    let (mut app, _commands) = app_with_history(200);
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .apply_preset("mini index regions", false)
    );
    for agg_id in 201..=400 {
        let trade = priced(agg_id);
        app.active_tab_mut()
            .ingest_live_trade_at(&trade, trade.timestamp_ms);
    }
    run_frame(&mut app, ctx);
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, ctx);
    app
}

fn tape_only(app: &mut QuantickApp, ctx: &egui::Context) {
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        quantick_layers::OrderflowSwitch::TapeOnly,
        true,
    );
    run_frame(app, ctx);
    run_frame(app, ctx);
    assert!(app.active_tab().tape().cached_config().tape_only());
}

fn auto_range(app: &QuantickApp) -> (f64, f64) {
    app.active_tab().flow_pane.frame.auto_range.expect("fitted")
}

/// The rule: tape only fits the tape's prints with the candles the beside
/// view would show at the live edge, so both views open on one price window
/// and switching between them never rescales the prices.
#[test]
fn tape_only_opens_on_the_beside_views_price_window() {
    let ctx = egui::Context::default();
    let mut app = narrow_tape_app(&ctx);
    let tape = app.active_tab().tape().tape_price_range().expect("prints");
    let beside = auto_range(&app);
    assert!(
        beside.1 - beside.0 > 50.0 * (tape.1 - tape.0),
        "the fixture's candles are far wider than its tape: {beside:?} vs {tape:?}"
    );
    tape_only(&mut app, &ctx);
    let alone = auto_range(&app);
    assert!(
        (alone.0 - beside.0).abs() < 1e-9 && (alone.1 - beside.1).abs() < 1e-9,
        "tape only {alone:?} opens where beside {beside:?} does"
    );
}

/// Squeezing the tape-only gutter stays upright well past the window the
/// beside view opens on: the flip waits for flat candles, not flat prints.
#[test]
fn squeezing_the_tape_only_gutter_stays_upright_past_the_beside_window() {
    let ctx = egui::Context::default();
    let mut app = narrow_tape_app(&ctx);
    let beside = auto_range(&app);
    tape_only(&mut app, &ctx);
    let opened = auto_range(&app);
    let gutter = app
        .active_tab()
        .flow_pane
        .frame
        .price_gutter
        .expect("gutter");
    let mut span = opened.1 - opened.0;
    for _ in 0..20 {
        if span > 4.0 * (beside.1 - beside.0) {
            return;
        }
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() + egui::vec2(0.0, 60.0),
        );
        let view = &app.active_tab().flow_pane.price_view;
        let (lo, hi) = view.resolve(opened);
        assert!(lo < hi, "the squeeze keeps an ordered span: {lo}..{hi}");
        assert!(
            !view.is_inverted(),
            "flipped at span {} of {beside:?}",
            hi - lo
        );
        span = hi - lo;
    }
    panic!("twenty squeezes never widened past {span}");
}
