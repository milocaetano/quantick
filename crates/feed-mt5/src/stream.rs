//! The bridge server: a local TCP listener the QuantickBridge EA connects to.
//!
//! MQL5 sockets are client-only, so the roles are inverted versus a normal
//! exchange feed: *we* listen, the terminal dials out. One bridge connection
//! is served at a time; when it drops, the server goes back to waiting — the
//! UI hears about every transition through [`Mt5Event::Status`], so "nothing
//! is charting" always has a visible, logged reason.
//!
//! Every noteworthy transition emits a structured `tracing` event with an
//! `event_code` (see the diagnosis table in the crate docs, `lib.rs`): an AI
//! or operator can reconstruct a session from logs alone.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use quantick_engine::Trade;

use crate::map::{MapOutcome, SideMode, TickMapper};
use crate::protocol::{self, BridgeMsg, SCHEMA_VERSION};
use crate::session::SeqTracker;

/// Default address the feed listens on for the bridge.
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:9100";

/// How the bridge server behaves for one symbol.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on (the EA dials this).
    pub listen_addr: String,
    /// The symbol this feed expects (hello mismatches are refused).
    pub symbol: String,
    /// Aggressor-side policy for the mapper.
    pub side_mode: SideMode,
    /// How long a fresh connection may take to say hello.
    pub hello_timeout: Duration,
    /// Max silence (no ticks, no heartbeats) before the bridge is presumed
    /// dead. The bridge heartbeats every ~5 s; 30 s means six missed beats.
    pub read_timeout: Duration,
}

impl ServerConfig {
    /// Sensible defaults for `symbol` on [`DEFAULT_LISTEN_ADDR`].
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            listen_addr: DEFAULT_LISTEN_ADDR.to_string(),
            symbol: symbol.into(),
            side_mode: SideMode::TickRule,
            hello_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
        }
    }
}

/// Where the feed currently stands, for honest labelling in UI and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Status {
    /// Listening; no bridge connected. The chart should say so, not pretend.
    Waiting {
        /// The actual bound address (resolves `:0` in tests).
        addr: String,
    },
    /// A bridge said hello and is streaming (or about to).
    Connected {
        /// Symbol as configured.
        symbol: String,
        /// The front-month contract the terminal actually streams.
        broker_symbol: String,
    },
    /// The bridge went away; the server is looping back to waiting.
    Lost {
        /// Why, e.g. `"bye: deinit"`, `"silent"`, `"eof"`.
        reason: String,
    },
}

/// One message from the bridge server to its consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Event {
    /// A connection-state transition.
    Status(Mt5Status),
    /// One complete historical block (may be empty), already mapped. Sent
    /// exactly once per `backfill_start`/`backfill_end` pair.
    Backfilled(Vec<Trade>),
    /// One live trade.
    Live(Trade),
}

/// A fatal server error (the non-fatal ones are events/logs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mt5Error {
    /// Could not bind the listen address (typically: port already in use by
    /// another quantick instance).
    Bind {
        /// The address we tried.
        addr: String,
        /// The OS error text.
        message: String,
    },
}

impl std::fmt::Display for Mt5Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mt5Error::Bind { addr, message } => {
                write!(f, "cannot listen on {addr} for the MT5 bridge: {message}")
            }
        }
    }
}

impl std::error::Error for Mt5Error {}

/// Why one bridge connection ended.
enum ConnEnd {
    /// The consumer dropped the event channel: shut the server down.
    UiGone,
    /// The bridge went away (reason for the status event); keep listening.
    BridgeGone(String),
}

/// Listen for the bridge and stream events until the consumer goes away.
///
/// Runs forever (accept → serve → back to waiting), returning `Ok(())` only
/// when the event receiver is dropped.
///
/// # Errors
///
/// Returns [`Mt5Error::Bind`] if the listen address cannot be bound.
pub async fn run_bridge_server(
    config: ServerConfig,
    tx: mpsc::Sender<Mt5Event>,
) -> Result<(), Mt5Error> {
    let listener = TcpListener::bind(&config.listen_addr)
        .await
        .map_err(|e| Mt5Error::Bind {
            addr: config.listen_addr.clone(),
            message: e.to_string(),
        })?;
    let bound = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| config.listen_addr.clone());
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_LISTENING",
        addr = %bound,
        symbol = %config.symbol,
        "listening for the MT5 bridge"
    );

    loop {
        if tx
            .send(Mt5Event::Status(Mt5Status::Waiting {
                addr: bound.clone(),
            }))
            .await
            .is_err()
        {
            return Ok(()); // consumer gone before anyone connected
        }

        let stream = match listener.accept().await {
            Ok((stream, peer)) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_CONNECTED",
                    peer = %peer,
                    "bridge socket connected; waiting for hello"
                );
                stream
            }
            Err(e) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_ACCEPT_FAILED",
                    error = %e,
                    "accept failed; continuing to listen"
                );
                continue;
            }
        };

        match serve_connection(stream, &config, &tx).await {
            ConnEnd::UiGone => return Ok(()),
            ConnEnd::BridgeGone(reason) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_LOST",
                    reason = %reason,
                    "bridge session over; back to waiting"
                );
                if tx
                    .send(Mt5Event::Status(Mt5Status::Lost { reason }))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
            }
        }
    }
}

