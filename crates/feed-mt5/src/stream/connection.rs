//! One bridge connection, from its hello to its last line.
//!
//! A connection is admitted, served, then closed; [`serve_connection`] reads as
//! those three phases. Serving waits on the socket and on the trader's paging
//! requests at once, drives the block state machines in [`super::blocks`], and
//! hands every finished thing to [`super::publish`]. What it remembers between
//! two wakes is [`Connection`], whose handlers each own one kind of input.

use std::ops::ControlFlow;

use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use quantick_engine::Trade;

use crate::latency::LatencyTracker;
use crate::map::{MapOutcome, PriceContext, TickMapper};
use crate::protocol::{self, BridgeMsg, FeedMsg, SCHEMA_VERSION};
use crate::session::SeqTracker;

use super::blocks::RatesBlock;
use super::depth::{DEPTH_CHECK_INTERVAL, DepthSession};
use super::events::{ConnEnd, Mt5Event, Mt5Status};
use super::publish::{publish_latency, send_live};
use super::reader::{BoundedLine, BoundedLineReader};
use super::{MAX_LINE_BYTES, ServerConfig, snippet};

/// Trades one paged block may hold before it stops growing.
///
/// The bridge is the side this crate cannot vouch for — the same reasoning
/// behind [`MAX_BARS_PER_BLOCK`] for candles, and more pressing here: a candle
/// block arrives once per session, a page can be opened again on every click.
/// A peer that opens `history_start` and never stops would otherwise grow this
/// vector until the feed task dies and takes the chart with it.
///
/// Sized well above any honest page: the consumer's own step tops out at 50 000
/// (the toolbar's `DragValue` range) and the Python bridge caps itself at
/// 200 000, so a block reaching this ceiling is a bridge that is not answering
/// the question it was asked.
const MAX_TRADES_PER_PAGE: usize = 250_000;

/// A page of older ticks under construction, and the live tape's tick-rule
/// context waiting for it to finish.
///
/// The context travels with the block rather than in a variable beside it so
/// the two cannot be separated: every path that takes the block back also gets
/// the context to put back, including the ones that discard it.
struct PagedBlock {
    /// The mapped trades collected so far.
    trades: Vec<Trade>,
    /// What the tick rule was reading before the block opened.
    resume: PriceContext,
    /// Trades dropped because the block hit [`MAX_TRADES_PER_PAGE`]. Reported
    /// once at the end rather than per tick, so a runaway bridge cannot turn
    /// the log into the second denial of service.
    over_cap: u64,
    /// Whether this block is a slice of the opening session rather than the
    /// answer to a click. Carried from the start marker to the end one because
    /// only the start says which kind it is, and only the end can act on it.
    opening: bool,
}

/// What woke the session loop: something the bridge said, or something the
/// trader asked for.
///
/// The two arrive on opposite halves of the same socket and the loop has to
/// wait on both at once; naming them lets the wait stay one `select!` with one
/// timeout rather than two loops racing over one reader.
enum SessionInput {
    /// Low-frequency depth availability check, independent of bridge liveness.
    DepthCheck,
    /// A line from the bridge (or the error that ended the read).
    Line(std::io::Result<BoundedLine>),
    /// The consumer wants ticks older than `before_utc_ms`.
    Request {
        /// How many ticks to ask for.
        count: u64,
        /// Oldest UTC millisecond the consumer holds; the request is for
        /// strictly older than this.
        before_utc_ms: i64,
    },
}

/// Wait on independent requests, depth deadlines and cancel-safe socket reads.
async fn next_session_input(
    lines: &mut BoundedLineReader<tokio::net::tcp::OwnedReadHalf>,
    mut pending_request: std::pin::Pin<&mut impl std::future::Future<Output = (u64, i64)>>,
    depth_check: &mut tokio::time::Interval,
) -> SessionInput {
    tokio::select! {
        // A busy tape cannot starve a click or the low-frequency depth check.
        biased;
        (count, before_utc_ms) = pending_request.as_mut() => {
            SessionInput::Request { count, before_utc_ms }
        }
        _ = depth_check.tick() => SessionInput::DepthCheck,
        // The reader retains each partial line across a cancelled fill_buf;
        // nothing consumed from the socket is lost when another branch wakes.
        line = lines.next_line() => SessionInput::Line(line),
    }
}

