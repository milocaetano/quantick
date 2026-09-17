use super::*;
use crate::FeedEvent;
use quantick_engine::Side;
use rust_decimal::Decimal;
use tokio::sync::mpsc;

/// Drain the same ordered diagnostic/trade pair that the host publishes.
async fn forward(continuity: &mut BinanceContinuity, trade: Trade, tx: &mpsc::Sender<FeedEvent>) {
    if let Some(event) = continuity.observe(&trade) {
        tx.send(FeedEvent::Continuity(event)).await.unwrap();
    }
    tx.send(FeedEvent::Live(trade)).await.unwrap();
}

#[tokio::test]
async fn binance_automatic_reconnect_reports_loss_on_the_host_channel() {
    use futures_util::SinkExt as _;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for id in [10, 14, 15] {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            let frame = format!(
                r#"{{"e":"aggTrade","s":"BTCUSDT","a":{id},"p":"1","q":"1","f":1,"l":1,"T":100,"m":false}}"#
            );
            socket.send(Message::Text(frame)).await.unwrap();
            socket.close(None).await.unwrap();
        }
    });
    let rest = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rest_url = format!("http://{}", rest.local_addr().unwrap());
    let rest_server = tokio::spawn(async move {
        let (mut socket, _) = rest.accept().await.unwrap();
        let mut request = [0; 4096];
        let mut read = 0;
        while read < request.len() && !request[..read].ends_with(b"\r\n\r\n") {
            let received = socket.read(&mut request[read..]).await.unwrap();
            assert!(received > 0, "HTTP request ended before its header");
            read += received;
        }
        assert!(request[..read].ends_with(b"\r\n\r\n"));
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]").await.unwrap();
    });
    let (tx, mut rx) = mpsc::channel(8);
    let (book_tx, _book_rx) = mpsc::channel(8);
    let (notice_tx, _notice_rx) = mpsc::channel(32);
    let (cmd_tx, cmd_rx) = mpsc::channel(8);
    let host = tokio::spawn(crate::binance::feed_task(
        "BTCUSDT".into(),
        tx,
        book_tx,
        notice_tx,
        cmd_rx,
        crate::binance::BinanceSource {
            http: quantick_feed_binance::BinanceHttp::with_base_url(rest_url),
            url,
            backoff: quantick_feed_binance::Backoff::new(
                Duration::from_millis(1),
                Duration::from_millis(1),
                7,
            ),
        },
    ));
    // Bound the whole production host run, including its initial REST boundary.
    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        for _ in 0..6 {
            events.push(rx.recv().await.unwrap());
        }
        events
    })
    .await
    .unwrap();
    assert!(matches!(&received[0], FeedEvent::Backfilled(t) if t.is_empty()));
    assert!(matches!(
        &received[1],
        FeedEvent::Continuity(FeedContinuity {
            gap: None,
            missing_messages: None,
            non_monotonic: false,
        })
    ));
    assert!(matches!(&received[2], FeedEvent::Live(t) if t.agg_id == 10));
    assert!(matches!(
        &received[3],
        FeedEvent::Continuity(FeedContinuity {
            missing_messages: Some(3),
            ..
        })
    ));
    assert!(matches!(&received[4], FeedEvent::Live(t) if t.agg_id == 14));
    assert!(matches!(&received[5], FeedEvent::Live(t) if t.agg_id == 15));
    assert!(
        rx.try_recv().is_err(),
        "contiguous reconnect must not invent loss"
    );
    drop(cmd_tx);
    tokio::time::timeout(Duration::from_secs(5), host)
        .await
        .unwrap()
        .unwrap();
    server.await.unwrap();
    rest_server.await.unwrap();
}

