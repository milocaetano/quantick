//! One Hyperliquid L2 WebSocket generation.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};

use quantick_orderbook::{DepthEvent, DepthStatus};

use super::mapper::{BookMapError, BookMapper, HYPERLIQUID_LEVELS_PER_SIDE};
use super::wire::{BookMessage, parse_book_message};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Why one L2 generation ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthSessionError {
    /// UI receiver was dropped.
    ConsumerClosed,
    /// Connect, read, or write failure.
    Transport(String),
    /// Server ended the connection.
    ServerClosed { reason: Option<String> },
    /// Malformed WebSocket envelope/image.
    Decode(String),
    /// Valid JSON carried unusable book data.
    Map(BookMapError),
}

impl DepthSessionError {
    /// Stable class for lifecycle status and logs.
    #[must_use]
    pub fn error_class(&self) -> &'static str {
        match self {
            Self::ConsumerClosed => "consumer_closed",
            Self::Transport(_) => "transport",
            Self::ServerClosed { .. } => "server_closed",
            Self::Decode(_) => "wire_decode",
            Self::Map(_) => "book_map",
        }
    }
}

impl std::fmt::Display for DepthSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConsumerClosed => f.write_str("depth consumer closed"),
            Self::Transport(error) => write!(f, "depth websocket error: {error}"),
            Self::ServerClosed { reason } => write!(
                f,
                "depth websocket closed{}",
                reason
                    .as_deref()
                    .map_or_else(String::new, |reason| format!(": {reason}"))
            ),
            Self::Decode(error) => write!(f, "depth payload decode error: {error}"),
            Self::Map(error) => write!(f, "depth image mapping error: {error}"),
        }
    }
}

impl std::error::Error for DepthSessionError {}

/// Connect, subscribe, and publish one generation until continuity ends.
///
/// # Errors
///
/// Returns a typed connection, decode, mapping, or consumer failure.
pub async fn run_depth_session(
    url: &str,
    symbol: &str,
    events: &Sender<DepthEvent>,
    mapper: &mut BookMapper,
    generation: u64,
) -> Result<(), DepthSessionError> {
    let symbol = symbol.to_uppercase();
    mapper.restart(generation);
    send_status(events, &symbol, generation, DepthStatus::Connecting).await?;
    info!(
        target: "quantick::depth",
        schema_version = 1_u8,
        event_code = "hyperliquid_depth_connecting",
        symbol,
        generation,
        url,
        action = "connect",
        "connecting Hyperliquid l2Book websocket"
    );
    let connection = tokio::select! {
        () = events.closed() => return Err(DepthSessionError::ConsumerClosed),
        result = tokio_tungstenite::connect_async(url) => result,
    }
    .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
    let (mut socket, _response) = connection;
    let subscription = serde_json::json!({
        "method": "subscribe",
        "subscription": { "type": "l2Book", "coin": symbol }
    });
    socket
        .send(Message::Text(subscription.to_string()))
        .await
        .map_err(|error| DepthSessionError::Transport(error.to_string()))?;

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;

    loop {
        tokio::select! {
            () = events.closed() => return Err(DepthSessionError::ConsumerClosed),
            _ = heartbeat.tick() => {
                socket
                    .send(Message::Text(r#"{"method":"ping"}"#.into()))
                    .await
                    .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
            }
            frame = socket.next() => {
                let Some(frame) = frame else {
                    return Err(DepthSessionError::ServerClosed { reason: None });
                };
                let message = frame
                    .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
                match message {
                    Message::Text(text) => {
                        match parse_book_message(text.as_str())
                            .map_err(|error| DepthSessionError::Decode(error.to_string()))?
                        {
                            BookMessage::Book(image) => {
                                let was_synchronized = mapper.is_synchronized();
                                let crossed_before = mapper.stats.crossed;
                                let mapped = mapper.map(&image).map_err(DepthSessionError::Map)?;
                                if mapper.stats.crossed > crossed_before {
                                    warn!(
                                        target: "quantick::depth",
                                        symbol,
                                        generation,
                                        crossed = mapper.stats.crossed,
                                        action = "keep_last_uncrossed_image",
                                        "Hyperliquid published a crossed L2 image"
                                    );
                                }
                                if let Some(event) = mapped {
                                    events
                                        .send(event)
                                        .await
                                        .map_err(|_| DepthSessionError::ConsumerClosed)?;
                                    if !was_synchronized && mapper.is_synchronized() {
                                        let status = mapper.synchronized_status();
                                        let (bid_levels, ask_levels) = match status {
                                            DepthStatus::Synchronized { bid_levels, ask_levels, .. } => {
                                                (bid_levels, ask_levels)
                                            }
                                            _ => unreachable!(),
                                        };
                                        send_status(
                                            events,
                                            &symbol,
                                            generation,
                                            status,
                                        )
                                        .await?;
                                        info!(
                                            target: "quantick::depth",
                                            schema_version = 1_u8,
                                            event_code = "hyperliquid_depth_synchronized",
                                            symbol,
                                            generation,
                                            bid_levels,
                                            ask_levels,
                                            coverage_levels_per_side = HYPERLIQUID_LEVELS_PER_SIDE,
                                            action = "publish",
                                            "Hyperliquid L2 capture is live"
                                        );
                                    }
                                }
                            }
                            BookMessage::Subscribed => {
                                debug!(target: "quantick::depth", symbol, generation, "l2Book subscription acknowledged");
                            }
                            BookMessage::Pong => {}
                            BookMessage::Other { channel } => {
                                debug!(target: "quantick::depth", symbol, generation, channel, "ignored unexpected channel");
                            }
                        }
                    }
                    Message::Ping(payload) => {
                        socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
                    }
                    Message::Close(frame) => {
                        let reason = frame.map(|frame| format!("{} {}", u16::from(frame.code), frame.reason));
                        return Err(DepthSessionError::ServerClosed { reason });
                    }
                    Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
                }
            }
        }
    }
}

async fn send_status(
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
    status: DepthStatus,
) -> Result<(), DepthSessionError> {
    events
        .send(DepthEvent::Status {
            symbol: symbol.to_owned(),
            generation,
            status,
        })
        .await
        .map_err(|_| DepthSessionError::ConsumerClosed)
}
