//! The opt-in candle overlay uses the context pane's own exact ladders.
use super::*;
use quantick_engine::{BarFootprint, Side, Trade};

fn print(id: u64, timestamp_ms: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side,
    }
}

fn context_fixture(
    ctx: &egui::Context,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
) {
    let (mut app, events, commands, _book) = test_app();
    let prints = (0..20)
        .map(|index| {
            print(
                index + 1,
                1_000 + index as i64,
                100 + 5 * index as i64,
                1,
                Side::Buy,
            )
        })
        .collect();
    events.try_send(FeedEvent::Backfilled(prints)).unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    app.active_tab_mut().time_panes[0]
        .spec
        .update(
            quantick_engine::bar_selection::SelectionCommand::Replace(BarSpec::Tick(4).into()),
            quantick_engine::bar_selection::BarInputAvailability::PRINTS,
        )
        .unwrap();
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().time_panes[0].set_layer_visible(
        ChartLayer::Footprint,
        false,
        &mut Default::default(),
    );
    run_frame(&mut app, ctx);
    assert_eq!(
        app.active_tab().time_panes[0].state.spec(),
        &BarSpec::Tick(4)
    );
    assert_eq!(
        app.active_tab().time_panes[0].state.tape_price_step(),
        Some(Decimal::from(5))
    );
    (app, events, commands)
}

fn candle_ink(output: &egui::FullOutput, chart: egui::Rect) -> Vec<egui::Shape> {
    output
        .shapes
        .iter()
        .filter(|shape| shape.clip_rect == chart)
        .filter(|shape| {
            matches!(
                shape.shape,
                egui::Shape::Rect(_) | egui::Shape::LineSegment { .. }
            )
        })
        .map(|shape| shape.shape.clone())
        .collect()
}

fn circle_at(output: &egui::FullOutput, point: egui::Pos2) -> bool {
    output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Circle(circle)
        if circle.center.distance(point) < 0.01 && circle.radius > 0.0 && circle.radius <= 4.0 && circle.fill.a() <= 100))
}

fn row_totals(ladder: &BarFootprint) -> (Decimal, Decimal, u64) {
    ladder
        .levels()
        .values()
        .fold((Decimal::ZERO, Decimal::ZERO, 0), |sum, level| {
            (
                sum.0 + level.buy,
                sum.1 + level.sell,
                sum.2 + level.trade_count,
            )
        })
}

#[test]
fn candle_aggression_opt_in_preserves_candles_and_the_right_tape() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    let layer = ChartLayer::CandleAggression;
    for (pane, _) in app.active_tab().panes() {
        assert!(
            !pane.layer_switched_on(layer, &app.style),
            "ordinary/BTC defaults stay off"
        );
    }
    let before = run_frame(&mut app, &ctx);
    let left = &app.active_tab().time_panes[0];
    assert!(
        left.state.bar_footprints().is_empty(),
        "no consumer allocates ladders yet"
    );
    let chart = left.frame.chart_rect.unwrap();
    let ink = candle_ink(&before, chart);
    assert!(!ink.is_empty(), "the test observes actual candle paint");
    let tape = format!("{:?}", app.active_tab().tape().cached_config());
    app.active_tab_mut().time_panes[0].set_layer_visible(layer, true, &mut Default::default());
    let after = run_frame(&mut app, &ctx);
    let left = &app.active_tab().time_panes[0];
    assert!(
        left.orderflow.is_none(),
        "context panes never acquire a book worker"
    );
    assert_eq!(left.state.bar_footprints().len(), 5);
    assert!(!left.layer_switched_on(ChartLayer::Footprint, &app.style));
    assert_eq!(
        candle_ink(&after, chart),
        ink,
        "accumulation cannot invoke footprint candle dressing"
    );
    assert_eq!(
        format!("{:?}", app.active_tab().tape().cached_config()),
        tape
    );
    assert!(
        !app.active_tab()
            .flow_pane
            .layer_switched_on(layer, &app.style)
    );
    app.active_tab_mut().time_panes[0].set_layer_visible(layer, false, &mut Default::default());
    run_frame(&mut app, &ctx);
    assert!(
        app.active_tab().time_panes[0]
            .state
            .bar_footprints()
            .is_empty(),
        "the last consumer releases the ladders"
    );
}

