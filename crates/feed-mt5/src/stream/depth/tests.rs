//! Deadline tests use an injected monotonic instant, without sleeping.

use super::*;
use crate::protocol::{Book, BridgeMsg, WireLevel};

fn session(levels: bool) -> DepthSession {
    let line = format!(
        "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\"symbol\":\"WIN\",\"broker_symbol\":\"WIN\",\"digits\":0,\"server_utc_offset_s\":0{}}}",
        if levels { ",\"book_levels\":5" } else { "" }
    );
    let BridgeMsg::Hello(hello) = protocol::parse_line(&line).unwrap() else {
        panic!("expected hello");
    };
    DepthSession::new(&hello, "WIN".to_owned())
}

fn book(seq: u64, bid: &str) -> Book {
    Book {
        seq,
        time_ms: seq as i64,
        bids: vec![WireLevel(bid.to_owned(), "3".to_owned())],
        asks: vec![WireLevel("110".to_owned(), "5".to_owned())],
    }
}

fn statuses(rx: &mut mpsc::Receiver<Mt5Event>) -> Vec<DepthStatus> {
    let mut statuses = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let Mt5Event::Depth(DepthEvent::Status { status, .. }) = event {
            statuses.push(status);
        }
    }
    statuses
}

#[tokio::test]
async fn absent_depth_stops_loading_at_deadline_and_stays_quiet() {
    let mut depth = session(true);
    let (tx, mut rx) = mpsc::channel(32);
    let capture = BookCaptureSwitch::new();
    capture.enable(100);
    let mut offset = 0;
    let start = Instant::now();
    depth.poll(&capture, &mut offset, &tx, start).await.unwrap();
    assert_eq!(statuses(&mut rx), [DepthStatus::Connecting]);
    depth
        .poll(
            &capture,
            &mut offset,
            &tx,
            start + INITIAL_DEPTH_WAIT - Duration::from_nanos(1),
        )
        .await
        .unwrap();
    assert!(statuses(&mut rx).is_empty());
    depth
        .poll(&capture, &mut offset, &tx, start + INITIAL_DEPTH_WAIT)
        .await
        .unwrap();
    assert_eq!(
        statuses(&mut rx),
        [DepthStatus::Disconnected {
            error_class: "no_depth_signal"
        }]
    );
    let offline_generation = depth.mapper.as_ref().unwrap().generation();
    for seconds in [11, 30, 60, 3600] {
        depth
            .poll(
                &capture,
                &mut offset,
                &tx,
                start + Duration::from_secs(seconds),
            )
            .await
            .unwrap();
        assert!(
            statuses(&mut rx).is_empty(),
            "no recurring loading or retry statuses"
        );
        assert_eq!(
            depth.mapper.as_ref().unwrap().generation(),
            offline_generation
        );
    }
    depth
        .observe(
            book(1, "100"),
            &capture,
            &mut offset,
            &tx,
            start + Duration::from_secs(3601),
        )
        .await
        .unwrap();
    assert!(
        matches!(rx.try_recv().unwrap(), Mt5Event::Depth(DepthEvent::Snapshot { generation, .. }) if generation == offline_generation)
    );
    assert!(matches!(
        statuses(&mut rx).as_slice(),
        [DepthStatus::Synchronized { .. }]
    ));
}

#[tokio::test]
async fn empty_and_invalid_images_do_not_extend_initial_wait() {
    let start = Instant::now();
    for invalid in [
        Book {
            seq: 1,
            time_ms: 1,
            bids: vec![],
            asks: vec![],
        },
        book(1, "bad"),
        book(1, "120"),
        Book {
            seq: 1,
            time_ms: 1,
            bids: vec![WireLevel("100".into(), "0".into())],
            asks: vec![],
        },
    ] {
        let mut depth = session(true);
        let (tx, mut rx) = mpsc::channel(32);
        let capture = BookCaptureSwitch::new();
        capture.enable(1);
        let mut offset = 0;
        depth.poll(&capture, &mut offset, &tx, start).await.unwrap();
        statuses(&mut rx);
        depth
            .observe(
                invalid,
                &capture,
                &mut offset,
                &tx,
                start + Duration::from_secs(9),
            )
            .await
            .unwrap();
        assert!(statuses(&mut rx).is_empty());
        depth
            .poll(&capture, &mut offset, &tx, start + INITIAL_DEPTH_WAIT)
            .await
            .unwrap();
        assert_eq!(
            statuses(&mut rx),
            [DepthStatus::Disconnected {
                error_class: "no_depth_signal"
            }]
        );
        assert!(!depth.mapper.as_ref().unwrap().is_synchronized());
    }
}

