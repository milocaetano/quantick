//! One synchronized Binance diff-depth WebSocket session.
//!
//! Correct bootstrap order matters:
//!
//! 1. open the WebSocket;
//! 2. buffer diff events while fetching a REST snapshot;
//! 3. discard events already represented by the snapshot;
//! 4. require the first applied event to cover `last_update_id + 1`;
//! 5. publish the snapshot and absolute deltas only after that bridge.
//!
//! A sequence gap after synchronization ends this session. The reconnect layer
//! opens a fresh socket and starts a new generation, so consumers can render a
//! visible data-quality gap rather than carrying stale liquidity forward.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt as _, StreamExt as _};
use quantick_orderbook::{BookDelta, BookSnapshot};
use tokio::net::TcpStream;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::CloseFrame},
};
use tracing::{info, warn};

use super::snapshot::{DepthSnapshotError, DepthSnapshotSource, MAX_DEPTH_LIMIT};
use super::sync::{DepthApplyError, DepthSynchronizer, SyncApplyOutcome};
use super::wire::{DepthSnapshot, DepthUpdate, DepthWireError, parse_update};

/// Maximum diff events retained while a REST snapshot is in flight.
pub const DEFAULT_DEPTH_BUFFER_CAPACITY: usize = 8_192;

/// Runtime parameters for depth sessions and reconnects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DepthSessionConfig {
    /// Requested REST levels per side, clamped to `1..=5000`.
    pub snapshot_limit: u16,
    /// Maximum diff events buffered during bootstrap.
    pub buffer_capacity: usize,
    /// Maximum snapshot refetches before reconnecting the WebSocket.
    pub max_snapshot_attempts: u32,
    /// First generation emitted by [`run_depth_with_reconnect`](super::reconnect::run_depth_with_reconnect).
    ///
    /// A controller should pass a monotonically increasing epoch when capture
    /// is disabled and later re-enabled, preventing stale events from an older
    /// task from sharing a generation with the new capture.
    pub initial_generation: u64,
}

impl Default for DepthSessionConfig {
    fn default() -> Self {
        Self {
            snapshot_limit: MAX_DEPTH_LIMIT,
            buffer_capacity: DEFAULT_DEPTH_BUFFER_CAPACITY,
            max_snapshot_attempts: 5,
            initial_generation: 1,
        }
    }
}

impl DepthSessionConfig {
    fn normalized(self) -> Self {
        Self {
            snapshot_limit: self.snapshot_limit.clamp(1, MAX_DEPTH_LIMIT),
            buffer_capacity: self.buffer_capacity.max(1),
            max_snapshot_attempts: self.max_snapshot_attempts.max(1),
            initial_generation: self.initial_generation,
        }
    }
}

/// Why a synchronized session must be discarded and rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthResyncReason {
    /// The REST snapshot was older than the earliest eligible buffered update.
    SnapshotTooOld {
        /// Snapshot update id.
        snapshot_update_id: u64,
        /// First id in the event that could not bridge it.
        first_update_id: u64,
        /// Final id in that event.
        final_update_id: u64,
    },
    /// At least one WebSocket update id was missed.
    SequenceGap {
        /// Next required update id.
        expected_update_id: u64,
        /// First received id.
        first_update_id: u64,
        /// Final received id.
        final_update_id: u64,
    },
    /// Snapshot bootstrap retried too many times on one connection.
    SnapshotAttemptsExhausted {
        /// Number of attempted snapshots.
        attempts: u32,
    },
}

/// Consumer-visible depth lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthStatus {
    /// WebSocket connection is being established.
    Connecting,
    /// WebSocket is open and updates are being buffered.
    Buffering {
        /// Configured hard buffer limit.
        capacity: usize,
    },
    /// A REST snapshot is being fetched while the socket remains live.
    SnapshotFetching {
        /// Attempt number within this socket generation.
        attempt: u32,
        /// Updates already buffered.
        buffered_updates: usize,
    },
    /// Snapshot and stream are continuous and consumer data is authoritative
    /// within the declared snapshot coverage.
    Synchronized {
        /// Current final update id.
        last_update_id: u64,
        /// Current stored bid level count.
        bid_levels: usize,
        /// Current stored ask level count.
        ask_levels: usize,
    },
    /// Current state is being discarded.
    Resyncing {
        /// Typed reason for the resync.
        reason: DepthResyncReason,
    },
    /// A connection generation ended.
    Disconnected {
        /// Stable machine-readable error class.
        error_class: &'static str,
    },
    /// Consumer cancellation stopped the reconnect loop.
    Stopped,
}

