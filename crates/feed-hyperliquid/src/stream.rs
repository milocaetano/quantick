//! Reconnecting Hyperliquid public-trades WebSocket.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use quantick_engine::Trade;
use tokio::sync::{mpsc::Sender, watch};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::reconnect::Backoff;
use crate::wire::{MappedBatch, TradeMapper, TradeMessage, parse_trade_message};

/// Hyperliquid mainnet WebSocket endpoint.
pub const HYPERLIQUID_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";

/// Application heartbeat cadence, safely below the venue's 60-second idle
/// disconnect threshold.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Why one trade connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeSessionError {
    /// UI receiver was dropped.
    ConsumerClosed,
    /// Connect, read, or write failure.
    Transport(String),
    /// Malformed protocol envelope or trade payload.
    Decode(String),
    /// Server sent a close frame or ended the stream.
    ServerClosed { reason: Option<String> },
}

/// One ordered fact from the Hyperliquid trade transport.
///
/// Batches retain their mapping ledger even when they contain no usable trade;
/// connection transitions share the same bounded channel so rapid or quiet
/// reconnects cannot be coalesced away from the facts they qualify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TradeStreamEvent {
    /// The venue acknowledged this connection's trade subscription.
    Connected,
    /// A previously acknowledged connection ended before the next attempt.
    Disconnected,
    /// One venue batch, including empty, all-overlap, stale, or malformed
    /// outcomes.
    Batch(MappedBatch),
}

enum SessionOutput<'a> {
    Legacy {
        trades: &'a Sender<Vec<Trade>>,
        connected: &'a watch::Sender<bool>,
    },
    Ordered(&'a Sender<TradeStreamEvent>),
}

impl SessionOutput<'_> {
    async fn closed(&self) {
        match self {
            Self::Legacy { trades, .. } => trades.closed().await,
            Self::Ordered(events) => events.closed().await,
        }
    }

    async fn publish_connected(&self) -> Result<(), TradeSessionError> {
        match self {
            Self::Legacy { connected, .. } => {
                let _ = connected.send(true);
                Ok(())
            }
            Self::Ordered(events) => events
                .send(TradeStreamEvent::Connected)
                .await
                .map_err(|_| TradeSessionError::ConsumerClosed),
        }
    }

    async fn publish_batch(&self, batch: MappedBatch) -> Result<(), TradeSessionError> {
        match self {
            Self::Legacy { trades, .. } => {
                if !batch.trades.is_empty() {
                    trades
                        .send(batch.trades)
                        .await
                        .map_err(|_| TradeSessionError::ConsumerClosed)?;
                }
                Ok(())
            }
            Self::Ordered(events) => events
                .send(TradeStreamEvent::Batch(batch))
                .await
                .map_err(|_| TradeSessionError::ConsumerClosed),
        }
    }
}

impl TradeSessionError {
    /// Stable class for logs.
    #[must_use]
    pub fn error_class(&self) -> &'static str {
        match self {
            Self::ConsumerClosed => "consumer_closed",
            Self::Transport(_) => "transport",
            Self::Decode(_) => "wire_decode",
            Self::ServerClosed { .. } => "server_closed",
        }
    }
}

impl std::fmt::Display for TradeSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConsumerClosed => f.write_str("trade consumer closed"),
            Self::Transport(error) => write!(f, "trade websocket error: {error}"),
            Self::Decode(error) => write!(f, "trade payload decode error: {error}"),
            Self::ServerClosed { reason } => write!(
                f,
                "trade websocket closed{}",
                reason
                    .as_deref()
                    .map_or_else(String::new, |reason| format!(": {reason}"))
            ),
        }
    }
}

impl std::error::Error for TradeSessionError {}

/// Run one subscribed trade connection until it closes.
///
/// The mapper lives outside a connection so the initial recovery batch and
/// reconnect replay are deduplicated against the already-published timeline.
///
/// # Errors
///
/// Returns a typed transport/protocol/consumer termination reason.
pub async fn run_trade_session(
    url: &str,
    symbol: &str,
    trades: &Sender<Vec<Trade>>,
    connected: &watch::Sender<bool>,
    mapper: &mut TradeMapper,
) -> Result<(), TradeSessionError> {
    let mut acknowledged = false;
    run_trade_session_core(
        url,
        symbol,
        &SessionOutput::Legacy { trades, connected },
        mapper,
        &mut acknowledged,
    )
    .await
}

