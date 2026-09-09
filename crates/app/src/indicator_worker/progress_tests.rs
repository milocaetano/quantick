use super::*;
use crate::worker_progress::{Age, Phase, tests::Gate};
use quantick_engine::Side;
use rust_decimal::Decimal;
use std::time::Duration;

fn print(id: u64, quantity: i64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: id as i64,
        price: Decimal::from(100),
        quantity: Decimal::from(quantity.abs()),
        side: if quantity > 0 { Side::Buy } else { Side::Sell },
    }
}
fn add() -> IndicatorCommand {
    IndicatorCommand::Add {
        slot: SlotId(0),
        source: IndicatorSource::Native {
            id: "native.cvd".into(),
            values: vec![],
        },
    }
}
fn acknowledge(worker: &IndicatorWorker) -> Receiver<()> {
    let (tx, rx) = channel();
    worker.send(IndicatorCommand::Flush(tx));
    rx
}
fn assert_output(events: Vec<IndicatorEvent>, preview: bool) {
    let mut views = crate::indicators::IndicatorViews::new();
    views.allocate_slot("native.cvd");
    let mut lane = vec![];
    for event in events {
        if let IndicatorEvent::Lane { samples, .. } = &event {
            lane = samples.clone();
        }
        views.apply(event);
    }
    assert_eq!(views.all()[0].columns, vec![vec![10.0]]);
    if preview {
        assert_eq!(views.all()[0].preview.as_ref().unwrap().values, vec![11.0]);
        assert_eq!(
            lane,
            vec![
                LaneSample {
                    close_time: 2,
                    values: vec![12.0]
                },
                LaneSample {
                    close_time: 3,
                    values: vec![11.0]
                }
            ]
        );
    }
}

#[test]
fn held_real_indicator_has_exact_backlog_progress_and_ordered_suffixes() {
    // Prescribed batches: Add (1), then Backfilled, two SetInputs, two
    // PartialUpdated and Flush (6). CVD: committed +10, preview +10+2-1.
    // Clock: first starts 0, ticket 2 accepted 10, second starts 40, ends 70.
    let clock = Gate::new();
    let first = clock.hold(Phase::Applying);
    let second = clock.hold(Phase::Applying);
    let publication = clock.hold(Phase::Publishing);
    let progress = WorkerProgress::with_clock(clock.clone());
    let worker = IndicatorWorker::spawn_with_progress(progress.clone());
    worker.send(add());
    first.reached();
    clock.at(10);
    worker.send(IndicatorCommand::Backfilled(vec![Bar::opened_by(&print(
        1, 10,
    ))]));
    for _ in 0..2 {
        worker.send(IndicatorCommand::SetInputs {
            slot: SlotId(0),
            values: vec![],
        });
    }
    let mut partial = Bar::opened_by(&print(2, 2));
    worker.send(IndicatorCommand::PartialUpdated {
        partial: Some(partial.clone()),
        run: vec![print(2, 2)],
        rungs: 2,
    });
    partial.extend(&print(3, -1));
    worker.send(IndicatorCommand::PartialUpdated {
        partial: Some(partial),
        run: vec![print(3, -1)],
        rungs: 2,
    });
    let ack = acknowledge(&worker);
    clock.at(40);
    let held = worker.progress();
    assert!(held.valid);
    assert_eq!(held.counts.accepted, 7);
    assert_eq!(
        (
            held.counts.queued,
            held.counts.inflight,
            held.counts.retired
        ),
        (6, 1, 0)
    );
    assert_eq!(held.oldest_wait, Age::Known(30));
    assert_eq!(held.processing_age, Age::Known(40));
    assert_eq!(held.since_progress, Age::Unknown);
    println!(
        "Q8_INDICATOR_DEGRADED {}",
        serde_json::to_string(&held).unwrap()
    );
    first.release();
    second.reached();
    let applying = worker.progress();
    assert_eq!(
        (
            applying.counts.queued,
            applying.counts.inflight,
            applying.counts.retired
        ),
        (0, 6, 1)
    );
    assert_eq!(applying.last_sample_residence, Age::Known(30));
    clock.at(70);
    second.release();
    publication.reached();
    let publishing = worker.progress();
    assert_eq!(publishing.phase, Phase::Publishing);
    assert_eq!(publishing.processing_age, Age::Known(30));
    assert_eq!(publishing.counts.inputs_superseded, 1);
    assert_eq!(publishing.counts.partials_superseded, 1);
    publication.release();
    ack.recv_timeout(Duration::from_secs(10))
        .expect("all indicator work published");
    let done = worker.progress();
    assert_eq!(done.instance, held.instance);
    assert_eq!(
        (
            done.counts.queued,
            done.counts.inflight,
            done.counts.retired,
            done.counts.cycles
        ),
        (0, 0, 7, 2)
    );
    assert_eq!(
        (
            done.counts.output_attempts,
            done.counts.output_successes,
            done.counts.output_failures
        ),
        (6, 6, 0)
    );
    assert_output(worker.drain_events(), true);
    println!(
        "Q8_INDICATOR_RECOVERED {}",
        serde_json::to_string(&done).unwrap()
    );
    let closed = clock.hold(Phase::Closed);
    drop(worker);
    closed.reached();
    assert_eq!(progress.snapshot().phase, Phase::Closed);
    assert_eq!(progress.snapshot().counts.unfinished, 0);
    closed.release();
}

