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

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use quantick_engine::Trade;
use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};

use crate::depth::BookMapper;
use crate::map::{MapOutcome, SideMode, TickMapper};
use crate::protocol::{self, BridgeMsg, SCHEMA_VERSION};
use crate::session::SeqTracker;

/// Default address the feed listens on for the bridge.
pub const DEFAULT_LISTEN_ADDR: &str = "127.0.0.1:9100";

/// Longest line the server will buffer. Protocol lines are a few hundred
/// bytes; anything larger is not the bridge, and an unbounded buffer would let
/// any local process exhaust memory by streaming bytes without a newline.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Runtime switch controlling whether DOM images are published.
///
/// MT5's book shares one socket with ticks and MQL5 gives us no back-channel
/// to ask the terminal to stop sending it, so "capture off" is a decision made
/// here: images are decoded and dropped rather than published. That costs a
/// JSON parse per image and nothing downstream — no book state, no history, no
/// projection. The alternative (tearing down the bridge session) would take the
/// trade stream down with it.
///
/// Cloning shares the switch; the consumer flips it from any thread.
#[derive(Debug, Clone, Default)]
pub struct BookCaptureSwitch(Arc<BookCaptureState>);

#[derive(Debug, Default)]
struct BookCaptureState {
    enabled: AtomicBool,
    /// Base generation chosen by the consumer. Each bridge session adds its own
    /// offset on top, so a reconnect never reuses a generation.
    base_generation: AtomicU64,
}

impl BookCaptureSwitch {
    /// A switch that starts disabled.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Publish depth from `base_generation` onwards.
    ///
    /// A base above the previous one discards whatever the old generation
    /// published; consumers use that to keep stale in-flight events from an
    /// earlier capture out of fresh history.
    pub fn enable(&self, base_generation: u64) {
        self.0
            .base_generation
            .store(base_generation, Ordering::Relaxed);
        self.0.enabled.store(true, Ordering::Release);
    }

    /// Stop publishing depth. The bridge session and the trade stream continue.
    pub fn disable(&self) {
        self.0.enabled.store(false, Ordering::Release);
    }

    /// Whether depth is currently published.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.0.enabled.load(Ordering::Acquire)
    }

    fn state(&self) -> (bool, u64) {
        // Acquire on `enabled` pairs with the release in `enable`, so a true
        // read never sees a stale base generation.
        let enabled = self.0.enabled.load(Ordering::Acquire);
        (enabled, self.0.base_generation.load(Ordering::Relaxed))
    }
}

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
    /// Runtime switch for Depth of Market publication.
    pub book_capture: BookCaptureSwitch,
}