#[tokio::test]
async fn binance_backfill_watermark_orders_equal_time_gap_before_first_live_trade() {
    use futures_util::SinkExt as _;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    const BACKFILL: &str = r#"[{"a":10,"p":"1","q":"1","f":1,"l":1,"T":100,"m":false}]"#;
    const FIRST_LIVE: &str =
        r#"{"e":"aggTrade","s":"BTCUSDT","a":14,"p":"1","q":"1","f":1,"l":1,"T":100,"m":false}"#;

    let rest = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rest_url = format!("http://{}", rest.local_addr().unwrap());
    let rest_server = tokio::spawn(async move {
        // A one-row history causes the paging algorithm to make one no-progress
        // request after the newest page. Both replies are literal and equal.
        for _ in 0..2 {
            let (mut socket, _) = rest.accept().await.unwrap();
            let mut request = [0; 4096];
            let mut read = 0;
            while read < request.len() && !request[..read].ends_with(b"\r\n\r\n") {
                let received = socket.read(&mut request[read..]).await.unwrap();
                assert!(received > 0, "HTTP request ended before its header");
                read += received;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BACKFILL}",
                BACKFILL.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });

    let websocket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let websocket_url = format!("ws://{}", websocket.local_addr().unwrap());
    let websocket_server = tokio::spawn(async move {
        let (socket, _) = websocket.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
        socket.send(Message::Text(FIRST_LIVE.into())).await.unwrap();
        socket.close(None).await.unwrap();
    });

    let (tx, mut rx) = mpsc::channel(8);
    let (book_tx, _book_rx) = mpsc::channel(8);
    let (notice_tx, _notice_rx) = mpsc::channel(8);
    let (cmd_tx, cmd_rx) = mpsc::channel(8);
    let host = tokio::spawn(crate::binance::feed_task(
        "BTCUSDT".into(),
        tx,
        book_tx,
        notice_tx,
        cmd_rx,
        crate::binance::BinanceSource {
            http: quantick_feed_binance::BinanceHttp::with_base_url(rest_url),
            url: websocket_url,
            backoff: quantick_feed_binance::Backoff::new(
                Duration::from_millis(1),
                Duration::from_millis(1),
                7,
            ),
        },
    ));

    let received = tokio::time::timeout(Duration::from_secs(5), async {
        [
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
            rx.recv().await.unwrap(),
        ]
    })
    .await
    .unwrap();
    assert!(matches!(&received[0], FeedEvent::Backfilled(trades) if trades == &[trade(10, 100)]));
    assert!(matches!(
        &received[1],
        FeedEvent::Continuity(FeedContinuity {
            gap: Some(FeedGap {
                from_ms: 100,
                to_ms: 100,
            }),
            missing_messages: Some(3),
            non_monotonic: false,
        })
    ));
    assert!(matches!(&received[2], FeedEvent::Live(t) if t == &trade(14, 100)));
    assert!(rx.try_recv().is_err());

    drop(rx);
    drop(cmd_tx);
    tokio::time::timeout(Duration::from_secs(5), host)
        .await
        .unwrap()
        .unwrap();
    rest_server.await.unwrap();
    websocket_server.await.unwrap();
}

fn trade(agg_id: u64, timestamp_ms: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[tokio::test]
async fn real_rest_handoff_covers_contiguous_overlap_forward_and_unknown_startup() {
    use crate::test_support::{BackfillFixture, binance_events};

    for (live, expected) in [
        ((11, 101), None),
        ((10, 100), Some((Some(0), None, true))),
        ((9, 99), Some((Some(0), None, true))),
        (
            (14, 120),
            Some((
                Some(3),
                Some(FeedGap {
                    from_ms: 100,
                    to_ms: 120,
                }),
                false,
            )),
        ),
        (
            (14, 100),
            Some((
                Some(3),
                Some(FeedGap {
                    from_ms: 100,
                    to_ms: 100,
                }),
                false,
            )),
        ),
    ] {
        let events = binance_events(
            BackfillFixture::Seed,
            &[live],
            if expected.is_some() { 3 } else { 2 },
        )
        .await;
        assert!(matches!(&events[0], FeedEvent::Backfilled(trades) if trades == &[trade(10, 100)]));
        if let Some((missing_messages, gap, non_monotonic)) = expected {
            assert!(
                matches!(events[1], FeedEvent::Continuity(event) if event == FeedContinuity { gap, missing_messages, non_monotonic })
            );
        }
        assert!(matches!(events.last(), Some(FeedEvent::Live(t)) if *t == trade(live.0, live.1)));
    }
    for backfill in [BackfillFixture::Empty, BackfillFixture::Failed] {
        let events = binance_events(backfill, &[(10, 100), (11, 101)], 4).await;
        assert!(matches!(&events[0], FeedEvent::Backfilled(trades) if trades.is_empty()));
        assert!(matches!(
            events[1],
            FeedEvent::Continuity(FeedContinuity {
                gap: None,
                missing_messages: None,
                non_monotonic: false
            })
        ));
        assert!(matches!(&events[2], FeedEvent::Live(t) if *t == trade(10, 100)));
        assert!(matches!(&events[3], FeedEvent::Live(t) if *t == trade(11, 101)));
    }
}

#[tokio::test]
async fn binance_reports_equal_timestamp_loss_before_the_unmodified_trade() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut continuity = BinanceContinuity::default();
    forward(&mut continuity, trade(10, 100), &tx).await;
    forward(&mut continuity, trade(14, 100), &tx).await;
    assert!(matches!(rx.recv().await, Some(FeedEvent::Live(t)) if t.agg_id == 10));
    assert!(matches!(
        rx.recv().await,
        Some(FeedEvent::Continuity(FeedContinuity {
            gap: Some(FeedGap {
                from_ms: 100,
                to_ms: 100
            }),
            missing_messages: Some(3),
            non_monotonic: false,
        }))
    ));
    assert!(matches!(rx.recv().await, Some(FeedEvent::Live(t)) if t == trade(14, 100)));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn binance_initial_attach_duplicates_and_reorders_do_not_invent_loss() {
    let (tx, mut rx) = mpsc::channel(16);
    let mut continuity = BinanceContinuity::default();
    for id in [500, 500, 499, 501] {
        forward(&mut continuity, trade(id, 100), &tx).await;
    }
    let mut ids = Vec::new();
    let mut integrity = FeedIntegrity::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            FeedEvent::Live(t) => ids.push(t.agg_id),
            FeedEvent::Continuity(event) => {
                assert_eq!(event.gap, None);
                integrity.observe(event);
            }
            _ => panic!("unexpected event"),
        }
    }
    assert_eq!(ids, [500, 500, 499, 501]);
    assert_eq!(integrity.missing_messages, 0);
    assert_eq!(integrity.non_monotonic, 2);
}

