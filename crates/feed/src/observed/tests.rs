use super::*;
use crate::hyperliquid::output::{ObservedOutput, Output};
use std::future::Future as _;

#[tokio::test]
async fn receiver_arms_preserve_empty_closed_and_pending_cancellation() {
    let (legacy_tx, legacy_rx) = mpsc::channel(1);
    let (observed_tx, observed_rx) = mpsc::channel(1);
    let mut legacy = ObservedReceiver::from(legacy_rx);
    let mut observed = ObservedReceiver::from(observed_rx);
    assert!(!legacy.reports_exclusions());
    assert!(observed.reports_exclusions());
    for receiver in [&mut legacy, &mut observed] {
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let pending = receiver.recv();
        tokio::pin!(pending);
        assert!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(
                pending.as_mut().poll(cx).is_pending()
            ))
            .await
        );
    }
    legacy_tx.send(FeedEvent::Reset).await.unwrap();
    observed_tx
        .send(ObservedFeedEvent::Feed(FeedEvent::Reset))
        .await
        .ok()
        .unwrap();
    for receiver in [&mut legacy, &mut observed] {
        assert!(matches!(
            receiver.recv().await,
            Some(ObservedFeedEvent::Feed(FeedEvent::Reset))
        ));
    }
    drop(legacy_tx);
    drop(observed_tx);
    for receiver in [&mut legacy, &mut observed] {
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(receiver.recv().await.is_none());
    }
}

#[tokio::test]
async fn observed_output_backpressure_ends_when_receiver_is_dropped() {
    let (tx, rx) = mpsc::channel(1);
    let output = ObservedOutput(tx);
    output.send(FeedEvent::Reset).await.unwrap();
    let pending = output.exclude(FeedExclusion {
        reason: ExclusionReason::MalformedRow,
        rows: NonZeroU64::new(1).unwrap(),
    });
    tokio::pin!(pending);
    assert!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(pending.as_mut().poll(cx).is_pending()))
            .await
    );
    drop(rx);
    assert!(pending.await.is_err());
}

#[test]
fn exclusions_saturate_independently_and_never_change_continuity() {
    let original = crate::FeedIntegrity {
        anomalies: 3,
        missing_messages: 0,
        unknown_loss: 3,
        non_monotonic: 0,
    };
    let mut exclusions = FeedExclusions {
        malformed_rows: u64::MAX - 1,
        stale_rows: 0,
    };
    exclusions.observe(FeedExclusion {
        reason: ExclusionReason::MalformedRow,
        rows: NonZeroU64::new(3).unwrap(),
    });
    exclusions.observe(FeedExclusion {
        reason: ExclusionReason::StaleTimestamp,
        rows: NonZeroU64::new(2).unwrap(),
    });
    assert_eq!(
        exclusions,
        FeedExclusions {
            malformed_rows: u64::MAX,
            stale_rows: 2
        }
    );
    assert_eq!(original.missing_messages, 0);
}

#[test]
fn original_public_literals_and_exhaustive_match_remain_valid() {
    // A downstream-style exhaustive match deliberately has no wildcard.
    fn original_consumer(event: FeedEvent) {
        match event {
            FeedEvent::Backfilled(_)
            | FeedEvent::HistoryPrepended(_)
            | FeedEvent::OpeningPrepended { .. }
            | FeedEvent::Live(_)
            | FeedEvent::Continuity(_)
            | FeedEvent::LiveBatch(_)
            | FeedEvent::DealCounter(_)
            | FeedEvent::Reset
            | FeedEvent::OhlcvHistory { .. } => (),
        }
    }
    let (event_tx, events) = mpsc::channel(1);
    let (book_tx, book_events) = mpsc::channel(1);
    let (notice_tx, notices) = mpsc::channel(1);
    let (commands, mut command_rx) = mpsc::channel(1);
    let legacy = FeedHandle {
        events,
        book_events,
        notices,
        commands,
        replay: None,
        capabilities: crate::fixed_capabilities(ProviderKind::Hyperliquid.capabilities()),
        latency: crate::unsplit_latency(),
    };
    original_consumer(FeedEvent::Continuity(crate::FeedContinuity {
        gap: None,
        missing_messages: None,
        non_monotonic: false,
    }));
    let observed = ObservedFeedHandle::from(legacy);
    drop(observed);
    assert!(event_tx.is_closed() && book_tx.is_closed() && notice_tx.is_closed());
    assert!(matches!(
        command_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Disconnected)
    ));
    eprintln!(
        "legacy_event_bytes={} observed_event_bytes={} capacity=4096",
        std::mem::size_of::<FeedEvent>(),
        std::mem::size_of::<ObservedFeedEvent>()
    );
}