/// Typed output consumed by the chart or another order-book client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthEvent {
    /// Lifecycle/status transition.
    Status {
        /// Upper-case Binance symbol.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// New status.
        status: DepthStatus,
    },
    /// Initial exchange-neutral snapshot, emitted exactly once per successful
    /// generation before its first delta.
    Snapshot {
        /// Upper-case Binance symbol.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// Local epoch milliseconds when the REST response was observed.
        ///
        /// Binance's snapshot response has no exchange timestamp, so this is
        /// intentionally labelled as local observation time.
        observed_at_ms: i64,
        /// Logical start time for the snapshot in the consumer timeline.
        ///
        /// A diff can be buffered before the REST response arrives, so its
        /// exchange event time may precede `observed_at_ms`. This value is at
        /// most one millisecond before the bridge event and never after the
        /// local observation, preventing a false backwards-time delta while
        /// keeping the observation timestamp available for diagnostics.
        effective_at_ms: i64,
        /// Validated exchange-neutral snapshot.
        snapshot: BookSnapshot,
    },
    /// One applied absolute update.
    Update {
        /// Upper-case Binance symbol.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// Binance exchange event time (`E`).
        event_time_ms: i64,
        /// Validated exchange-neutral absolute delta.
        delta: BookDelta,
    },
}

/// Failure ending one WebSocket generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthSessionError {
    /// Consumer receiver was dropped.
    ConsumerClosed,
    /// WebSocket connect/read/write failure.
    Transport(String),
    /// Server closed the WebSocket.
    ServerClosed {
        /// Optional close-code/reason summary.
        reason: Option<String>,
    },
    /// REST snapshot failure.
    Snapshot(DepthSnapshotError),
    /// Malformed diff payload.
    Wire(DepthWireError),
    /// Synchronization or generic order-book failure.
    Apply(DepthApplyError),
    /// Bootstrap event buffer reached its hard limit.
    BufferOverflow {
        /// Configured hard limit.
        capacity: usize,
        /// First update id retained, when known.
        first_update_id: Option<u64>,
        /// Last final update id retained, when known.
        last_update_id: Option<u64>,
    },
    /// Too many stale snapshot attempts on the same WebSocket.
    SnapshotAttemptsExhausted {
        /// Attempts made.
        attempts: u32,
    },
}

impl DepthSessionError {
    /// Stable class suitable for structured logs and consumer status.
    #[must_use]
    pub fn error_class(&self) -> &'static str {
        match self {
            Self::ConsumerClosed => "consumer_closed",
            Self::Transport(_) => "transport",
            Self::ServerClosed { .. } => "server_closed",
            Self::Snapshot(_) => "snapshot",
            Self::Wire(_) => "wire_decode",
            Self::Apply(DepthApplyError::Gap { .. }) => "sequence_gap",
            Self::Apply(_) => "book_apply",
            Self::BufferOverflow { .. } => "buffer_overflow",
            Self::SnapshotAttemptsExhausted { .. } => "snapshot_attempts_exhausted",
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
            Self::Snapshot(error) => error.fmt(f),
            Self::Wire(error) => write!(f, "depth update decode error: {error}"),
            Self::Apply(error) => error.fmt(f),
            Self::BufferOverflow { capacity, .. } => {
                write!(f, "depth bootstrap buffer reached capacity {capacity}")
            }
            Self::SnapshotAttemptsExhausted { attempts } => {
                write!(f, "depth snapshot did not bridge after {attempts} attempts")
            }
        }
    }
}

impl std::error::Error for DepthSessionError {}