impl ServerConfig {
    /// Sensible defaults for `symbol` on [`DEFAULT_LISTEN_ADDR`], with depth
    /// capture off (it costs nothing until a consumer asks for it).
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            listen_addr: DEFAULT_LISTEN_ADDR.to_string(),
            symbol: symbol.into(),
            side_mode: SideMode::TickRule,
            hello_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            book_capture: BookCaptureSwitch::new(),
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
    /// One order-book event, in the provider-neutral depth vocabulary.
    ///
    /// Only produced while [`BookCaptureSwitch`] is enabled and the bridge
    /// declares depth support.
    Depth(DepthEvent),
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
    let listener = TcpListener::bind(&config.listen_addr).await.map_err(|e| {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BIND_FAILED",
            addr = %config.listen_addr,
            error = %e,
            "cannot bind the bridge listen address"
        );
        Mt5Error::Bind {
            addr: config.listen_addr.clone(),
            message: e.to_string(),
        }
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

    // Every capture generation this server ever opens gets its own offset on
    // top of the consumer's base, so no reconnect and no mid-session resync
    // can reuse a generation a consumer already retired.
    let mut generation_offset: u64 = 0;

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

        match serve_connection(stream, &config, &tx, &mut generation_offset).await {
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
///
/// `generation_offset` is the server-wide capture-generation cursor; this
/// function advances it whenever depth capture needs a fresh generation.
async fn serve_connection(
    stream: TcpStream,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
    generation_offset: &mut u64,
) -> ConnEnd {
    let mut lines = BoundedLineReader::new(stream);

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
        Ok(Ok(BoundedLine::Eof)) => {
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_EOF",
                "connection closed before hello"
            );
            return ConnEnd::BridgeGone("closed before hello".to_string());
        }
        Ok(Ok(BoundedLine::TooLong)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_LINE_TOO_LONG",
                max_bytes = MAX_LINE_BYTES as u64,
                "first line exceeded the size cap; dropping the connection"
            );
            return ConnEnd::BridgeGone("oversized hello".to_string());
        }
        Ok(Ok(BoundedLine::NotUtf8 { len })) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_UNDECODABLE_LINE",
                error = "invalid utf-8",
                line_bytes = len as u64,
                "first line was not valid protocol; dropping the connection"
            );
            return ConnEnd::BridgeGone("undecodable hello".to_string());
        }
        Ok(Ok(BoundedLine::Line(line))) => match protocol::parse_line(&line) {
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
    let mut depth = DepthSession::new(&hello, config.symbol.clone());
    depth.log_capability();

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
            Ok(Ok(BoundedLine::Eof)) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EOF",
                    "bridge closed the socket"
                );
                break ConnEnd::BridgeGone("eof".to_string());
            }
            Ok(Ok(BoundedLine::TooLong)) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_LINE_TOO_LONG",
                    max_bytes = MAX_LINE_BYTES as u64,
                    "peer streamed an oversized line; dropping the session"
                );
                break ConnEnd::BridgeGone("oversized line".to_string());
            }
            Ok(Ok(BoundedLine::NotUtf8 { len })) => {
                undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = "invalid utf-8",
                    line_bytes = len as u64,
                    total_undecodable = undecodable,
                    "skipping an undecodable line"
                );
                continue;
            }
            Ok(Ok(BoundedLine::Line(line))) => line,
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
            Ok(BridgeMsg::Book(image)) => {
                match depth
                    .observe(image, &config.book_capture, generation_offset, tx)
                    .await
                {
                    Ok(()) => {}
                    Err(()) => break ConnEnd::UiGone,
                }
            }
            Ok(BridgeMsg::Heartbeat(hb)) => {
                if let Some(offset) = hb.server_utc_offset_s {
                    mapper.set_server_utc_offset_s(offset);
                    depth.set_server_utc_offset_s(offset);
                }
                // A heartbeat is the natural moment to notice that a consumer
                // is waiting for depth this bridge cannot send.
                if depth
                    .report_missing_capability(&config.book_capture, tx)
                    .await
                    .is_err()
                {
                    break ConnEnd::UiGone;
                }
                debug!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
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
    // A consumer that was capturing depth must hear that this generation ended,
    // so it renders the discontinuity instead of connecting liquidity across it.
    depth.close(tx).await;
    end
}

/// Depth capture state for one bridge connection.
///
/// Split out because it is the only stateful thing in the message loop besides
/// tick mapping, and it must stay correct across three independent events: the
/// consumer toggling capture, the terminal losing images, and the session
/// ending.
struct DepthSession {
    symbol: String,
    /// `None` when the bridge declared no Depth of Market support.
    mapper: Option<BookMapper>,
    /// Whether the consumer has been told a generation is open.
    publishing: bool,
    last_seq: Option<u64>,
    missing_capability_reported: bool,
}

impl DepthSession {
    fn new(hello: &protocol::Hello, symbol: String) -> Self {
        Self {
            mapper: hello.book_levels.map(|levels| {
                BookMapper::new(
                    symbol.clone(),
                    0,
                    Some(levels),
                    hello.tick_size.as_deref(),
                    hello.server_utc_offset_s,
                )
            }),
            symbol,
            publishing: false,
            last_seq: None,
            missing_capability_reported: false,
        }
    }

