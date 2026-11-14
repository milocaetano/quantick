use super::*;
use crate::indicators::IndicatorViews;
use quantick_engine::{BarBuilder as _, Side, TickBarBuilder, Trade, golden as engine_golden};

/// The ladder of `run` as the worker samples it.
fn lane_prefixes(run: &[Trade], rungs: usize) -> Vec<Bar> {
    let mut forming = FormingRun::default();
    forming.extend(run.to_vec());
    forming.prefixes(rungs)
}
use quantick_indicators::{PlotId, SourceId, native::Ema};
use rust_decimal::Decimal;

pub(super) fn trade(i: u64) -> Trade {
    Trade {
        agg_id: i,
        timestamp_ms: 1_000 + i as i64 * 100,
        // A deterministic ±2 wiggle so the EMA has something to smooth.
        price: Decimal::from(100 + (i % 5) as i64 - 2),
        quantity: Decimal::ONE,
        side: if i.is_multiple_of(2) {
            Side::Buy
        } else {
            Side::Sell
        },
    }
}

fn bars_and_partial(n_trades: u64, tick: u64) -> (Vec<Bar>, Option<Bar>) {
    let trades: Vec<Trade> = (1..=n_trades).map(trade).collect();
    let mut builder = TickBarBuilder::new(tick);
    let bars = engine_golden::replay(&mut builder, &trades);
    (bars, builder.partial().cloned())
}

/// The §6 host/worker property: applying the delta-event stream to the
/// UI-side views reproduces exactly the columns a directly-driven host
/// holds — backfill, live closes, preview and all.
#[test]
fn delta_stream_replays_to_identical_columns() {
    let (bars, partial) = bars_and_partial(11, 2);
    let split = bars.len() - 2;

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Native {
            id: "native.ema".to_owned(),
            values: vec![InputValue::Int(3), InputValue::Source(SourceId::Close)],
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars[..split].to_vec()));
    for bar in &bars[split..] {
        worker.send(IndicatorCommand::BarClosed(bar.clone()));
    }
    worker.send(IndicatorCommand::PartialUpdated {
        partial: partial.clone(),
        run: Vec::new(),
        rungs: 0,
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(Ema::new(3, SourceId::Close)));
    for bar in &bars {
        host.push_closed_bar(bar);
    }
    host.set_partial(partial.as_ref());

    let view = &views.all()[0];
    assert_eq!(view.descriptor.title, "EMA(3)");
    assert_eq!(
        format!("{:?}", view.columns[0]),
        format!("{:?}", host.plots(id).unwrap().column(PlotId::new(0))),
        "delta replay must equal the direct host run, bit for bit"
    );
    assert!(partial.is_some(), "fixture must exercise the preview path");
    assert_eq!(
        view.preview.as_ref().map(|f| format!("{:?}", f.values)),
        host.preview(id).map(|f| format!("{:?}", f.values)),
    );
}

/// A Rebuild (spec switch shape) replaces the columns wholesale and the
/// result matches a host fed the new bars from scratch.
#[test]
fn rebuild_resyncs_the_views() {
    let (coarse, _) = bars_and_partial(12, 3);
    let (fine, fine_partial) = bars_and_partial(12, 2);

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(coarse));
    worker.send(IndicatorCommand::Rebuild(
        fine.clone(),
        fine_partial.clone(),
    ));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    let mut host = IndicatorHost::new();
    let id = host.add(Box::new(quantick_indicators::native::Cvd::new()));
    host.rebuild(&fine, fine_partial.as_ref());

    let view = &views.all()[0];
    assert_eq!(
        format!("{:?}", view.columns[0]),
        format!("{:?}", host.plots(id).unwrap().column(PlotId::new(0))),
    );
    assert_eq!(view.rows, fine.len());
}

