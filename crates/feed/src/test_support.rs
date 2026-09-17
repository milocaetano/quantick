//! Literal loopback transports for cross-crate integrity tests.
//!
//! Only test builds and the explicit `test-support` feature expose this module.
//! Every returned event comes from the production source and host; no event is
//! reconstructed from the expected result. No fixture sends host commands that
//! could start a real depth/history connection.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::FeedEvent;

/// A literal successful, empty or failed initial REST boundary.
#[derive(Clone, Copy)]
pub enum BackfillFixture {
    /// One aggregate, ID 10 at market time 100.
    Seed,
    /// A successful empty response.
    Empty,
    /// An HTTP error, not an empty success.
    Failed,
}

/// Capture the real Binance host over literal local REST and WebSocket input.
/// `event_count` is supplied by the independently specified expected sequence.
/// Panics on a fixture/transport failure or missing expected event.
pub async fn binance_events(
    backfill: BackfillFixture,
    live: &[(u64, i64)],
    event_count: usize,
) -> Vec<FeedEvent> {
    let rest = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rest_url = format!("http://{}", rest.local_addr().unwrap());
    let rest_server = tokio::spawn(async move {
        let (status, body) = match backfill {
            BackfillFixture::Seed => (
                "200 OK",
                r#"[{"a":10,"p":"1","q":"1","f":1,"l":1,"T":100,"m":false}]"#,
            ),
            BackfillFixture::Empty => ("200 OK", "[]"),
            BackfillFixture::Failed => ("400 Bad Request", "fixture unavailable"),
        };
        // The one-row history makes one no-progress paging request. Returning
        // the same literal reply also tolerates a different backfill target.
        loop {
            let (mut socket, _) = rest.accept().await.unwrap();
            let mut request = [0; 4096];
            let mut read = 0;
            while read < request.len() && !request[..read].ends_with(b"\r\n\r\n") {
                let received = socket.read(&mut request[read..]).await.unwrap();
                assert!(received > 0, "HTTP request ended before its header");
                read += received;
            }
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let websocket = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", websocket.local_addr().unwrap());
    let live = live.to_vec();
    let server = tokio::spawn(async move {
        let (socket, _) = websocket.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
        for (id, time) in live {
            let frame = format!(
                r#"{{"e":"aggTrade","s":"BTCUSDT","a":{id},"p":"1","q":"1","f":1,"l":1,"T":{time},"m":false}}"#
            );
            socket.send(Message::Text(frame)).await.unwrap();
        }
        // Leave the connection open until the fixture has captured its tape.
        std::future::pending::<()>().await;
    });
    let (tx, mut rx) = mpsc::channel(128);
    let (book_tx, _book_rx) = mpsc::channel(8);
    let (notice_tx, _notice_rx) = mpsc::channel(16);
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
    let events = collect(&mut rx, event_count).await;
    drop(cmd_tx);
    tokio::time::timeout(Duration::from_secs(3), host)
        .await
        .unwrap()
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "unexpected event after host shutdown"
    );
    server.abort();
    rest_server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert!(rest_server.await.unwrap_err().is_cancelled());
    events
}

async fn accept_subscription(
    listener: &TcpListener,
) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
    let (stream, _) = listener.accept().await.unwrap();
    let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
    let message = socket.next().await.unwrap().unwrap();
    assert!(matches!(message, Message::Text(text) if text.contains("\"type\":\"trades\"")));
    socket
}

/// Capture three real Hyperliquid sessions: quiet, mixed recovery and overlap.
/// The final session has no usable trade; its stale row and disconnect still
/// have to reach the consumer. Panics on missing events or transport failures.
pub async fn hyperliquid_events() -> Vec<FeedEvent> {
    const ACK: &str = r#"{"channel":"subscriptionResponse","data":{}}"#;
    const EMPTY: &str = r#"{"channel":"trades","data":[]}"#;
    const MIXED: &str = r#"{"channel":"trades","data":[{"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7},{"coin":"BTC","side":"X","px":"1","sz":"1","time":201,"tid":8}]}"#;
    const OVERLAP: &str = r#"{"channel":"trades","data":[{"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7}]}"#;
    const STALE: &str = r#"{"channel":"trades","data":[{"coin":"BTC","side":"A","px":"1","sz":"1","time":199,"tid":9}]}"#;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        for frames in [&[ACK][..], &[ACK, EMPTY, MIXED], &[ACK, OVERLAP, STALE]] {
            let mut socket = accept_subscription(&listener).await;
            for frame in frames {
                socket.send(Message::Text((*frame).into())).await.unwrap();
            }
            socket.close(None).await.unwrap();
        }
    });
    let (tx, mut rx) = mpsc::channel(16);
    let (book_tx, _book_rx) = mpsc::channel(8);
    let (notice_tx, _notice_rx) = mpsc::channel(16);
    let (cmd_tx, cmd_rx) = mpsc::channel(8);
    let host = tokio::spawn(crate::hyperliquid::feed_task(
        "BTC".into(),
        tx,
        book_tx,
        notice_tx,
        cmd_rx,
        crate::hyperliquid::HyperliquidSource {
            url,
            backoff: quantick_feed_hyperliquid::Backoff::new(
                Duration::from_millis(1),
                Duration::from_millis(1),
                7,
            ),
        },
    ));
    let events = collect(&mut rx, 7).await;
    drop(cmd_tx);
    tokio::time::timeout(Duration::from_secs(3), host)
        .await
        .unwrap()
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "unexpected event after host shutdown"
    );
    server.await.unwrap();
    events
}

async fn collect(rx: &mut mpsc::Receiver<FeedEvent>, count: usize) -> Vec<FeedEvent> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        for _ in 0..count {
            events.push(rx.recv().await.expect("host ended before expected event"));
        }
        assert!(rx.try_recv().is_err(), "unexpected extra host event");
        events
    })
    .await
    .expect("expected host event missing")
}
