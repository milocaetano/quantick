use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

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
    let p = WorkerProgress::with_clock(clock.clone());
    let (tx, _rx) = channel();
    assert_eq!(p.snapshot().since_progress, Age::NotApplicable);
    clock.at(10);
    p.send(&tx, ()).unwrap();
    clock.at(20);
    p.send(&tx, ()).unwrap();
    clock.at(30);
    p.begin(1);
    assert_eq!(p.snapshot().last_sample_residence, Age::Known(20));
    clock.at(40);
    p.send(&tx, ()).unwrap();
    clock.at(70);
    let s = p.snapshot();
    assert_eq!(s.sampled_ticket, Some(3));
    assert_eq!(s.last_sampled_ticket, Some(1));
    assert_eq!(s.sample_age, Age::Known(30));
    assert_eq!(s.oldest_wait, Age::Unknown);
    assert_eq!(s.processing_age, Age::Known(40));
    assert_eq!(s.since_progress, Age::Unknown);
    p.finish(false);
    p.begin(2);
    p.finish(false);
    assert_eq!(p.snapshot().counts.retired, 3);
    assert_eq!(p.snapshot().since_progress, Age::NotApplicable);
}

#[test]
fn overflow_and_clock_regression_are_explicit() {
    let clock = Gate::new();
    let p = WorkerProgress::with_clock(clock.clone());
    let (tx, _rx) = channel();
    clock.at(20);
    p.send(&tx, ()).unwrap();
    clock.at(10);
    assert_eq!(p.snapshot().oldest_wait, Age::Invalid);
    p.lock().counts.accepted = u64::MAX;
    p.send(&tx, ()).unwrap();
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
    let p = WorkerProgress::with_clock(Arc::new(Unavailable));
    let (tx, _rx) = channel();
    p.send(&tx, ()).unwrap();
    assert_eq!(p.snapshot().oldest_wait, Age::Unknown);
}

#[test]
fn coalescing_subsets_survive_an_unfinished_domain_batch() {
    // Three accepted commands; two input supersessions occurred before a
    // domain unwind. They remain subsets of the three unfinished commands.
    let p = WorkerProgress::new();
    let (tx, _rx) = channel();
    for _ in 0..3 {
        p.send(&tx, ()).unwrap();
    }
    let observed = p.clone();
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
fn send_racing_receiver_destruction_is_unfinished_after_terminal_observation() {
    let p = WorkerProgress::new();
    let (tx, rx) = channel();
    drop(p.lifecycle());
    // The receiver still exists for the small interval after run() returns.
    p.send(&tx, ()).unwrap();
    let s = p.snapshot();
    assert_eq!(s.phase, Phase::Closed);
    assert_eq!(
        (s.counts.accepted, s.counts.queued, s.counts.unfinished),
        (1, 0, 1)
    );
    drop(rx);
    assert!(p.send(&tx, ()).is_err());
    assert_eq!(p.snapshot().counts.failed_sends, 1);
}
