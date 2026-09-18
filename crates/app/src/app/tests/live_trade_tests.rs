use super::*;
use quantick_chart_interaction::live_trade_plan::{LiveTradePlan, LiveTradeStage};

// The unchanged force-bar integration fixture's region, parameters and prints.
fn armed_app() -> (QuantickApp, mpsc::Sender<FeedEvent>, drawings::DrawingId) {
    let (mut app, events, _commands, _book) = test_app();
    let rectangle = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .unwrap();
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.drawings.place(rectangle, ChartPoint::at(0.0, 100.0));
    pane.drawings.place(rectangle, ChartPoint::at(30.0, 110.0));
    let drawing = pane.drawings.items()[0].id;
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.window = 3;
    form.min_range = "0".to_owned();
    app.tabs
        .runtime_mut(app.tabs.active_index())
        .arm_strategy_instance(
            &mut *app.audio.alerts,
            PaneSide::Flow,
            drawing,
            &form,
            "test BF".to_owned(),
        )
        .unwrap();
    (app, events, drawing)
}

fn print(id: u64, price: &str) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id: id,
        timestamp_ms: 1_700_000_000_000 + id as i64 * 100,
        price: Decimal::from_str_exact(price).unwrap(),
        quantity: Decimal::ONE,
        side: quantick_engine::Side::Buy,
    }
}

fn scenario() -> Vec<quantick_engine::Trade> {
    let mut trades = Vec::new();
    for (open, close) in [
        ("100", "101"),
        ("101", "102"),
        ("102", "103"),
        ("103", "107"),
    ] {
        for _ in 0..49 {
            trades.push(print(trades.len() as u64 + 1, open));
        }
        trades.push(print(trades.len() as u64 + 1, close));
    }
    trades.push(print(201, "107.5"));
    trades
}

fn ingest(app: &mut QuantickApp, trades: &[quantick_engine::Trade]) {
    for trade in trades {
        app.active_tab_mut()
            .ingest_live_trade_at(trade, trade.timestamp_ms);
    }
}

fn position(app: &QuantickApp) -> Option<quantick_sim::Position> {
    app.active_tab().paper.account().venue().position().cloned()
}

#[test]
fn live_trade_mutant_fills_on_the_trigger_print_instead_of_the_next_print() {
    let mut trades = scenario();
    // The original flat trigger leaves mark103, equal to its stop: an early
    // strategy would be refused before filling. Keep identical bar OHLC but
    // make the prior mark104 so this mutant isolates the fill-order hazard.
    trades[198].price = Decimal::from_str_exact("104").unwrap();
    let (mut canonical, _events, _) = armed_app();
    let (mut mutant, _events, _) = armed_app();
    ingest(&mut canonical, &trades[..199]);
    ingest(&mut mutant, &trades[..199]);
    assert!(position(&canonical).is_none());
    assert!(position(&mutant).is_none());
    assert_eq!(
        canonical.active_tab().paper.account().mark_price(),
        Some(trades[198].price)
    );
    assert_eq!(
        mutant.active_tab().paper.account().mark_price(),
        Some(trades[198].price)
    );

    ingest(&mut canonical, &trades[199..200]);
    let bar = canonical
        .active_tab()
        .flow_pane
        .state
        .bars()
        .last()
        .unwrap();
    assert_eq!(
        (bar.open, bar.low, bar.close, bar.high),
        (
            Decimal::from_str_exact("103").unwrap(),
            Decimal::from_str_exact("103").unwrap(),
            Decimal::from_str_exact("107").unwrap(),
            Decimal::from_str_exact("107").unwrap()
        )
    );
    mutant.active_tab_mut().ingest_live_trade_test_order(
        &trades[199],
        [
            LiveTradeStage::PaneTrades,
            LiveTradeStage::StrategyEvaluation,
            LiveTradeStage::PaperTrade,
        ],
        || panic!("an ordinary trading strategy must not read the alarm clock"),
    );
    assert!(
        !canonical.active_tab().paper.is_flat(),
        "the trigger queued an order"
    );
    assert!(
        position(&canonical).is_none(),
        "the triggering print was already consumed"
    );
    let early = position(&mutant).expect("the invalid order lets the trigger fill its own order");
    assert_eq!(
        (early.opened_agg_id, early.opened_ms, early.avg_price),
        (
            200,
            trades[199].timestamp_ms,
            Decimal::from_str_exact("107").unwrap()
        )
    );

    ingest(&mut canonical, &trades[200..]);
    let correct = position(&canonical).expect("the next real print fills the queued order");
    assert_eq!(
        (correct.opened_agg_id, correct.opened_ms, correct.avg_price),
        (
            201,
            trades[200].timestamp_ms,
            Decimal::from_str_exact("107.5").unwrap()
        )
    );
    assert_ne!(early.opened_ms, correct.opened_ms);
    assert_ne!(early.avg_price, correct.avg_price);
}

