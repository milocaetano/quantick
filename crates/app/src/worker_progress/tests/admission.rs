//! The bounded endpoint's own ledger: a full channel parks and folds, the
//! counts say so, a pump drains, and a gone worker's parked commands are
//! counted as failed sends rather than vanishing.

use super::*;
use std::sync::mpsc::sync_channel;

/// Equal parities fold, summing, so one fold is distinguishable from none.
fn sum_equal_parity(older: &mut u64, newer: u64) -> Option<u64> {
    if *older % 2 == newer % 2 {
        *older += newer;
        None
    } else {
        Some(newer)
    }
}

#[test]
fn a_full_channel_parks_folds_and_counts_and_a_pump_drains_it() {
    let producer = WorkerProgress::new();
    let observer = producer.observer().clone();
    let (tx, rx) = sync_channel(2);
    let tx = producer.bind_merging(tx, sum_equal_parity);
    for command in [1, 3, 2, 4, 5] {
        tx.send(command).expect("the worker is alive");
    }
    // 1 and 3 fit; 2 parks, 4 folds into it, 5 parks behind it.
    let counts = observer.snapshot().counts;
    assert_eq!(
        (
            counts.accepted,
            counts.parked,
            counts.deferred,
            counts.coalesced_parked
        ),
        (2, 2, 3, 1)
    );
    assert_eq!(counts.queued, 2, "backlog is what the channel holds");
    tx.pump();
    assert_eq!(
        observer.snapshot().counts.parked,
        2,
        "a full channel takes nothing"
    );
    assert_eq!(rx.try_recv(), Ok(1));
    assert_eq!(rx.try_recv(), Ok(3));
    tx.pump();
    let counts = observer.snapshot().counts;
    assert_eq!((counts.accepted, counts.parked), (4, 0));
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![6, 5]);
    assert_eq!(counts.deferred, 3, "deferred is cumulative");
    assert_eq!(counts.failed_sends, 0);
}

#[test]
fn a_gone_worker_turns_every_parked_command_into_a_counted_failure() {
    let producer = WorkerProgress::new();
    let observer = producer.observer().clone();
    let (tx, rx) = sync_channel(1);
    let tx = producer.bind(tx);
    for command in [1_u64, 2, 3] {
        tx.send(command).expect("the worker is alive");
    }
    assert_eq!(observer.snapshot().counts.parked, 2);
    drop(rx);
    tx.pump();
    let counts = observer.snapshot().counts;
    assert_eq!((counts.parked, counts.failed_sends), (0, 2));
    assert!(tx.send(4).is_err(), "the command comes back to its owner");
    assert_eq!(observer.snapshot().counts.failed_sends, 3);
}