/// What happened to one attempt to ask the bridge for older ticks.
enum PageRequestOutcome {
    /// Written to the socket; the block will arrive on the read side.
    Sent,
    /// This bridge does not read its socket, so nothing was written.
    Refused,
    /// The write failed; the session is over.
    WriteFailed(std::io::Error),
}

/// Put one request for older ticks on the wire.
///
/// Refuses rather than writes when the bridge never declared it reads: an
/// unread request would sit in the peer's receive buffer, and on the Expert
/// Advisor a full buffer eventually blocks the terminal thread that sends
/// ticks — so a chart asking for history would stop the chart.
async fn answer_page_request(
    outgoing: &mut tokio::net::tcp::OwnedWriteHalf,
    symbol: &str,
    mapper: &TickMapper,
    can_page: bool,
    count: u64,
    before_utc_ms: i64,
) -> PageRequestOutcome {
    if !can_page {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_LOAD_OLDER_UNSUPPORTED",
            symbol,
            requested = count,
            action = "answer_empty",
            advice = "update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks",
            "this bridge session does not answer requests for older ticks"
        );
        return PageRequestOutcome::Refused;
    }
    // The terminal stamps everything in server time, so the cursor crosses in
    // its clock; the mapper owns that offset and its heartbeat refreshes.
    let before_ms = mapper.to_server_ms(before_utc_ms);
    let line = protocol::encode_line(&FeedMsg::LoadOlder { count, before_ms });
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = "MT5_LOAD_OLDER_REQUESTED",
        symbol,
        requested = count,
        before_ms,
        before_utc_ms,
        "asking the terminal for ticks older than the chart's oldest"
    );
    match outgoing.write_all(line.as_bytes()).await {
        Ok(()) => PageRequestOutcome::Sent,
        Err(e) => PageRequestOutcome::WriteFailed(e),
    }
}

/// Serve one bridge connection to completion.
///
/// Three phases, in this order and only this order: [`admit`] the hello, serve
/// the [`Connection`] until something ends it, then [`Connection::close`] it.
///
/// `generation_offset` is the server-wide capture-generation cursor; this
/// function advances it whenever depth capture needs a fresh generation.
pub(super) async fn serve_connection(
    stream: TcpStream,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
    generation_offset: &mut u64,
) -> ConnEnd {
    // Split so the session can write while parked on a read. The write half is
    // used at most once per trader click; the read half carries every tick.
    let (incoming, mut outgoing) = stream.into_split();
    let mut lines = BoundedLineReader::new(incoming);

    let hello = match admit(&mut lines, config, tx).await {
        Ok(hello) => hello,
        Err(end) => return end,
    };

    // Parked here across the whole session rather than rebuilt each pass. The
    // read loop polls this once per inbound line, and a future recreated every
    // time would re-register with the `Notify` on every tick — paying a
    // synchronization cost per print for something that fires when a trader
    // clicks. Pinned once, a poll is a load and a return.
    let pending_request = config.history_pager.take_request();
    tokio::pin!(pending_request);

    let mut depth_check = tokio::time::interval(DEPTH_CHECK_INTERVAL);
    depth_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut connection = Connection::open(&hello, config, tx, generation_offset);

    let end = loop {
        // Only inbound data or a paging request refresh bridge liveness.
        // The depth timer must never keep a silent socket alive.
        let input = tokio::time::timeout_at(
            connection.read_deadline,
            next_session_input(&mut lines, pending_request.as_mut(), &mut depth_check),
        )
        .await;

        let step = match input {
            Err(_) => connection.on_silence(),
            Ok(SessionInput::DepthCheck) => connection.on_depth_check().await,
            Ok(SessionInput::Request {
                count,
                before_utc_ms,
            }) => {
                pending_request.set(config.history_pager.take_request());
                connection
                    .on_page_request(&mut outgoing, count, before_utc_ms)
                    .await
            }
            Ok(SessionInput::Line(read)) => connection.on_read(read).await,
        };
        if let ControlFlow::Break(end) = step {
            break end;
        }
    };

    connection.close().await;
    end
}

/// Phase 1: the first message must be a hello that matches what we expect, and
/// the consumer hears about the session before anything else does.
async fn admit(
    lines: &mut BoundedLineReader<tokio::net::tcp::OwnedReadHalf>,
    config: &ServerConfig,
    tx: &mpsc::Sender<Mt5Event>,
) -> Result<protocol::Hello, ConnEnd> {
    let hello = read_hello(lines, config).await?;
    refuse_foreign_hello(&hello, config)?;
    announce_session(&hello, tx).await?;
    Ok(hello)
}

