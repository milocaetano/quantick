use super::*;

#[test]
fn only_consumer_state_can_cross_threads_or_share_producer_work() {
    // An accidental Clone/Send/Sync implementation makes these selections
    // ambiguous at compile time, before the fixture can be run.
    trait NotClone<A> {
        fn check() {}
    }
    impl<T: ?Sized> NotClone<()> for T {}
    impl<T: Clone> NotClone<u8> for T {}
    let _ = <WorkerProgress as NotClone<_>>::check;
    let _ = <ObservedSender<()> as NotClone<_>>::check;

    trait NotSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> NotSend<()> for T {}
    impl<T: ?Sized + Send> NotSend<u8> for T {}
    let _ = <ObservedSender<()> as NotSend<_>>::check;
    let _ = <ProgressObserver as NotSend<_>>::check;

    trait NotSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> NotSync<()> for T {}
    impl<T: ?Sized + Sync> NotSync<u8> for T {}
    let _ = <ObservedSender<()> as NotSync<_>>::check;
    let _ = <ProgressObserver as NotSync<_>>::check;

    fn shared_consumer<T: Send + Sync>() {}
    shared_consumer::<SharedProgress>();
}

#[test]
fn pending_sample_does_not_lock_unsampled_sends_or_lose_the_next_sample() {
    // Ticket 1 at 10 remains sampled while another thread owns the sample
    // slot. Ticket 2 must enqueue and account at 20 before that lock releases.
    // Admission at 30 retires ticket 1's sample. Ticket 3 at 40 may then be
    // sampled, but unsampled ticket 2 is still the oldest waiting command.
    let clock = Gate::new();
    let producer = WorkerProgress::with_clock(clock.clone());
    let observer = producer.observer().clone();
    let shared = producer.consumer();
    let (tx, rx) = std::sync::mpsc::sync_channel(TEST_QUEUE);
    let tx = producer.bind(tx);
    clock.at(10);
    tx.send(1).unwrap();
    let (held_tx, held_rx) = channel();
    let (release_tx, release_rx) = channel();
    let consumer = shared.clone();
    let worker_clock = clock.clone();
    let holder = std::thread::spawn(move || {
        let slot = consumer.lock_sample();
        assert_eq!(slot.sample.unwrap().ticket, 1);
        held_tx.send(()).unwrap();
        release_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("unsampled send completes while the slot is held");
        assert_eq!(slot.sample.unwrap().ticket, 1);
        drop(slot);
        worker_clock.at(30);
        consumer.begin(1);
        consumer.finish(false);
    });
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    clock.at(20);
    tx.send(2).unwrap();
    release_tx.send(()).unwrap();
    holder.join().expect("sample handoff fixture completed");
    let s = observer.snapshot();
    assert!(s.valid);
    assert_eq!(
        (s.counts.accepted, s.counts.retired, s.counts.queued),
        (2, 1, 1)
    );
    assert_eq!(s.last_sampled_ticket, Some(1));
    assert_eq!(s.last_sample_residence, Age::Known(20));
    assert_eq!(s.oldest_wait, Age::Unknown);
    clock.at(40);
    tx.send(3).unwrap();
    clock.at(50);
    let s = observer.snapshot();
    assert_eq!(s.sampled_ticket, Some(3));
    assert_eq!(s.sample_age, Age::Known(10));
    assert_eq!(s.oldest_wait, Age::Unknown);
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
}

