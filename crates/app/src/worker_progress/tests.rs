use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// Room enough that no fixture here ever finds its queue full; the bounded
/// behaviour itself is `worker_backlog`'s and `live_envelope`'s to test.
pub(crate) const TEST_QUEUE: usize = 1 << 12;

mod admission;
mod ownership;
pub(crate) mod protocol;

pub(crate) struct Gate {
    now: AtomicU64,
    holds: Mutex<VecDeque<PhaseHold>>,
}
struct PhaseHold {
    phase: Phase,
    reached: Sender<()>,
    release: Receiver<bool>,
}
pub(crate) struct Hold {
    reached: Receiver<()>,
    release: Sender<bool>,
}
impl Hold {
    pub(crate) fn reached(&self) {
        self.reached
            .recv_timeout(Duration::from_secs(10))
            .expect("worker reached the prescribed phase");
    }
    pub(crate) fn release(self) {
        self.release.send(false).expect("held worker is alive");
    }
    pub(crate) fn unwind(self) {
        self.release.send(true).expect("held worker is alive");
    }
}
impl Gate {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(0),
            holds: Mutex::new(VecDeque::new()),
        })
    }
    pub(crate) fn at(&self, ns: u64) {
        self.now.store(ns, Ordering::SeqCst);
    }
    pub(crate) fn hold(&self, phase: Phase) -> Hold {
        let (tx, reached) = channel();
        let (release, rx) = channel();
        self.holds.lock().unwrap().push_back(PhaseHold {
            phase,
            reached: tx,
            release: rx,
        });
        Hold { reached, release }
    }
}
impl ProgressClock for Gate {
    fn source(&self) -> &'static str {
        "fixture_explicit"
    }
    fn now_ns(&self) -> Option<u64> {
        Some(self.now.load(Ordering::SeqCst))
    }
    fn phase(&self, phase: Phase) {
        let step = {
            let mut holds = self.holds.lock().unwrap();
            if holds.front().is_some_and(|s| s.phase == phase) {
                holds.pop_front()
            } else {
                None
            }
        };
        if let Some(PhaseHold {
            reached: tx,
            release: rx,
            ..
        }) = step
        {
            tx.send(()).unwrap();
            assert!(
                !rx.recv_timeout(Duration::from_secs(10))
                    .expect("fixture releases worker"),
                "prescribed worker unwind"
            );
        }
    }
}

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
    let producer = WorkerProgress::new();
    let p = producer.observer().clone();
    let observed = producer.consumer();
    let (tx, _rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    for _ in 0..3 {
        tx.send(()).unwrap();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let _lifecycle = observed.lifecycle();
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
    let (output_tx, output_rx) = channel();
    for _ in 0..3 {
        tx.send(()).unwrap();
    }
    let worker = std::thread::spawn(move || {
        let _lifecycle = observed.lifecycle();
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
    let producer = WorkerProgress::new();
    let p = producer.observer().clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    drop(producer.consumer().lifecycle());
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
