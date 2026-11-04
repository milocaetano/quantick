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
/// beside view opens on: the flip waits for flat candles, not flat prints, and
/// arrives where the beside view's would, near `FLIP_SPAN_FACTOR` spans of it.
#[test]
fn squeezing_the_tape_only_gutter_stays_upright_past_the_beside_window() {
    let ctx = egui::Context::default();
    let mut app = narrow_tape_app(&ctx);
    let beside = auto_range(&app);
    let threshold = (beside.1 - beside.0) * crate::price_view::FLIP_SPAN_FACTOR;
    tape_only(&mut app, &ctx);
    let opened = auto_range(&app);
    let gutter = app
        .active_tab()
        .flow_pane
        .frame
        .price_gutter
        .expect("gutter");
    let mut span = opened.1 - opened.0;
    for _ in 0..40 {
        drag_chart(
            &mut app,
            &ctx,
            gutter.center(),
            gutter.center() + egui::vec2(0.0, 60.0),
        );
        let view = &app.active_tab().flow_pane.price_view;
        let (lo, hi) = view.resolve(opened);
        assert!(lo < hi, "the squeeze keeps an ordered span: {lo}..{hi}");
        if view.is_inverted() {
            let ratio = span / threshold;
            assert!(
                (0.9..=1.1).contains(&ratio),
                "flipped at {span}, {ratio} of the beside threshold {threshold}"
            );
            return;
        }
        span = hi - lo;
    }
    panic!("forty squeezes never flipped, at {span} of {threshold}");
}

/// A print of the narrow tape. With a `spike`, that trade prints at 150 and the
/// ones after it come ten minutes later, so the tape holds none from before.
fn narrow_trade(agg_id: u64, spike: Option<u64>) -> quantick_engine::Trade {
    let mut trade = trade(agg_id);
    trade.price = Decimal::from(101) + Decimal::new((agg_id % 2) as i64, 1);
    if let Some(spike) = spike {
        if agg_id == spike {
            trade.price = Decimal::from(150);
        }
        if agg_id > spike {
            trade.timestamp_ms += 600_000;
        }
    }
    trade
}

/// A native split with the live strip on the trail, its lane at `share` and a
/// spike at trade `spike`, drawn beside the candles.
fn spiked_app(ctx: &egui::Context, spike: Option<u64>, share: f32) -> QuantickApp {
    let (mut app, _commands) = app_with_history(200);
    let tab = app.active_tab_mut();
    assert!(tab.tape_mut().apply_preset("mini index regions", false));
    tab.tape_mut()
        .edit_config(|config| config.live_lane.width_share = share);
    let mut effects = Default::default();
    tab.flow_pane
        .set_layer_visible(ChartLayer::LiveStrip, true, &mut effects);
    for agg_id in 201..=400 {
        let trade = narrow_trade(agg_id, spike);
        tab.ingest_live_trade_at(&trade, trade.timestamp_ms);
    }
    run_frame(&mut app, ctx);
    app.active_tab_mut().tape_mut().flush_for_test();
    run_frame(&mut app, ctx);
    let frame = &app.active_tab().flow_pane.frame;
    let (candles, divider) = (frame.candles_right, frame.lane_divider_x);
    let strip = candles
        .zip(divider)
        .is_some_and(|(candles, divider)| candles < divider);
    assert!(
        strip,
        "the strip stands on the trail: {candles:?} {divider:?}"
    );
    app
}

/// The trade whose candle stands just left of the beside view's candles at
/// the default share, its strip on.
fn trade_left_of_the_beside_window(ctx: &egui::Context) -> u64 {
    let app = spiked_app(
        ctx,
        None,
        quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE,
    );
    let pane = &app.active_tab().flow_pane;
    let chart = pane.frame.chart_rect.expect("laid out");
    let width = pane.frame.candles_right.expect("candles") - chart.left();
    let total = pane.state.bars().len() + usize::from(pane.state.partial().is_some());
    let (start, _) = pane.model.viewport.visible_range(width, total);
    let left = &pane.state.bars()[start - 1];
    u64::try_from((left.open_time - 1_000) / 100).expect("a fixture trade")
}

fn leave_tape_only(app: &mut QuantickApp, ctx: &egui::Context) {
    crate::orderflow_view::layers::set_layer_switch(
        app.active_tab_mut().tape_mut(),
        quantick_layers::OrderflowSwitch::TapeOnly,
        false,
    );
    run_frame(app, ctx);
    run_frame(app, ctx);
}

/// Review of #757: tape only measured the candles without the strip the beside
/// view stands on its trail, about 84 px more, and fitted a spike the beside
/// view never shows.
#[test]
fn tape_only_leaves_out_the_spike_the_strip_hides_beside() {
    let ctx = egui::Context::default();
    let spike = trade_left_of_the_beside_window(&ctx);
    let share = quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE;
    let mut app = spiked_app(&ctx, Some(spike), share);
    let beside = auto_range(&app);
    assert!(beside.1 < 150.0, "beside leaves the spike out: {beside:?}");
    tape_only(&mut app, &ctx);
    assert_eq!(auto_range(&app), beside, "and so does tape only");
}

/// Every way back beside the candles opens at the default share, so tape only
/// fits against that view, whatever share the tape had before.
#[test]
fn tape_only_fits_the_beside_view_it_returns_to() {
    let ctx = egui::Context::default();
    let spike = trade_left_of_the_beside_window(&ctx);
    let mut app = spiked_app(&ctx, Some(spike), 0.2);
    let narrow_tape = auto_range(&app);
    assert!(
        narrow_tape.1 > 150.0,
        "a narrower tape shows the spike: {narrow_tape:?}"
    );
    tape_only(&mut app, &ctx);
    let alone = auto_range(&app);
    assert!(alone.1 < 150.0, "tape only leaves it out: {alone:?}");
    leave_tape_only(&mut app, &ctx);
    let lane = &app.active_tab().tape().cached_config().live_lane;
    assert_eq!(
        lane.width_share,
        quantick_orderflow::config::DEFAULT_LIVE_LANE_SHARE
    );
    assert_eq!(auto_range(&app), alone, "the beside view it returned to");
}