#[test]
fn live_trade_batches_preserve_per_print_fills_panes_and_one_arrival_clock_per_drain() {
    let trades = scenario();
    let (mut singles, single_events, single_drawing) = armed_app();
    let (mut batches, batch_events, batch_drawing) = armed_app();
    let mut single_clock_reads = 0;
    let mut batch_clock_reads = 0;
    for chunk in trades.chunks(50) {
        for trade in chunk {
            single_events
                .blocking_send(FeedEvent::Live(trade.clone()))
                .unwrap();
        }
        batch_events
            .blocking_send(FeedEvent::LiveBatch(chunk.to_vec()))
            .unwrap();
        let arrival = chunk.last().unwrap().timestamp_ms + 20;
        let single_id = singles.tabs.active_id();
        singles
            .active_tab_mut()
            .drain_feed_with_clock(single_id, || {
                single_clock_reads += 1;
                arrival
            });
        let batch_id = batches.tabs.active_id();
        batches
            .active_tab_mut()
            .drain_feed_with_clock(batch_id, || {
                batch_clock_reads += 1;
                arrival
            });
        assert_eq!(position(&singles), position(&batches));
        assert_eq!(
            singles.active_tab().live_trades,
            batches.active_tab().live_trades
        );
        let left: Vec<_> = singles.active_tab().panes().collect();
        let right: Vec<_> = batches.active_tab().panes().collect();
        assert_eq!(left.len(), right.len());
        for ((left, left_side), (right, right_side)) in left.into_iter().zip(right) {
            assert_eq!(left_side, right_side);
            assert_eq!(
                left.state.trades().iter().collect::<Vec<_>>(),
                right.state.trades().iter().collect::<Vec<_>>()
            );
            assert_eq!(left.state.bars(), right.state.bars());
            assert_eq!(left.state.partial(), right.state.partial());
        }
    }
    assert_eq!((single_clock_reads, batch_clock_reads), (5, 5));
    for (app, drawing) in [(&singles, single_drawing), (&batches, batch_drawing)] {
        let filled = position(app).unwrap();
        assert_eq!(
            (filled.opened_agg_id, filled.opened_ms, filled.avg_price),
            (
                201,
                trades[200].timestamp_ms,
                Decimal::from_str_exact("107.5").unwrap()
            )
        );
        assert_eq!(
            app.active_tab()
                .flow_pane
                .strategies
                .anchors
                .for_drawing(drawing)
                .unwrap()
                .armed
                .state(),
            &quantick_strategy::ArmedState::InPosition
        );
    }
}