/// Wait for the first line and decode it as a hello; anything else drops the
/// connection, each way under its own reason.
async fn read_hello(
    lines: &mut BoundedLineReader<tokio::net::tcp::OwnedReadHalf>,
    config: &ServerConfig,
) -> Result<protocol::Hello, ConnEnd> {
    match tokio::time::timeout(config.hello_timeout, lines.next_line()).await {
        Err(_) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HELLO_TIMEOUT",
                timeout_s = config.hello_timeout.as_secs(),
                "connection said nothing; dropping it"
            );
            Err(ConnEnd::BridgeGone("hello timeout".to_string()))
        }
        Ok(Err(e)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_SOCKET_ERROR",
                error = %e,
                "socket error before hello; dropping the connection"
            );
            Err(ConnEnd::BridgeGone(format!(
                "socket error before hello: {e}"
            )))
        }
        Ok(Ok(BoundedLine::Eof)) => {
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BRIDGE_EOF",
                "connection closed before hello"
            );
            Err(ConnEnd::BridgeGone("closed before hello".to_string()))
        }
        Ok(Ok(BoundedLine::TooLong)) => {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_LINE_TOO_LONG",
                max_bytes = MAX_LINE_BYTES as u64,
                "first line exceeded the size cap; dropping the connection"
            );
            Err(ConnEnd::BridgeGone("oversized hello".to_string()))
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
            Err(ConnEnd::BridgeGone("undecodable hello".to_string()))
        }
        Ok(Ok(BoundedLine::Line(line))) => match protocol::parse_line(&line) {
            Ok(BridgeMsg::Hello(h)) => Ok(h),
            Ok(other) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    got = ?other,
                    "first message was not a hello; dropping the connection"
                );
                Err(ConnEnd::BridgeGone("no hello".to_string()))
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
                Err(ConnEnd::BridgeGone("undecodable hello".to_string()))
            }
        },
    }
}

/// A hello from another protocol version or another symbol is refused.
fn refuse_foreign_hello(hello: &protocol::Hello, config: &ServerConfig) -> Result<(), ConnEnd> {
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
        return Err(ConnEnd::BridgeGone(format!(
            "schema mismatch (bridge {})",
            hello.schema
        )));
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
        return Err(ConnEnd::BridgeGone(format!(
            "symbol mismatch ({})",
            hello.symbol
        )));
    }
    Ok(())
}

/// Whether this bridge reads what is written to it.
///
/// Absent means no: writing to a bridge that never reads would fill its
/// receive buffer and, on the EA, block the terminal thread that sends ticks.
fn can_page(hello: &protocol::Hello) -> bool {
    hello.history_paging.unwrap_or(false)
}

/// Log the accepted hello and publish the connected status.
async fn announce_session(
    hello: &protocol::Hello,
    tx: &mpsc::Sender<Mt5Event>,
) -> Result<(), ConnEnd> {
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
        tape = ?hello.tape,
        "bridge session established"
    );
    let can_page = can_page(hello);
    if tx
        .send(Mt5Event::Status(Mt5Status::Connected {
            symbol: hello.symbol.clone(),
            broker_symbol: hello.broker_symbol.clone(),
            tape: hello.tape,
            book_levels: hello.book_levels,
            rates: hello.rates.unwrap_or(false),
            history_paging: can_page,
            deal_counter: hello.deal_counter.unwrap_or(false),
        }))
        .await
        .is_err()
    {
        return Err(ConnEnd::UiGone);
    }
    info!(
        target: "quantick::feed",
        schema_version = 1_u8,
        event_code = if can_page {
            "MT5_HISTORY_PAGING_AVAILABLE"
        } else {
            "MT5_HISTORY_PAGING_UNSUPPORTED"
        },
        symbol = %hello.symbol,
        bridge = %hello.bridge,
        bridge_version = %hello.bridge_version,
        advice = if can_page {
            "-"
        } else {
            "update the bridge (bridge/mt5/quantick_bridge.py) to page older ticks"
        },
        "whether this session can be asked for ticks older than it sent"
    );
    Ok(())
}

/// A send result as a loop step: a consumer that stopped listening ends the
/// session.
fn delivered<E>(sent: Result<(), E>) -> ControlFlow<ConnEnd> {
    match sent {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(ConnEnd::UiGone),
    }
}