impl From<DepthSnapshotError> for DepthSessionError {
    fn from(error: DepthSnapshotError) -> Self {
        Self::Snapshot(error)
    }
}

impl From<DepthWireError> for DepthSessionError {
    fn from(error: DepthWireError) -> Self {
        Self::Wire(error)
    }
}

impl From<DepthApplyError> for DepthSessionError {
    fn from(error: DepthApplyError) -> Self {
        Self::Apply(error)
    }
}

type BinanceSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Build a raw Spot diff-depth URL at Binance's fastest documented cadence.
#[must_use]
pub fn depth_stream_url(base: &str, symbol: &str) -> String {
    format!("{base}/{}@depth@100ms", symbol.to_lowercase())
}

/// Decode one diff-depth text frame without network access.
///
/// # Errors
///
/// Returns the typed wire error for malformed data.
pub fn decode_depth_text(text: &str) -> Result<DepthUpdate, DepthWireError> {
    parse_update(text)
}

/// Connect and run one synchronized depth generation.
///
/// The WebSocket is always connected before `source.fetch_depth` is first
/// called. The function returns on transport close/error, unsafe sequence gap,
/// snapshot failure or consumer cancellation; use
/// [`run_depth_with_reconnect`](super::reconnect::run_depth_with_reconnect) for
/// a persistent feed.
///
/// `synchronized` is set to `true` once the snapshot and stream bridge —
/// the reconnect loop uses it to reset its backoff after a healthy session,
/// so only sustained failure escalates the delay.
///
/// # Errors
///
/// Returns [`DepthSessionError`] and never publishes an unbridged snapshot.
pub async fn run_depth_session<S: DepthSnapshotSource>(
    url: &str,
    symbol: &str,
    source: &S,
    events: &Sender<DepthEvent>,
    config: DepthSessionConfig,
    generation: u64,
    synchronized: &mut bool,
) -> Result<(), DepthSessionError> {
    let config = config.normalized();
    let symbol = symbol.to_uppercase();
    send_status(events, &symbol, generation, DepthStatus::Connecting).await?;
    info!(
        target: "quantick::depth",
        schema_version = 1_u8,
        event_code = "depth_ws_connecting",
        symbol = symbol.as_str(),
        generation,
        url,
        action = "connect",
        "connecting Binance diff-depth websocket"
    );

    let connection = tokio::select! {
        () = events.closed() => return Err(DepthSessionError::ConsumerClosed),
        result = tokio_tungstenite::connect_async(url) => result,
    }
    .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
    let (mut ws, _response) = connection;
    info!(
        target: "quantick::depth",
        schema_version = 1_u8,
        event_code = "depth_ws_connected",
        symbol = symbol.as_str(),
        generation,
        action = "buffer",
        "Binance diff-depth websocket connected"
    );
    send_status(
        events,
        &symbol,
        generation,
        DepthStatus::Buffering {
            capacity: config.buffer_capacity,
        },
    )
    .await?;

    let mut buffered = VecDeque::new();
    let mut synchronizer = DepthSynchronizer::new(&symbol, generation);

    // Bootstrap may refetch a snapshot on the same already-open socket if the
    // first eligible buffered event is ahead of it.
    for attempt in 1..=config.max_snapshot_attempts {
        send_status(
            events,
            &symbol,
            generation,
            DepthStatus::SnapshotFetching {
                attempt,
                buffered_updates: buffered.len(),
            },
        )
        .await?;
        let (snapshot, observed_at_ms) = fetch_snapshot_while_buffering(
            &mut ws,
            source,
            &symbol,
            config.snapshot_limit,
            config.buffer_capacity,
            &mut buffered,
            events,
            generation,
        )
        .await?;
        info!(
            target: "quantick::depth",
            schema_version = 1_u8,
            event_code = "depth_snapshot_received",
            symbol = symbol.as_str(),
            generation,
            attempt,
            snapshot_update_id = snapshot.last_update_id,
            bid_levels = snapshot.bids.len(),
            ask_levels = snapshot.asks.len(),
            buffered_updates = buffered.len(),
            action = "install",
            "Binance depth snapshot received"
        );

        synchronizer.reset();
        synchronizer.install_snapshot(&snapshot, usize::from(config.snapshot_limit))?;
        let consumer_snapshot = snapshot
            .to_book_snapshot(usize::from(config.snapshot_limit))
            .map_err(DepthApplyError::Book)?;

        match bridge_snapshot(
            &mut ws,
            &mut synchronizer,
            &mut buffered,
            events,
            &symbol,
            generation,
            observed_at_ms,
            consumer_snapshot,
        )
        .await?
        {
            BridgeOutcome::Synchronized => {
                *synchronized = true;
                return run_synchronized(&mut ws, &mut synchronizer, events, &symbol, generation)
                    .await;
            }
            BridgeOutcome::SnapshotTooOld {
                snapshot_update_id,
                update,
            } => {
                let reason = DepthResyncReason::SnapshotTooOld {
                    snapshot_update_id,
                    first_update_id: update.first_update_id,
                    final_update_id: update.final_update_id,
                };
                warn!(
                    target: "quantick::depth",
                    schema_version = 1_u8,
                    event_code = "depth_snapshot_too_old",
                    symbol = symbol.as_str(),
                    generation,
                    attempt,
                    snapshot_update_id,
                    first_update_id = update.first_update_id,
                    final_update_id = update.final_update_id,
                    action = "refetch_snapshot",
                    "depth snapshot cannot bridge buffered stream"
                );
                send_status(
                    events,
                    &symbol,
                    generation,
                    DepthStatus::Resyncing { reason },
                )
                .await?;
                buffered.push_front(update);
            }
        }
    }

    let attempts = config.max_snapshot_attempts;
    send_status(
        events,
        &symbol,
        generation,
        DepthStatus::Resyncing {
            reason: DepthResyncReason::SnapshotAttemptsExhausted { attempts },
        },
    )
    .await?;
    Err(DepthSessionError::SnapshotAttemptsExhausted { attempts })
}

