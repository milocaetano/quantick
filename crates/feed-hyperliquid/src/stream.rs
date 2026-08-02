//! Reconnecting Hyperliquid public-trades WebSocket.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use quantick_engine::Trade;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use crate::reconnect::Backoff;
use crate::wire::{TradeMapper, TradeMessage, parse_trade_message};

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
    mapper: &mut TradeMapper,
) -> Result<(), TradeSessionError> {
    let symbol = symbol.to_uppercase();
    info!(target: "quantick::feed", symbol, url, "connecting Hyperliquid trades websocket");
    let connection = tokio::select! {
        () = trades.closed() => return Err(TradeSessionError::ConsumerClosed),
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
            () = trades.closed() => return Err(TradeSessionError::ConsumerClosed),
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
                                if !batch.trades.is_empty() {
                                    trades
                                        .send(batch.trades)
                                        .await
                                        .map_err(|_| TradeSessionError::ConsumerClosed)?;
                                }
                            }
                            TradeMessage::Subscribed => {
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

/// Keep the trade stream alive across API-server disconnects.
pub async fn run_trades_with_reconnect(
    url: &str,
    symbol: &str,
    trades: &Sender<Vec<Trade>>,
    mut mapper: TradeMapper,
    mut backoff: Backoff,
) {
    let symbol = symbol.to_uppercase();
    loop {
        let published_before = mapper.published_count();
        let result = run_trade_session(url, &symbol, trades, &mut mapper).await;
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