/// The reply a request gets when there is no page to give it.
fn empty_page() -> Mt5Event {
    Mt5Event::HistoryPage {
        trades: Vec::new(),
        exhausted: false,
        scanned_to_utc_ms: None,
    }
}

/// Phase 2's state: everything one admitted connection knows between two wakes.
///
/// The handlers below return [`ControlFlow`]: `Continue` waits for the next
/// input, `Break` carries the reason the session ended into phase 3.
struct Connection<'a> {
    config: &'a ServerConfig,
    tx: &'a mpsc::Sender<Mt5Event>,
    generation_offset: &'a mut u64,
    /// Whether the bridge declared that it reads its socket.
    can_page: bool,
    /// The offset the hello declared. Candles are dated by it, not by the
    /// offset heartbeats later refresh.
    hello_utc_offset_s: i64,
    mapper: TickMapper,
    tracker: SeqTracker,
    highest_tick_ms: Option<i64>,
    latency: LatencyTracker,
    deals: crate::deals::DealSampler,
    /// Whether the tape has already been reported late. Edge-triggered, so a
    /// session that stays behind logs the diagnosis once instead of once per
    /// sample, and its recovery is logged too — a report with no matching
    /// recovery is how an operator reads "it never came back".
    lag_reported: bool,
    /// Whether the consumer's queue has already been reported full. Same edge
    /// trigger as the lag report, for the same reason.
    backpressure_reported: bool,
    backfill: Option<Vec<Trade>>,
    candles: Option<RatesBlock>,
    undecodable: u64,
    depth: DepthSession,
    /// The paged block being collected, if the bridge is mid-answer, and the
    /// live tape's tick-rule context parked for the duration.
    page: Option<PagedBlock>,
    /// When the bridge counts as silent unless something refreshes this.
    read_deadline: tokio::time::Instant,
}

impl<'a> Connection<'a> {
    /// Build the state for an admitted hello.
    fn open(
        hello: &protocol::Hello,
        config: &'a ServerConfig,
        tx: &'a mpsc::Sender<Mt5Event>,
        generation_offset: &'a mut u64,
    ) -> Self {
        // The bridge declares what its venue prints; the configured `side_mode` is
        // a policy, `tape` is a fact about the instrument, and a fact the feed
        // cannot observe for itself must come from the side that can see it.
        let mapper =
            TickMapper::new(config.side_mode, hello.server_utc_offset_s).with_tape(hello.tape);
        let deals = crate::deals::DealSampler::new(hello.server_utc_offset_s);
        let depth = DepthSession::new(hello, config.symbol.clone());
        depth.log_capability();
        Self {
            config,
            tx,
            generation_offset,
            can_page: can_page(hello),
            hello_utc_offset_s: hello.server_utc_offset_s,
            mapper,
            tracker: SeqTracker::new(),
            highest_tick_ms: None,
            latency: LatencyTracker::new(),
            deals,
            lag_reported: false,
            backpressure_reported: false,
            backfill: None,
            candles: None,
            undecodable: 0,
            depth,
            page: None,
            read_deadline: tokio::time::Instant::now() + config.read_timeout,
        }
    }