#[test]
fn indicator_unwind_failed_send_and_explicit_replacement_keep_separate_identity() {
    let clock = Gate::new();
    let hold = clock.hold(Phase::Applying);
    let terminal = clock.hold(Phase::Unwound);
    let progress = WorkerProgress::with_clock(clock.clone());
    let worker = IndicatorWorker::spawn_with_progress(progress.clone());
    worker.send(add());
    hold.reached();
    worker.send(IndicatorCommand::Backfilled(vec![Bar::opened_by(&print(
        1, 10,
    ))]));
    let abandoned_ack = acknowledge(&worker);
    hold.unwind();
    terminal.reached();
    let stopped = worker.progress();
    assert_eq!(stopped.phase, Phase::Unwound);
    assert_eq!(
        (
            stopped.counts.accepted,
            stopped.counts.retired,
            stopped.counts.unfinished
        ),
        (3, 0, 3)
    );
    terminal.release();
    assert!(matches!(
        abandoned_ack.recv_timeout(Duration::from_secs(10)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
    worker.send(add());
    assert_eq!(worker.progress().counts.failed_sends, 1);
    let replacement = IndicatorWorker::spawn();
    replacement.send(add());
    replacement.send(IndicatorCommand::Backfilled(vec![Bar::opened_by(&print(
        1, 10,
    ))]));
    acknowledge(&replacement)
        .recv_timeout(Duration::from_secs(10))
        .expect("replacement replay published");
    assert_ne!(replacement.progress().instance, stopped.instance);
    assert_eq!(replacement.progress().counts.accepted, 3);
    assert_output(replacement.drain_events(), false);
    println!(
        "Q8_INDICATOR_TERMINAL {}",
        serde_json::to_string(&worker.progress()).unwrap()
    );
}

#[test]
fn receiver_disconnection_does_not_turn_completed_cycles_into_delivered_events() {
    let clock = Gate::new();
    let hold = clock.hold(Phase::Applying);
    let progress = WorkerProgress::with_clock(clock);
    let mut worker = IndicatorWorker::spawn_with_progress(progress);
    worker.send(add());
    hold.reached();
    let (_, unused_receiver) = channel();
    drop(std::mem::replace(&mut worker.events, unused_receiver));
    worker.send(IndicatorCommand::Backfilled(vec![Bar::opened_by(&print(
        1, 10,
    ))]));
    let ack = acknowledge(&worker);
    hold.release();
    ack.recv_timeout(Duration::from_secs(10))
        .expect("domain cycles completed despite disconnected event receiver");
    let s = worker.progress();
    assert_eq!(
        (s.counts.accepted, s.counts.retired, s.counts.cycles),
        (3, 3, 2)
    );
    assert_eq!(
        (
            s.counts.output_attempts,
            s.counts.output_successes,
            s.counts.output_failures
        ),
        (6, 0, 6)
    );
}

#[test]
fn publication_unwind_keeps_delivered_errors_separate_from_unfinished_work() {
    // Unknown source Add emits Rebuilt and Error during Applying. At the
    // publication hold those two events already exist; no cycle completed.
    let clock = Gate::new();
    let publication = clock.hold(Phase::Publishing);
    let terminal = clock.hold(Phase::Unwound);
    let worker = IndicatorWorker::spawn_with_progress(WorkerProgress::with_clock(clock));
    worker.send(IndicatorCommand::Add {
        slot: SlotId(0),
        source: IndicatorSource::Native {
            id: "fixture.missing".into(),
            values: vec![],
        },
    });
    publication.reached();
    let events = worker.drain_events();
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], IndicatorEvent::Rebuilt { .. }));
    assert!(matches!(events[1], IndicatorEvent::Error { .. }));
    let before = worker.progress();
    assert_eq!(
        (
            before.counts.inflight,
            before.counts.retired,
            before.counts.output_successes
        ),
        (1, 0, 2)
    );
    publication.unwind();
    terminal.reached();
    let after = worker.progress();
    assert_eq!(
        (
            after.counts.accepted,
            after.counts.unfinished,
            after.counts.retired,
            after.counts.output_successes
        ),
        (1, 1, 0, 2)
    );
    assert_eq!(after.phase, Phase::Unwound);
    terminal.release();
}
