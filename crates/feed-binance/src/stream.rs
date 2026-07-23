//! Live aggTrade WebSocket stream.
//!
//! Connects to Binance's `<symbol>@aggTrade` WebSocket, decodes each message to
//! an engine [`Trade`], and forwards trades on a channel the app drains each
//! frame. Reconnect and gap detection are layered on top in #26; this module is
//! the single-connection happy path.
//!
//! The decode step ([`decode_text`]) is pure and unit-tested; the connection
//! runner ([`run_agg_trade_stream`]) is exercised by an `#[ignore]`d live test.

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::sync::mpsc::Sender;
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
/// # Errors
///
/// Returns [`FeedError::Http`] on connection or socket errors.
pub async fn run_agg_trade_stream(
    url: &str,
    tx: &Sender<Trade>,
    tracker: &mut ContinuityTracker,
) -> Result<u64, FeedError> {
    info!(target: "quantick::feed", url, "connecting aggTrade websocket");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|e| FeedError::Http(e.to_string()))?;
    info!(target: "quantick::feed", url, "aggTrade websocket connected");

    let mut forwarded: u64 = 0;
    while let Some(frame) = ws.next().await {
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

    #[tokio::test]
    #[ignore = "hits the live Binance websocket; run manually with --ignored"]
    async fn live_stream_forwards_a_few_trades() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let url = agg_trade_url(BINANCE_WS_BASE, "BTCUSDT");
        let handle = tokio::spawn(async move {
            let mut tracker = ContinuityTracker::new();
            run_agg_trade_stream(&url, &tx, &mut tracker).await
        });

        let mut seen = 0;
        while seen < 5 {
            match tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv()).await {
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