async fn run_trade_session_core(
    url: &str,
    symbol: &str,
    output: &SessionOutput<'_>,
    mapper: &mut TradeMapper,
    acknowledged: &mut bool,
) -> Result<(), TradeSessionError> {
    let symbol = symbol.to_uppercase();
    info!(target: "quantick::feed", symbol, url, "connecting Hyperliquid trades websocket");
    let connection = tokio::select! {
        () = output.closed() => return Err(TradeSessionError::ConsumerClosed),
        result = tokio_tungstenite::connect_async(url) => result,
    }
    .map_err(|error| TradeSessionError::Transport(error.to_string()))?;
    let (mut socket, _response) = connection;
    let subscription = serde_json::json!({
        "method": "subscribe",
        "subscription": { "type": "trades", "coin": symbol }
    });
    socket
        .send(Message::Text(subscription.to_string()))
        .await
        .map_err(|error| TradeSessionError::Transport(error.to_string()))?;
    info!(target: "quantick::feed", symbol, "Hyperliquid trades websocket subscribed");

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            () = output.closed() => return Err(TradeSessionError::ConsumerClosed),
            _ = heartbeat.tick() => {
                socket
                    .send(Message::Text(r#"{"method":"ping"}"#.into()))
                    .await
                    .map_err(|error| TradeSessionError::Transport(error.to_string()))?;
            }
            frame = socket.next() => {
                let Some(frame) = frame else {
                    return Err(TradeSessionError::ServerClosed { reason: None });
                };
                let message = frame
                    .map_err(|error| TradeSessionError::Transport(error.to_string()))?;
                match message {
                    Message::Text(text) => {
                        match parse_trade_message(text.as_str())
                            .map_err(|error| TradeSessionError::Decode(error.to_string()))?
                        {
                            TradeMessage::Trades(raw) => {
                                let batch = mapper.map_batch(raw);
                                if !batch.errors.is_empty() || batch.stale > 0 {
                                    warn!(
                                        target: "quantick::feed",
                                        symbol,
                                        malformed = batch.errors.len(),
                                        stale = batch.stale,
                                        duplicates = batch.duplicates,
                                        first_error = batch.errors.first().map(ToString::to_string),
                                        action = "skip_invalid_rows",
                                        "Hyperliquid trade batch was only partially usable"
                                    );
                                } else if batch.duplicates > 0 {
                                    debug!(
                                        target: "quantick::feed",
                                        symbol,
                                        duplicates = batch.duplicates,
                                        "suppressed Hyperliquid recovery overlap"
                                    );
                                }
                                output.publish_batch(batch).await?;
                            }
                            TradeMessage::Subscribed => {
                                if !*acknowledged {
                                    *acknowledged = true;
                                    output.publish_connected().await?;
                                }
                                debug!(target: "quantick::feed", symbol, "trade subscription acknowledged");
                            }
                            TradeMessage::Pong => {}
                            TradeMessage::Other { channel } => {
                                debug!(target: "quantick::feed", symbol, channel, "ignored unexpected channel");
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|error| TradeSessionError::Transport(error.to_string()))?;
                    }
                    Message::Close(frame) => {
                        let reason = frame.map(|frame| format!("{} {}", u16::from(frame.code), frame.reason));
                        return Err(TradeSessionError::ServerClosed { reason });
                    }
                    Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
                }
            }
        }
    }
}

/// Keep the full-fidelity ordered event stream alive across disconnects.
///
/// Unlike the compatibility trade/watch API, connection transitions and every
/// mapped batch share one bounded channel. A connection that never reached an
/// acknowledged subscription does not fabricate a disconnect transition.
pub async fn run_trade_events_with_reconnect(
    url: &str,
    symbol: &str,
    events: &Sender<TradeStreamEvent>,
    mut mapper: TradeMapper,
    mut backoff: Backoff,
) {
    let symbol = symbol.to_uppercase();
    loop {
        let published_before = mapper.published_count();
        let mut acknowledged = false;
        let result = run_trade_session_core(
            url,
            &symbol,
            &SessionOutput::Ordered(events),
            &mut mapper,
            &mut acknowledged,
        )
        .await;
        if mapper.published_count() > published_before {
            backoff.reset();
        }
        match result {
            Ok(()) => unreachable!("the open-ended trade session never returns success"),
            Err(TradeSessionError::ConsumerClosed) => {
                info!(target: "quantick::feed", symbol, "trade event consumer gone; stopping Hyperliquid feed");
                return;
            }
            Err(error) => {
                if acknowledged && events.send(TradeStreamEvent::Disconnected).await.is_err() {
                    return;
                }
                warn!(
                    target: "quantick::feed",
                    symbol,
                    error_class = error.error_class(),
                    %error,
                    action = "reconnect",
                    "Hyperliquid trade session ended"
                );
            }
        }
        if events.is_closed() {
            return;
        }
        let delay = backoff.next_delay();
        info!(
            target: "quantick::feed",
            symbol,
            attempt = backoff.attempt(),
            delay_ms = delay.as_millis() as u64,
            "backing off before Hyperliquid trade reconnect"
        );
        tokio::select! {
            () = events.closed() => return,
            () = tokio::time::sleep(delay) => {}
        }
    }
}

/// Keep the trade stream alive across API-server disconnects.
pub async fn run_trades_with_reconnect(
    url: &str,
    symbol: &str,
    trades: &Sender<Vec<Trade>>,
    connected: &watch::Sender<bool>,
    mut mapper: TradeMapper,
    mut backoff: Backoff,
) {
    let symbol = symbol.to_uppercase();
    loop {
        let published_before = mapper.published_count();
        let result = run_trade_session(url, &symbol, trades, connected, &mut mapper).await;
        if mapper.published_count() > published_before {
            // A connection that carried data was healthy, however long it
            // lasted. The next isolated disconnect starts at the base delay.
            backoff.reset();
        }
        match result {
            Ok(()) => unreachable!("the open-ended trade session never returns success"),
            Err(TradeSessionError::ConsumerClosed) => {
                info!(target: "quantick::feed", symbol, "trade consumer gone; stopping Hyperliquid feed");
                return;
            }
            Err(error) => {
                // Publish the transport loss before the reconnect wait. A
                // quiet tape is not evidence either way; only the socket and
                // subscription lifecycle drive this signal.
                let _ = connected.send(false);
                warn!(
                    target: "quantick::feed",
                    symbol,
                    error_class = error.error_class(),
                    %error,
                    action = "reconnect",
                    "Hyperliquid trade session ended"
                );
            }
        }
        if trades.is_closed() {
            return;
        }
        let delay = backoff.next_delay();
        info!(
            target: "quantick::feed",
            symbol,
            attempt = backoff.attempt(),
            delay_ms = delay.as_millis() as u64,
            "backing off before Hyperliquid trade reconnect"
        );
        tokio::select! {
            () = trades.closed() => return,
            () = tokio::time::sleep(delay) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn ordered_quiet_cycles_survive_delayed_drain_and_consumer_close() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let mut socket = accept_subscription(&listener).await;
                socket
                    .send(Message::Text(
                        r#"{"channel":"subscriptionResponse","data":{}}"#.into(),
                    ))
                    .await
                    .unwrap();
                socket.close(None).await.unwrap();
            }
            done_tx.send(()).unwrap();
        });
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let source = tokio::spawn(async move {
            run_trade_events_with_reconnect(
                &url,
                "BTC",
                &tx,
                TradeMapper::new("BTC"),
                Backoff::new(Duration::from_millis(1), Duration::from_millis(1), 7),
            )
            .await;
        });
        tokio::time::timeout(Duration::from_secs(3), done_rx)
            .await
            .unwrap()
            .unwrap();
        // This delayed drain is the same observation schedule as the legacy
        // baseline reproduction: two whole quiet cycles are already over.
        for _ in 0..3 {
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(3), rx.recv())
                    .await
                    .unwrap(),
                Some(TradeStreamEvent::Connected)
            ));
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(3), rx.recv())
                    .await
                    .unwrap(),
                Some(TradeStreamEvent::Disconnected)
            ));
        }
        drop(rx);
        tokio::time::timeout(Duration::from_secs(3), source)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn ordered_source_stops_when_consumer_closes_with_undrained_batches() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let (sent_tx, sent_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let mut socket = accept_subscription(&listener).await;
            socket
                .send(Message::Text(
                    r#"{"channel":"subscriptionResponse","data":{}}"#.into(),
                ))
                .await
                .unwrap();
            socket
                .send(Message::Text(r#"{"channel":"trades","data":[]}"#.into()))
                .await
                .unwrap();
            sent_tx.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let source = tokio::spawn(async move {
            run_trade_events_with_reconnect(
                &url,
                "BTC",
                &tx,
                TradeMapper::new("BTC"),
                Backoff::new(Duration::from_millis(1), Duration::from_millis(1), 7),
            )
            .await;
        });
        tokio::time::timeout(Duration::from_secs(3), sent_rx)
            .await
            .unwrap()
            .unwrap();
        // The receiver deliberately does not drain either frame. Closure
        // stops the real source regardless of its current scheduling point.
        drop(rx);
        tokio::time::timeout(Duration::from_secs(3), source)
            .await
            .unwrap()
            .unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn ordered_batch_send_pending_on_full_channel_observes_consumer_close() {
        use std::future::Future as _;
        use std::task::Poll;

        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.send(TradeStreamEvent::Connected).await.unwrap();
        let output = SessionOutput::Ordered(&tx);
        let send = output.publish_batch(MappedBatch::default());
        tokio::pin!(send);
        // Poll the real send to Pending before closing the receiver. Unlike
        // a socket-write barrier, this proves the backpressure branch ran.
        std::future::poll_fn(|cx| {
            assert!(matches!(send.as_mut().poll(cx), Poll::Pending));
            Poll::Ready(())
        })
        .await;
        drop(rx);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), send)
                .await
                .unwrap(),
            Err(TradeSessionError::ConsumerClosed)
        );
    }

    #[tokio::test]
    async fn unacknowledged_failure_has_no_edge_and_all_rejected_batch_is_retained() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut socket = accept_subscription(&listener).await;
            socket.send(Message::Text("not-json".into())).await.unwrap();
            drop(socket);
            let mut socket = accept_subscription(&listener).await;
            socket
                .send(Message::Text(
                    r#"{"channel":"subscriptionResponse","data":{}}"#.into(),
                ))
                .await
                .unwrap();
            socket.send(Message::Text(r#"{"channel":"trades","data":[{"coin":"BTC","side":"X","px":"1","sz":"1","time":100,"tid":1}]}"#.into())).await.unwrap();
            socket.close(None).await.unwrap();
        });
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let source = tokio::spawn(async move {
            run_trade_events_with_reconnect(
                &url,
                "BTC",
                &tx,
                TradeMapper::new("BTC"),
                Backoff::new(Duration::from_millis(1), Duration::from_millis(1), 7),
            )
            .await;
        });
        let events = tokio::time::timeout(Duration::from_secs(3), async {
            [
                rx.recv().await.unwrap(),
                rx.recv().await.unwrap(),
                rx.recv().await.unwrap(),
            ]
        })
        .await
        .unwrap();
        assert!(matches!(events[0], TradeStreamEvent::Connected));
        assert!(
            matches!(&events[1], TradeStreamEvent::Batch(batch) if batch.trades.is_empty() && batch.errors.len() == 1 && batch.stale == 0 && batch.duplicates == 0)
        );
        assert!(matches!(events[2], TradeStreamEvent::Disconnected));
        assert!(rx.try_recv().is_err());
        drop(rx);
        tokio::time::timeout(Duration::from_secs(3), source)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
    }
    use tokio::sync::{Notify, watch};

    async fn wait_for_status(status: &mut watch::Receiver<bool>, expected: bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                status.changed().await.expect("connection status closed");
                if *status.borrow_and_update() == expected {
                    return;
                }
            }
        })
        .await
        .expect("timed out waiting for connection status");
    }

    async fn accept_subscription(
        listener: &TcpListener,
    ) -> tokio_tungstenite::WebSocketStream<tokio::net::TcpStream> {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
        let subscription = socket.next().await.unwrap().unwrap();
        assert!(
            matches!(subscription, Message::Text(text) if text.contains("\"type\":\"trades\""))
        );
        socket
    }

    #[tokio::test]
    async fn connection_status_tracks_disconnect_and_resubscribe() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let close_first = Arc::new(Notify::new());
        let allow_second_ack = Arc::new(Notify::new());
        let close_second = Arc::new(Notify::new());
        let server = {
            let close_first = Arc::clone(&close_first);
            let allow_second_ack = Arc::clone(&allow_second_ack);
            let close_second = Arc::clone(&close_second);
            tokio::spawn(async move {
                let mut socket = accept_subscription(&listener).await;
                socket
                    .send(Message::Text(
                        r#"{"channel":"subscriptionResponse","data":{}}"#.into(),
                    ))
                    .await
                    .unwrap();
                close_first.notified().await;
                socket.close(None).await.unwrap();

                let mut socket = accept_subscription(&listener).await;
                allow_second_ack.notified().await;
                socket
                    .send(Message::Text(
                        r#"{"channel":"subscriptionResponse","data":{}}"#.into(),
                    ))
                    .await
                    .unwrap();
                close_second.notified().await;
                let _ = socket.close(None).await;
            })
        };

        let (trades_tx, trades_rx) = tokio::sync::mpsc::channel(8);
        let (connected_tx, mut connected_rx) = watch::channel(false);
        let feed = tokio::spawn(async move {
            run_trades_with_reconnect(
                &format!("ws://{address}"),
                "BTC",
                &trades_tx,
                &connected_tx,
                TradeMapper::new("BTC"),
                Backoff::new(Duration::from_millis(2), Duration::from_millis(2), 7),
            )
            .await;
        });

        wait_for_status(&mut connected_rx, true).await;
        close_first.notify_one();
        wait_for_status(&mut connected_rx, false).await;
        allow_second_ack.notify_one();
        wait_for_status(&mut connected_rx, true).await;

        drop(trades_rx);
        close_second.notify_one();
        tokio::time::timeout(Duration::from_secs(2), feed)
            .await
            .expect("feed did not stop")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server did not stop")
            .unwrap();
    }

    #[tokio::test]
    async fn ordered_events_retain_quiet_cycles_empty_overlap_and_rejected_batches() {
        const ACK: &str = r#"{"channel":"subscriptionResponse","data":{}}"#;
        const EMPTY: &str = r#"{"channel":"trades","data":[]}"#;
        const MIXED: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7},
            {"coin":"BTC","side":"X","px":"1","sz":"1","time":201,"tid":8}
        ]}"#;
        const OVERLAP: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"B","px":"1","sz":"1","time":200,"tid":7}
        ]}"#;
        const STALE: &str = r#"{"channel":"trades","data":[
            {"coin":"BTC","side":"A","px":"1","sz":"1","time":199,"tid":9}
        ]}"#;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            // Cycle one is quiet: an acknowledged connection closes without
            // any trade batch. Its transition must not be inferred from tape.
            let mut socket = accept_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            // A repeated acknowledgement is not a second connection edge.
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.close(None).await.unwrap();

            // Cycle two proves that an empty recovery unit and a partially
            // malformed unit are both first-class ordered source facts.
            let mut socket = accept_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.send(Message::Text(EMPTY.into())).await.unwrap();
            socket.send(Message::Text(MIXED.into())).await.unwrap();
            socket.close(None).await.unwrap();

            // Cycle three is rapid and contains no usable trade: one replayed
            // overlap plus one previously unseen stale row.
            let mut socket = accept_subscription(&listener).await;
            socket.send(Message::Text(ACK.into())).await.unwrap();
            socket.send(Message::Text(OVERLAP.into())).await.unwrap();
            socket.send(Message::Text(STALE.into())).await.unwrap();
            socket.close(None).await.unwrap();
        });

        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(2);
        let feed = tokio::spawn(async move {
            run_trade_events_with_reconnect(
                &format!("ws://{address}"),
                "BTC",
                &events_tx,
                TradeMapper::new("BTC"),
                Backoff::new(Duration::from_millis(1), Duration::from_millis(1), 7),
            )
            .await;
        });

        let events = tokio::time::timeout(Duration::from_secs(5), async {
            let mut events = Vec::new();
            for _ in 0..10 {
                events.push(events_rx.recv().await.unwrap());
            }
            events
        })
        .await
        .unwrap();

        assert!(matches!(events[0], TradeStreamEvent::Connected));
        assert!(matches!(events[1], TradeStreamEvent::Disconnected));
        assert!(matches!(events[2], TradeStreamEvent::Connected));
        assert!(
            matches!(&events[3], TradeStreamEvent::Batch(batch) if batch == &MappedBatch::default())
        );
        assert!(matches!(
            &events[4],
            TradeStreamEvent::Batch(batch)
                if batch.trades.len() == 1
                    && batch.trades[0].timestamp_ms == 200
                    && batch.errors.len() == 1
                    && batch.stale == 0
                    && batch.duplicates == 0
        ));
        assert!(matches!(events[5], TradeStreamEvent::Disconnected));
        assert!(matches!(events[6], TradeStreamEvent::Connected));
        assert!(matches!(
            &events[7],
            TradeStreamEvent::Batch(batch)
                if batch.trades.is_empty()
                    && batch.errors.is_empty()
                    && batch.stale == 0
                    && batch.duplicates == 1
        ));
        assert!(matches!(
            &events[8],
            TradeStreamEvent::Batch(batch)
                if batch.trades.is_empty()
                    && batch.errors.is_empty()
                    && batch.stale == 1
                    && batch.duplicates == 0
        ));
        assert!(matches!(events[9], TradeStreamEvent::Disconnected));

        drop(events_rx);
        tokio::time::timeout(Duration::from_secs(2), feed)
            .await
            .expect("feed did not stop")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server did not stop")
            .unwrap();
    }
}