    /// The read deadline passed with nothing refreshing it.
    fn on_silence(&self) -> ControlFlow<ConnEnd> {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_SILENT",
            timeout_s = self.config.read_timeout.as_secs(),
            "no ticks or heartbeats within the timeout; presuming the bridge dead"
        );
        ControlFlow::Break(ConnEnd::BridgeGone("silent".to_string()))
    }

    /// The low-frequency depth availability timer fired.
    async fn on_depth_check(&mut self) -> ControlFlow<ConnEnd> {
        delivered(
            self.depth
                .check(&self.config.book_capture, self.generation_offset, self.tx)
                .await,
        )
    }

    /// The consumer asked for ticks older than `before_utc_ms`.
    async fn on_page_request(
        &mut self,
        outgoing: &mut tokio::net::tcp::OwnedWriteHalf,
        count: u64,
        before_utc_ms: i64,
    ) -> ControlFlow<ConnEnd> {
        self.read_deadline = tokio::time::Instant::now() + self.config.read_timeout;
        match answer_page_request(
            outgoing,
            &self.config.symbol,
            &self.mapper,
            self.can_page,
            count,
            before_utc_ms,
        )
        .await
        {
            // Asked. The answer arrives as an ordinary block of lines
            // on the read side, like every other thing the bridge says.
            PageRequestOutcome::Sent => ControlFlow::Continue(()),
            // Nothing to ask: this bridge cannot page. The consumer is
            // still owed the one reply every request gets, or its
            // spinner runs forever.
            PageRequestOutcome::Refused => {
                self.config.history_pager.settle_owed();
                delivered(self.tx.send(empty_page()).await)
            }
            // A socket that cannot be written to cannot be read from
            // either; end the session and let the reconnect handle it.
            // The owed reply is sent by `close`, which covers
            // every way a session can end while a page is outstanding.
            PageRequestOutcome::WriteFailed(e) => {
                ControlFlow::Break(ConnEnd::BridgeGone(format!("socket error on write: {e}")))
            }
        }
    }

    /// One read from the socket: a line to decode, or the thing that ended it.
    async fn on_read(&mut self, read: std::io::Result<BoundedLine>) -> ControlFlow<ConnEnd> {
        let line = match read {
            Err(e) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_SOCKET_ERROR",
                    error = %e,
                    "socket error; dropping the session"
                );
                return ControlFlow::Break(ConnEnd::BridgeGone(format!("socket error: {e}")));
            }
            Ok(BoundedLine::Eof) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EOF",
                    "bridge closed the socket"
                );
                return ControlFlow::Break(ConnEnd::BridgeGone("eof".to_string()));
            }
            Ok(BoundedLine::TooLong) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_LINE_TOO_LONG",
                    max_bytes = MAX_LINE_BYTES as u64,
                    "peer streamed an oversized line; dropping the session"
                );
                return ControlFlow::Break(ConnEnd::BridgeGone("oversized line".to_string()));
            }
            Ok(BoundedLine::NotUtf8 { len }) => {
                self.undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = "invalid utf-8",
                    line_bytes = len as u64,
                    total_undecodable = self.undecodable,
                    "skipping an undecodable line"
                );
                return ControlFlow::Continue(());
            }
            Ok(BoundedLine::Line(line)) => line,
        };
        self.read_deadline = tokio::time::Instant::now() + self.config.read_timeout;
        if line.trim().is_empty() {
            return ControlFlow::Continue(());
        }

        match protocol::parse_line(&line) {
            Err(e) => {
                self.undecodable += 1;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_UNDECODABLE_LINE",
                    error = %e,
                    snippet = %snippet(&line),
                    total_undecodable = self.undecodable,
                    "skipping an undecodable line"
                );
                ControlFlow::Continue(())
            }
            Ok(message) => self.on_message(message).await,
        }
    }

    /// Route one decoded message to the arm that owns it.
    async fn on_message(&mut self, message: BridgeMsg) -> ControlFlow<ConnEnd> {
        match message {
            BridgeMsg::Tick(tick) => self.on_tick(&tick).await,
            BridgeMsg::Book(image) => delivered(
                self.depth
                    .observe(
                        image,
                        &self.config.book_capture,
                        self.generation_offset,
                        self.tx,
                        tokio::time::Instant::now(),
                    )
                    .await,
            ),
            BridgeMsg::Heartbeat(hb) => self.on_heartbeat(&hb).await,
            BridgeMsg::BackfillStart { count_hint } => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_START",
                    count_hint = ?count_hint,
                    "bridge is sending history"
                );
                self.backfill = Some(Vec::new());
                ControlFlow::Continue(())
            }
            BridgeMsg::BackfillEnd {} => {
                let batch = self.backfill.take().unwrap_or_default();
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BACKFILL_END",
                    trades = batch.len(),
                    "history block complete"
                );
                delivered(self.tx.send(Mt5Event::Backfilled(batch)).await)
            }
            BridgeMsg::HistoryStart {
                count_hint,
                opening,
            } => {
                self.on_history_start(count_hint, opening);
                ControlFlow::Continue(())
            }
            BridgeMsg::HistoryEnd {
                exhausted,
                scanned_to_ms,
                remaining,
            } => {
                self.on_history_end(exhausted, scanned_to_ms, remaining)
                    .await
            }
            BridgeMsg::RatesStart {
                interval_ms,
                count_hint,
            } => {
                self.on_rates_start(interval_ms, count_hint);
                ControlFlow::Continue(())
            }
            BridgeMsg::Rate(chunk) => {
                match self.candles.as_mut() {
                    Some(block) => block.absorb(&chunk),
                    // A batch outside a block has no interval to be measured in,
                    // and guessing one would misdate every bar in it.
                    None => warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_PROTOCOL_VIOLATION",
                        bars = chunk.bars.len(),
                        action = "drop_chunk",
                        "candles arrived outside a rates block; dropping them"
                    ),
                }
                ControlFlow::Continue(())
            }
            BridgeMsg::RatesEnd { partial } => self.on_rates_end(partial).await,
            BridgeMsg::Bye { reason } => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_BYE",
                    reason = %reason,
                    "bridge said goodbye"
                );
                ControlFlow::Break(ConnEnd::BridgeGone(format!("bye: {reason}")))
            }
            BridgeMsg::Hello(_) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    "second hello mid-session; ignoring it"
                );
                ControlFlow::Continue(())
            }
        }
    }

    /// One tick: its sequence anomaly, then its deal reading, then its print.
    async fn on_tick(&mut self, tick: &protocol::Tick) -> ControlFlow<ConnEnd> {
        let tick_ms = self.mapper.to_utc_ms(tick.time_ms);
        let advances = self
            .tracker
            .highest()
            .is_none_or(|highest| tick.seq > highest);
        if let Some(anomaly) = self.tracker.observe(tick.seq)
            && let Some(from_ms) = self.highest_tick_ms
        {
            delivered(
                self.tx
                    .send(Mt5Event::SequenceAnomaly {
                        anomaly,
                        from_ms,
                        to_ms: tick_ms,
                    })
                    .await,
            )?;
        }
        if advances {
            self.highest_tick_ms = Some(tick_ms);
        }
        // Ahead of the print it stamps; quote-only ticks carry it too.
        if let Some(sample) = self.deals.observe(tick) {
            delivered(self.tx.send(Mt5Event::DealCounter(sample)).await)?;
        }
        if let MapOutcome::Trade { trade, .. } = self.mapper.map(tick) {
            return self.route_trade(trade, tick).await;
        }
        ControlFlow::Continue(())
    }

    /// A tick belongs to whichever block is open around it. The paged block is
    /// checked first because it is the one that can open mid-session: everything
    /// else is live by default, and a paged tick delivered as live would append
    /// history to the front of the tape.
    async fn route_trade(&mut self, trade: Trade, tick: &protocol::Tick) -> ControlFlow<ConnEnd> {
        match (self.page.as_mut(), self.backfill.as_mut()) {
            (Some(block), _) => {
                if block.trades.len() < MAX_TRADES_PER_PAGE {
                    block.trades.push(trade);
                } else {
                    block.over_cap = block.over_cap.saturating_add(1);
                }
            }
            (None, Some(buf)) => buf.push(trade),
            (None, None) => {
                // Live prints only. A backfill or paged tick is as
                // old as the history it belongs to, and measuring
                // latency from one would report minutes of delay on
                // a chart that is perfectly current.
                self.latency.observe_live(tick.time_ms, tick.sent_ms);
                // Sampled *before* the trade is handed downstream,
                // never after. `send_live` waits when the consumer
                // is not draining, and a clock read on the far side
                // of that wait charges the consumer's own queueing
                // to the wire — collapsing the very gap the health
                // view publishes these two figures to expose.
                if self.latency.due() {
                    delivered(
                        publish_latency(
                            &mut self.latency,
                            &self.mapper,
                            &self.config.symbol,
                            &mut self.lag_reported,
                            self.tx,
                        )
                        .await,
                    )?;
                }
                delivered(
                    send_live(
                        self.tx,
                        trade,
                        &self.config.symbol,
                        &mut self.backpressure_reported,
                    )
                    .await,
                )?;
            }
        }
        ControlFlow::Continue(())
    }

    /// A heartbeat: the end of a deal round, a fresh clock offset, the latency
    /// report of a thin tape, and the depth capability check.
    async fn on_heartbeat(&mut self, hb: &protocol::Heartbeat) -> ControlFlow<ConnEnd> {
        // A second without a tick ends the round a reading waits on
        // — before the clock moves, so the reading is dated with the
        // ticks it covered.
        if let Some(sample) = self.deals.finish() {
            delivered(self.tx.send(Mt5Event::DealCounter(sample)).await)?;
        }
        if let Some(offset) = hb.server_utc_offset_s {
            self.mapper.set_server_utc_offset_s(offset);
            self.depth.set_server_utc_offset_s(offset);
            self.deals.set_server_utc_offset_s(offset);
        }
        // The beat a thin tape is measured on: a symbol printing once
        // a minute never reaches the per-print sampling bound, and
        // would otherwise never report at all. The figures are still
        // the newest print's own, so a quiet stretch reports how late
        // that print was and never how long ago it was.
        delivered(
            publish_latency(
                &mut self.latency,
                &self.mapper,
                &self.config.symbol,
                &mut self.lag_reported,
                self.tx,
            )
            .await,
        )?;
        // A heartbeat is the natural moment to notice that a consumer
        // is waiting for depth this bridge cannot send.
        delivered(
            self.depth
                .report_missing_capability(&self.config.book_capture, self.tx)
                .await,
        )?;
        debug!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_HEARTBEAT",
            seq_last = hb.seq_last,
            ticks_sent = hb.ticks_sent,
            "bridge heartbeat"
        );
        ControlFlow::Continue(())
    }

    /// A paged block opens: park the live tape's tick-rule context behind it.
    fn on_history_start(&mut self, count_hint: Option<u64>, opening: bool) {
        if !opening && !self.config.history_pager.is_in_flight() {
            // Nobody asked. Collect it anyway rather than letting its
            // ticks fall through as live prints — the block is history,
            // and charting it at the front of the tape is the one
            // outcome worse than dropping it. Logged under the same
            // code as the matching end: one condition, one thing to
            // grep for, both halves of the event findable.
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HISTORY_PAGE_UNSOLICITED",
                action = "collect_and_discard",
                "history_start with no request outstanding"
            );
        }
        info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_HISTORY_PAGE_START",
            count_hint = ?count_hint,
            "bridge is sending a page of older ticks"
        );
        // A block already open means the bridge restarted mid-send.
        // Put the live context back before parking it again, or the
        // second start would park an already-emptied one and the tape
        // would never get its price back.
        if let Some(stale) = self.page.take() {
            self.mapper.restore_price_context(stale.resume);
        }
        // The page's ticks are hours older than the live tape; the tick
        // rule must read them against each other, not against the
        // newest price on the chart.
        self.page = Some(PagedBlock {
            trades: Vec::new(),
            resume: self.mapper.take_price_context(),
            over_cap: 0,
            opening,
        });
    }

    /// A paged block closes: give the tape its context back and deliver the
    /// block as whichever kind its start said it was.
    async fn on_history_end(
        &mut self,
        exhausted: bool,
        scanned_to_ms: Option<i64>,
        remaining: Option<u64>,
    ) -> ControlFlow<ConnEnd> {
        let Some(PagedBlock {
            trades,
            resume,
            over_cap,
            opening,
        }) = self.page.take()
        else {
            return self.on_startless_history_end().await;
        };
        // The live tape resumes from the price it left off at, never
        // from wherever the page ended.
        self.mapper.restore_price_context(resume);
        if over_cap > 0 {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HISTORY_PAGE_TRUNCATED",
                cap = MAX_TRADES_PER_PAGE as u64,
                dropped = over_cap,
                "a page exceeded the per-block cap; the surplus was dropped"
            );
        }
        if opening {
            // Nobody asked for this, so nothing is settled: a click
            // made while the session is still filling in stays owed its
            // own reply. The slice is still prepended — it is the
            // trader's morning.
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_OPENING_PAGE",
                trades = trades.len(),
                remaining = ?remaining,
                "a slice of the opening session is ready to prepend"
            );
            return delivered(
                self.tx
                    .send(Mt5Event::OpeningPage { trades, remaining })
                    .await,
            );
        }
        if !self.config.history_pager.settle_owed() {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HISTORY_PAGE_UNSOLICITED",
                trades = trades.len(),
                action = "discard",
                "a page nobody asked for; discarding it rather than prepending"
            );
            return ControlFlow::Continue(());
        }
        info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_HISTORY_PAGE_END",
            trades = trades.len(),
            exhausted,
            scanned_to_ms = ?scanned_to_ms,
            "page of older ticks complete"
        );
        // Converted here, where the offset lives, for the same reason
        // the outbound cursor is: the consumer speaks UTC and this is
        // the one place that tracks what the terminal's clock is doing.
        let scanned_to_utc_ms = scanned_to_ms.map(|ms| self.mapper.to_utc_ms(ms));
        delivered(
            self.tx
                .send(Mt5Event::HistoryPage {
                    trades,
                    exhausted,
                    scanned_to_utc_ms,
                })
                .await,
        )
    }

    /// The start went missing — an undecodable line ahead of it is enough. The
    /// block's ticks have already been charted as live, which cannot be undone
    /// here; what must not also happen is the pager staying latched. Without
    /// settling, `request` refuses every later click for the rest of the session
    /// and the button silently stops working.
    async fn on_startless_history_end(&mut self) -> ControlFlow<ConnEnd> {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_PROTOCOL_VIOLATION",
            action = "settle_and_answer_empty",
            "history_end without a history_start; its ticks went out as live"
        );
        if self.config.history_pager.settle_owed() {
            delivered(self.tx.send(empty_page()).await)?;
        }
        ControlFlow::Continue(())
    }

    /// A candle block opens, replacing any block still running.
    fn on_rates_start(&mut self, interval_ms: i64, count_hint: Option<u64>) {
        info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_RATES_START",
            interval_ms,
            count_hint = ?count_hint,
            "bridge is sending historical candles"
        );
        if let Some(open) = self.candles.take() {
            // A block already running: the bridge restarted mid-send,
            // or two are interleaved. Either way what was collected
            // belongs to a window this new header does not describe.
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_PROTOCOL_VIOLATION",
                bars = open.len(),
                action = "discard_open_block",
                "a second rates_start arrived inside an open block; discarding the first"
            );
        }
        self.candles = Some(RatesBlock::new(interval_ms, self.hello_utc_offset_s));
    }

    /// A candle block closes and is delivered whole.
    async fn on_rates_end(&mut self, partial: bool) -> ControlFlow<ConnEnd> {
        let Some(block) = self.candles.take() else {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_PROTOCOL_VIOLATION",
                action = "ignore",
                "rates_end without a rates_start; ignoring it"
            );
            return ControlFlow::Continue(());
        };
        let (interval_ms, bars, clipped) = block.finish(&self.config.symbol);
        // Either side may know the block is short: the bridge from
        // its paging, this decoder from its own cap. Kept apart in
        // the log — they point at different things to go fix.
        let bridge_said_partial = partial;
        let partial = bridge_said_partial || clipped;
        if partial {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_RATES_PARTIAL",
                symbol = %self.config.symbol,
                bars = bars.len(),
                bridge_said_partial,
                clipped_here = clipped,
                "the candle block is short of what was asked for"
            );
        }
        delivered(
            self.tx
                .send(Mt5Event::Rates {
                    interval_ms,
                    bars,
                    partial,
                })
                .await,
        )
    }

    /// Phase 3: however the session ended, discard what never finished and
    /// answer what is still owed. The consumer may be gone, so nothing here can
    /// end anything further.
    async fn close(mut self) {
        if self.backfill.is_some() {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_PARTIAL_BACKFILL_DISCARDED",
                "session ended mid-backfill; discarding the incomplete block"
            );
        }
        // Every request gets exactly one reply, including the ones this session
        // died holding — whether it had been sent to the bridge or was still
        // queued. The alternative is a spinner that outlives the connection it was
        // waiting on. `abandon` also clears the request itself: it belongs to the
        // connection that is ending, and replaying it against the next session
        // would page from a cursor that one never sent.
        if self.config.history_pager.abandon() {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_HISTORY_PAGE_UNANSWERED",
                partial_trades = self.page.as_ref().map_or(0, |block| block.trades.len()),
                action = "answer_empty",
                "session ended with a page outstanding; answering it empty"
            );
            let _ = self.tx.send(empty_page()).await;
        }
        if let Some(block) = self.candles {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_PARTIAL_RATES_DISCARDED",
                bars = block.len(),
                "session ended mid-candle-block; discarding it (the next session re-sends)"
            );
        }
        self.mapper.stats.log_summary(&self.config.symbol);
        // The reading a round was still waiting on, dated at the last tick it
        // covered. Dropped, the next session would re-emit its value at its own
        // first round, and the recording would carry the wrong time.
        if let Some(sample) = self.deals.finish() {
            let _ = self.tx.send(Mt5Event::DealCounter(sample)).await;
        }
        // A consumer that was capturing depth must hear that this generation ended,
        // so it renders the discontinuity instead of connecting liquidity across it.
        self.depth.close(self.tx).await;
    }
}
