//! Live aggTrade WebSocket stream.
//!
//! Connects to Binance's `<symbol>@aggTrade` WebSocket, decodes each message to
//! an engine [`Trade`], and forwards trades on a channel the app drains each
//! frame. Reconnect and gap detection are layered on top in #26; this module is
//! the single-connection happy path.
//!
//! The decode step ([`decode_text`]) is pure and unit-tested; the connection
//! runner ([`run_agg_trade_stream`]) is exercised by an `#[ignore]`d live test.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::sync::{mpsc::Sender, watch};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use quantick_engine::Trade;

use crate::backfill::FeedError;
use crate::continuity::ContinuityTracker;
use crate::wire;

/// Binance's public single-stream WebSocket base URL.
pub const BINANCE_WS_BASE: &str = "wss://stream.binance.com:9443/ws";

/// Build the aggTrade stream URL for `symbol` (lower-cased, as Binance expects).
#[must_use]
pub fn agg_trade_url(base: &str, symbol: &str) -> String {
    format!("{base}/{}@aggTrade", symbol.to_lowercase())
}

/// Decode one WebSocket text frame into an engine [`Trade`].
///
/// # Errors
///
/// Returns [`FeedError`] if the frame is not a valid aggTrade or a field cannot
/// be mapped (e.g. a malformed price).
pub fn decode_text(text: &str) -> Result<Trade, FeedError> {
    let raw = wire::parse_ws_message(text).map_err(|e| FeedError::Decode(e.to_string()))?;
    Ok(raw.to_trade()?)
}

