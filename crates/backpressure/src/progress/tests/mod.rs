use super::*;
use crate::progress::test_support::*;
use std::time::Instant;

mod admission;
mod ownership;
mod protocol;

/// The clock the window installs: elapsed time since the worker started.
struct Monotonic(Instant);
impl ProgressClock for Monotonic {
    fn now_ns(&self) -> Option<u64> {
        self.0.elapsed().as_nanos().try_into().ok()
    }
}

/// A progress on the monotonic clock, as the window builds one.
pub(crate) fn fresh() -> WorkerProgress {
    WorkerProgress::with_clock(Arc::new(Monotonic(Instant::now())))
}
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

#[test]
fn sampled_ticket_does_not_claim_unobserved_head_or_idle_stall() {
    // Independent schedule: ticket 1 at 10, ticket 2 at 20. Admit only 1 at
    // 30, then ticket 3 at 40: ticket 2 has no timestamp, so oldest is unknown.
    let clock = Gate::new();
    let producer = WorkerProgress::with_clock(clock.clone());
    let p = producer.observer().clone();
    let observed = producer.consumer();
    let (tx, _rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    assert_eq!(p.snapshot().since_progress, Age::NotApplicable);
    clock.at(10);
    tx.send(()).unwrap();
    clock.at(20);
    tx.send(()).unwrap();
    clock.at(30);
    observed.begin(1);
    assert_eq!(p.snapshot().last_sample_residence, Age::Known(20));
    clock.at(40);
    tx.send(()).unwrap();
    clock.at(70);
    let s = p.snapshot();
    assert_eq!(s.sampled_ticket, Some(3));
    assert_eq!(s.last_sampled_ticket, Some(1));
    assert_eq!(s.sample_age, Age::Known(30));
    assert_eq!(s.oldest_wait, Age::Unknown);
    assert_eq!(s.processing_age, Age::Known(40));
    assert_eq!(s.since_progress, Age::Unknown);
    observed.finish(false);
    observed.begin(2);
    observed.finish(false);
    assert_eq!(p.snapshot().counts.retired, 3);
    assert_eq!(p.snapshot().since_progress, Age::NotApplicable);
}

#[test]
fn overflow_and_clock_regression_are_explicit() {
    let clock = Gate::new();
    let producer = WorkerProgress::with_clock(clock.clone());
    let p = producer.observer().clone();
    let (tx, _rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    clock.at(20);
    tx.send(()).unwrap();
    clock.at(10);
    assert_eq!(p.snapshot().oldest_wait, Age::Invalid);
    let mut admission = p.0.admission.get();
    admission.accepted = u64::MAX;
    p.0.admission.set(admission);
    tx.send(()).unwrap();
    assert!(!p.snapshot().valid);
    assert_eq!(p.snapshot().counts.accepted, u64::MAX);
}

#[test]
fn unavailable_clock_is_unknown() {
    struct Unavailable;
    impl ProgressClock for Unavailable {
        fn now_ns(&self) -> Option<u64> {
            None
        }
    }
    let producer = WorkerProgress::with_clock(Arc::new(Unavailable));
    let p = producer.observer().clone();
    let (tx, _rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    tx.send(()).unwrap();
    assert_eq!(p.snapshot().oldest_wait, Age::Unknown);
}

#[test]
fn coalescing_subsets_survive_an_unfinished_domain_batch() {
    // Three accepted commands; two input supersessions occurred before a
    // domain unwind. They remain subsets of the three unfinished commands.
    let producer = fresh();
    let p = producer.observer().clone();
    let observed = producer.consumer();
    let (tx, _rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    for _ in 0..3 {
        tx.send(()).unwrap();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _lifecycle = observed.lifecycle(std::thread::panicking);
        observed.begin(3);
        let mut subsets = Coalescing::new(&observed);
        subsets.inputs = 2;
        panic!("prescribed domain unwind after coalescing");
    }));
    assert!(result.is_err());
    let s = p.snapshot();
    assert!(s.valid);
    assert_eq!(s.phase, Phase::Unwound);
    assert_eq!(
        (
            s.counts.accepted,
            s.counts.retired,
            s.counts.unfinished,
            s.counts.inputs_superseded
        ),
        (3, 0, 3, 2)
    );
}

#[test]
fn publishing_unwind_records_nonzero_subsets_once_and_keeps_delivered_output() {
    // Three admitted commands contain two input supersessions. One output is
    // delivered before the Publishing callback unwinds; no cycle completes.
    let clock = Gate::new();
    let publishing = clock.hold(Phase::Publishing);
    let terminal = clock.hold(Phase::Unwound);
    let producer = WorkerProgress::with_clock(clock);
    let p = producer.observer().clone();
    let observed = producer.consumer();
    let (tx, rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    let (output_tx, output_rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    for _ in 0..3 {
        tx.send(()).unwrap();
    }
    let worker = std::thread::spawn(move || {
        let _lifecycle = observed.lifecycle(std::thread::panicking);
        for _ in 0..3 {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        observed.begin(3);
        let mut subsets = Coalescing::new(&observed);
        subsets.inputs = 2;
        ObservedOutput {
            sender: &output_tx,
            progress: &observed,
        }
        .send(17)
        .unwrap();
        subsets.publishing();
        panic!("Publishing callback should have unwound");
    });
    publishing.reached();
    assert_eq!(output_rx.recv_timeout(Duration::from_secs(10)).unwrap(), 17);
    let before = p.snapshot();
    assert!(before.valid);
    assert_eq!(before.phase, Phase::Publishing);
    assert_eq!(
        before.counts,
        Counts {
            accepted: 3,
            inflight: 3,
            inputs_superseded: 2,
            output_attempts: 1,
            output_successes: 1,
            ..Counts::default()
        }
    );
    publishing.unwind();
    terminal.reached();
    let after = p.snapshot();
    assert!(after.valid);
    assert_eq!(after.phase, Phase::Unwound);
    assert_eq!(
        after.counts,
        Counts {
            accepted: 3,
            unfinished: 3,
            inputs_superseded: 2,
            output_attempts: 1,
            output_successes: 1,
            ..Counts::default()
        }
    );
    terminal.release();
    assert!(worker.join().is_err());
}

#[test]
fn send_racing_receiver_destruction_is_unfinished_after_terminal_observation() {
    let producer = fresh();
    let p = producer.observer().clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    drop(producer.consumer().lifecycle(std::thread::panicking));
    let tx = producer.bind(tx);
    // The receiver still exists for the small interval after run() returns.
    tx.send(()).unwrap();
    let s = p.snapshot();
    assert_eq!(s.phase, Phase::Closed);
    assert_eq!(
        (s.counts.accepted, s.counts.queued, s.counts.unfinished),
        (1, 0, 1)
    );
    drop(rx);
    assert!(tx.send(()).is_err());
    assert_eq!(p.snapshot().counts.failed_sends, 1);
}
