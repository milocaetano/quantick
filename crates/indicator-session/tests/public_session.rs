//! An actual headless consumer of the public command/effect contract.
use quantick_engine::{Bar, Side, Trade};
use quantick_indicator_session::{
    IndicatorCommand as Command, IndicatorEvent as Event, IndicatorSession,
    IndicatorSource as Source, InputsRebound, SessionEffects, SlotId,
};
use rust_decimal::Decimal;

#[derive(Default)]
struct Recorder(Vec<Event>);
impl SessionEffects for Recorder {
    fn event(&mut self, event: Event) {
        self.0.push(event);
    }
    fn inputs_rebound(&mut self, _: InputsRebound<'_>) {
        panic!("this trace never rebinds inputs");
    }
}
fn trade(id: u64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: id as i64,
        price: Decimal::from(100),
        quantity: Decimal::from(quantity),
        side,
    }
}
fn add(slot: u64, id: &str) -> Command {
    Command::Add {
        slot: SlotId(slot),
        source: Source::Native {
            id: id.into(),
            values: vec![],
        },
    }
}
fn run(session: &mut IndicatorSession, commands: Vec<Command>, recorder: &mut impl SessionEffects) {
    let mut applying = session.begin_batch(commands.iter().enumerate());
    let mut partials = 0;
    for (index, command) in commands.into_iter().enumerate() {
        applying.apply(index, command, &mut partials, recorder);
    }
    applying.finish_applying().publish(recorder);
}

#[test]
fn public_session_preserves_immediate_errors_and_incremental_cvd_trace() {
    let mut session = IndicatorSession::new();
    let mut recorder = Recorder::default();
    let first = trade(2, 2, Side::Buy);
    let second = trade(3, 1, Side::Sell);
    let mut partial = Bar::opened_by(&first);
    partial.extend(&second);
    run(
        &mut session,
        vec![
            add(0, "fixture.missing"),
            add(1, "native.cvd"),
            Command::Backfilled(vec![Bar::opened_by(&trade(1, 10, Side::Buy))]),
            Command::PartialUpdated {
                partial: Some(partial.clone()),
                run: vec![first, second],
                rungs: 2,
            },
        ],
        &mut recorder,
    );
    assert_eq!(recorder.0.len(), 5);
    assert!(
        matches!(&recorder.0[0], Event::Rebuilt { slot: SlotId(0), descriptor, rows: 0, .. } if descriptor.title == "fixture.missing")
    );
    assert!(
        matches!(&recorder.0[1], Event::Error { slot: SlotId(0), error } if error.message == "`fixture.missing` is not a native indicator this build ships")
    );
    assert!(
        matches!(&recorder.0[2], Event::Rebuilt { slot: SlotId(1), columns, rows: 1, stale: None, .. } if columns == &[vec![10.0]])
    );
    assert!(
        matches!(&recorder.0[3], Event::Preview { slot: SlotId(1), frame: Some(frame) } if frame.values == [11.0])
    );
    assert!(
        matches!(&recorder.0[4], Event::Lane { slot: SlotId(1), samples } if samples.len() == 2 && samples[0].values == [12.0] && samples[1].values == [11.0])
    );
    recorder.0.clear();
    run(
        &mut session,
        vec![Command::BarClosed(partial)],
        &mut recorder,
    );
    assert_eq!(recorder.0.len(), 3);
    assert!(
        matches!(&recorder.0[0], Event::Appended { slot: SlotId(1), row, .. } if row == &[11.0])
    );
    assert!(matches!(&recorder.0[1], Event::Preview { frame: None, .. }));
    assert!(matches!(&recorder.0[2], Event::Lane { samples, .. } if samples.is_empty()));
    let d = session.lane_diagnostics();
    assert_eq!((d.len, d.capacity), (0, 0));
    assert!(
        d.folds >= 2,
        "retired work remains observable without retaining the run"
    );
}

#[test]
fn a_panicking_error_recipient_precedes_insertion_of_the_failed_slot() {
    struct PanicOnError;
    impl SessionEffects for PanicOnError {
        fn event(&mut self, event: Event) {
            if matches!(event, Event::Error { .. }) {
                panic!("controlled Applying recipient unwind");
            }
        }
        fn inputs_rebound(&mut self, _: InputsRebound<'_>) {
            unreachable!();
        }
    }
    let mut session = IndicatorSession::new();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(
            &mut session,
            vec![add(7, "fixture.missing")],
            &mut PanicOnError,
        );
    }));
    assert!(outcome.is_err());
    let mut recorder = Recorder::default();
    run(
        &mut session,
        vec![Command::Reload {
            slot: SlotId(7),
            source: Source::Native {
                id: "native.cvd".into(),
                values: vec![],
            },
        }],
        &mut recorder,
    );
    assert!(
        recorder.0.is_empty(),
        "the interrupted Add did not insert a slot that Reload could heal"
    );
}

#[test]
fn adding_a_live_slot_again_replaces_its_instance_instead_of_orphaning_it() {
    let mut session = IndicatorSession::new();
    let mut recorder = Recorder::default();
    run(
        &mut session,
        vec![add(1, "native.cvd"), add(1, "native.cvd")],
        &mut recorder,
    );
    assert_eq!(
        session.live_instances(),
        1,
        "the second Add replaced the first"
    );
    run(
        &mut session,
        vec![Command::Remove(SlotId(1))],
        &mut recorder,
    );
    assert_eq!(
        session.live_instances(),
        0,
        "Remove reaches the only instance the slot ever had"
    );
}