#[test]
fn live_trade_resume_paths_seed_the_mark_without_filling_a_queued_order() {
    let trades = scenario();
    for kind in 0..5 {
        let (mut app, events, drawing) = armed_app();
        ingest(&mut app, &trades[..200]);
        assert!(!app.active_tab().paper.is_flat());
        assert!(position(&app).is_none());
        app.active_tab_mut().resume_floor_ms = Some(trades[199].timestamp_ms);
        let window = trades[199..].to_vec();
        let event = match kind {
            0 => FeedEvent::Backfilled(window),
            1 => FeedEvent::HistoryPrepended(window),
            2 => FeedEvent::OpeningPrepended {
                trades: window,
                remaining: Some(0),
            },
            3 => FeedEvent::LiveBatch(window),
            _ => FeedEvent::Live(trades[200].clone()),
        };
        events.blocking_send(event).unwrap();
        let tab_id = app.tabs.active_id();
        app.active_tab_mut()
            .drain_feed_with_clock(tab_id, || panic!("resumed history is not live arrival"));
        assert!(
            position(&app).is_none(),
            "resume path {kind} filled against history"
        );
        assert_eq!(app.active_tab().resume_floor_ms, None);
        assert_eq!(app.active_tab().live_trades, 200);
        assert_eq!(app.active_tab().history_trades, 1);
        assert_eq!(
            app.active_tab().paper.account().mark_price(),
            Some(trades[200].price)
        );
        assert_eq!(app.active_tab().flow_pane.state.trades().len(), 201);
        assert!(matches!(
            app.active_tab()
                .flow_pane
                .strategies
                .anchors
                .for_drawing(drawing)
                .unwrap()
                .armed
                .state(),
            quantick_strategy::ArmedState::Fired { .. }
        ));
        let next = print(202, "108");
        ingest(&mut app, std::slice::from_ref(&next));
        let filled = position(&app).unwrap();
        assert_eq!(
            (filled.opened_agg_id, filled.opened_ms, filled.avg_price),
            (202, next.timestamp_ms, next.price)
        );
    }
}

#[test]
fn live_trade_resume_does_not_evaluate_a_new_force_bar() {
    let trades = scenario();
    let (mut app, events, drawing) = armed_app();
    ingest(&mut app, &trades[..150]);
    // The first actual drain settles launch's deferred reanchor. This anchor
    // names the first real bar; an undated anchor would instead keep its
    // distance beyond the original empty series and exclude the trigger.
    app.active_tab_mut()
        .flow_pane
        .drawings
        .set_times(0, &[Some(trades[0].timestamp_ms), None]);
    assert!(
        app.active_tab()
            .flow_pane
            .strategies
            .region(&app.active_tab().flow_pane.drawings, drawing, 3)
            .unwrap()
            .1
    );
    app.active_tab_mut().resume_floor_ms = Some(trades[149].timestamp_ms);
    events
        .blocking_send(FeedEvent::Backfilled(trades[149..200].to_vec()))
        .unwrap();
    let tab_id = app.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 4);
    assert!(
        app.active_tab()
            .flow_pane
            .strategies
            .region(&app.active_tab().flow_pane.drawings, drawing, 3)
            .unwrap()
            .1
    );
    assert!(
        app.active_tab().paper.is_flat(),
        "no strategy command comes from resumed history"
    );
    assert!(position(&app).is_none());
    ingest(&mut app, &trades[200..]);
    assert!(
        !app.active_tab().paper.is_flat(),
        "the next ordinary trade runs the queued bar evaluation"
    );
    assert!(
        position(&app).is_none(),
        "evaluation still follows paper consumption"
    );
    assert!(matches!(
        app.active_tab()
            .flow_pane
            .strategies
            .anchors
            .for_drawing(drawing)
            .unwrap()
            .armed
            .state(),
        quantick_strategy::ArmedState::Fired { .. }
    ));
}

#[test]
fn live_trade_clock_is_read_only_when_an_alarm_is_present() {
    let (mut app, _events, drawing) = armed_app();
    let mut reads = 0;
    app.active_tab_mut().ingest_live_trade_test_order(
        &print(1, "100"),
        LiveTradePlan::stages(),
        || {
            reads += 1;
            1_000
        },
    );
    assert_eq!(reads, 0);
    let mut form =
        crate::strategy_presets::StoredPreset::starting_point(quantick_engine::Side::Buy);
    form.alarm = true;
    let setup = form.to_kernel().unwrap().alarm.unwrap();
    let instance = app
        .active_tab_mut()
        .flow_pane
        .strategies
        .anchors
        .instances
        .iter_mut()
        .find(|instance| instance.drawing == drawing)
        .unwrap();
    instance.alarm = Some(quantick_strategy::SignalAlarm::new(setup.params));
    app.active_tab_mut().ingest_live_trade_test_order(
        &print(2, "100"),
        LiveTradePlan::stages(),
        || {
            reads += 1;
            2_000
        },
    );
    assert_eq!(reads, 1);
}
