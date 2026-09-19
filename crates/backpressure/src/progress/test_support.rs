//! What a test drives a worker with: a clock the test advances and can park
//! the worker on at a named phase, and the ledger schedules the repairs were
//! proven against.
//!
//! Compiled for this crate's own tests and, under the `test-support` feature,
//! for the tests of a crate that runs real workers on this port — the window
//! drives its indicator and book workers through the same [`Gate`] so the
//! schedule is the schedule, not a copy of it. Never part of a shipping
//! build: the feature is a dev-dependency's to ask for.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Age, ObservedSender, Phase, ProgressClock, ProgressSnapshot};

/// Room enough that no fixture here ever finds its queue full; the bounded
/// behaviour itself is `worker_backlog`'s and `live_envelope`'s to test.
pub const TEST_QUEUE: usize = 1 << 12;

pub struct Gate {
    now: AtomicU64,
    holds: Mutex<VecDeque<PhaseHold>>,
}
struct PhaseHold {
    phase: Phase,
    reached: Sender<()>,
    release: Receiver<bool>,
}
pub struct Hold {
    reached: Receiver<()>,
    release: Sender<bool>,
}
impl Hold {
    pub fn reached(&self) {
        self.reached
            .recv_timeout(Duration::from_secs(10))
            .expect("worker reached the prescribed phase");
    }
    pub fn release(self) {
        self.release.send(false).expect("held worker is alive");
    }
    pub fn unwind(self) {
        self.release.send(true).expect("held worker is alive");
    }
}
impl Gate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            now: AtomicU64::new(0),
            holds: Mutex::new(VecDeque::new()),
        })
    }
    pub fn at(&self, ns: u64) {
        self.now.store(ns, Ordering::SeqCst);
    }
    pub fn hold(&self, phase: Phase) -> Hold {
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

/// The ledger's six core counts, asserted together.
pub fn core(s: &ProgressSnapshot, instance: Option<u64>, expected: [u64; 6]) {
    assert!(s.valid);
    assert_eq!(s.instance, instance);
    assert_eq!(
        [
            s.counts.accepted,
            s.counts.queued,
            s.counts.inflight,
            s.counts.retired,
            s.counts.unfinished,
            s.counts.failed_sends,
        ],
        expected
    );
    assert_eq!(s.counts.output_failures, 0);
    assert_eq!(s.counts.output_attempts, s.counts.output_successes);
}

/// Every age reads not-applicable: nothing is in flight.
pub fn inactive(s: &ProgressSnapshot) {
    assert_eq!(s.sampled_ticket, None);
    assert_eq!(s.sample_age, Age::NotApplicable);
    assert_eq!(s.oldest_wait, Age::NotApplicable);
    assert_eq!(s.processing_age, Age::NotApplicable);
    assert_eq!(s.since_progress, Age::NotApplicable);
}

/// Wait for the worker's acknowledgement of a flush.
pub fn acknowledge(rx: Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("actual worker Flush acknowledged the completed publication");
}

/// One observation of a frozen schedule, printed as JSON.
pub fn evidence(stage: &str, snapshot: &ProgressSnapshot) {
    println!(
        "Q8_REPAIR_PROTOCOL {}",
        serde_json::json!({"schedule": stage, "observation": snapshot})
    );
}

/// S1-S3 run against the caller's real worker consumer and actual Flush port.
pub fn delayed_bookkeeping<T>(
    clock: &Arc<Gate>,
    commands: &ObservedSender<T>,
    flush: impl Fn(Sender<()>) -> T,
) {
    let p = commands.progress.observer();
    let instance = p.snapshot().instance;
    assert!(instance.is_some_and(|id| id > 0));
    let first = clock.hold(Phase::Applying);
    let publication = clock.hold(Phase::Publishing);
    clock.at(120);
    let (tx, a1) = channel();
    assert!(commands.sender.send(flush(tx)).is_ok());
    first.reached();
    clock.at(150);
    first.release();
    publication.reached();
    clock.at(180);
    // Park the worker in the Idle callback that closes this cycle, before it
    // can acknowledge the flush or receive anything else. Every send later in
    // this schedule is made against a parked worker; S2 below was the one that
    // was not, which left the producer's acceptance of ticket 2 racing the
    // worker's admission of it and made `last_sampled_ticket` a coin flip.
    let parked = clock.hold(Phase::Idle);
    publication.release();
    parked.reached();
    // The cycle is complete — applied, published, retired and counted — while
    // the producer has recorded no acceptance at all: the real flush waits on
    // nothing the producer still owes. The reader is momentarily incomplete
    // for exactly that reason (one admitted command, zero accepted), so the
    // core() invariants do not hold here and are not claimed.
    let unaccounted = p.snapshot();
    assert!(!unaccounted.valid);
    assert_eq!(unaccounted.phase, Phase::Idle);
    assert_eq!(unaccounted.counts.accepted, 0);
    assert_eq!(unaccounted.counts.retired, 1);
    assert_eq!(
        (
            unaccounted.counts.cycles,
            unaccounted.counts.mailbox_replacements
        ),
        (1, 1)
    );
    evidence("S1-cycle-completed-unaccounted", &unaccounted);
    clock.at(200);
    commands.progress.record_send(true);

    for now in [230, 240] {
        clock.at(now);
        let s = p.snapshot();
        core(&s, instance, [1, 0, 0, 1, 0, 0]);
        inactive(&s);
        assert_eq!(s.phase, Phase::Idle);
        assert_eq!((s.counts.cycles, s.counts.mailbox_replacements), (1, 1));
        assert_eq!(s.last_sampled_ticket, Some(1));
        assert_eq!(s.last_sample_residence, Age::Unknown);
        evidence("S1-S2-delayed-accounting", &s);
    }

    let second = clock.hold(Phase::Applying);
    let idle = clock.hold(Phase::Idle);
    clock.at(260);
    let (tx, a2) = channel();
    // The producer's whole ordinary send — enqueue then acceptance — completes
    // while the worker is still parked above, so ticket 2 meets a slot that
    // still holds the unobserved ticket 1 and is never sampled. Releasing the
    // park afterwards is what lets the worker admit both under one ledger, and
    // the acknowledgement the delayed accounting never blocked arrives with it.
    assert!(commands.send(flush(tx)).is_ok());
    parked.release();
    acknowledge(a1);
    second.reached();
    let s = p.snapshot();
    core(&s, instance, [2, 0, 1, 1, 0, 0]);
    assert_eq!(s.last_sampled_ticket, Some(1));
    assert_eq!(s.last_sample_residence, Age::Unknown);
    assert_eq!(s.sampled_ticket, None);
    clock.at(280);
    second.release();
    idle.reached();

    clock.at(400);
    let (tx, a3) = channel();
    assert!(commands.send(flush(tx)).is_ok());
    clock.at(430);
    let s = p.snapshot();
    core(&s, instance, [3, 1, 0, 2, 0, 0]);
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!((s.counts.cycles, s.counts.mailbox_replacements), (2, 2));
    assert_eq!(s.sampled_ticket, Some(3));
    assert_eq!(s.sample_age, Age::Known(30));
    assert_eq!(s.oldest_wait, Age::Known(30));
    assert_eq!(s.processing_age, Age::NotApplicable);
    assert_eq!(s.since_progress, Age::Known(150));
    assert_eq!(s.last_sampled_ticket, Some(1));
    assert_eq!(s.last_sample_residence, Age::Unknown);
    evidence("S3-waiting-recovery", &s);

    let third = clock.hold(Phase::Applying);
    clock.at(450);
    idle.release();
    acknowledge(a2);
    third.reached();
    let s = p.snapshot();
    core(&s, instance, [3, 0, 1, 2, 0, 0]);
    assert_eq!(s.phase, Phase::Applying);
    assert_eq!(s.last_sampled_ticket, Some(3));
    assert_eq!(s.last_sample_residence, Age::Known(50));
    assert_eq!(s.sampled_ticket, None);
    assert_eq!(s.sample_age, Age::NotApplicable);
    assert_eq!(s.oldest_wait, Age::NotApplicable);
    assert_eq!(s.processing_age, Age::Known(0));
    assert_eq!(s.since_progress, Age::Known(170));
    evidence("S3-observed-residence", &s);
    clock.at(480);
    third.release();
    acknowledge(a3);
    clock.at(500);
    let s = p.snapshot();
    core(&s, instance, [3, 0, 0, 3, 0, 0]);
    inactive(&s);
    assert_eq!(s.phase, Phase::Idle);
    assert_eq!((s.counts.cycles, s.counts.mailbox_replacements), (3, 3));
    assert_eq!(s.last_sampled_ticket, Some(3));
    assert_eq!(s.last_sample_residence, Age::Known(50));
    evidence("S3-completed-recovery", &s);
}