    fn log_capability(&self) {
        match &self.mapper {
            Some(_) => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_AVAILABLE",
                symbol = %self.symbol,
                "bridge declares Depth of Market support"
            ),
            None => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
                symbol = %self.symbol,
                action = "trades_only",
                "bridge declares no Depth of Market; the heatmap will stay empty \
                 (recompile bridge/mt5/QuantickBridge.mq5, or the terminal refused the DOM)"
            ),
        }
    }

    fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        if let Some(mapper) = self.mapper.as_mut() {
            mapper.set_server_utc_offset_s(offset_s);
        }
    }

    /// Handle one image. `Err(())` means the consumer is gone.
    async fn observe(
        &mut self,
        image: protocol::Book,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        if self.mapper.is_none() {
            // A bridge sending images it never declared is a version skew, not
            // data to trust silently.
            return Ok(());
        }
        let (enabled, base_generation) = capture.state();
        let lost_images = self.images_lost(image.seq);
        let mapper = self.mapper.as_mut().expect("checked above");
        if !enabled {
            if self.publishing {
                let generation = mapper.generation();
                self.publishing = false;
                self.last_seq = None;
                mapper.restart(generation); // next capture starts from a snapshot
                send_depth_status(tx, &self.symbol, generation, DepthStatus::Stopped).await?;
            }
            return Ok(());
        }

        // Open a generation when capture starts, when the consumer moves its
        // base, or when images were lost and the diff would silently bridge a
        // moment we never observed.
        let wanted = base_generation.saturating_add(*generation_offset);
        if !self.publishing || mapper.generation() != wanted || lost_images {
            if lost_images {
                send_depth_status(
                    tx,
                    &self.symbol,
                    mapper.generation(),
                    DepthStatus::Resyncing {
                        reason: DepthResyncReason::SourceRestarted {
                            cause: "book_images_lost",
                        },
                    },
                )
                .await?;
            }
            *generation_offset = generation_offset.saturating_add(1);
            let generation = base_generation.saturating_add(*generation_offset);
            mapper.restart(generation);
            self.publishing = true;
            send_depth_status(tx, &self.symbol, generation, DepthStatus::Connecting).await?;
        }
        self.last_seq = Some(image.seq);

        let Some(event) = mapper.map(&image) else {
            return Ok(());
        };
        let synchronized =
            matches!(event, DepthEvent::Snapshot { .. }).then(|| mapper.synchronized_status());
        let generation = mapper.generation();
        if tx.send(Mt5Event::Depth(event)).await.is_err() {
            return Err(());
        }
        if let Some(status) = synchronized {
            send_depth_status(tx, &self.symbol, generation, status).await?;
        }
        Ok(())
    }

    /// Whether images went missing (or the bridge restarted its counter)
    /// between the last one and `seq`.
    fn images_lost(&self, seq: u64) -> bool {
        match self.last_seq {
            Some(last) => seq != last.saturating_add(1),
            None => false,
        }
    }

    /// Tell a waiting consumer, once, that this bridge cannot supply depth.
    async fn report_missing_capability(
        &mut self,
        capture: &BookCaptureSwitch,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        let (enabled, base_generation) = capture.state();
        if self.mapper.is_some() || self.missing_capability_reported || !enabled {
            return Ok(());
        }
        self.missing_capability_reported = true;
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
            symbol = %self.symbol,
            action = "report_disconnected",
            "depth capture is on but this bridge sends no Depth of Market"
        );
        // Tagged with the consumer's own base generation: a status below the
        // generation floor it is watching would be discarded as stale, and the
        // chart would keep waiting for a book that is never coming.
        send_depth_status(
            tx,
            &self.symbol,
            base_generation,
            DepthStatus::Disconnected {
                error_class: "bridge_without_depth",
            },
        )
        .await
    }

    /// End the generation when the bridge session ends.
    async fn close(&mut self, tx: &mpsc::Sender<Mt5Event>) {
        if let Some(mapper) = self.mapper.as_ref() {
            mapper.stats.log_summary(&self.symbol);
            if self.publishing {
                let _ = send_depth_status(
                    tx,
                    &self.symbol,
                    mapper.generation(),
                    DepthStatus::Disconnected {
                        error_class: "bridge_lost",
                    },
                )
                .await;
            }
        }
    }
}

