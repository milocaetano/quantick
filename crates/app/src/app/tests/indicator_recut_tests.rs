//! A pane's indicators must follow its own bars through a spec change, whatever
//! the venue's refold does to the pane beside it.

use super::*;

fn finish_indicator_commands(pane: &mut pane::ChartPane) {
    let (tx, rx) = std::sync::mpsc::channel();
    pane.indicator_worker
        .send(crate::indicator_worker::WorkerCommand::Flush(tx));
    assert_eq!(pane.indicator_worker.await_reply(&rx), Some(()));
    pane.apply_indicator_events();
}

/// Finding: the refold reported "a pane's prefix changed" for the whole tab,
/// and the spec switch took that to mean its own pane had rebuilt. With the
/// time pane's prefix moving, the flow pane kept the rows of its old bars: a
/// force bar's colours landed on candles they were never measured on, and the
/// CVD drew the start of the old series against today's axis.
#[test]
fn a_spec_change_rebuilds_its_pane_when_only_the_neighbour_prefix_moves() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands, _book) = test_app();
    let tab_id = app.runtime.tabs.active_id();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    events
        .try_send(FeedEvent::Backfilled((1..=400).map(trade).collect()))
        .unwrap();
    app.active_tab_mut()
        .drain_feed_with_clock(tab_id, || 100_000);
    app.active_tab_mut().deliver_ohlcv_slice(
        tab_id,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(120),
        quantick_feed::OhlcvSlice::Last { complete: true },
    );

    let flow = &mut app.active_tab_mut().flow_pane;
    flow.add_indicator(crate::indicator_worker::IndicatorSource::Script {
        name: "Rows".to_owned(),
        text: "indicator(\"Rows\")\nplot(close)".to_owned(),
    });
    finish_indicator_commands(flow);
    let before = flow.indicators.all()[0].rows;

    // The neighbour's next refold has something to change.
    app.active_tab_mut().time_panes[0].install_history_prefix(venue_history(10));

    let spec = BarSpec::Tick(7);
    app.active_tab_mut().flow_pane.spec.retain(spec);
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();

    let flow = &mut app.active_tab_mut().flow_pane;
    assert_eq!(flow.state.spec(), &spec, "the bars were re-cut");
    finish_indicator_commands(flow);
    let bars = flow.series_read().closed_bars().len();
    assert_ne!(bars, before, "the new rule cuts a different number of bars");
    assert_eq!(
        flow.indicators.all()[0].rows,
        bars,
        "one indicator row per bar of the rule on screen"
    );
}

/// Whatever re-cut the bars without telling the worker, a settled worker's rows
/// come back to the chart's bars rather than painting the wrong candles for good.
#[test]
fn indicator_rows_cut_under_another_rule_heal_once_the_worker_settles() {
    let (mut app, events, _commands, _book) = test_app();
    let tab_id = app.runtime.tabs.active_id();
    events
        .try_send(FeedEvent::Backfilled((1..=400).map(trade).collect()))
        .unwrap();
    app.active_tab_mut()
        .drain_feed_with_clock(tab_id, || 100_000);
    let flow = &mut app.active_tab_mut().flow_pane;
    flow.add_indicator(crate::indicator_worker::IndicatorSource::Script {
        name: "Rows".to_owned(),
        text: "indicator(\"Rows\")
plot(close)"
            .to_owned(),
    });
    finish_indicator_commands(flow);

    // The startup-scoped setter sends no rebuild: the bars move, the rows do not.
    flow.set_spec(BarSpec::Tick(7));
    let bars = flow.series_read().closed_bars().len();
    assert_ne!(flow.indicators.all()[0].rows, bars, "out of step");

    flow.apply_indicator_events();
    finish_indicator_commands(flow);
    assert_eq!(flow.indicators.all()[0].rows, bars, "healed");
}

/// The same rule addressed at a context pane: the bit is the pane's own, so the
/// flow pane's moving prefix does not stand in for the time pane's rebuild.
#[test]
fn a_context_pane_spec_change_rebuilds_it_when_only_the_flow_prefix_moves() {
    let ctx = egui::Context::default();
    let (mut app, events, _commands, _book) = test_app();
    let tab_id = app.runtime.tabs.active_id();
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    events
        .try_send(FeedEvent::Backfilled((1..=400).map(trade).collect()))
        .unwrap();
    app.active_tab_mut()
        .drain_feed_with_clock(tab_id, || 100_000);
    app.active_tab_mut().deliver_ohlcv_slice(
        tab_id,
        quantick_feed::OHLCV_BASE_INTERVAL_MS,
        venue_history(120),
        quantick_feed::OhlcvSlice::Last { complete: true },
    );
    app.active_tab_mut()
        .flow_pane
        .install_history_prefix(venue_history(10));

    // The chart's selector moves off time, so the pane leaves its prefix too; the
    // rows are read against what it holds once the worker has answered.
    let time = &mut app.active_tab_mut().time_panes[0];
    time.add_indicator(crate::indicator_worker::IndicatorSource::Script {
        name: "Rows".to_owned(),
        text: "indicator(\"Rows\")
plot(close)"
            .to_owned(),
    });
    finish_indicator_commands(time);
    time.spec
        .update(
            quantick_engine::bar_selection::SelectionCommand::Select(
                crate::state::BarKind::Tick.label(),
            ),
            quantick_engine::bar_selection::BarInputAvailability::ALL,
        )
        .unwrap();
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();

    let time = &mut app.active_tab_mut().time_panes[0];
    finish_indicator_commands(time);
    assert_eq!(
        time.indicators.all()[0].rows,
        time.series_read().closed_bars().len(),
        "one indicator row per bar of the rule on screen"
    );
}