/// Connect to `url` and forward decoded trades on `tx` until the socket closes.
///
/// Each trade is passed through `tracker` first, so gaps, reorders and
/// duplicates are detected and logged (the tracker is owned by the reconnect
/// loop, so it spans reconnects). Server pings are answered with pongs (Binance
/// disconnects otherwise). A frame that fails to decode is logged and skipped
/// rather than tearing down the whole stream.
///
/// Returns the number of trades forwarded — on a clean server close, or when the
/// consumer drops `tx`. Returns [`FeedError`] on a transport error.
///
/// No inbound frame of any kind for `idle_timeout` ends the connection: a
/// half-open socket otherwise waits forever. A client ping goes out every
/// half window, so a quiet but healthy socket answers in time — Binance's own
/// pings come every 20 s on Spot but only every 3 minutes on USD-M.
///
/// # Errors
///
/// Returns [`FeedError::Http`] on connection or socket errors and
/// [`FeedError::IdleTimeout`] when the socket falls silent.
pub async fn run_agg_trade_stream(
    url: &str,
    tx: &Sender<Trade>,
    tracker: &mut ContinuityTracker,
    connected: &watch::Sender<bool>,
    idle_timeout: Duration,
) -> Result<u64, FeedError> {
    info!(target: "quantick::feed", url, "connecting aggTrade websocket");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| FeedError::Http(e.to_string()))?;
    let _ = connected.send(true);
    info!(target: "quantick::feed", url, "aggTrade websocket connected");

    let mut forwarded: u64 = 0;
    let mut probe = tokio::time::interval((idle_timeout / 2).max(Duration::from_millis(1)));
    probe.set_missed_tick_behavior(MissedTickBehavior::Delay);
    probe.tick().await;
    let silence = tokio::time::sleep(idle_timeout);
    tokio::pin!(silence);
    loop {
        // Biased toward a waiting frame: after a slow hand-off to the
        // consumer, a buffered frame proves the socket alive.
        let frame = tokio::select! {
            biased;
            frame = ws.next() => frame,
            _ = probe.tick() => {
                ws.send(Message::Ping(Default::default()))
                    .await
                    .map_err(|e| FeedError::Http(e.to_string()))?;
                continue;
            }
            () = &mut silence => {
                warn!(
                    target: "quantick::feed",
                    url,
                    idle_ms = idle_timeout.as_millis() as u64,
                    forwarded,
                    "no aggTrade websocket frame within the idle timeout"
                );
                return Err(FeedError::IdleTimeout(idle_timeout));
            }
        };
        let Some(frame) = frame else { break };
        let msg = frame.map_err(|e| FeedError::Http(e.to_string()))?;
        match msg {
            Message::Text(text) => match decode_text(text.as_str()) {
                Ok(trade) => {
                    // Detect (and log) any sequence anomaly, but still forward
                    // the trade — labelling a hole, not patching it.
                    let _ = tracker.observe(&trade);
                    forwarded += 1;
                    if tx.send(trade).await.is_err() {
                        debug!(target: "quantick::feed", forwarded, "consumer dropped; stopping stream");
                        return Ok(forwarded);
                    }
                }
                Err(error) => {
                    warn!(target: "quantick::feed", %error, "undecodable aggTrade frame; skipping");
                }
            },
            Message::Ping(payload) => {
                ws.send(Message::Pong(payload))
                    .await
                    .map_err(|e| FeedError::Http(e.to_string()))?;
            }
            Message::Close(frame) => {
                info!(target: "quantick::feed", ?frame, forwarded, "server closed the websocket");
                return Ok(forwarded);
            }
            Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
        }
        // Restarted once the frame is handled, so time spent waiting on the
        // consumer never counts as socket silence.
        silence.as_mut().reset(Instant::now() + idle_timeout);
    }
    debug!(target: "quantick::feed", forwarded, "websocket stream ended");
    Ok(forwarded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    #[test]
    fn url_lowercases_the_symbol() {
        assert_eq!(
            agg_trade_url(BINANCE_WS_BASE, "BTCUSDT"),
            "wss://stream.binance.com:9443/ws/btcusdt@aggTrade"
        );
    }

    #[test]
    fn decodes_a_live_ws_frame() {
        let frame = r#"{"e":"aggTrade","E":1700000000450,"s":"BTCUSDT","a":42,
            "p":"36000.30","q":"0.750","f":1,"l":3,"T":1700000000450,"m":true,"M":true}"#;
        let trade = decode_text(frame).unwrap();
        assert_eq!(trade.agg_id, 42);
        assert_eq!(trade.price, Decimal::from_str("36000.30").unwrap());
        assert_eq!(trade.quantity, Decimal::from_str("0.750").unwrap());
        // m=true => aggressor is the seller.
        assert_eq!(trade.side, Side::Sell);
    }

    #[test]
    fn undecodable_frame_is_an_error_not_a_panic() {
        assert!(decode_text("{not valid json").is_err());
    }

    const IDLE: Duration = Duration::from_millis(400);

    /// One connection to a local socket that `serve` drives, at [`IDLE`].
    async fn stream_against<F, Fut>(serve: F) -> Result<u64, FeedError>
    where
        F: FnOnce(tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>) -> Fut
            + Send
            + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = agg_trade_url(
            &format!("ws://{}", listener.local_addr().unwrap()),
            "BTCUSDT",
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve(tokio_tungstenite::accept_async(stream).await.unwrap()).await;
        });
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let (connected, _status) = tokio::sync::watch::channel(false);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            run_agg_trade_stream(&url, &tx, &mut ContinuityTracker::new(), &connected, IDLE),
        )
        .await
        .expect("the stream neither timed out nor ended");
        server.abort();
        result
    }

    /// A socket that completes the handshake and then never reads or writes
    /// again — not even a pong — ends with the idle error, not a hang.
    #[tokio::test]
    async fn a_silent_socket_ends_with_the_idle_error() {
        let result = stream_against(|socket| async move {
            std::future::pending::<()>().await;
            drop(socket);
        })
        .await;
        assert!(
            matches!(result, Err(FeedError::IdleTimeout(after)) if after == IDLE),
            "{result:?}"
        );
    }

    /// A socket that carries no trades but answers the client's pings stays
    /// connected past several idle windows, until the server closes it.
    #[tokio::test]
    async fn pongs_keep_a_quiet_socket_alive() {
        let result = stream_against(|mut socket| async move {
            // Reading is what makes tungstenite answer each ping with a pong.
            let _ = tokio::time::timeout(IDLE * 3, async {
                while let Some(Ok(_)) = socket.next().await {}
            })
            .await;
            socket.close(None).await.unwrap();
        })
        .await;
        assert!(matches!(result, Ok(0)), "{result:?}");
    }

    #[tokio::test]
    #[ignore = "hits the live Binance websocket; run manually with --ignored"]
    async fn live_stream_forwards_a_few_trades() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (connected_tx, _connected_rx) = tokio::sync::watch::channel(false);
        let url = agg_trade_url(BINANCE_WS_BASE, "BTCUSDT");
        let handle = tokio::spawn(async move {
            let mut tracker = ContinuityTracker::new();
            run_agg_trade_stream(
                &url,
                &tx,
                &mut tracker,
                &connected_tx,
                Duration::from_secs(30),
            )
            .await
        });

        let mut seen = 0;
        while seen < 5 {
            match tokio::time::timeout(Duration::from_secs(20), rx.recv()).await {
                Ok(Some(trade)) => {
                    assert!(trade.price > Decimal::ZERO);
                    seen += 1;
                }
                Ok(None) => break,
                Err(_) => panic!("timed out waiting for live trades"),
            }
        }
        handle.abort();
        assert!(seen >= 1, "expected at least one live trade");
    }
}