enum BridgeOutcome {
    Synchronized,
    SnapshotTooOld {
        snapshot_update_id: u64,
        update: DepthUpdate,
    },
}

#[allow(clippy::too_many_arguments)]
async fn bridge_snapshot(
    ws: &mut BinanceSocket,
    synchronizer: &mut DepthSynchronizer,
    buffered: &mut VecDeque<DepthUpdate>,
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
    observed_at_ms: i64,
    consumer_snapshot: BookSnapshot,
) -> Result<BridgeOutcome, DepthSessionError> {
    let snapshot_update_id = consumer_snapshot.last_update_id();
    let mut consumer_snapshot = Some(consumer_snapshot);
    loop {
        let update = match buffered.pop_front() {
            Some(update) => update,
            None => next_update(ws, events).await?,
        };
        match synchronizer.apply(&update) {
            Ok(SyncApplyOutcome::Stale { .. }) => {}
            Ok(SyncApplyOutcome::Applied {
                bridged_snapshot: true,
                final_update_id,
                bid_levels,
                ask_levels,
                ..
            }) => {
                let snapshot = consumer_snapshot
                    .take()
                    .expect("bridge publishes its snapshot exactly once");
                send_event(
                    events,
                    DepthEvent::Snapshot {
                        symbol: symbol.to_string(),
                        generation,
                        observed_at_ms,
                        effective_at_ms: snapshot_effective_at_ms(
                            observed_at_ms,
                            update.event_time_ms,
                        ),
                        snapshot,
                    },
                )
                .await?;
                publish_update(events, symbol, generation, &update).await?;
                send_status(
                    events,
                    symbol,
                    generation,
                    DepthStatus::Synchronized {
                        last_update_id: final_update_id,
                        bid_levels,
                        ask_levels,
                    },
                )
                .await?;
                info!(
                    target: "quantick::depth",
                    schema_version = 1_u8,
                    event_code = "depth_sync_ready",
                    symbol,
                    generation,
                    snapshot_update_id,
                    last_update_id = final_update_id,
                    bid_levels,
                    ask_levels,
                    buffered_remaining = buffered.len(),
                    action = "stream",
                    "depth snapshot and websocket are synchronized"
                );

                // Apply and publish events that arrived while the snapshot was
                // fetched before switching to direct socket consumption.
                while let Some(pending) = buffered.pop_front() {
                    apply_live_update(synchronizer, events, symbol, generation, &pending).await?;
                }
                return Ok(BridgeOutcome::Synchronized);
            }
            Ok(SyncApplyOutcome::Applied {
                bridged_snapshot: false,
                ..
            }) => {
                unreachable!("awaiting-bridge phase must mark its first apply as a bridge")
            }
            Err(DepthApplyError::Gap { .. }) => {
                return Ok(BridgeOutcome::SnapshotTooOld {
                    snapshot_update_id,
                    update,
                });
            }
            Err(error) => return Err(error.into()),
        }
    }
}

