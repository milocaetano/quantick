//! Before-extraction characterizations through the actual bounded worker.
use super::*;
use crate::worker_progress::{Phase, tests::Gate};
use std::time::{Duration, Instant};

fn native(slot: u64, id: &str) -> IndicatorCommand {
    IndicatorCommand::Add {
        slot: SlotId(slot),
        source: IndicatorSource::Native {
            id: id.into(),
            values: vec![],
        },
    }
}

#[test]
fn an_applying_error_blocks_later_markers_and_keeps_raw_event_order() {
    let progress = WorkerProgress::new();
    let observed = progress.consumer();
    let (tx, rx) = sync_channel(INDICATOR_COMMAND_QUEUE);
    let commands = progress.bind_merging(tx, fold_commands);
    let (output, events) = sync_channel(1);
    let (probe_tx, probe) = channel();
    let (ack_tx, ack) = channel();
    for command in [
        native(0, "fixture.missing").into(),
        crate::indicator_worker::WorkerCommand::InspectLane(probe_tx),
        native(1, "native.cvd").into(),
        crate::indicator_worker::WorkerCommand::Flush(ack_tx),
    ] {
        assert!(commands.send(command).is_ok());
    }
    let thread = std::thread::spawn(move || run_observed(&rx, &output, observed));
    let deadline = Instant::now() + Duration::from_secs(10);
    while commands.snapshot().counts.output_blocked == 0 {
        assert!(Instant::now() < deadline, "Applying send did not block");
        std::thread::yield_now();
    }
    let blocked = commands.snapshot();
    assert_eq!(blocked.phase, Phase::Applying);
    assert_eq!(blocked.counts.output_successes, 1);
    assert!(matches!(
        probe.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    assert!(matches!(
        ack.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let mut raw = Vec::new();
    for _ in 0..5 {
        raw.push(
            events
                .recv_timeout(Duration::from_secs(10))
                .expect("ordered output"),
        );
    }
    assert!(matches!(
        raw[0],
        IndicatorEvent::Rebuilt {
            slot: SlotId(0),
            rows: 0,
            ..
        }
    ));
    assert!(matches!(
        raw[1],
        IndicatorEvent::Error {
            slot: SlotId(0),
            ..
        }
    ));
    assert!(matches!(
        raw[2],
        IndicatorEvent::Rebuilt {
            slot: SlotId(1),
            ..
        }
    ));
    assert!(matches!(
        raw[3],
        IndicatorEvent::Preview {
            slot: SlotId(1),
            ..
        }
    ));
    assert!(matches!(
        raw[4],
        IndicatorEvent::Lane {
            slot: SlotId(1),
            ..
        }
    ));
    assert_eq!(probe.recv_timeout(Duration::from_secs(10)).unwrap().len, 0);
    ack.recv_timeout(Duration::from_secs(10))
        .expect("whole batch completed");
    assert_eq!(commands.snapshot().counts.retired, 4);
    drop(commands);
    thread.join().expect("worker closes normally");
}

#[test]
fn a_middle_flush_waits_for_later_inputs_while_a_probe_sees_its_position() {
    let clock = Gate::new();
    let publication = clock.hold(Phase::Publishing);
    let progress = WorkerProgress::with_clock(clock);
    let (worker, run) = IndicatorWorker::prepared_for_test(progress);
    let (ack_tx, ack) = channel();
    let (probe_tx, probe) = channel();
    let first = tests::trade(1);
    let second = tests::trade(2);
    let mut partial = Bar::opened_by(&first);
    let one = partial.clone();
    partial.extend(&second);
    for command in [
        IndicatorCommand::Add {
            slot: SlotId(0),
            source: IndicatorSource::Script {
                name: "marker.pine".into(),
                text: "//@version=5\nindicator(\"marker\")\nk = input.int(1, \"k\")\nplot(close * k)\n".into(),
            },
        }.into(),
        IndicatorCommand::SetInputs { slot: SlotId(0), values: vec![InputValue::Int(2)] }.into(),
        crate::indicator_worker::WorkerCommand::Flush(ack_tx),
        IndicatorCommand::Remove(SlotId(99)).into(),
        IndicatorCommand::SetInputs { slot: SlotId(0), values: vec![InputValue::Int(3)] }.into(),
        IndicatorCommand::PartialUpdated { partial: Some(one), run: vec![first], rungs: 2 }.into(),
        crate::indicator_worker::WorkerCommand::InspectLane(probe_tx),
        IndicatorCommand::PartialUpdated { partial: Some(partial), run: vec![second], rungs: 2 }.into(),
    ] { worker.send(command); }
    let thread = std::thread::spawn(run);
    publication.reached();
    let at_marker = probe.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(at_marker.len, 1, "probe precedes the second run extension");
    assert_eq!(at_marker.folds, 1, "probe precedes the final lane walk");
    assert!(matches!(
        ack.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let snapshot = worker.progress();
    assert_eq!(snapshot.counts.inputs_superseded, 1);
    assert_eq!(snapshot.counts.partials_superseded, 1);
    publication.release();
    ack.recv_timeout(Duration::from_secs(10))
        .expect("middle Flush waits for all publication");
    let events = worker.drain_events();
    assert!(
        matches!(&events[0], IndicatorEvent::Rebuilt { inputs, .. } if inputs == &[InputValue::Int(3)])
    );
    assert!(matches!(&events[2], IndicatorEvent::Lane { samples, .. } if samples.len() == 2));
    assert_eq!(worker.progress().counts.retired, 8);
    drop(worker);
    thread.join().expect("worker closes normally");
}

/// New core/shell seam proof, not a claimed before-extraction panic baseline.
#[test]
fn applying_effect_unwind_keeps_the_reached_subsets_in_the_actual_runtime_guard() {
    struct PanicOnError;
    impl SessionEffects for PanicOnError {
        fn event(&mut self, event: IndicatorEvent) {
            if matches!(event, IndicatorEvent::Error { .. }) {
                panic!("controlled Applying recipient unwind");
            }
        }
        fn inputs_rebound(&mut self, _: InputsRebound<'_>) {
            unreachable!();
        }
    }
    let progress = WorkerProgress::new();
    let observed = progress.consumer();
    let (tx, _rx) = sync_channel(INDICATOR_COMMAND_QUEUE);
    let sender = progress.bind_merging(tx, fold_commands);
    let one = tests::trade(1);
    let partial = Bar::opened_by(&one);
    let commands = vec![
        IndicatorCommand::SetInputs {
            slot: SlotId(9),
            values: vec![],
        },
        IndicatorCommand::SetInputs {
            slot: SlotId(9),
            values: vec![],
        },
        IndicatorCommand::PartialUpdated {
            partial: Some(partial.clone()),
            run: vec![one],
            rungs: 1,
        },
        IndicatorCommand::PartialUpdated {
            partial: Some(partial),
            run: vec![],
            rungs: 1,
        },
        native(0, "fixture.missing"),
    ];
    let mut session = IndicatorSession::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _lifecycle = observed.lifecycle();
        observed.begin(commands.len());
        let mut subsets = Coalescing::new(&observed);
        let mut applying = session.begin_batch(commands.iter().enumerate());
        subsets.inputs = applying.inputs_superseded();
        for (index, command) in commands.into_iter().enumerate() {
            applying.apply(index, command, &mut subsets.partials, &mut PanicOnError);
        }
        panic!("error effect should have unwound first");
    }));
    assert!(outcome.is_err());
    let snapshot = sender.snapshot();
    assert_eq!(snapshot.phase, Phase::Unwound);
    assert_eq!(snapshot.counts.inputs_superseded, 1);
    assert_eq!(snapshot.counts.partials_superseded, 1);
    assert_eq!(snapshot.counts.retired, 0);
}