/// Serve one bridge connection to completion.
async fn serve_connection(
    stream: TcpStream,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
) -> ConnEnd {
    let mut lines = BufReader::new(stream).lines();

    // 1. The first message must be a hello that matches what we expect.
    let hello = match tokio::time::timeout(config.hello_timeout, lines.next_line()).await {
        Err(_) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HELLO_TIMEOUT",
                timeout_s = config.hello_timeout.as_secs(),
                "connection said nothing; dropping it"
            );
            return ConnEnd::BridgeGone("hello timeout".to_string());
        }
        Ok(Err(e)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_SOCKET_ERROR",
                error = %e,
                "socket error before hello; dropping the connection"
            );
            return ConnEnd::BridgeGone(format!("socket error before hello: {e}"));
        }
        Ok(Ok(None)) => {
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_EOF",
                "connection closed before hello"
            );
            return ConnEnd::BridgeGone("closed before hello".to_string());
        }
        Ok(Ok(Some(line))) => match protocol::parse_line(&line) {
            Ok(BridgeMsg::Hello(h)) => h,
            Ok(other) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    got = ?other,
                    "first message was not a hello; dropping the connection"
                );
                return ConnEnd::BridgeGone("no hello".to_string());
            }
            Err(e) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = %e,
                    snippet = %snippet(&line),
                    "first line was not valid protocol; dropping the connection"
                );
                return ConnEnd::BridgeGone("undecodable hello".to_string());
            }
        },
    };

    if hello.schema != SCHEMA_VERSION {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SCHEMA_MISMATCH",
            bridge_schema = hello.schema,
            our_schema = SCHEMA_VERSION,
            bridge = %hello.bridge,
            bridge_version = %hello.bridge_version,
            "bridge speaks a different protocol version; refusing"
        );
        return ConnEnd::BridgeGone(format!("schema mismatch (bridge {})", hello.schema));
    }
    if hello.symbol != config.symbol {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_SYMBOL_MISMATCH",
            expected = %config.symbol,
            got = %hello.symbol,
            "bridge streams a different symbol than configured; refusing"
        );
        return ConnEnd::BridgeGone(format!("symbol mismatch ({})", hello.symbol));
    }

    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_HELLO_OK",
        bridge = %hello.bridge,
        bridge_version = %hello.bridge_version,
        symbol = %hello.symbol,
        broker_symbol = %hello.broker_symbol,
        digits = hello.digits,
        server_utc_offset_s = hello.server_utc_offset_s,
        "bridge session established"
    );
    if tx
        .send(Mt5Event::Status(Mt5Status::Connected {
            symbol: hello.symbol.clone(),
            broker_symbol: hello.broker_symbol.clone(),
        }))
        .await
        .is_err()
    {
        return ConnEnd::UiGone;
    }

    // 2. Stream messages until something ends the session.
    let mut mapper = TickMapper::new(config.side_mode, hello.server_utc_offset_s);
    let mut tracker = SeqTracker::new();
    let mut backfill: Option<Vec<Trade>> = None;
    let mut undecodable: u64 = 0;

    let end = loop {
        let line = match tokio::time::timeout(config.read_timeout, lines.next_line()).await {
            Err(_) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_SILENT",
                    timeout_s = config.read_timeout.as_secs(),
                    "no ticks or heartbeats within the timeout; presuming the bridge dead"
                );
                break ConnEnd::BridgeGone("silent".to_string());
            }
            Ok(Err(e)) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_SOCKET_ERROR",
                    error = %e,
                    "socket error; dropping the session"
                );
                break ConnEnd::BridgeGone(format!("socket error: {e}"));
            }
            Ok(Ok(None)) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EOF",
                    "bridge closed the socket"
                );
                break ConnEnd::BridgeGone("eof".to_string());
            }
            Ok(Ok(Some(line))) => line,
        };
        if line.trim().is_empty() {
            continue;
        }

        match protocol::parse_line(&line) {
            Err(e) => {
                undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = %e,
                    snippet = %snippet(&line),
                    total_undecodable = undecodable,
                    "skipping an undecodable line"
                );
            }
            Ok(BridgeMsg::Tick(tick)) => {
                let _ = tracker.observe(tick.seq);
                if let MapOutcome::Trade { trade, .. } = mapper.map(&tick) {
                    match backfill.as_mut() {
                        Some(buf) => buf.push(trade),
                        None => {
                            if tx.send(Mt5Event::Live(trade)).await.is_err() {
                                break ConnEnd::UiGone;
                            }
                        }
                    }
                }
            }
            Ok(BridgeMsg::Heartbeat(hb)) => {
                if let Some(offset) = hb.server_utc_offset_s {
                    mapper.set_server_utc_offset_s(offset);
                }
                debug!(
                    target: "quantick::feed",
                    event_code = "MT5_HEARTBEAT",
                    seq_last = hb.seq_last,
                    ticks_sent = hb.ticks_sent,
                    "bridge heartbeat"
                );
            }
            Ok(BridgeMsg::BackfillStart { count_hint }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_START",
                    count_hint = ?count_hint,
                    "bridge is sending history"
                );
                backfill = Some(Vec::new());
            }
            Ok(BridgeMsg::BackfillEnd {}) => {
                let batch = backfill.take().unwrap_or_default();
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_END",
                    trades = batch.len(),
                    "history block complete"
                );
                if tx.send(Mt5Event::Backfilled(batch)).await.is_err() {
                    break ConnEnd::UiGone;
                }
            }
            Ok(BridgeMsg::Bye { reason }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_BYE",
                    reason = %reason,
                    "bridge said goodbye"
                );
                break ConnEnd::BridgeGone(format!("bye: {reason}"));
            }
            Ok(BridgeMsg::Hello(_)) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    "second hello mid-session; ignoring it"
                );
            }
        }
    };

    if backfill.is_some() {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_PARTIAL_BACKFILL_DISCARDED",
            "session ended mid-backfill; discarding the incomplete block"
        );
    }
    mapper.stats.log_summary(&config.symbol);
    end
}

/// First 120 chars of a line, for log context without flooding.
fn snippet(line: &str) -> &str {
    &line[..line.len().min(120)]
}