async fn run_synchronized(
    ws: &mut BinanceSocket,
    synchronizer: &mut DepthSynchronizer,
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
) -> Result<(), DepthSessionError> {
    loop {
        let update = next_update(ws, events).await?;
        apply_live_update(synchronizer, events, symbol, generation, &update).await?;
    }
}

async fn apply_live_update(
    synchronizer: &mut DepthSynchronizer,
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
    update: &DepthUpdate,
) -> Result<(), DepthSessionError> {
    match synchronizer.apply(update) {
        Ok(SyncApplyOutcome::Stale { .. }) => Ok(()),
        Ok(SyncApplyOutcome::Applied { .. }) => {
            publish_update(events, symbol, generation, update).await
        }
        Err(
            error @ DepthApplyError::Gap {
                expected_update_id,
                got_first_update_id,
                got_final_update_id,
                ..
            },
        ) => {
            let reason = DepthResyncReason::SequenceGap {
                expected_update_id,
                first_update_id: got_first_update_id,
                final_update_id: got_final_update_id,
            };
            send_status(
                events,
                symbol,
                generation,
                DepthStatus::Resyncing { reason },
            )
            .await?;
            Err(error.into())
        }
        Err(error) => Err(error.into()),
    }
}

async fn publish_update(
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
    update: &DepthUpdate,
) -> Result<(), DepthSessionError> {
    let delta = update.to_book_delta().map_err(DepthApplyError::Book)?;
    send_event(
        events,
        DepthEvent::Update {
            symbol: symbol.to_string(),
            generation,
            event_time_ms: update.event_time_ms,
            delta,
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn fetch_snapshot_while_buffering<S: DepthSnapshotSource>(
    ws: &mut BinanceSocket,
    source: &S,
    symbol: &str,
    limit: u16,
    buffer_capacity: usize,
    buffered: &mut VecDeque<DepthUpdate>,
    events: &Sender<DepthEvent>,
    generation: u64,
) -> Result<(DepthSnapshot, i64), DepthSessionError> {
    let fetch = source.fetch_depth(symbol, limit);
    tokio::pin!(fetch);
    loop {
        tokio::select! {
            () = events.closed() => return Err(DepthSessionError::ConsumerClosed),
            result = &mut fetch => {
                let snapshot = result?;
                return Ok((snapshot, wall_clock_ms()));
            }
            frame = ws.next() => {
                if let Some(update) = decode_socket_frame(ws, frame).await? {
                    if buffered.len() >= buffer_capacity {
                        let first_update_id = buffered.front().map(|event| event.first_update_id);
                        let last_update_id = buffered.back().map(|event| event.final_update_id);
                        warn!(
                            target: "quantick::depth",
                            schema_version = 1_u8,
                            event_code = "depth_buffer_overflow",
                            symbol,
                            generation,
                            capacity = buffer_capacity,
                            ?first_update_id,
                            ?last_update_id,
                            action = "reconnect",
                            "depth bootstrap buffer is full"
                        );
                        return Err(DepthSessionError::BufferOverflow {
                            capacity: buffer_capacity,
                            first_update_id,
                            last_update_id,
                        });
                    }
                    buffered.push_back(update);
                }
            }
        }
    }
}

async fn next_update(
    ws: &mut BinanceSocket,
    events: &Sender<DepthEvent>,
) -> Result<DepthUpdate, DepthSessionError> {
    loop {
        let frame = tokio::select! {
            () = events.closed() => return Err(DepthSessionError::ConsumerClosed),
            frame = ws.next() => frame,
        };
        if let Some(update) = decode_socket_frame(ws, frame).await? {
            return Ok(update);
        }
    }
}

async fn decode_socket_frame(
    ws: &mut BinanceSocket,
    frame: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
) -> Result<Option<DepthUpdate>, DepthSessionError> {
    let Some(frame) = frame else {
        return Err(DepthSessionError::ServerClosed { reason: None });
    };
    let message = frame.map_err(|error| DepthSessionError::Transport(error.to_string()))?;
    match message {
        Message::Text(text) => Ok(Some(decode_depth_text(text.as_str())?)),
        Message::Ping(payload) => {
            ws.send(Message::Pong(payload))
                .await
                .map_err(|error| DepthSessionError::Transport(error.to_string()))?;
            Ok(None)
        }
        Message::Close(frame) => Err(DepthSessionError::ServerClosed {
            reason: frame.as_ref().map(close_reason),
        }),
        Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => Ok(None),
    }
}

fn close_reason(frame: &CloseFrame<'_>) -> String {
    format!("code={} reason={}", frame.code, frame.reason)
}

async fn send_status(
    events: &Sender<DepthEvent>,
    symbol: &str,
    generation: u64,
    status: DepthStatus,
) -> Result<(), DepthSessionError> {
    send_event(
        events,
        DepthEvent::Status {
            symbol: symbol.to_string(),
            generation,
            status,
        },
    )
    .await
}

async fn send_event(
    events: &Sender<DepthEvent>,
    event: DepthEvent,
) -> Result<(), DepthSessionError> {
    events
        .send(event)
        .await
        .map_err(|_| DepthSessionError::ConsumerClosed)
}

fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn snapshot_effective_at_ms(observed_at_ms: i64, bridge_event_time_ms: i64) -> i64 {
    observed_at_ms.min(bridge_event_time_ms.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use rust_decimal::Decimal;
    use tokio::net::TcpListener;

    use super::*;
    use crate::depth::DepthLevel;

    #[derive(Clone)]
    struct FakeSnapshotSource {
        tcp_accepted: Arc<AtomicBool>,
    }

    impl DepthSnapshotSource for FakeSnapshotSource {
        async fn fetch_depth(
            &self,
            symbol: &str,
            limit: u16,
        ) -> Result<DepthSnapshot, DepthSnapshotError> {
            assert_eq!(symbol, "BTCUSDT");
            assert_eq!(limit, 100);
            assert!(
                self.tcp_accepted.load(Ordering::SeqCst),
                "snapshot fetch began before the websocket transport was accepted"
            );
            // Leave enough time for the server's first diff to reach the
            // client's bootstrap buffer before this snapshot completes.
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            Ok(DepthSnapshot {
                last_update_id: 100,
                bids: vec![DepthLevel {
                    price: Decimal::from(99),
                    quantity: Decimal::from(5),
                }],
                asks: vec![DepthLevel {
                    price: Decimal::from(101),
                    quantity: Decimal::from(7),
                }],
            })
        }
    }

    #[test]
    fn url_lowercases_symbol_and_requests_100ms_depth() {
        assert_eq!(
            depth_stream_url("wss://stream.binance.com:9443/ws", "BTCUSDT"),
            "wss://stream.binance.com:9443/ws/btcusdt@depth@100ms"
        );
    }

    #[test]
    fn config_normalizes_unsafe_zero_values() {
        let config = DepthSessionConfig {
            snapshot_limit: 0,
            buffer_capacity: 0,
            max_snapshot_attempts: 0,
            initial_generation: 42,
        }
        .normalized();
        assert_eq!(config.snapshot_limit, 1);
        assert_eq!(config.buffer_capacity, 1);
        assert_eq!(config.max_snapshot_attempts, 1);
        assert_eq!(config.initial_generation, 42);
    }

    #[test]
    fn decode_is_pure_and_preserves_ids() {
        let update =
            decode_depth_text(r#"{"E":10,"s":"BTCUSDT","U":101,"u":103,"b":[],"a":[]}"#).unwrap();
        assert_eq!(update.first_update_id, 101);
        assert_eq!(update.final_update_id, 103);
    }

    #[test]
    fn snapshot_effective_time_precedes_an_earlier_buffered_bridge() {
        assert_eq!(snapshot_effective_at_ms(1_000, 900), 899);
    }

    #[test]
    fn snapshot_effective_time_keeps_an_earlier_observation() {
        assert_eq!(snapshot_effective_at_ms(800, 900), 800);
    }

    #[tokio::test]
    async fn offline_session_connects_then_buffers_and_bridges_before_publish() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let tcp_accepted = Arc::new(AtomicBool::new(false));
        let accepted_for_server = tcp_accepted.clone();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            accepted_for_server.store(true, Ordering::SeqCst);
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            ws.send(Message::Text(
                r#"{"E":900,"s":"BTCUSDT","U":101,"u":101,"b":[["99","4"]],"a":[]}"#.into(),
            ))
            .await
            .unwrap();
            // The client closes when the test drops its event receiver.
            while ws.next().await.is_some() {}
        });

        let source = FakeSnapshotSource { tcp_accepted };
        let (events_tx, mut events_rx) = tokio::sync::mpsc::channel(16);
        let config = DepthSessionConfig {
            snapshot_limit: 100,
            buffer_capacity: 8,
            max_snapshot_attempts: 2,
            initial_generation: 40,
        };
        let url = format!("ws://{address}");
        let session = tokio::spawn(async move {
            let mut synchronized = false;
            let result = run_depth_session(
                &url,
                "BTCUSDT",
                &source,
                &events_tx,
                config,
                40,
                &mut synchronized,
            )
            .await;
            (result, synchronized)
        });

        assert!(matches!(
            events_rx.recv().await,
            Some(DepthEvent::Status {
                generation: 40,
                status: DepthStatus::Connecting,
                ..
            })
        ));
        assert!(matches!(
            events_rx.recv().await,
            Some(DepthEvent::Status {
                status: DepthStatus::Buffering { capacity: 8 },
                ..
            })
        ));
        assert!(matches!(
            events_rx.recv().await,
            Some(DepthEvent::Status {
                status: DepthStatus::SnapshotFetching { attempt: 1, .. },
                ..
            })
        ));

        match events_rx.recv().await {
            Some(DepthEvent::Snapshot {
                generation,
                observed_at_ms,
                effective_at_ms,
                snapshot,
                ..
            }) => {
                assert_eq!(generation, 40);
                assert!(observed_at_ms > 900);
                assert_eq!(effective_at_ms, 899);
                assert_eq!(snapshot.last_update_id(), 100);
            }
            other => panic!("expected synchronized snapshot, got {other:?}"),
        }
        match events_rx.recv().await {
            Some(DepthEvent::Update {
                generation,
                event_time_ms,
                delta,
                ..
            }) => {
                assert_eq!(generation, 40);
                assert_eq!(event_time_ms, 900);
                assert_eq!(delta.first_update_id(), 101);
                assert_eq!(delta.final_update_id(), 101);
            }
            other => panic!("expected bridge delta, got {other:?}"),
        }
        assert!(matches!(
            events_rx.recv().await,
            Some(DepthEvent::Status {
                generation: 40,
                status: DepthStatus::Synchronized {
                    last_update_id: 101,
                    ..
                },
                ..
            })
        ));

        drop(events_rx);
        let (result, synchronized) =
            tokio::time::timeout(std::time::Duration::from_secs(1), session)
                .await
                .expect("session responds to consumer cancellation")
                .unwrap();
        assert_eq!(result, Err(DepthSessionError::ConsumerClosed));
        assert!(synchronized, "the reconnect loop resets backoff on this");
        server.abort();
    }
}
