//! Adversarial ledger protocol schedules frozen independently in
//! Q8-PERF-001-repair1-independent-schedules.md (S1-S4). Direct enqueue plus
//! the production record_send helper exposes the bookkeeping interval; it is
//! not a replacement for ordinary send-wrapper or domain-worker tests.
//! Repair 2 preserves the schedules while local acceptance replaces the old
//! producer mutex. Only the consumer Arc moves into the worker thread.

use super::*;

fn core(s: &ProgressSnapshot, instance: Option<u64>, expected: [u64; 6]) {
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

fn inactive(s: &ProgressSnapshot) {
    assert_eq!(s.sampled_ticket, None);
    assert_eq!(s.sample_age, Age::NotApplicable);
    assert_eq!(s.oldest_wait, Age::NotApplicable);
    assert_eq!(s.processing_age, Age::NotApplicable);
    assert_eq!(s.since_progress, Age::NotApplicable);
}

fn acknowledge(rx: Receiver<()>) {
    rx.recv_timeout(Duration::from_secs(10))
        .expect("actual worker Flush acknowledged the completed publication");
}

fn evidence(stage: &str, snapshot: &ProgressSnapshot) {
    println!(
        "Q8_REPAIR_PROTOCOL {}",
        serde_json::json!({"schedule": stage, "observation": snapshot})
    );
}

/// S1-S3 run against the caller's real worker consumer and actual Flush port.
pub(crate) fn delayed_bookkeeping<T>(
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
    publication.release();
    acknowledge(a1); // Successful-send accounting is still delayed here.
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
    assert!(commands.send(flush(tx)).is_ok());
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal {
    Retired,
    Unwound,
    Unadmitted,
}

fn terminal_schedule(branch: Terminal) {
    let clock = Gate::new();
    let producer = WorkerProgress::with_clock(clock.clone());
    let p = producer.observer().clone();
    let instance = p.snapshot().instance;
    assert!(instance.is_some_and(|id| id > 0));
    let (tx, rx) = channel();
    let observed = producer.consumer();
    let tx = producer.bind(tx);
    let (done_tx, done_rx) = channel();
    clock.at(100);
    tx.sender.send(()).unwrap();
    let worker_clock = clock.clone();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let lifecycle = observed.lifecycle();
            if branch != Terminal::Unadmitted {
                rx.recv_timeout(Duration::from_secs(10)).unwrap();
                worker_clock.at(120);
                observed.begin(1);
            }
            worker_clock.at(150);
            match branch {
                Terminal::Retired => {
                    observed.finish(false);
                    worker_clock.at(180);
                }
                Terminal::Unwound => panic!("S4 prescribed admitted unfinished unwind"),
                Terminal::Unadmitted => {}
            }
            drop(lifecycle);
        }));
        done_tx.send((rx, result.is_err())).unwrap();
    });
    let (rx, unwound) = done_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("terminal transition completed while producer bookkeeping was held");
    worker.join().expect("checked protocol consumer join");
    assert_eq!(unwound, branch == Terminal::Unwound);
    clock.at(200);
    tx.progress.record_send(true);

    let phase = if branch == Terminal::Unwound {
        Phase::Unwound
    } else {
        Phase::Closed
    };
    let (retired, unfinished, cycles) = if branch == Terminal::Retired {
        (1, 0, 1)
    } else {
        (0, 1, 0)
    };
    clock.at(230);
    let s = p.snapshot();
    core(&s, instance, [1, 0, 0, retired, unfinished, 0]);
    inactive(&s);
    assert_eq!(s.phase, phase);
    assert_eq!(s.counts.cycles, cycles);
    evidence(&format!("S4-{branch:?}-successful"), &s);
    drop(rx);
    clock.at(260);
    assert!(tx.send(()).is_err());
    clock.at(280);
    let s = p.snapshot();
    core(&s, instance, [1, 0, 0, retired, unfinished, 1]);
    inactive(&s);
    assert_eq!(s.phase, phase);
    assert_eq!(s.counts.cycles, cycles);
    evidence(&format!("S4-{branch:?}-failed"), &s);
}

#[test]
fn late_success_after_retirement_and_closure_is_not_unfinished() {
    terminal_schedule(Terminal::Retired);
}

#[test]
fn late_success_after_unwind_is_unfinished() {
    terminal_schedule(Terminal::Unwound);
}

#[test]
fn late_success_without_admission_is_unfinished() {
    terminal_schedule(Terminal::Unadmitted);
}
