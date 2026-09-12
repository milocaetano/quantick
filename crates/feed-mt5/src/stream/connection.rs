//! One bridge connection, from its hello to its last line.
//!
//! The session loop lives here: it waits on the socket and on the trader's
//! paging requests at once, drives the block state machines in
//! [`super::blocks`], and hands every finished thing to [`super::publish`].

use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use quantick_engine::Trade;

use crate::latency::LatencyTracker;
use crate::map::{MapOutcome, PriceContext, TickMapper};
use crate::protocol::{self, BridgeMsg, FeedMsg, SCHEMA_VERSION};
use crate::session::SeqTracker;

use super::blocks::{DepthSession, RatesBlock};
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
        tape = ?hello.tape,
        "bridge session established"
    );
    // Absent means no: writing to a bridge that never reads would fill its
    // receive buffer and, on the EA, block the terminal thread that sends ticks.
    let can_page = hello.history_paging.unwrap_or(false);
    if tx
        .send(Mt5Event::Status(Mt5Status::Connected {
            symbol: hello.symbol.clone(),
            broker_symbol: hello.broker_symbol.clone(),
            tape: hello.tape,
            book_levels: hello.book_levels,
            rates: hello.rates.unwrap_or(false),
            history_paging: can_page,
        }))
        .await
        .is_err()
    {
        return ConnEnd::UiGone;
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

    // 2. Stream messages until something ends the session.
    //
    // The bridge declares what its venue prints; the configured `side_mode` is
    // a policy, `tape` is a fact about the instrument, and a fact the feed
    // cannot observe for itself must come from the side that can see it.
    let mut mapper =
        TickMapper::new(config.side_mode, hello.server_utc_offset_s).with_tape(hello.tape);
    let mut tracker = SeqTracker::new();
    let mut latency = LatencyTracker::new();
    // Whether the tape has already been reported late. Edge-triggered, so a
    // session that stays behind logs the diagnosis once instead of once per
    // sample, and its recovery is logged too — a report with no matching
    // recovery is how an operator reads "it never came back".
    let mut lag_reported = false;
    // Whether the consumer's queue has already been reported full. Same edge
    // trigger as the lag report, for the same reason.
    let mut backpressure_reported = false;
    let mut backfill: Option<Vec<Trade>> = None;
    let mut candles: Option<RatesBlock> = None;
    let mut undecodable: u64 = 0;
    let mut depth = DepthSession::new(&hello, config.symbol.clone());
    depth.log_capability();
    // The paged block being collected, if the bridge is mid-answer, and the
    // live tape's tick-rule context parked for the duration.
    let mut page: Option<PagedBlock> = None;

    // Parked here across the whole session rather than rebuilt each pass. The
    // read loop polls this once per inbound line, and a future recreated every
    // time would re-register with the `Notify` on every tick — paying a
    // synchronization cost per print for something that fires when a trader
    // clicks. Pinned once, a poll is a load and a return.
    let pending_request = config.history_pager.take_request();
    tokio::pin!(pending_request);

    let end = loop {
        // One wait covers both directions, under the bridge-liveness timeout.
        //
        // The timeout is rebuilt each pass, so a wake from *either* branch
        // restarts it: a click buys a silent bridge one more `read_timeout`
        // before it is declared lost. That is a bounded and rare extension — a
        // click is a human action, and the pager allows one outstanding at a
        // time — not an indefinite one, which is why the timeout stays out here
        // rather than being tracked against the read alone.
        let input = tokio::time::timeout(config.read_timeout, async {
            tokio::select! {
                // Biased so a busy tape cannot starve the click: this branch is
                // ready at most once per trader action, the read branch on
                // nearly every pass.
                biased;
                (count, before_utc_ms) = &mut pending_request => {
                    pending_request.set(config.history_pager.take_request());
                    SessionInput::Request { count, before_utc_ms }
                }
                // `next_line` is cancel-safe, and the reader outlives this
                // `select!` so the state it keeps is still there next pass.
                // Its awaits are all `fill_buf`, and every byte it takes from
                // the `BufReader` is appended to the partial-line buffer in the
                // same synchronous step that consumes it — including the
                // multi-`fill_buf` path a line longer than the buffer takes.
                // So a cancelled poll leaves the two exactly as consistent as
                // an uncancelled one: nothing consumed is unrecorded, and
                // nothing recorded is unconsumed.
                line = lines.next_line() => SessionInput::Line(line),
            }
        })
        .await;

        let line = match input {
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
            Ok(SessionInput::Request {
                count,
                before_utc_ms,
            }) => {
                match answer_page_request(
                    &mut outgoing,
                    &config.symbol,
                    &mapper,
                    can_page,
                    count,
                    before_utc_ms,
                )
                .await
                {
                    // Asked. The answer arrives as an ordinary block of lines
                    // on the read side, like every other thing the bridge says.
                    PageRequestOutcome::Sent => continue,
                    // Nothing to ask: this bridge cannot page. The consumer is
                    // still owed the one reply every request gets, or its
                    // spinner runs forever.
                    PageRequestOutcome::Refused => {
                        config.history_pager.settle_owed();
                        if tx
                            .send(Mt5Event::HistoryPage {
                                trades: Vec::new(),
                                exhausted: false,
                                scanned_to_utc_ms: None,
                            })
                            .await
                            .is_err()
                        {
                            break ConnEnd::UiGone;
                        }
                        continue;
                    }
                    // A socket that cannot be written to cannot be read from
                    // either; end the session and let the reconnect handle it.
                    // The owed reply is sent by the tail below, which covers
                    // every way a session can end while a page is outstanding.
                    PageRequestOutcome::WriteFailed(e) => {
                        break ConnEnd::BridgeGone(format!("socket error on write: {e}"));
                    }
                }
            }
            Ok(SessionInput::Line(Err(e))) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_SOCKET_ERROR",
                    error = %e,
                    "socket error; dropping the session"
                );
                break ConnEnd::BridgeGone(format!("socket error: {e}"));
            }
            Ok(SessionInput::Line(Ok(BoundedLine::Eof))) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BRIDGE_EOF",
                    "bridge closed the socket"
                );
                break ConnEnd::BridgeGone("eof".to_string());
            }
            Ok(SessionInput::Line(Ok(BoundedLine::TooLong))) => {
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_LINE_TOO_LONG",
                    max_bytes = MAX_LINE_BYTES as u64,
                    "peer streamed an oversized line; dropping the session"
                );
                break ConnEnd::BridgeGone("oversized line".to_string());
            }
            Ok(SessionInput::Line(Ok(BoundedLine::NotUtf8 { len }))) => {
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
            Ok(SessionInput::Line(Ok(BoundedLine::Line(line)))) => line,
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
                    // A tick belongs to whichever block is open around it. The
                    // paged block is checked first because it is the one that
                    // can open mid-session: everything else is live by default,
                    // and a paged tick delivered as live would append history to
                    // the front of the tape.
                    match (page.as_mut(), backfill.as_mut()) {
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
                            latency.observe_live(tick.time_ms, tick.sent_ms);
                            // Sampled *before* the trade is handed downstream,
                            // never after. `send_live` waits when the consumer
                            // is not draining, and a clock read on the far side
                            // of that wait charges the consumer's own queueing
                            // to the wire — collapsing the very gap the health
                            // view publishes these two figures to expose.
                            if latency.due()
                                && publish_latency(
                                    &mut latency,
                                    &mapper,
                                    &config.symbol,
                                    &mut lag_reported,
                                    tx,
                                )
                                .await
                                .is_err()
                            {
                                break ConnEnd::UiGone;
                            }
                            if send_live(tx, trade, &config.symbol, &mut backpressure_reported)
                                .await
                                .is_err()
                            {
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
                // The beat a thin tape is measured on: a symbol printing once
                // a minute never reaches the per-print sampling bound, and
                // would otherwise never report at all. The figures are still
                // the newest print's own, so a quiet stretch reports how late
                // that print was and never how long ago it was.
                if publish_latency(&mut latency, &mapper, &config.symbol, &mut lag_reported, tx)
                    .await
                    .is_err()
                {
                    break ConnEnd::UiGone;
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
            Ok(BridgeMsg::HistoryStart {
                count_hint,
                opening,
            }) => {
                if !opening && !config.history_pager.is_in_flight() {
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
                if let Some(stale) = page.take() {
                    mapper.restore_price_context(stale.resume);
                }
                // The page's ticks are hours older than the live tape; the tick
                // rule must read them against each other, not against the
                // newest price on the chart.
                page = Some(PagedBlock {
                    trades: Vec::new(),
                    resume: mapper.take_price_context(),
                    over_cap: 0,
                    opening,
                });
            }
            Ok(BridgeMsg::HistoryEnd {
                exhausted,
                scanned_to_ms,
                remaining,
            }) => {
                let Some(PagedBlock {
                    trades,
                    resume,
                    over_cap,
                    opening,
                }) = page.take()
                else {
                    // The start went missing — an undecodable line ahead of it
                    // is enough. The block's ticks have already been charted as
                    // live, which cannot be undone here; what must not also
                    // happen is the pager staying latched. Without settling,
                    // `request` refuses every later click for the rest of the
                    // session and the button silently stops working.
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_PROTOCOL_VIOLATION",
                        action = "settle_and_answer_empty",
                        "history_end without a history_start; its ticks went out as live"
                    );
                    if config.history_pager.settle_owed()
                        && tx
                            .send(Mt5Event::HistoryPage {
                                trades: Vec::new(),
                                exhausted: false,
                                scanned_to_utc_ms: None,
                            })
                            .await
                            .is_err()
                    {
                        break ConnEnd::UiGone;
                    }
                    continue;
                };
                // The live tape resumes from the price it left off at, never
                // from wherever the page ended.
                mapper.restore_price_context(resume);
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
                    if tx
                        .send(Mt5Event::OpeningPage { trades, remaining })
                        .await
                        .is_err()
                    {
                        break ConnEnd::UiGone;
                    }
                    continue;
                }
                if !config.history_pager.settle_owed() {
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_HISTORY_PAGE_UNSOLICITED",
                        trades = trades.len(),
                        action = "discard",
                        "a page nobody asked for; discarding it rather than prepending"
                    );
                    continue;
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
                let scanned_to_utc_ms = scanned_to_ms.map(|ms| mapper.to_utc_ms(ms));
                if tx
                    .send(Mt5Event::HistoryPage {
                        trades,
                        exhausted,
                        scanned_to_utc_ms,
                    })
                    .await
                    .is_err()
                {
                    break ConnEnd::UiGone;
                }
            }
            Ok(BridgeMsg::RatesStart {
                interval_ms,
                count_hint,
            }) => {
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_RATES_START",
                    interval_ms,
                    count_hint = ?count_hint,
                    "bridge is sending historical candles"
                );
                if let Some(open) = candles.take() {
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
                candles = Some(RatesBlock::new(interval_ms, hello.server_utc_offset_s));
            }
            Ok(BridgeMsg::Rate(chunk)) => match candles.as_mut() {
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
            },
            Ok(BridgeMsg::RatesEnd { partial }) => match candles.take() {
                Some(block) => {
                    let (interval_ms, bars, clipped) = block.finish(&config.symbol);
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
                            symbol = %config.symbol,
                            bars = bars.len(),
                            bridge_said_partial,
                            clipped_here = clipped,
                            "the candle block is short of what was asked for"
                        );
                    }
                    if tx
                        .send(Mt5Event::Rates {
                            interval_ms,
                            bars,
                            partial,
                        })
                        .await
                        .is_err()
                    {
                        break ConnEnd::UiGone;
                    }
                }
                None => warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_PROTOCOL_VIOLATION",
                    action = "ignore",
                    "rates_end without a rates_start; ignoring it"
                ),
            },
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
    // Every request gets exactly one reply, including the ones this session
    // died holding — whether it had been sent to the bridge or was still
    // queued. The alternative is a spinner that outlives the connection it was
    // waiting on. `abandon` also clears the request itself: it belongs to the
    // connection that is ending, and replaying it against the next session
    // would page from a cursor that one never sent.
    if config.history_pager.abandon() {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_HISTORY_PAGE_UNANSWERED",
            partial_trades = page.as_ref().map_or(0, |block| block.trades.len()),
            action = "answer_empty",
            "session ended with a page outstanding; answering it empty"
        );
        let _ = tx
            .send(Mt5Event::HistoryPage {
                trades: Vec::new(),
                exhausted: false,
                scanned_to_utc_ms: None,
            })
            .await;
    }
    if let Some(block) = candles {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_PARTIAL_RATES_DISCARDED",
            bars = block.len(),
            "session ended mid-candle-block; discarding it (the next session re-sends)"
        );
    }
    mapper.stats.log_summary(&config.symbol);
    // A consumer that was capturing depth must hear that this generation ended,
    // so it renders the discontinuity instead of connecting liquidity across it.
    depth.close(tx).await;
    end
}
