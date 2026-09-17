use super::*;

fn prints(start: u64, end: u64) -> Vec<quantick_engine::Trade> {
    (start..end).map(trade).collect()
}

fn anchor(app: &QuantickApp) -> (f32, Option<i64>, bool) {
    let drawing = &app.active_tab().flow_pane.drawings.items()[0];
    (
        drawing.points[0].bar,
        drawing.points[0].time_ms,
        drawing.off_series,
    )
}

#[test]
fn source_drain_reset_retains_empty_anchor_debt_then_settles_populated_series() {
    let (mut app, events, _commands, _book) = test_app();
    events
        .try_send(FeedEvent::Backfilled(prints(100, 250)))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().flow_pane.slots(), 3);
    let tool = drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .unwrap();
    let pane = &mut app.active_tab_mut().flow_pane;
    pane.drawings.place(tool, ChartPoint::at(1.5, 100.0));
    pane.drawings.place(tool, ChartPoint::at(2.5, 110.0));
    pane.drawings.set_times(
        0,
        &[Some(trade(175).timestamp_ms), Some(trade(225).timestamp_ms)],
    );
    let before = anchor(&app);
    assert_eq!(before, (1.5, Some(trade(175).timestamp_ms), false));
    events.try_send(FeedEvent::Reset).unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().flow_pane.slots(), 0);
    assert_eq!(anchor(&app), before);
    app.active_tab_mut().drain_feed();
    assert_eq!(
        anchor(&app),
        before,
        "empty drain cannot consume the anchor debt"
    );
    // One older bar shifts the same market instant from slot1 to slot2.
    events
        .try_send(FeedEvent::Backfilled(prints(50, 250)))
        .unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().flow_pane.slots(), 4);
    let settled = anchor(&app);
    assert_eq!(settled, (2.5, Some(trade(175).timestamp_ms), false));
    // A real user edit after settlement survives another empty drain: debt is gone.
    app.active_tab_mut().flow_pane.drawings.items_mut()[0].points[0].bar = 3.25;
    app.active_tab_mut().drain_feed();
    assert_eq!(anchor(&app), (3.25, Some(trade(175).timestamp_ms), false));
    eprintln!(
        "SOURCE_ANCHOR before={before:?} populated={settled:?} next_empty={:?}",
        anchor(&app)
    );
}

#[test]
fn source_drain_intrinsic_and_final_publication_counts_are_literal() {
    let cases = [
        ("empty", vec![], 0, 0),
        ("empty-live-batch", vec![FeedEvent::LiveBatch(vec![])], 0, 0),
        (
            "live",
            vec![FeedEvent::Live(trade(101)), FeedEvent::Live(trade(102))],
            1,
            1,
        ),
        (
            "live-batch",
            vec![FeedEvent::LiveBatch(prints(101, 103))],
            1,
            1,
        ),
        (
            "backfill",
            vec![FeedEvent::Backfilled(prints(100, 160))],
            1,
            0,
        ),
        ("empty-backfill", vec![FeedEvent::Backfilled(vec![])], 1, 0),
        (
            "prepend",
            vec![FeedEvent::HistoryPrepended(prints(10, 20))],
            1,
            0,
        ),
        (
            "empty-prepend",
            vec![FeedEvent::HistoryPrepended(vec![])],
            1,
            0,
        ),
        (
            "opening-prepend",
            vec![FeedEvent::OpeningPrepended {
                trades: prints(10, 20),
                remaining: Some(0),
            }],
            1,
            0,
        ),
        ("reset", vec![FeedEvent::Reset], 2, 0),
        (
            "live-reset",
            vec![FeedEvent::Live(trade(101)), FeedEvent::Reset],
            3,
            1,
        ),
        (
            "reset-live",
            vec![FeedEvent::Reset, FeedEvent::Live(trade(101))],
            3,
            1,
        ),
        (
            "backfill-live",
            vec![
                FeedEvent::Backfilled(prints(100, 160)),
                FeedEvent::Live(trade(160)),
            ],
            2,
            1,
        ),
    ];
    for (label, queued, expected, expected_clock) in cases {
        let (mut app, events, _commands, _book) = test_app();
        let ctx = egui::Context::default();
        app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        assert_eq!(app.active_tab().panes().count(), 2);
        let before: Vec<_> = app
            .active_tab()
            .panes()
            .map(|(pane, side)| (side, pane.indicator_worker.partial_updates_for_test()))
            .collect();
        for event in queued {
            events.try_send(event).unwrap();
        }
        let mut clocks = 0;
        app.active_tab_mut().drain_feed_with_clock(|| {
            clocks += 1;
            100_000
        });
        assert_eq!(clocks, expected_clock, "{label}: lazy arrival clock");
        for (side, before) in before {
            let pane = app.active_tab().pane(side);
            let sent = pane.indicator_worker.partial_updates_for_test() - before;
            assert_eq!(
                sent, expected,
                "{label} {side:?}: intrinsic plus final publications"
            );
            eprintln!(
                "SOURCE_PUBLICATION case={label} pane={side:?} partial_sends={sent} clocks={clocks} trades={} partial={:?}",
                pane.state.trades().len(),
                pane.state.partial()
            );
        }
    }
}