#[test]
fn candle_aggression_keeps_same_millisecond_tick_ownership_and_current_partial() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands) = context_fixture(&ctx);
    app.active_tab_mut().time_panes[0].set_layer_visible(
        ChartLayer::CandleAggression,
        true,
        &mut Default::default(),
    );
    run_frame(&mut app, &ctx);
    for (id, price, quantity, side) in [
        (21, 120, 1, Side::Buy),
        (22, 125, 2, Side::Sell),
        (23, 130, 3, Side::Buy),
        (24, 135, 4, Side::Sell),
        (25, 140, 9, Side::Buy),
    ] {
        events
            .try_send(FeedEvent::Live(print(id, 10_000, price, quantity, side)))
            .unwrap();
    }
    run_frame(&mut app, &ctx);
    let state = &app.active_tab().time_panes[0].state;
    assert_eq!(state.bars().len(), 6);
    assert_eq!(
        row_totals(&state.bar_footprints()[5]),
        (Decimal::from(4), Decimal::from(6), 4)
    );
    assert_eq!(
        row_totals(state.partial_footprint().unwrap()),
        (Decimal::from(9), Decimal::ZERO, 1)
    );
    // The next print remains in that forming bar. No bar-close refresh may
    // rescue a renderer that accidentally reads the throttled ladder snapshot.
    events
        .try_send(FeedEvent::Live(print(26, 10_000, 145, 16, Side::Buy)))
        .unwrap();
    let output = run_frame(&mut app, &ctx);
    let left = &app.active_tab().time_panes[0];
    assert_eq!(
        row_totals(left.state.partial_footprint().unwrap()),
        (Decimal::from(25), Decimal::ZERO, 2)
    );
    let chart = left.frame.chart_rect.unwrap();
    let range = left.price_view.resolve(left.frame.auto_range.unwrap());
    let scale = PriceScale::from_range(range.0, range.1, chart.top(), chart.bottom());
    let x = left.viewport.x_center(6, chart.right(), left.slots());
    assert!(
        circle_at(&output, egui::pos2(x, scale.y(145.0))),
        "the current native-price print is painted in its actual tick-bar slot this frame"
    );
}

#[test]
fn candle_aggression_is_reachable_by_the_existing_named_layer_action() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    let pane_id = app.active_tab().time_panes[0].id.to_string();
    let directory = gateway_test_directory("candle-aggression-layer");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    for visible in [true, false] {
        let input = json!({"tab_id":app.tabs.active_id().to_string(),"pane_id":pane_id,"layer_id":"candle_aggression","visible":visible});
        let (response, _) = unkeyed_call(&mut app, &mut client, "layers.visibility.set", input);
        let result = success_result(&response);
        assert_eq!(result["layer"]["requested"], visible);
        assert_eq!(result["layer"]["effective"], visible);
        assert_eq!(result["layer"]["scope"], "pane");
        let (read, _) = unkeyed_call(
            &mut app,
            &mut client,
            "snapshot.read",
            json!({"scopes":["layers.visibility"]}),
        );
        let scope = &success_result(&read)["scopes"]["layers.visibility"]["value"];
        let pane = scope["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pane| pane["pane_id"] == pane_id)
            .unwrap();
        let layer = pane["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|layer| layer["id"] == "candle_aggression")
            .unwrap();
        assert_eq!(layer["requested"], visible);
        assert_eq!(layer["effective"], visible);
        assert!(app.active_tab().time_panes[0].orderflow.is_none());
    }
    disable_test_gateway(&mut app, &ctx);
}

#[test]
fn candle_aggression_paints_after_co_enabled_footprint_plates() {
    let ctx = egui::Context::default();
    let (mut app, _events, _commands) = context_fixture(&ctx);
    let left = &mut app.active_tab_mut().time_panes[0];
    left.viewport.zoom(5.0);
    left.set_footprint_override(Some(crate::footprint_config::FootprintConfig {
        style: crate::footprint_config::FootprintStyle::Split,
        show_numbers: false,
        show_delta_totals: false,
        ..Default::default()
    }));
    for layer in [ChartLayer::Footprint, ChartLayer::CandleAggression] {
        left.set_layer_visible(layer, true, &mut Default::default());
    }
    run_frame(&mut app, &ctx);
    let output = run_frame(&mut app, &ctx);
    let chart = app.active_tab().time_panes[0].frame.chart_rect.unwrap();
    let mut witnessed = false;
    for (dot_index, shape) in output
        .shapes
        .iter()
        .enumerate()
        .filter(|(_, shape)| shape.clip_rect == chart)
    {
        let egui::Shape::Circle(dot) = &shape.shape else {
            continue;
        };
        if dot.fill.a() != 89 {
            continue;
        }
        for (plate_index, shape) in output
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, shape)| shape.clip_rect == chart)
        {
            if let egui::Shape::Rect(plate) = &shape.shape
                && plate.rounding == egui::Rounding::same(2.0)
                && plate.fill != egui::Color32::TRANSPARENT
                && plate.rect.contains(dot.center)
            {
                witnessed = true;
                assert!(
                    plate_index < dot_index,
                    "a footprint plate must never cover an enabled native-price dot"
                );
            }
        }
    }
    assert!(
        witnessed,
        "the fixture must exercise overlapping footprint plates and aggression dots"
    );
}
