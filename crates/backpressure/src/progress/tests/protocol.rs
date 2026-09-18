//! Adversarial ledger protocol schedules frozen independently in
//! Q8-PERF-001-repair1-independent-schedules.md (S1-S4). Direct enqueue plus
//! the production record_send helper exposes the bookkeeping interval; it is
//! not a replacement for ordinary send-wrapper or domain-worker tests.
//! Repair 2 preserves the schedules while local acceptance replaces the old
//! producer mutex. Only the consumer Arc moves into the worker thread.

use super::*;

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
    let (tx, rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let observed = producer.consumer();
    let tx = producer.bind(tx);
    let (done_tx, done_rx) = channel();
    clock.at(100);
    tx.sender.send(()).unwrap();
    let worker_clock = clock.clone();
    let worker = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let lifecycle = observed.lifecycle(std::thread::panicking);
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