/// Publish one depth status. `Err(())` means the consumer is gone.
async fn send_depth_status(
    tx: &mpsc::Sender<Mt5Event>,
    symbol: &str,
    generation: u64,
    status: DepthStatus,
) -> Result<(), ()> {
    tx.send(Mt5Event::Depth(DepthEvent::Status {
        symbol: symbol.to_string(),
        generation,
        status,
    }))
    .await
    .map_err(|_| ())
}

/// First 120 chars of a line, for log context without flooding. Truncates on
/// a char boundary: a byte-index slice would panic mid-codepoint.
fn snippet(line: &str) -> &str {
    match line.char_indices().nth(120) {
        Some((i, _)) => &line[..i],
        None => line,
    }
}

/// One read from the bounded line reader.
enum BoundedLine {
    /// A complete UTF-8 line (terminator stripped).
    Line(String),
    /// A complete line that was not valid UTF-8; skippable, per PROTOCOL.md.
    NotUtf8 {
        /// Length of the rejected line, in bytes.
        len: usize,
    },
    /// The peer streamed more than [`MAX_LINE_BYTES`] without a newline.
    TooLong,
    /// The peer closed the connection.
    Eof,
}

/// What one `fill_buf` round decided (split out so the borrow of the reader's
/// internal buffer ends before `consume`).
enum ReadStep {
    Eof,
    Line(usize),
    TooLong(usize),
    More(usize),
}

/// A newline-delimited reader that never buffers more than
/// [`MAX_LINE_BYTES`], unlike `AsyncBufReadExt::lines`. Cancel-safe: the only
/// await is `fill_buf`, and bytes move out of the socket buffer and into the
/// line buffer within a single poll.
struct BoundedLineReader {
    reader: BufReader<TcpStream>,
    buf: Vec<u8>,
}

impl BoundedLineReader {
    fn new(stream: TcpStream) -> Self {
        Self {
            reader: BufReader::new(stream),
            buf: Vec::new(),
        }
    }

    async fn next_line(&mut self) -> std::io::Result<BoundedLine> {
        loop {
            let step = {
                let available = self.reader.fill_buf().await?;
                if available.is_empty() {
                    ReadStep::Eof
                } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                    if self.buf.len() + pos > MAX_LINE_BYTES {
                        ReadStep::TooLong(pos + 1)
                    } else {
                        self.buf.extend_from_slice(&available[..pos]);
                        ReadStep::Line(pos + 1)
                    }
                } else if self.buf.len() + available.len() > MAX_LINE_BYTES {
                    ReadStep::TooLong(available.len())
                } else {
                    self.buf.extend_from_slice(available);
                    ReadStep::More(available.len())
                }
            };
            match step {
                ReadStep::Eof => {
                    // A trailing unterminated line still counts, like
                    // `lines()` behaves.
                    if self.buf.is_empty() {
                        return Ok(BoundedLine::Eof);
                    }
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::Line(consume) => {
                    self.reader.consume(consume);
                    return Ok(Self::finish(std::mem::take(&mut self.buf)));
                }
                ReadStep::TooLong(consume) => {
                    self.reader.consume(consume);
                    self.buf.clear();
                    return Ok(BoundedLine::TooLong);
                }
                ReadStep::More(consume) => self.reader.consume(consume),
            }
        }
    }

    fn finish(mut line: Vec<u8>) -> BoundedLine {
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        match String::from_utf8(line) {
            Ok(text) => BoundedLine::Line(text),
            Err(e) => BoundedLine::NotUtf8 {
                len: e.as_bytes().len(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::snippet;

    #[test]
    fn snippet_truncates_on_char_boundaries() {
        // 1 ASCII byte then two-byte chars: byte 120 falls mid-codepoint,
        // which the old byte slice panicked on.
        let line = format!("x{}", "é".repeat(200));
        assert_eq!(snippet(&line).chars().count(), 120);

        let short = "short line";
        assert_eq!(snippet(short), short);

        let exact: String = "a".repeat(120);
        assert_eq!(snippet(&exact), exact);
    }
}