#[test]
fn contended_admission_finishes_before_sample_unlock_and_recovers_known_residence() {
    // Ticket 1 is sampled at 10. The consumer must receive it and complete at
    // 30 while this fixture still owns the actual sample lock. Snapshot at 50
    // must say Unknown, not the missed 20 ns residence or a fabricated 40 ns.
    // Ticket 2 acknowledges the stale slot at 80; ticket 3 waits 100 -> 130.
    let clock = Gate::new();
    let producer = WorkerProgress::with_clock(clock.clone());
    let observer = producer.observer().clone();
    let shared = producer.consumer();
    let instance = observer.snapshot().instance;
    let (tx, rx) = std::sync::mpsc::sync_channel::<(u64, Sender<()>)>(TEST_QUEUE);
    let tx = producer.bind(tx);
    let (admit, permission) = channel();
    let (ack1, done1) = channel();
    clock.at(10);
    tx.send((1, ack1)).unwrap();
    let slot = shared.lock_sample();
    assert_eq!(slot.sample.unwrap().ticket, 1);
    let consumer = shared.clone();
    let thread = std::thread::spawn(move || {
        for expected in 1..=3 {
            let (ticket, ack) = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("ordinary endpoint delivered the next fixture command");
            assert_eq!(ticket, expected);
            permission
                .recv_timeout(Duration::from_secs(10))
                .expect("fixture permits this batch after successful send bookkeeping");
            consumer.begin(1);
            consumer.finish(false);
            ack.send(()).unwrap();
        }
    });
    clock.at(30);
    admit.send(()).unwrap();
    done1
        .recv_timeout(Duration::from_secs(10))
        .expect("begin and finish cannot wait for the held sample mutex");
    // The acknowledgement above must arrive before this release. Replacing
    // try_sample with a blocking acquisition fails the timeout, not the oracle.
    drop(slot);
    clock.at(50);
    let missed = observer.snapshot();
    assert!(missed.valid);
    assert_eq!(missed.instance, instance);
    assert_eq!(missed.phase, Phase::Idle);
    assert_eq!(
        (
            missed.counts.accepted,
            missed.counts.queued,
            missed.counts.inflight,
            missed.counts.retired,
            missed.counts.unfinished,
            missed.counts.failed_sends,
        ),
        (1, 0, 0, 1, 0, 0)
    );
    assert_eq!(missed.last_sampled_ticket, Some(1));
    assert_eq!(missed.last_sample_residence, Age::Unknown);
    assert_eq!(missed.sampled_ticket, None);
    assert_eq!(missed.sample_age, Age::NotApplicable);
    assert_eq!(missed.oldest_wait, Age::NotApplicable);
    assert_eq!(missed.processing_age, Age::NotApplicable);
    assert_eq!(missed.since_progress, Age::NotApplicable);

    let (ack2, done2) = channel();
    clock.at(60);
    tx.send((2, ack2)).unwrap();
    clock.at(80);
    admit.send(()).unwrap();
    done2.recv_timeout(Duration::from_secs(10)).unwrap();
    let acknowledged = observer.snapshot();
    assert!(acknowledged.valid);
    assert_eq!(acknowledged.counts.retired, 2);
    assert_eq!(acknowledged.last_sampled_ticket, Some(1));
    assert_eq!(acknowledged.last_sample_residence, Age::Unknown);

    let (ack3, done3) = channel();
    clock.at(100);
    tx.send((3, ack3)).unwrap();
    clock.at(115);
    let waiting = observer.snapshot();
    assert_eq!(waiting.sampled_ticket, Some(3));
    assert_eq!(waiting.sample_age, Age::Known(15));
    assert_eq!(waiting.oldest_wait, Age::Known(15));
    clock.at(130);
    admit.send(()).unwrap();
    done3.recv_timeout(Duration::from_secs(10)).unwrap();
    thread.join().expect("all three fixture commands retired");
    clock.at(150);
    let recovered = observer.snapshot();
    assert!(recovered.valid);
    assert_eq!(recovered.instance, instance);
    assert_eq!(
        (
            recovered.counts.accepted,
            recovered.counts.retired,
            recovered.counts.cycles,
            recovered.counts.queued,
            recovered.counts.inflight,
            recovered.counts.unfinished,
            recovered.counts.failed_sends,
        ),
        (3, 3, 3, 0, 0, 0, 0)
    );
    assert_eq!(recovered.last_sampled_ticket, Some(3));
    assert_eq!(recovered.last_sample_residence, Age::Known(30));
    println!(
        "Q8_REPAIR2_CONTENTION {}",
        serde_json::json!({"missed": missed, "recovered": recovered})
    );
}
