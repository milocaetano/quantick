use super::*;
use crate::indicators::IndicatorViews;
use quantick_engine::Side;
use rust_decimal::Decimal;

fn print(id: u64, quantity: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: id as i64,
        price: Decimal::from(100),
        quantity: Decimal::from(quantity.abs()),
        side: if quantity > 0 { Side::Buy } else { Side::Sell },
    }
}

fn bar(run: &[Trade]) -> Bar {
    let mut bar = Bar::opened_by(&run[0]);
    for trade in &run[1..] {
        bar.extend(trade);
    }
    bar
}

fn update(run: &[Trade], new: &[Trade], rungs: usize) -> IndicatorCommand {
    IndicatorCommand::PartialUpdated {
        partial: (!run.is_empty()).then(|| bar(run)),
        run: new.to_vec(),
        rungs,
    }
}

fn add() -> IndicatorCommand {
    IndicatorCommand::Add {
        slot: SlotId(0),
        source: IndicatorSource::Native {
            id: "native.cvd".to_owned(),
            values: Vec::new(),
        },
    }
}

/// Queue everything before running: this exercises one actual worker batch.
fn one_batch(commands: Vec<IndicatorCommand>) -> (IndicatorViews, Vec<LaneSample>) {
    let (tx, rx) = sync_channel(INDICATOR_COMMAND_QUEUE);
    let (events, output) = sync_channel(INDICATOR_EVENT_QUEUE);
    let progress = crate::worker_progress::monotonic();
    let observed = progress.consumer();
    let tx = progress.bind_merging(tx, fold_commands);
    for command in commands {
        tx.send(command.into()).unwrap();
    }
    drop(tx);
    run_observed(&rx, &events, observed);
    drop(events);
    collect(output.try_iter().collect())
}

/// Flush-only batches may emit an empty Lane in the existing event protocol.
/// Keep the last nonempty publication to inspect the evaluated partial itself.
fn collect(events: Vec<IndicatorEvent>) -> (IndicatorViews, Vec<LaneSample>) {
    let mut views = IndicatorViews::new();
    views.allocate_slot("native.cvd");
    let mut last_lane = Vec::new();
    for event in events {
        if let IndicatorEvent::Lane { samples, .. } = &event
            && !samples.is_empty()
        {
            last_lane = samples.clone();
        }
        views.apply(event);
    }
    (views, last_lane)
}

fn assert_final(views: &IndicatorViews, lane: &[LaneSample]) {
    // History is +10. The signed live prints are +2,-1,+4,-2,+3,-1,+5.
    // Seven prints sampled with three rungs end at prints 3,6,7.
    assert_eq!(views.all()[0].columns, vec![vec![10.0]]);
    assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![20.0]);
    assert_eq!(
        lane,
        &[
            LaneSample {
                close_time: 3,
                values: vec![15.0]
            },
            LaneSample {
                close_time: 6,
                values: vec![15.0]
            },
            LaneSample {
                close_time: 7,
                values: vec![20.0]
            },
        ]
    );
}

#[test]
fn irregular_drains_and_batched_commands_match_independent_cvd_rungs() {
    let prints: Vec<_> = [2, -1, 4, -2, 3, -1, 5]
        .into_iter()
        .enumerate()
        .map(|(index, quantity)| print(index as u64 + 1, quantity))
        .collect();
    let history = bar(&[print(0, 10)]);
    let (whole, whole_lane) = one_batch(vec![
        add(),
        IndicatorCommand::Backfilled(vec![history.clone()]),
        update(&prints, &prints, 3),
    ]);
    assert_final(&whole, &whole_lane);
    let (batched, batched_lane) = one_batch(vec![
        add(),
        IndicatorCommand::Backfilled(vec![history.clone()]),
        update(&prints[..1], &prints[..1], 3),
        update(&prints[..4], &prints[1..4], 3),
        update(&prints[..4], &[], 3),
        update(&prints[..6], &prints[4..6], 3),
        update(&prints, &prints[6..], 3),
    ]);
    assert_final(&batched, &batched_lane);
    let worker = IndicatorWorker::spawn();
    worker.send(add());
    worker.send(IndicatorCommand::Backfilled(vec![history]));
    let mut before = 0;
    let mut events = Vec::new();
    for after in [1, 4, 4, 6, 7] {
        worker.send(update(&prints[..after], &prints[before..after], 3));
        worker.flush();
        events.extend(worker.drain_events());
        assert_eq!(worker.retained_lane_for_test().0, after);
        before = after;
    }
    let (separate, separate_lane) = collect(events);
    assert_final(&separate, &separate_lane);
}

#[test]
fn close_cuts_an_earlier_partial_before_the_next_run_in_the_same_batch() {
    let old = [print(1, 2), print(2, -1)];
    let next = [print(3, 7), print(4, -2)];
    let (views, lane) = one_batch(vec![
        add(),
        IndicatorCommand::Backfilled(vec![bar(&[print(0, 10)])]),
        update(&old, &old, 8),
        IndicatorCommand::BarClosed(bar(&old)),
        update(&next, &next[..1], 8),
        update(&next, &next[1..], 8),
    ]);
    assert_eq!(views.all()[0].columns, vec![vec![10.0, 11.0]]);
    assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![16.0]);
    assert_eq!(
        lane,
        vec![
            LaneSample {
                close_time: 3,
                values: vec![18.0]
            },
            LaneSample {
                close_time: 4,
                values: vec![16.0]
            },
        ]
    );
    let (closed, lane) = one_batch(vec![
        add(),
        update(&old, &old, 8),
        IndicatorCommand::BarClosed(bar(&old)),
    ]);
    assert_eq!(closed.all()[0].columns, vec![vec![1.0]]);
    assert!(closed.all()[0].preview.is_none());
    assert!(lane.is_empty());
}