#[test]
fn source_drain_same_interpreter_early_reanchor_leaves_real_coordinates_stale() {
    use quantick_chart_interaction::source_drain_plan::{SourceDrainPlan, SourceDrainStage::*};
    for early in [false, true] {
        let (mut app, events, _commands, _book) = test_app();
        events
            .try_send(FeedEvent::Backfilled(prints(100, 250)))
            .unwrap();
        app.active_tab_mut().drain_feed();
        let tool = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "rectangle")
            .unwrap();
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.drawings.place(tool, ChartPoint::at(1.5, 100.0));
        pane.drawings.place(tool, ChartPoint::at(2.5, 110.0));
        pane.drawings.set_times(
            0,
            &[Some(trade(175).timestamp_ms), Some(trade(225).timestamp_ms)],
        );
        events.try_send(FeedEvent::Reset).unwrap();
        app.active_tab_mut().drain_feed();
        app.active_tab_mut().drain_feed();
        assert_eq!(anchor(&app), (1.5, Some(18500), false));
        events
            .try_send(FeedEvent::Backfilled(prints(50, 250)))
            .unwrap();
        if early {
            app.active_tab_mut().drain_feed_with_stages(
                || panic!("backfill must not read the arrival clock"),
                [
                    PrepareSymbol,
                    SettleReanchors,
                    ReceiveAvailable,
                    PublishLatestPartial,
                    LandGap,
                    TickDealRecording,
                ],
            );
        } else {
            app.active_tab_mut().drain_feed_with_stages(
                || panic!("backfill must not read the arrival clock"),
                SourceDrainPlan::stages(),
            );
        }
        assert_eq!(app.active_tab().flow_pane.slots(), 4);
        assert_eq!(
            anchor(&app),
            (if early { 1.5 } else { 2.5 }, Some(18500), false)
        );
        app.active_tab_mut().drain_feed();
        assert_eq!(
            anchor(&app),
            (2.5, Some(18500), false),
            "early schedule only catches up next drain"
        );
    }
}

fn finish_indicator_commands(pane: &mut pane::ChartPane) {
    let (tx, rx) = std::sync::mpsc::channel();
    pane.indicator_worker
        .send(crate::indicator_worker::IndicatorCommand::Flush(tx));
    assert_eq!(
        pane.indicator_worker.await_reply(&rx),
        Some(()),
        "actual worker publication acknowledgement"
    );
    pane.apply_indicator_events();
}

#[test]
fn source_drain_same_interpreter_early_publication_omits_the_actual_worker_preview() {
    use quantick_chart_interaction::source_drain_plan::{SourceDrainPlan, SourceDrainStage::*};
    for early in [false, true] {
        let (mut app, events, _commands, _book) = test_app();
        let pane = &mut app.active_tab_mut().flow_pane;
        pane.add_indicator(crate::indicator_worker::IndicatorSource::Script {
            name: "Drain close".to_owned(),
            text: "indicator(\"Drain close\")\nplot(close)".to_owned(),
        });
        finish_indicator_commands(pane);
        assert_eq!(pane.indicators.all().len(), 1);
        assert!(pane.indicators.all()[0].error.is_none());
        assert!(pane.indicators.all()[0].preview.is_none());
        let before = pane.indicator_worker.partial_updates_for_test();
        events
            .try_send(FeedEvent::LiveBatch(prints(101, 103)))
            .unwrap();
        if early {
            app.active_tab_mut().drain_feed_with_stages(
                || 100_000,
                [
                    PrepareSymbol,
                    PublishLatestPartial,
                    ReceiveAvailable,
                    LandGap,
                    SettleReanchors,
                    TickDealRecording,
                ],
            );
        } else {
            app.active_tab_mut()
                .drain_feed_with_stages(|| 100_000, SourceDrainPlan::stages());
        }
        let pane = &mut app.active_tab_mut().flow_pane;
        assert_eq!(pane.state.trades().len(), 2);
        let partial = pane.state.partial().unwrap();
        assert_eq!(
            (partial.open, partial.close, partial.close_time),
            (Decimal::new(1001, 1), Decimal::new(1002, 1), 11200)
        );
        assert_eq!(
            pane.indicator_worker.partial_updates_for_test() - before,
            usize::from(!early)
        );
        finish_indicator_commands(pane);
        let view = &pane.indicators.all()[0];
        assert!(view.error.is_none());
        if early {
            assert!(
                view.preview.is_none(),
                "ingress occurred but the worker received no latest partial"
            );
        } else {
            assert_eq!(view.preview.as_ref().unwrap().values, vec![100.2]);
        }
    }
}