/// Removing a slot stops its events; the other slot keeps flowing.
#[test]
fn remove_isolates_the_survivor() {
    let (bars, _) = bars_and_partial(8, 2);
    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let doomed = views.allocate_slot("test.indicator");
    let survivor = views.allocate_slot("test.indicator");
    for (slot, source) in [
        (
            doomed,
            IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        ),
        (
            survivor,
            IndicatorSource::Native {
                id: "native.cvd".to_owned(),
                values: Vec::new(),
            },
        ),
    ] {
        worker.send(IndicatorCommand::Add { slot, source });
    }
    worker.send(IndicatorCommand::Backfilled(bars[..2].to_vec()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert_eq!(views.all().len(), 2);

    views.remove(doomed);
    worker.send(IndicatorCommand::Remove(doomed));
    worker.send(IndicatorCommand::BarClosed(bars[2].clone()));
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert_eq!(views.all().len(), 1);
    assert_eq!(views.all()[0].slot, survivor);
    assert_eq!(views.all()[0].rows, 3, "the survivor saw the new bar");
}

/// The rung budget is a ceiling on cost, and the newest print is never the
/// one dropped to meet it: the lane's right edge is the live edge.
#[test]
fn a_ladder_respects_its_budget_and_always_reaches_the_newest_print() {
    let run: Vec<Trade> = (1..=50).map(trade).collect();

    for rungs in [1_usize, 3, 7, 64] {
        let prefixes = lane_prefixes(&run, rungs);
        assert!(
            prefixes.len() <= rungs.max(1) + 1,
            "{rungs} rungs asked for, {} produced",
            prefixes.len()
        );
        let last = prefixes.last().expect("a non-empty run has rungs");
        assert_eq!(
            last.trade_count,
            run.len() as u64,
            "the last rung is the whole run"
        );
        assert_eq!(last.close_time, run[run.len() - 1].timestamp_ms);
    }
}

/// A rung is a prefix of the run, folded exactly as a builder folds. The
/// last one must therefore *be* the forming bar the chart is drawing —
/// otherwise the lane's right edge and the candle beside it disagree.
#[test]
fn the_last_rung_is_the_forming_bar_itself() {
    let (_, partial) = bars_and_partial(11, 4);
    let partial = partial.expect("the fixture leaves a bar forming");
    let run: Vec<Trade> = (1..=11)
        .map(trade)
        .filter(|t| t.timestamp_ms >= partial.open_time)
        .collect();

    let prefixes = lane_prefixes(&run, 8);
    assert_eq!(prefixes.last(), Some(&partial));
}

/// No lane (`rungs == 0`) and no run are both "walk nothing": a chart
/// without a tape must not pay for a ladder it cannot draw.
#[test]
fn no_lane_and_no_run_both_produce_no_rungs() {
    let run: Vec<Trade> = (1..=5).map(trade).collect();
    assert!(lane_prefixes(&run, 0).is_empty());
    assert!(lane_prefixes(&[], 16).is_empty());
}

/// End to end through the worker: a partial published with its run comes
/// back as lane samples whose last rung equals the slot's own preview.
/// The curve's live end and the pane's headline number are the same fact,
/// and this is what keeps them from drifting apart.
#[test]
fn the_lane_samples_end_where_the_preview_does() {
    let (bars, partial) = bars_and_partial(11, 4);
    let partial = partial.expect("the fixture leaves a bar forming");
    let run: Vec<Trade> = (1..=11)
        .map(trade)
        .filter(|t| t.timestamp_ms >= partial.open_time)
        .collect();

    let (worker, run_worker) =
        IndicatorWorker::prepared_for_test(crate::worker_progress::monotonic());
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.send(IndicatorCommand::PartialUpdated {
        partial: Some(partial),
        run: run.clone(),
        rungs: 8,
    });
    // Keep the lane update and barrier in one batch: a later Flush-only
    // batch publishes an empty Lane in the existing event protocol.
    let (ack_tx, ack_rx) = channel();
    worker.send(WorkerCommand::Flush(ack_tx));
    let thread = std::thread::spawn(run_worker);
    worker
        .await_reply(&ack_rx)
        .expect("the lane batch finishes publication");
    for event in worker.drain_events() {
        views.apply(event);
    }

    drop(worker);
    thread.join().expect("the indicator worker closes normally");

    let view = &views.all()[0];
    assert!(
        !view.lane.is_empty(),
        "a run with a lane budget produces rungs"
    );
    assert_eq!(
        view.lane.last().map(|sample| sample.close_time),
        run.last().map(|t| t.timestamp_ms),
        "the newest rung sits at the newest print"
    );
    assert_eq!(
        view.lane
            .last()
            .map(|sample| format!("{:?}", sample.values)),
        view.preview
            .as_ref()
            .map(|frame| format!("{:?}", frame.values)),
        "the lane's live end is the preview"
    );
    assert!(
        view.lane
            .windows(2)
            .all(|pair| pair[0].close_time <= pair[1].close_time),
        "rungs are in occurrence order"
    );
}

/// A publish with no lane budget clears whatever the lane was drawing.
/// Toggling the tape off must not leave a frozen curve behind.
#[test]
fn dropping_the_lane_clears_the_samples_already_published() {
    let (bars, partial) = bars_and_partial(11, 4);
    let partial = partial.expect("the fixture leaves a bar forming");
    let run: Vec<Trade> = (1..=11)
        .map(trade)
        .filter(|t| t.timestamp_ms >= partial.open_time)
        .collect();

    let worker = IndicatorWorker::spawn();
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    worker.send(IndicatorCommand::Add {
        slot,
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    });
    worker.send(IndicatorCommand::Backfilled(bars.clone()));
    worker.send(IndicatorCommand::PartialUpdated {
        partial: Some(partial.clone()),
        run,
        rungs: 8,
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }
    assert!(!views.all()[0].lane.is_empty(), "the fixture drew a lane");

    worker.send(IndicatorCommand::PartialUpdated {
        partial: Some(partial),
        run: Vec::new(),
        rungs: 0,
    });
    worker.flush();
    for event in worker.drain_events() {
        views.apply(event);
    }

    let view = &views.all()[0];
    assert!(view.lane.is_empty(), "no lane, no curve");
    assert!(
        view.preview.is_some(),
        "and the forming bar still previews — the walk restored it"
    );
}