#[tokio::test]
async fn unchanged_valid_depth_refreshes_freshness_and_stale_depth_recovers_as_snapshot() {
    let start = Instant::now();
    let mut depth = session(true);
    let (tx, mut rx) = mpsc::channel(32);
    let capture = BookCaptureSwitch::new();
    capture.enable(10);
    let mut offset = 0;
    depth
        .observe(book(1, "100"), &capture, &mut offset, &tx, start)
        .await
        .unwrap();
    statuses(&mut rx);
    depth
        .observe(
            book(2, "100"),
            &capture,
            &mut offset,
            &tx,
            start + Duration::from_secs(29),
        )
        .await
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "identical confirmed image needs no downstream update"
    );
    depth
        .poll(&capture, &mut offset, &tx, start + Duration::from_secs(30))
        .await
        .unwrap();
    assert!(statuses(&mut rx).is_empty());
    depth
        .observe(
            Book {
                seq: 3,
                time_ms: 3,
                bids: vec![],
                asks: vec![],
            },
            &capture,
            &mut offset,
            &tx,
            start + Duration::from_secs(58),
        )
        .await
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "empty image does not silently erase the previous ladder"
    );
    depth
        .poll(&capture, &mut offset, &tx, start + Duration::from_secs(59))
        .await
        .unwrap();
    assert_eq!(
        statuses(&mut rx),
        [DepthStatus::Disconnected {
            error_class: "stale_depth"
        }]
    );
    depth
        .observe(
            book(4, "100"),
            &capture,
            &mut offset,
            &tx,
            start + Duration::from_secs(60),
        )
        .await
        .unwrap();
    assert!(matches!(
        rx.try_recv().unwrap(),
        Mt5Event::Depth(DepthEvent::Snapshot { .. })
    ));
    assert!(matches!(
        statuses(&mut rx).as_slice(),
        [DepthStatus::Synchronized { .. }]
    ));
}

#[tokio::test]
async fn capture_changes_without_images_reset_wait_and_missing_capability_reports() {
    let start = Instant::now();
    for supported in [true, false] {
        let mut depth = session(supported);
        let (tx, mut rx) = mpsc::channel(32);
        let capture = BookCaptureSwitch::new();
        let mut offset = 0;
        depth.poll(&capture, &mut offset, &tx, start).await.unwrap();
        assert!(statuses(&mut rx).is_empty());
        capture.enable(100);
        depth.poll(&capture, &mut offset, &tx, start).await.unwrap();
        assert_eq!(statuses(&mut rx).len(), 1);
        capture.enable(200);
        depth
            .poll(&capture, &mut offset, &tx, start + Duration::from_secs(9))
            .await
            .unwrap();
        assert_eq!(
            statuses(&mut rx).len(),
            1,
            "new consumer generation receives its own status"
        );
        depth
            .poll(&capture, &mut offset, &tx, start + Duration::from_secs(10))
            .await
            .unwrap();
        assert!(statuses(&mut rx).is_empty());
        capture.disable();
        depth
            .poll(&capture, &mut offset, &tx, start + Duration::from_secs(11))
            .await
            .unwrap();
        assert_eq!(statuses(&mut rx).len(), usize::from(supported));
        capture.enable(200);
        depth
            .poll(&capture, &mut offset, &tx, start + Duration::from_secs(12))
            .await
            .unwrap();
        assert_eq!(statuses(&mut rx).len(), 1);
    }
}

#[tokio::test]
async fn reconnect_after_offline_keeps_generations_monotonic_and_accepts_identical_depth() {
    let start = Instant::now();
    let (tx, mut rx) = mpsc::channel(32);
    let capture = BookCaptureSwitch::new();
    capture.enable(100);
    let mut offset = 0;
    let mut first = session(true);
    first
        .observe(book(1, "100"), &capture, &mut offset, &tx, start)
        .await
        .unwrap();
    statuses(&mut rx);
    first
        .poll(&capture, &mut offset, &tx, start + STALE_DEPTH_WAIT)
        .await
        .unwrap();
    let offline_generation = first.mapper.as_ref().unwrap().generation();
    statuses(&mut rx);
    first.close(&tx).await;
    assert_eq!(
        statuses(&mut rx),
        [DepthStatus::Disconnected {
            error_class: "bridge_lost"
        }]
    );
    let mut reconnected = session(true);
    reconnected
        .observe(
            book(1, "100"),
            &capture,
            &mut offset,
            &tx,
            start + Duration::from_secs(31),
        )
        .await
        .unwrap();
    assert!(
        matches!(rx.try_recv().unwrap(), Mt5Event::Depth(DepthEvent::Status { generation, status: DepthStatus::Connecting, .. }) if generation > offline_generation)
    );
    assert!(
        matches!(rx.try_recv().unwrap(), Mt5Event::Depth(DepthEvent::Snapshot { generation, .. }) if generation > offline_generation)
    );
    assert!(matches!(
        statuses(&mut rx).as_slice(),
        [DepthStatus::Synchronized { .. }]
    ));
}