#[test]
fn rebuild_backfill_vanished_partial_and_disabled_lane_cut_stale_runs() {
    let stale = [print(1, 90)];
    let current = [print(2, 3), print(3, -1)];
    for reset in [
        IndicatorCommand::Rebuild(vec![bar(&[print(0, 20)])], None),
        IndicatorCommand::Backfilled(vec![bar(&[print(0, 20)])]),
    ] {
        let (views, lane) = one_batch(vec![
            add(),
            update(&stale, &stale, 8),
            reset,
            update(&current, &current, 8),
        ]);
        assert_eq!(views.all()[0].columns, vec![vec![20.0]]);
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![22.0]);
        assert_eq!(
            lane,
            vec![
                LaneSample {
                    close_time: 2,
                    values: vec![23.0]
                },
                LaneSample {
                    close_time: 3,
                    values: vec![22.0]
                },
            ]
        );
    }
    for reset in [update(&[], &[], 8), update(&stale, &[], 0)] {
        let (views, lane) = one_batch(vec![
            add(),
            update(&stale, &stale, 8),
            reset,
            update(&current, &current, 8),
            update(&current, &[], 8),
        ]);
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![2.0]);
        assert_eq!(
            lane,
            vec![
                LaneSample {
                    close_time: 2,
                    values: vec![3.0]
                },
                LaneSample {
                    close_time: 3,
                    values: vec![2.0]
                },
            ]
        );
    }
}

#[test]
fn retired_epoch_releases_peak_capacity_and_preserves_small_run_outputs() {
    fn publish(worker: &IndicatorWorker, views: &mut IndicatorViews) -> Vec<LaneSample> {
        worker.flush();
        let mut lane = Vec::new();
        for event in worker.drain_events() {
            // Flush/probe-only batches may publish an empty lane afterwards.
            if let IndicatorEvent::Lane { samples, .. } = &event
                && !samples.is_empty()
            {
                lane = samples.clone();
            }
            views.apply(event);
        }
        lane
    }

    let large: Vec<_> = (1..=4096)
        .map(|id| print(id, if id % 2 == 1 { 1 } else { -1 }))
        .collect();
    let history = bar(&[print(0, 10)]);
    for (reset, mut columns) in [
        (IndicatorCommand::BarClosed(bar(&large)), vec![10.0, 10.0]),
        (
            IndicatorCommand::Backfilled(vec![history.clone()]),
            vec![10.0],
        ),
        (
            IndicatorCommand::Rebuild(vec![history.clone()], None),
            vec![10.0],
        ),
    ] {
        let worker = IndicatorWorker::spawn();
        let mut views = IndicatorViews::new();
        views.allocate_slot("native.cvd");
        worker.send(add());
        worker.send(IndicatorCommand::Backfilled(vec![history.clone()]));
        worker.send(update(&large[..2048], &large[..2048], 2));
        publish(&worker, &mut views);
        assert_eq!(worker.retained_lane_for_test().0, 2048);
        worker.send(update(&large, &large[2048..], 2));
        let lane = publish(&worker, &mut views);
        let (length, peak) = worker.retained_lane_for_test();
        assert_eq!(length, 4096);
        assert!(peak >= 4096);
        assert_eq!(views.all()[0].columns, vec![vec![10.0]]);
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![10.0]);
        assert_eq!(
            lane,
            vec![
                LaneSample {
                    close_time: 2048,
                    values: vec![10.0]
                },
                LaneSample {
                    close_time: 4096,
                    values: vec![10.0]
                },
            ]
        );

        worker.send(reset);
        assert!(publish(&worker, &mut views).is_empty());
        assert_eq!(worker.retained_lane_for_test(), (0, 0));
        assert_eq!(views.all()[0].columns, vec![columns.clone()]);
        assert!(views.all()[0].preview.is_none());

        // Two successive small epochs must not inherit the retired peak.
        for (run, expected, preview) in [
            ([print(4097, 7), print(4098, -2)], [17.0, 15.0], 15.0),
            ([print(4099, 3), print(4100, -1)], [18.0, 17.0], 17.0),
        ] {
            worker.send(update(&run[..1], &run[..1], 2));
            publish(&worker, &mut views);
            assert_eq!(worker.retained_lane_for_test().0, 1);
            worker.send(update(&run, &run[1..], 2));
            let lane = publish(&worker, &mut views);
            let (length, capacity) = worker.retained_lane_for_test();
            assert_eq!(length, 2);
            assert!(capacity < peak);
            assert_eq!(views.all()[0].columns, vec![columns.clone()]);
            assert_eq!(
                views.all()[0].preview.as_ref().unwrap().values,
                vec![preview]
            );
            assert_eq!(
                lane,
                vec![
                    LaneSample {
                        close_time: run[0].timestamp_ms,
                        values: vec![expected[0]]
                    },
                    LaneSample {
                        close_time: run[1].timestamp_ms,
                        values: vec![expected[1]]
                    },
                ]
            );
            worker.send(update(&run, &[], 2));
            assert_eq!(publish(&worker, &mut views), lane);
            assert_eq!(worker.retained_lane_for_test(), (length, capacity));
            worker.send(IndicatorCommand::BarClosed(bar(&run)));
            assert!(publish(&worker, &mut views).is_empty());
            assert_eq!(worker.retained_lane_for_test(), (0, 0));
            columns.push(preview);
            assert_eq!(views.all()[0].columns, vec![columns.clone()]);
            assert!(views.all()[0].preview.is_none());
        }
    }
}