#[test]
fn missing_ticks_and_unknown_interruptions_have_distinct_saturating_counters() {
    let mut integrity = FeedIntegrity::default();
    integrity.observe(FeedContinuity::mt5(
        SeqAnomaly::Gap {
            expected: 2,
            got: 6,
            missing: 4,
        },
        100,
        90,
    ));
    integrity.observe(FeedContinuity {
        gap: None,
        missing_messages: None,
        non_monotonic: false,
    });
    assert_eq!(
        integrity,
        FeedIntegrity {
            anomalies: 2,
            missing_messages: 4,
            unknown_loss: 1,
            non_monotonic: 0,
        }
    );
    integrity.missing_messages = u64::MAX;
    integrity.observe(FeedContinuity::mt5(
        SeqAnomaly::Gap {
            expected: 2,
            got: 6,
            missing: 4,
        },
        100,
        100,
    ));
    assert_eq!(integrity.missing_messages, u64::MAX);
}

#[test]
fn mt5_reconnect_labels_unknown_loss_once_before_the_resumed_trade() {
    let mut pending = Some(100);
    let event = mt5_reconnect_gap(&trade(1, 120), &mut pending);
    assert!(matches!(
        event,
        Some(FeedContinuity {
            gap: Some(FeedGap {
                from_ms: 100,
                to_ms: 120
            }),
            missing_messages: None,
            non_monotonic: false,
        })
    ));
    assert_eq!(mt5_reconnect_gap(&trade(2, 121), &mut pending), None);
    assert_eq!(pending, None);
}

#[tokio::test]
async fn late_binance_reorder_cannot_move_the_next_gap_backwards() {
    let (tx, mut rx) = mpsc::channel(8);
    let mut continuity = BinanceContinuity::default();
    for (id, ms) in [(100, 1000), (90, 500), (102, 1020)] {
        forward(&mut continuity, trade(id, ms), &tx).await;
    }
    let mut gaps = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let FeedEvent::Continuity(FeedContinuity {
            gap: Some(gap),
            missing_messages,
            ..
        }) = event
        {
            assert_eq!(missing_messages, Some(1));
            gaps.push(gap);
        }
    }
    assert_eq!(
        gaps,
        [FeedGap {
            from_ms: 1000,
            to_ms: 1020
        }]
    );
}

#[tokio::test]
#[ignore = "manual hot-path benchmark; prints baseline and candidate timings"]
async fn benchmark_contiguous_binance_delivery() {
    use std::hint::black_box;
    use std::time::Instant;
    const PRINTS: u64 = 1_000_000;
    const FRAME: &str = r#"{"e":"aggTrade","s":"BTCUSDT","a":1,"p":"36000.30","q":"0.750","f":1,"l":3,"T":1700000000450,"m":true}"#;
    let mut timings = [Vec::new(), Vec::new()];
    for round in 0..20 {
        for candidate in [round % 2 == 0, round % 2 != 0] {
            let (tx, mut rx) = mpsc::channel(8);
            let mut socket_tracker = quantick_feed_binance::ContinuityTracker::new();
            let mut host_tracker = BinanceContinuity::default();
            let start = Instant::now();
            for id in 0..PRINTS {
                let mut print =
                    quantick_feed_binance::stream::decode_text(black_box(FRAME)).unwrap();
                print.agg_id = id;
                black_box(socket_tracker.observe(&print));
                if candidate && let Some(event) = host_tracker.observe(&print) {
                    tx.send(FeedEvent::Continuity(event)).await.unwrap();
                }
                tx.send(FeedEvent::Live(print)).await.unwrap();
                black_box(rx.recv().await);
            }
            timings[usize::from(candidate)].push(start.elapsed().as_nanos() as f64 / PRINTS as f64);
        }
    }
    println!(
        "paired_baseline_ns={:?}\npaired_candidate_ns={:?}",
        timings[0], timings[1]
    );
    for values in &mut timings {
        values.sort_by(f64::total_cmp);
    }
    println!(
        "contiguous Binance decode/tracker/channel, {PRINTS} prints x20: baseline_median_ns={:.2} candidate_median_ns={:.2}",
        (timings[0][9] + timings[0][10]) / 2.0,
        (timings[1][9] + timings[1][10]) / 2.0
    );
    println!(
        "baseline_ns={:?}\ncandidate_ns={:?}",
        timings[0], timings[1]
    );
}
