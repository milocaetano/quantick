//! MetaTrader 5 backend: the local QuantickBridge listener → [`FeedEvent`]s.
//!
//! MT5 has no public REST/WebSocket API, so the roles invert versus Binance:
//! `quantick-feed-mt5` listens on a local TCP port and the QuantickBridge EA
//! (running inside the logged-in terminal, see `bridge/mt5/README.md`) dials
//! out and streams ticks. No credentials exist anywhere in this path.
//!
//! Translation to the UI contract, honestly:
//!
//! - An **empty [`FeedEvent::Backfilled`] is sent immediately** — MT5 has no
//!   fetch-on-demand history, so the "initial backfill" resolves at once and
//!   the chart opens honestly empty ("connecting to WIN$N …") until the
//!   bridge connects.
//! - The bridge's backfill block (recent `CopyTicks` history) arrives as
//!   [`FeedEvent::HistoryPrepended`] — but only while no trade has been
//!   forwarded yet; after that, recovered history is forwarded as live to
//!   keep the retained stream ordered (logged as such, never silent).
//! - **Reconnect overlap is dropped, not double-counted**: the bridge
//!   re-sends its recent-history window on every session, and synthetic ids
//!   restart, so the only stable cross-session key is time. Recovered trades
//!   are forwarded only when strictly newer than the last forwarded trade;
//!   the dropped overlap count is logged. (Trades sharing the last forwarded
//!   millisecond are dropped too — losing a same-ms tick to a reconnect is
//!   honest; silently inflating bars is not.)
//! - **"Load older" works when the bridge says it does.** The transport is
//!   push-only for everything the terminal volunteers, and the one exception is
//!   this: a bridge whose hello declares `history_paging` reads its socket, so
//!   a request crosses it and a block of older ticks comes back. A bridge that
//!   does not declare it is never written to — an unread request would fill its
//!   receive buffer and, in the Expert Advisor, block the terminal thread that
//!   sends ticks. Either way the request is answered exactly once (an empty
//!   prepend when there is nothing to send), so the UI's loader always
//!   resolves.
//! - **Order-book capture is supported** when the bridge declares a Depth of
//!   Market. The terminal republishes a complete DOM image on every change and
//!   `quantick-feed-mt5` diffs those into the same snapshot-plus-delta stream
//!   Binance produces, so the heatmap runs on one shared pipeline. Capture is
//!   a switch on the running session rather than a task to start and stop: the
//!   book shares one socket with ticks, and MQL5 offers no back-channel to ask
//!   the terminal to stop sending it.
//!
//! Synthetic ids caveat: `agg_id` restarts at 1 on every bridge session, so
//! ids may repeat across reconnects within one chart lifetime. Bars are built
//! from trade order, not ids, so the chart is unaffected; anything keying on
//! `agg_id` across sessions must not, and this is the place that documents it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use quantick_feed_mt5::{
    BookCaptureSwitch, HistoryPager, Mt5Error, Mt5Event, Mt5Status, ServerConfig, SideMode,
    TapeKind, run_bridge_server,
};

use crate::config::{FeedCapabilities, MetaTraderSettings, Mt5SideSource, ProviderKind};

use super::mt5_bridge::{Supervision, supervise};
use super::{DepthEvent, FeedCommand, FeedEvent, FeedHandle, FeedNotice};

/// Depth events are independent from the established trade channel. Sized like
/// the Binance backend's: a B3 book republishes far faster than the UI drains,
/// and this absorbs bursts without either dropping deltas or stalling trades.
const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;

/// Capacity of the notice channel. Notices are rare (one per connection
/// transition) and the UI drains every frame; this only has to outlast a
/// burst of bridge output arriving while the chart is busy.
const NOTICE_CHANNEL_CAPACITY: usize = 32;

/// Gap between attempts to bind a listen port that refused to open.
///
/// A taken port is not a verdict, it is a moment: the holder is usually a tab
/// being torn down, a quantick instance being closed, or a stale process the
/// user is about to kill. Retrying is what turns "restart the tab once the
/// port frees" into "the chart comes back on its own". Local binds cost
/// microseconds, so the interval only decides how quickly recovery is noticed.
const BIND_RETRY: Duration = Duration::from_secs(2);

/// Start the MetaTrader feed for `symbol`: listen for the bridge on the
/// configured address and translate its stream into [`FeedEvent`]s.
#[must_use]
pub fn spawn(symbol: &str, settings: &MetaTraderSettings) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel::<DepthEvent>(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel::<FeedNotice>(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    // Until a bridge says hello, the provider's own answer stands: this
    // terminal usually streams an exchange contract with a book and a tape,
    // and the session narrows that the moment it knows better.
    let (caps_tx, caps_rx) = watch::channel(ProviderKind::MetaTrader.capabilities());
    let symbol = symbol.to_string();
    let settings = settings.clone();
    std::thread::Builder::new()
        .name("quantick-feed-mt5".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build feed runtime");
            runtime.block_on(feed_task(
                symbol, settings, tx, book_tx, notice_tx, caps_tx, cmd_rx,
            ));
        })
        .expect("spawn mt5 feed thread");
    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        capabilities: caps_rx,
        commands: cmd_tx,
        replay: None,
    }
}

/// What a session that just said hello can really offer.
///
/// The facts the chart cannot observe for itself: whether this symbol has a
/// book and whether it has a tape. Both come from the bridge, and both are
/// per-symbol — the same terminal serves a B3 contract with real depth and real
/// prints, and a broker CFD with neither.
///
/// Candle history is not one of them. The hello's `rates` flag is a *hint* that
/// a block is coming, not a capability: nothing here can be fetched on demand,
/// so publishing it at hello would advertise data that does not exist yet, and
/// a consumer asking on the strength of it would cache the honest empty answer
/// and never see a reason to ask again. What is published instead is
/// `has_block` — whether a block is in hand *now* — which is false at every
/// hello except a reconnect that still holds the previous session's, and rises
/// when the block lands.
fn session_capabilities(
    tape: TapeKind,
    book_levels: Option<u32>,
    has_block: bool,
    ohlcv_generation: u64,
    history_paging: bool,
) -> FeedCapabilities {
    FeedCapabilities {
        book_capture: book_levels.is_some_and(|levels| levels > 0),
        // Whether *this session's* bridge answers a request for older ticks.
        // Not a property of MetaTrader and not one of the provider: the same
        // build talks to a bridge that pages and to one that does not, and
        // guessing from the provider name would offer the trader a button that
        // silently returns nothing.
        history_paging,
        traded_volume: tape == TapeKind::Trades,
        ohlcv_history: has_block,
        // Carried across the hello rather than reset: a reconnect does not
        // un-deliver the blocks that came before it.
        ohlcv_generation,
    }
}

/// The earlier of two optional timestamps, treating `None` as "no opinion".
///
/// Not `Option::min`: that orders `None` *below* every `Some`, so folding a
/// fresh batch into an empty cursor with it yields `None` — a chart that just
/// drew its opening block would go on reporting it holds nothing, and every
/// "load older" would be refused as "nothing charted yet".
fn earlier(current: Option<i64>, candidate: Option<i64>) -> Option<i64> {
    match (current, candidate) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (held, None) => held,
        (None, fresh) => fresh,
    }
}

#[allow(clippy::too_many_arguments)]
async fn feed_task(
    symbol: String,
    settings: MetaTraderSettings,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    caps_tx: watch::Sender<FeedCapabilities>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    // Resolve the UI's initial history load immediately: there is no
    // fetch-on-demand history on MT5. Bridge history arrives as a prepend.
    if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
        return; // UI gone
    }

    // One port carries one symbol, so where this feed listens is a question
    // about *this* symbol. Resolved once here and handed to both the listener
    // and the autostarted bridge, which is what keeps the two agreeing.
    let endpoint = settings.endpoint_for(&symbol);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "MT5_ENDPOINT_RESOLVED",
        symbol = %symbol,
        listen_addr = %endpoint.listen_addr,
        from_ports_map = endpoint.from_ports_map,
        "resolved this symbol's bridge port"
    );

    let mut server_cfg = ServerConfig::new(symbol.clone());
    server_cfg.listen_addr = endpoint.listen_addr.clone();
    server_cfg.side_mode = match settings.side_source {
        Mt5SideSource::TickRule => SideMode::TickRule,
        Mt5SideSource::Flags => SideMode::Flags,
    };

    // Selecting a MetaTrader feed is one action; starting a bridge by hand
    // afterwards was the second one this removes. The supervisor holds off
    // until it is sure nobody else is already feeding us.
    let bridge_connected = Arc::new(AtomicBool::new(false));
    let autostart = settings.bridge_autostart.then(|| {
        tokio::spawn(supervise(
            settings.clone(),
            Supervision {
                symbol: symbol.clone(),
                endpoint: endpoint.clone(),
                connected: Arc::clone(&bridge_connected),
                notices: notice_tx.clone(),
            },
        ))
    });
    if !settings.bridge_autostart {
        // Nothing is coming unless the user brings it: say so rather than
        // leaving an empty chart to be interpreted.
        let _ = notice_tx
            .send(FeedNotice::working(format!(
                "waiting for a MetaTrader bridge on {}",
                endpoint.listen_addr
            )))
            .await;
    }

    // The switch lives on this side of the server task so UI commands can flip
    // depth capture without disturbing the bridge session or the trade stream.
    let book_capture = BookCaptureSwitch::new();
    server_cfg.book_capture = book_capture.clone();
    // Same arrangement for the back-channel: held here, read by whichever
    // session is running. A pager that lived inside the session would be
    // recreated on every reconnect, and the click that arrived during one would
    // have nowhere to land.
    let history_pager = HistoryPager::new();
    server_cfg.history_pager = history_pager.clone();

    let (mt5_tx, mut mt5_rx) = mpsc::channel::<Mt5Event>(4096);
    let mut server = tokio::spawn(run_bridge_server(server_cfg.clone(), mt5_tx));
    // The bind error last reported to the user, so a port that stays taken
    // produces one attention card rather than one per retry.
    let mut reported_bind_error: Option<String> = None;

    // Whether any trade reached the UI yet: the first non-empty history block
    // may be prepended only into an empty chart (see module docs).
    let mut forwarded_any = false;
    // Newest trade timestamp forwarded to the UI. Reconnect history overlaps
    // what was already streamed live; only strictly-newer trades pass.
    let mut last_forwarded_ms = i64::MIN;
    // Oldest trade timestamp forwarded to the UI: the floor a page's overlap is
    // trimmed against.
    //
    // Tracked here rather than asked of the chart because this is the only
    // place that sees every trade *before* the UI decides what to keep: a tab
    // that trimmed its retained window would otherwise page from the trim
    // point and re-fetch what it just dropped, forever. `None` until something
    // has been forwarded — there is no "older than nothing".
    let mut oldest_forwarded_ms: Option<i64> = None;
    // Where the *next* page is asked from — deliberately not the same number.
    //
    // A page can move the search hours and yield no trades at all: a pre-open
    // stretch is thousands of quote-only ticks that map to nothing, and a
    // window over a closed market holds none to begin with. Paging from the
    // oldest *trade* would re-request that identical window on every click and
    // the trader could never get past it. So this follows whichever is older,
    // the oldest trade in hand or how far the bridge said it searched.
    let mut paging_floor_ms: Option<i64> = None;
    // Whether a request is outstanding with no reply yet.
    //
    // The chart counts loads: `Tab::request_older_history` begins one for every
    // command it queues, and only a `HistoryPrepended` ends one. So every
    // command must be answered exactly once, including the ones no bridge will
    // ever serve — a session that died holding one, a listener that never came
    // back. This flag is what lets those be answered from here.
    let mut page_outstanding = false;
    // The candle block the bridge pushed, kept for whoever asks later.
    //
    // Every other provider fetches candles when the pane requests them. Nothing
    // on MetaTrader answers that: the back-channel carries one message and it is
    // for ticks, the Expert Advisor never reads its socket at all, and no bridge
    // implements a candle request — so the block arrives when the bridge decides
    // and simply does. Holding it here is what lets this provider answer the
    // same `FetchOhlcv` as the others: the request does not reach a venue, it
    // reads what already arrived.
    let mut candles: Option<OhlcvBlock> = None;
    // How many times the candle answer has changed. The boolean capability is a
    // latch — it rises with the first block and cannot fall — so an empty first
    // block would otherwise be the last word: a consumer that cached that
    // emptiness would never see another edge, and the full block from the next
    // routine reconnect would be held forever behind a pane that stopped
    // asking. Every block moves this, including a replacement.
    let mut ohlcv_generation: u64 = 0;

    loop {
        tokio::select! {
            maybe_event = mt5_rx.recv() => {
                match maybe_event {
                    Some(Mt5Event::Status(status)) => {
                        // Any status proves the listener is up: a bind failure
                        // that later repeats deserves a fresh report.
                        reported_bind_error = None;
                        // The connection's own story, told where the user is
                        // looking. A connected bridge clears whatever the
                        // startup reported; a lost one replaces it.
                        let notice = match &status {
                            Mt5Status::Connected {
                                tape,
                                book_levels,
                                history_paging,
                                ..
                            } => {
                                bridge_connected.store(true, Ordering::Relaxed);
                                // What this symbol really offers is known only
                                // now. Publishing it withdraws the affordances
                                // it cannot back — before the user clicks one.
                                //
                                // Candle history reports what is held rather
                                // than what the hello promised: on a first
                                // connection nothing is, and on a reconnect the
                                // previous session's block still is.
                                let _ = caps_tx.send(session_capabilities(
                                    *tape,
                                    *book_levels,
                                    candles.is_some(),
                                    ohlcv_generation,
                                    *history_paging,
                                ));
                                FeedNotice::Connected
                            }
                            Mt5Status::Waiting { .. } => FeedNotice::working(
                                "waiting for the MetaTrader bridge to connect",
                            ),
                            // Losing the bridge clears the flag as well as
                            // reporting it. The supervisor reads the flag as
                            // "is one feeding us *now*", so a terminal that
                            // restarts mid-session gets picked back up —
                            // without this, "reconnecting" is a promise
                            // nobody keeps.
                            Mt5Status::Lost { .. } => {
                                bridge_connected.store(false, Ordering::Relaxed);
                                // A click can land in the gap between a session
                                // clearing its pager and this status arriving:
                                // `bridge_connected` still reads true, so the
                                // request is queued against a connection that is
                                // already gone. Nothing downstream will ever
                                // answer it, and the chart counts loads — so it
                                // is answered here.
                                if page_outstanding {
                                    page_outstanding = false;
                                    warn!(
                                        target: "quantick::app",
                                        schema_version = 1_u8,
                                        event_code = "MT5_LOAD_OLDER_REFUSED",
                                        symbol = %symbol,
                                        reason = "session_lost",
                                        action = "answer_empty",
                                        "the bridge went away with a page request outstanding"
                                    );
                                    if tx
                                        .send(FeedEvent::HistoryPrepended(Vec::new()))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                FeedNotice::reconnecting(
                                    "the MetaTrader bridge disconnected — reconnecting",
                                )
                            }
                        };
                        let _ = notice_tx.send(notice).await;
                        log_status(&symbol, &status);
                    }
                    Some(Mt5Event::Backfilled(batch)) => {
                        if batch.is_empty() {
                            continue;
                        }
                        if forwarded_any {
                            // Reconnect history: forward only what the UI has
                            // not already seen. Labelled, not hidden.
                            let resent = batch.len();
                            let fresh: Vec<_> = batch
                                .into_iter()
                                .filter(|t| t.timestamp_ms > last_forwarded_ms)
                                .collect();
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_RECOVERED_HISTORY_AS_LIVE",
                                symbol = %symbol,
                                count = fresh.len(),
                                overlap_dropped = resent - fresh.len(),
                                "bridge re-sent history after a reconnect; forwarding the unseen tail as live"
                            );
                            for trade in fresh {
                                last_forwarded_ms = last_forwarded_ms.max(trade.timestamp_ms);
                                oldest_forwarded_ms =
                                    earlier(oldest_forwarded_ms, Some(trade.timestamp_ms));
                                paging_floor_ms = earlier(paging_floor_ms, oldest_forwarded_ms);
                                if tx.send(FeedEvent::Live(trade)).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            forwarded_any = true;
                            last_forwarded_ms = batch
                                .iter()
                                .map(|t| t.timestamp_ms)
                                .max()
                                .unwrap_or(last_forwarded_ms);
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_HISTORY_READY",
                                symbol = %symbol,
                                count = batch.len(),
                                "bridge history ready"
                            );
                            oldest_forwarded_ms = earlier(
                                oldest_forwarded_ms,
                                batch.iter().map(|t| t.timestamp_ms).min(),
                            );
                            paging_floor_ms = earlier(paging_floor_ms, oldest_forwarded_ms);
                            if tx.send(FeedEvent::HistoryPrepended(batch)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(Mt5Event::HistoryPage {
                        trades,
                        exhausted,
                        scanned_to_utc_ms,
                    }) => {
                        // The reply this request was owed. Cleared before
                        // anything can fail, so no later `break` leaves the
                        // flag set on a task that is ending.
                        page_outstanding = false;
                        // The answer to one click. Empty is a legitimate answer
                        // and still has to be forwarded: `HistoryPrepended` is
                        // what stops the chart's loading indicator, so an empty
                        // block swallowed here would be a spinner that never
                        // stops.
                        info!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "MT5_HISTORY_PAGE_READY",
                            symbol = %symbol,
                            count = trades.len(),
                            exhausted,
                            scanned_to_utc_ms = ?scanned_to_utc_ms,
                            "a page of older ticks is ready to prepend"
                        );
                        // Only trades strictly older than the chart's oldest
                        // pass. The bridge answers on whole-second boundaries
                        // (`copy_ticks_range` takes no finer unit), so the page
                        // can carry the far side of the cursor's own
                        // millisecond — prepending those would draw prints the
                        // chart already holds a second time.
                        let served = trades.len();
                        let floor = oldest_forwarded_ms.unwrap_or(i64::MAX);
                        let older: Vec<_> = trades
                            .into_iter()
                            .filter(|t| t.timestamp_ms < floor)
                            .collect();
                        if served != older.len() {
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_HISTORY_PAGE_OVERLAP_DROPPED",
                                symbol = %symbol,
                                dropped = served - older.len(),
                                kept = older.len(),
                                "the page overlapped what the chart already holds"
                            );
                        }
                        oldest_forwarded_ms = earlier(
                            oldest_forwarded_ms,
                            older.iter().map(|t| t.timestamp_ms).min(),
                        );
                        // The search cursor follows the *search*, so a page that
                        // crossed hours of quote-only ticks and mapped none of
                        // them still moves the next click past them. A bridge
                        // that reports nothing leaves this on the trades, which
                        // is the old behaviour and no worse than it.
                        paging_floor_ms = earlier(
                            earlier(paging_floor_ms, oldest_forwarded_ms),
                            scanned_to_utc_ms,
                        );
                        if tx.send(FeedEvent::HistoryPrepended(older)).await.is_err() {
                            break;
                        }
                        if exhausted {
                            // The terminal reached its own oldest tick for this
                            // symbol, so the button has nothing left to fetch.
                            // Withdrawing it is the same rule every other
                            // affordance follows: never offer what nothing can
                            // back. A reconnect re-publishes the capability from
                            // the fresh hello, which is right — a terminal that
                            // downloaded more history in the meantime has more
                            // to give.
                            info!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_HISTORY_EXHAUSTED",
                                symbol = %symbol,
                                action = "withdraw_paging",
                                "the terminal has no ticks older than the chart now holds"
                            );
                            caps_tx.send_modify(|caps| caps.history_paging = false);
                        }
                    }
                    Some(Mt5Event::Depth(event)) => {
                        // Backpressure rather than dropping: after the opening
                        // snapshot every event is an absolute delta, and a
                        // dropped one desynchronizes the book until the next
                        // generation. A full buffer means the UI is stalled,
                        // and the trade stream is buffered too.
                        if book_tx.send(event).await.is_err() {
                            break; // UI gone
                        }
                    }
                    Some(Mt5Event::SessionBusy {
                        peer,
                        peer_symbol,
                        diagnosis,
                        advice,
                    }) => {
                        // The refusal has always been logged; it has never been
                        // *seen*. Until now the chart went on saying "waiting
                        // for the bridge" while the answer sat in a file — and
                        // the person who needs it is the one who just attached
                        // a second EA, looking at that window.
                        warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "MT5_SESSION_BUSY",
                            symbol = %symbol,
                            peer = %peer,
                            peer_symbol = %peer_symbol.as_deref().unwrap_or("-"),
                            diagnosis,
                            action = "notify_user",
                            "another bridge was refused on this port"
                        );
                        let headline = match peer_symbol.as_deref() {
                            Some(other) => format!(
                                "another MetaTrader bridge tried to use {symbol}'s port (it streams {other})"
                            ),
                            None => format!(
                                "another MetaTrader bridge tried to use {symbol}'s port"
                            ),
                        };
                        // try_send, not send: unlike a connection transition
                        // this repeats for as long as the mistake lasts — an EA
                        // retrying on its own timer produces one per attempt.
                        // Awaiting a full channel would stall the feed itself,
                        // and losing a duplicate of a message already on screen
                        // costs nothing.
                        let _ = notice_tx.try_send(FeedNotice::attention(headline, advice));
                    }
                    Some(Mt5Event::Rates {
                        interval_ms,
                        bars,
                        partial,
                    }) => {
                        // Not forwarded on arrival: nobody may have asked yet,
                        // and an unrequested reply would resolve a load the
                        // pane never started. Held until it does ask.
                        info!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "MT5_RATES_READY",
                            symbol = %symbol,
                            interval_ms,
                            bars = bars.len(),
                            partial,
                            "bridge pushed candle history; holding it for the next request"
                        );
                        candles = Some(OhlcvBlock {
                            interval_ms,
                            bars,
                            complete: !partial,
                        });
                        // A block is in hand, and — the part the boolean cannot
                        // say — the answer just changed. A replacement block on
                        // a reconnect moves the counter even though the flag was
                        // already true, which is the only way a consumer holding
                        // an empty first block ever learns to ask again.
                        ohlcv_generation = ohlcv_generation.saturating_add(1);
                        caps_tx.send_modify(|caps| {
                            caps.ohlcv_history = true;
                            caps.ohlcv_generation = ohlcv_generation;
                        });
                    }
                    Some(Mt5Event::Live(trade)) => {
                        forwarded_any = true;
                        last_forwarded_ms = last_forwarded_ms.max(trade.timestamp_ms);
                        // A live print sets the floor only on a chart that has
                        // none: after that the oldest trade is behind, not
                        // ahead, and `min` keeps it there.
                        oldest_forwarded_ms =
                            earlier(oldest_forwarded_ms, Some(trade.timestamp_ms));
                        paging_floor_ms = earlier(paging_floor_ms, oldest_forwarded_ms);
                        if tx.send(FeedEvent::Live(trade)).await.is_err() {
                            break; // UI gone
                        }
                    }
                    None => {
                        // The server ended: a bind failure (retry it — ports
                        // free themselves when the holder goes away), a fatal
                        // crash, or shutdown. Whatever happens, UI commands
                        // keep being served so no loader hangs on a dead feed.
                        //
                        // Including the one already in the pager. A session that
                        // ends cleanly answers its own outstanding request; a
                        // task that *panicked* never reached that code, and the
                        // pager it left behind is gone with it. Either way the
                        // chart is still counting a load nobody will end.
                        if page_outstanding {
                            page_outstanding = false;
                            warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "MT5_LOAD_OLDER_REFUSED",
                                symbol = %symbol,
                                reason = "listener_ended",
                                action = "answer_empty",
                                "the bridge listener ended with a page request outstanding"
                            );
                            if tx
                                .send(FeedEvent::HistoryPrepended(Vec::new()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        match (&mut server).await {
                            Ok(Err(e)) => {
                                error!(
                                    target: "quantick::app",
                                    schema_version = 1_u8,
                                    event_code = "MT5_BIND_FAILED",
                                    symbol = %symbol,
                                    listen_addr = %endpoint.listen_addr,
                                    from_ports_map = endpoint.from_ports_map,
                                    %e,
                                    retry_in_s = BIND_RETRY.as_secs(),
                                    "MT5 bridge listener could not bind (another quantick, or \
                                     another symbol on this port?); retrying until it frees"
                                );
                                // A port already taken is the ordinary failure
                                // once several symbols stream at once, and a
                                // log line is not where the user is looking.
                                // Reported once per distinct error, not once
                                // per retry: the card would otherwise repaint
                                // every few seconds while the holder lives.
                                let message = e.to_string();
                                if reported_bind_error.as_deref() != Some(message.as_str()) {
                                    reported_bind_error = Some(message);
                                    let _ = notice_tx.send(bind_failure_notice(&symbol, &e)).await;
                                }
                                if !serve_commands_for(
                                    BIND_RETRY,
                                    &symbol,
                                    &tx,
                                    &mut cmd_rx,
                                    &book_capture,
                                    candles.as_ref(),
                                )
                                .await
                                {
                                    break; // UI gone
                                }
                                let (mt5_tx, new_rx) = mpsc::channel::<Mt5Event>(4096);
                                mt5_rx = new_rx;
                                server = tokio::spawn(run_bridge_server(server_cfg.clone(), mt5_tx));
                            }
                            Ok(Ok(())) => {
                                idle_serve_commands(&symbol, &tx, &mut cmd_rx, &book_capture, candles.as_ref()).await;
                                return;
                            }
                            Err(e) => {
                                error!(
                                    target: "quantick::app",
                                    schema_version = 1_u8,
                                    event_code = "MT5_SERVER_PANIC",
                                    symbol = %symbol,
                                    %e,
                                    "MT5 bridge listener crashed"
                                );
                                idle_serve_commands(&symbol, &tx, &mut cmd_rx, &book_capture, candles.as_ref()).await;
                                return;
                            }
                        }
                    }
                }
                if tx.is_closed() {
                    break;
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    // The one command a live session answers differently from a
                    // dead one. Everything else is stateless enough for
                    // `answer_command` to serve from anywhere, which is why the
                    // bind-retry and dead-listener paths can share it.
                    Some(FeedCommand::LoadOlder { count }) => {
                        match request_older_history(
                            &symbol,
                            count,
                            paging_floor_ms,
                            &history_pager,
                            bridge_connected.load(Ordering::Relaxed),
                            &tx,
                        )
                        .await
                        {
                            RequestOutcome::Asked => page_outstanding = true,
                            RequestOutcome::AnsweredHere => {}
                            RequestOutcome::UiGone => break,
                        }
                    }
                    Some(cmd) => {
                        if !answer_command(&symbol, cmd, &tx, &book_capture, candles.as_ref()).await {
                            break; // UI gone
                        }
                    }
                    None => break, // UI dropped the command sender: it's gone
                }
            }
        }
    }
    server.abort();
    // Dropping the supervisor's task drops the child handle, and the child was
    // spawned with kill-on-drop: a bridge quantick started never outlives the
    // feed that wanted it.
    if let Some(autostart) = autostart {
        autostart.abort();
    }
}

/// What to tell the user when this symbol's port could not be opened.
///
/// Three different things produce this, and only one of them is fixed in the
/// config file. The likeliest by far — now that tabs make opening the same
/// market twice a single click — is a second tab already charting this symbol,
/// which no `[metatrader.ports]` edit can help: one port carries one symbol,
/// so the map has nowhere left to put it. Prescribing the config edit for that
/// case would send someone to the wrong file, so all three are named and the
/// actionable one leads.
///
/// Which one it actually is cannot be decided from here — this feed knows only
/// that the bind failed, not what else the app has open. Naming the causes in
/// likelihood order is the honest version of that.
fn bind_failure_notice(symbol: &str, error: &Mt5Error) -> FeedNotice {
    let Mt5Error::Bind { addr, .. } = error;
    FeedNotice::attention(
        format!("quantick could not open the MetaTrader port for {symbol}"),
        format!(
            "Nothing can listen on {addr}. Most likely another tab in this window already \
             charts {symbol} — one port carries one symbol, so close it and this tab takes \
             over. Otherwise: another quantick instance is holding {addr}, or a different \
             symbol is mapped to that port under [metatrader.ports]. quantick keeps \
             retrying, so freeing the port is enough — the chart reconnects on its own."
        ),
    )
}

/// Answer UI commands for `duration`, then return `true`; `false` means the
/// UI went away first. This is the wait between two bind attempts: a plain
/// `sleep` here would leave a loader hanging for its length, and a loader that
/// can hang is the thing this feed promises not to have.
async fn serve_commands_for(
    duration: Duration,
    symbol: &str,
    tx: &mpsc::Sender<FeedEvent>,
    cmd_rx: &mut mpsc::Receiver<FeedCommand>,
    book_capture: &BookCaptureSwitch,
    candles: Option<&OhlcvBlock>,
) -> bool {
    let deadline = tokio::time::Instant::now() + duration;
    loop {
        tokio::select! {
            () = tokio::time::sleep_until(deadline) => return true,
            maybe_cmd = cmd_rx.recv() => match maybe_cmd {
                Some(cmd) => {
                    if !answer_command(symbol, cmd, tx, book_capture, candles).await {
                        return false;
                    }
                }
                None => return false,
            },
        }
    }
}

/// After a fatal listener error, keep answering UI commands honestly (empty
/// replies) so no loader ever spins forever on a dead feed.
async fn idle_serve_commands(
    symbol: &str,
    tx: &mpsc::Sender<FeedEvent>,
    cmd_rx: &mut mpsc::Receiver<FeedCommand>,
    book_capture: &BookCaptureSwitch,
    candles: Option<&OhlcvBlock>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        if !answer_command(symbol, cmd, tx, book_capture, candles).await {
            return;
        }
    }
}

/// The candle block a bridge session pushed, waiting to be asked for.
struct OhlcvBlock {
    interval_ms: i64,
    bars: Vec<quantick_engine::Bar>,
    /// Whether the bridge delivered the whole span it was asked for.
    complete: bool,
}

/// What became of one "load older" command.
///
/// Three outcomes and no boolean, because the caller has to tell two of them
/// apart: a request the bridge will answer later leaves a reply owed, and one
/// answered on the spot does not.
enum RequestOutcome {
    /// On the wire. The reply arrives later as an [`Mt5Event::HistoryPage`].
    Asked,
    /// Already answered from here — nothing will page, and the caller owes
    /// nothing further.
    AnsweredHere,
    /// The UI is gone; stop.
    UiGone,
}

/// Put one "load older" in front of the running bridge session.
///
/// Every command leaves here either asked or answered, never neither. The chart
/// *counts* loads — `Tab::request_older_history` begins one per queued command
/// and only a `HistoryPrepended` ends one — so a command that produced no reply
/// leaves the count permanently above zero and the "loading history…" overlay
/// on the chart for the rest of the session.
async fn request_older_history(
    symbol: &str,
    count: usize,
    paging_floor_ms: Option<i64>,
    pager: &HistoryPager,
    bridge_connected: bool,
    tx: &mpsc::Sender<FeedEvent>,
) -> RequestOutcome {
    let (before_utc_ms, refusal) = match paging_floor_ms {
        _ if !bridge_connected => (0, Some("no_bridge_session")),
        // Nothing has been charted, so there is no "older" to name. This is the
        // ordinary state of a chart between opening and the bridge's first
        // block, not an error.
        None => (0, Some("nothing_charted_yet")),
        // Bound rather than unwrapped: a cursor that silently defaulted would
        // send the bridge walking back from the end of time, and the compiler
        // is a better guarantee than the comment saying it cannot happen.
        Some(floor) => (floor, None),
    };
    // A page already in flight refuses this one, and a refused click is
    // answered like any other unservable one. It is tempting to stay silent and
    // let the outstanding reply cover both, but the chart counted two loads and
    // will only ever see one reply.
    let refusal = refusal
        .or_else(|| (!pager.request(count as u64, before_utc_ms)).then_some("already_pending"));
    if let Some(reason) = refusal {
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_LOAD_OLDER_REFUSED",
            symbol,
            requested = count,
            reason,
            action = "answer_empty",
            "cannot page older history right now; answering the request empty"
        );
        return if tx
            .send(FeedEvent::HistoryPrepended(Vec::new()))
            .await
            .is_ok()
        {
            RequestOutcome::AnsweredHere
        } else {
            RequestOutcome::UiGone
        };
    }
    RequestOutcome::Asked
}

/// Answer one UI command. Returns false when the UI is gone.
async fn answer_command(
    symbol: &str,
    cmd: FeedCommand,
    tx: &mpsc::Sender<FeedEvent>,
    book_capture: &BookCaptureSwitch,
    candles: Option<&OhlcvBlock>,
) -> bool {
    match cmd {
        // Reached only from the paths with no listener behind them — a bind
        // that keeps failing, a server that died. The live path handles this
        // command in `request_older_history`, where a bridge session exists to
        // ask. Here there is nothing to ask, and an unanswered request would
        // leave the chart's loading indicator spinning against a dead feed.
        FeedCommand::LoadOlder { count } => {
            warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_LOAD_OLDER_REFUSED",
                symbol,
                requested = count,
                reason = "no_listener",
                action = "answer_empty",
                "no bridge listener is running; answering the request empty"
            );
            tx.send(FeedEvent::HistoryPrepended(Vec::new()))
                .await
                .is_ok()
        }
        FeedCommand::SetBookCapture {
            enabled,
            initial_generation,
        } => {
            if enabled {
                book_capture.enable(initial_generation);
            } else {
                book_capture.disable();
            }
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_SET",
                symbol,
                enabled,
                initial_generation,
                action = if enabled { "publish_depth" } else { "stop_publishing_depth" },
                "book capture switched on the running bridge session"
            );
            true
        }
        FeedCommand::RestartBookCapture { initial_generation } => {
            // A fresh base generation retires whatever the previous one
            // published; the next image opens a new snapshot.
            book_capture.enable(initial_generation);
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_CAPTURE_RESTARTED",
                symbol,
                initial_generation,
                action = "resnapshot_on_next_image",
                "book capture restarted at a fresh generation"
            );
            true
        }
        FeedCommand::FetchOhlcv {
            span_ms,
            slice_ms,
            before_ms,
        } => {
            // Answered from what the bridge already pushed. `span_ms` is not a
            // request that can be made here — the bridge chose its own reach
            // before this feed could say anything (its `--rates-months`) — so
            // it is logged against what actually arrived rather than silently
            // ignored. `slice_ms` is declined for the same reason it would be
            // pointless: progressive slicing shortens a wait made of venue
            // round trips, and there are none here — the block is already in
            // memory, so cutting it into replies would only make the chart
            // rebuild itself several times over to arrive at the same frame.
            let (interval_ms, bars, complete) = match candles {
                Some(block) => (block.interval_ms, block.bars.clone(), block.complete),
                // Nothing held is not a short answer — it is no answer yet, and
                // the generation is what will say when that changes.
                None => (super::OHLCV_BASE_INTERVAL_MS, Vec::new(), true),
            };
            info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "MT5_OHLCV_ANSWERED",
                symbol,
                requested_span_ms = span_ms,
                interval_ms,
                bars = bars.len(),
                // The empty answer has two very different causes, and the log
                // has to separate them: a session that sends no candles at all,
                // and one whose block simply has not arrived yet.
                source = if candles.is_some() { "bridge_block" } else { "nothing_held" },
                complete,
                requested_slice_ms = slice_ms.unwrap_or(0),
                // A *load older* asks for candles before an instant. The
                // bridge's block is the whole of this feed's reach — it is
                // pushed, never fetched — so the answer is the same block
                // again, and the merge on the other side finds nothing new.
                // Logged rather than dropped: "asked for older and got no
                // older" is a fact about the bridge's `--rates-months`, and an
                // operator reading this needs to see the request that went
                // unmet rather than infer it.
                requested_before_ms = before_ms.unwrap_or(0),
                reach = if before_ms.is_some() { "bridge_block_is_the_whole_reach" } else { "opening_request" },
                delivery = "single_reply",
                "answered a candle-history request"
            );
            tx.send(FeedEvent::OhlcvHistory {
                interval_ms,
                bars,
                slice: super::OhlcvSlice::Last { complete },
            })
            .await
            .is_ok()
        }
        // Transport commands belong to a recorded session; a live bridge has
        // no playhead to move. Ignored rather than refused — the UI only shows
        // the transport while a replay is the source.
        FeedCommand::Replay(_) => true,
    }
}

/// Surface bridge-connection transitions in the app's log stream.
fn log_status(symbol: &str, status: &Mt5Status) {
    match status {
        Mt5Status::Waiting { addr } => info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_WAITING_FOR_BRIDGE",
            symbol,
            addr = %addr,
            "waiting for the QuantickBridge EA (see bridge/mt5/README.md)"
        ),
        Mt5Status::Connected {
            symbol: hello_symbol,
            broker_symbol,
            tape,
            book_levels,
            rates,
            history_paging,
        } => info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_STREAMING",
            symbol,
            hello_symbol = %hello_symbol,
            broker_symbol = %broker_symbol,
            // The two facts that decide what the chart may offer — right where
            // "why is the volume-bar option greyed out?" gets answered.
            tape = ?tape,
            book_levels = book_levels.unwrap_or(0),
            rates,
            // And the one that decides whether "load older" does anything.
            history_paging,
            "bridge connected and streaming"
        ),
        Mt5Status::Lost { reason } => warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "MT5_BRIDGE_LOST",
            symbol,
            reason = %reason,
            "bridge session ended; feed keeps listening"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt as _;

    async fn notice_matching(
        notices: &mut mpsc::Receiver<FeedNotice>,
        predicate: impl Fn(&FeedNotice) -> bool,
    ) -> FeedNotice {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notice = notices.recv().await.expect("notice channel closed");
                if predicate(&notice) {
                    return notice;
                }
            }
        })
        .await
        .expect("timed out waiting for feed notice")
    }

    /// Spawn the feed on an ephemeral port and return its handle.
    fn test_feed(symbol: &str) -> FeedHandle {
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:0".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        spawn(symbol, &settings)
    }

    #[test]
    fn resolves_the_initial_backfill_immediately_and_answers_commands() {
        let mut feed = test_feed("WIN$N");

        // The UI contract: exactly one Backfilled reply, straight away.
        assert!(matches!(
            feed.events.blocking_recv(),
            Some(FeedEvent::Backfilled(trades)) if trades.is_empty()
        ));

        // Unsupported commands are answered, never left hanging.
        feed.commands
            .blocking_send(FeedCommand::LoadOlder { count: 100 })
            .unwrap();
        assert!(matches!(
            feed.events.blocking_recv(),
            Some(FeedEvent::HistoryPrepended(trades)) if trades.is_empty()
        ));
        feed.commands
            .blocking_send(FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation: 1,
            })
            .unwrap();
        feed.commands
            .blocking_send(FeedCommand::RestartBookCapture {
                initial_generation: 2,
            })
            .unwrap();
        // Book channel stays open and empty.
        assert!(matches!(
            feed.book_events.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn a_bridge_session_flows_history_then_live_into_feed_events() {
        // End-to-end inside the app layer: fake bridge over real TCP. A fixed
        // high port (no other test binds one) because the bound address stays
        // internal to the feed thread.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19171".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(empty)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        assert!(empty.is_empty());

        // Give the listener a moment to bind, then connect as the bridge.
        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19171").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("could not reach the feed listener");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":-10800}\n",
            "{\"type\":\"backfill_start\",\"count_hint\":3}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"100\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"101\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"backfill_end\"}\n",
            "{\"type\":\"tick\",\"seq\":3,\"time_ms\":1002,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"100\",\"volume\":2,\"flags\":1080}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // History block: seq 1 honestly dropped (no context), seq 2 = buy.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for history")
            .expect("feed closed");
        let FeedEvent::HistoryPrepended(history) = event else {
            panic!("expected the bridge history as a prepend");
        };
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].agg_id, 2);

        // Live tick: downtick = sell.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the live trade")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live trade");
        };
        assert_eq!(trade.agg_id, 3);
        assert_eq!(trade.side, quantick_engine::Side::Sell);
    }

    #[tokio::test]
    async fn a_mapped_symbol_listens_on_its_own_port_not_the_shared_one() {
        // The whole multi-symbol feature in one assertion: the port the map
        // names is the port a bridge finds. `listen_addr` deliberately points
        // somewhere else, so nothing here can pass by falling through to it.
        let mut ports = std::collections::BTreeMap::new();
        ports.insert("XAUUSD".to_string(), 19176_u16);
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19177".to_string(),
            ports,
            side_source: Mt5SideSource::TickRule,
            // This test is the bridge; a second one racing it would make the
            // assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("XAUUSD", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19176").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("nothing is listening on this symbol's mapped port");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"XAUUSD\",\"broker_symbol\":\"XAUUSD\",\"digits\":2,",
            "\"server_utc_offset_s\":10800}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"4000.00\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"4001.00\",\"volume\":1,\"flags\":1080}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // seq 1 has no tick-rule context; seq 2 is an uptick.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the trade")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live trade off the mapped port");
        };
        assert_eq!(trade.agg_id, 2);
        assert_eq!(trade.side, quantick_engine::Side::Buy);

        // And the shared default was never bound for this symbol.
        assert!(
            tokio::net::TcpStream::connect("127.0.0.1:19177")
                .await
                .is_err(),
            "a mapped symbol must not also occupy the shared listen_addr"
        );
    }

    /// Two MetaTrader tabs, two symbols, two listeners — the shape §11 asks
    /// for and the reason `[metatrader.ports]` exists.
    ///
    /// The window opens one feed per tab, so this is what two MT5 tabs
    /// actually do: each `spawn` resolves its own symbol through
    /// `endpoint_for` and binds only that port. A bridge dialling either one
    /// reaches the tab that asked for it, and neither can steal the other's.
    #[tokio::test]
    async fn two_symbols_listen_on_two_ports_at_once() {
        let mut ports = std::collections::BTreeMap::new();
        ports.insert("XAUUSD".to_string(), 19186_u16);
        ports.insert("US500".to_string(), 19187_u16);
        let settings = MetaTraderSettings {
            // Deliberately elsewhere: nothing here may pass by falling through
            // to the shared address.
            listen_addr: "127.0.0.1:19188".to_string(),
            ports,
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridges; a real one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };

        // Two tabs' worth of feeds, alive at the same time.
        let mut gold = spawn("XAUUSD", &settings);
        let mut index = spawn("US500", &settings);
        for feed in [&mut gold, &mut index] {
            let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
                panic!("expected the immediate empty backfill");
            };
        }

        async fn dial(port: u16) -> tokio::net::TcpStream {
            for _ in 0..50 {
                if let Ok(sock) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                    return sock;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("nothing is listening on port {port}");
        }

        let mut gold_sock = dial(19186).await;
        let mut index_sock = dial(19187).await;

        fn script(symbol: &str, first: &str, second: &str) -> String {
            format!(
                "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\
                 \"symbol\":\"{symbol}\",\"broker_symbol\":\"{symbol}\",\"digits\":2,\
                 \"server_utc_offset_s\":10800}}\n\
                 {{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\
                 \"last\":\"{first}\",\"volume\":1,\"flags\":1080}}\n\
                 {{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\
                 \"last\":\"{second}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        // Distinct prices, so a trade arriving on the wrong feed is visible
        // rather than plausible.
        gold_sock
            .write_all(script("XAUUSD", "4000.00", "4001.00").as_bytes())
            .await
            .unwrap();
        gold_sock.flush().await.unwrap();
        index_sock
            .write_all(script("US500", "5000.00", "4999.00").as_bytes())
            .await
            .unwrap();
        index_sock.flush().await.unwrap();

        async fn next_trade(feed: &mut FeedHandle) -> quantick_engine::Trade {
            let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the trade")
                .expect("feed closed");
            let FeedEvent::Live(trade) = event else {
                panic!("expected a live trade");
            };
            trade
        }

        let gold_trade = next_trade(&mut gold).await;
        let index_trade = next_trade(&mut index).await;
        assert_eq!(
            gold_trade.price,
            rust_decimal::Decimal::new(400_100, 2),
            "the gold tab's port carries the gold bridge's prints"
        );
        assert_eq!(gold_trade.side, quantick_engine::Side::Buy, "an uptick");
        assert_eq!(
            index_trade.price,
            rust_decimal::Decimal::new(499_900, 2),
            "and the index tab's port its own"
        );
        assert_eq!(
            index_trade.side,
            quantick_engine::Side::Sell,
            "a downtick, so the two streams cannot have been crossed"
        );

        // Neither took the shared address on the way.
        assert!(
            tokio::net::TcpStream::connect("127.0.0.1:19188")
                .await
                .is_err(),
            "mapped symbols must leave the shared listen_addr free"
        );
    }

    #[test]
    fn a_port_that_will_not_open_names_the_port_and_the_way_out() {
        // The failure multi-symbol charting made ordinary: two feeds asking
        // for one port. It used to reach a log line only, leaving the second
        // chart blank with nothing on screen to act on.
        let FeedNotice::Attention {
            headline,
            next_step,
        } = bind_failure_notice(
            "US500",
            &Mt5Error::Bind {
                addr: "127.0.0.1:9102".to_string(),
                message: "address already in use".to_string(),
            },
        )
        else {
            panic!("a dead listener must ask for attention");
        };
        assert!(headline.contains("US500"), "headline: {headline}");
        assert!(next_step.contains("127.0.0.1:9102"), "step: {next_step}");
        // All three causes are named, because this feed cannot tell which one
        // it is — and the one a config edit fixes is the least likely.
        assert!(
            next_step.contains("another tab") && next_step.contains("US500"),
            "the likeliest cause leads, and it names the symbol: {next_step}"
        );
        assert!(
            next_step.contains("another quantick"),
            "a second instance is a cause too: {next_step}"
        );
        assert!(
            next_step.contains("[metatrader.ports]"),
            "and so is a mapped collision: {next_step}"
        );
        assert!(
            next_step.contains("keeps retrying"),
            "the user must know freeing the port is enough: {next_step}"
        );
    }

    /// The failure behind "I picked the new contract and it never connects":
    /// the port is briefly held — an old tab tearing down, a stale instance —
    /// and the feed used to give up on it forever. Now it retries until the
    /// port frees and the chart comes back with no user action at all.
    #[tokio::test]
    async fn a_taken_port_is_retried_until_it_frees() {
        // Hold the port before the feed spawns, so its first bind loses.
        let holder = tokio::net::TcpListener::bind("127.0.0.1:19191")
            .await
            .expect("hold the port");
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19191".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        // The failure reaches the user, and names the way out.
        let notice = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Attention { .. })
        })
        .await;
        let FeedNotice::Attention { next_step, .. } = notice else {
            unreachable!("the predicate matched an attention notice");
        };
        assert!(next_step.contains("127.0.0.1:19191"), "step: {next_step}");

        // A dead feed still answers commands while the port is taken.
        feed.commands
            .send(FeedCommand::LoadOlder { count: 10 })
            .await
            .expect("the feed is listening");
        let answered = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::HistoryPrepended(trades)) => return trades,
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("a loader would hang here");
        assert!(answered.is_empty());

        // Free the port. Nothing else is done: no restart, no new tab.
        drop(holder);

        // The retry binds, and a bridge connects as if nothing happened.
        let mut sock = None;
        for _ in 0..100 {
            match tokio::net::TcpStream::connect("127.0.0.1:19191").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        let mut sock = sock.expect("the feed never rebound the freed port");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINV26\",\"digits\":0,",
            "\"server_utc_offset_s\":-10800}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"170000\",\"volume\":1,\"flags\":1080}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"170005\",\"volume\":1,\"flags\":1080}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        let event = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::Live(trade)) => return trade,
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("timed out: the freed port never streamed");
        assert_eq!(event.agg_id, 2);
    }

    #[test]
    fn a_session_reports_exactly_what_its_symbol_offers() {
        // An exchange contract on the Python bridge: prints trades, publishes
        // a book, sends candles, answers "load older".
        let exchange = session_capabilities(TapeKind::Trades, Some(10), true, 0, true);
        assert!(exchange.traded_volume);
        assert!(exchange.book_capture);
        assert!(exchange.ohlcv_history);
        assert!(exchange.history_paging);

        // A broker-quoted CFD behind the Expert Advisor: none of the four.
        // Every fact comes from the same hello, and none is inferable from the
        // provider being MetaTrader.
        let cfd = session_capabilities(TapeKind::Quotes, None, false, 0, false);
        assert!(!cfd.traded_volume);
        assert!(!cfd.book_capture);
        assert!(!cfd.ohlcv_history);
        assert!(!cfd.history_paging);

        // A bridge that subscribed to a DOM with no levels in it has no book
        // either — "declared" is not "has".
        assert!(!session_capabilities(TapeKind::Trades, Some(0), true, 0, true).book_capture);

        // The four are independent: a CFD the Python bridge serves still has
        // candles and still pages, because the terminal keeps rates and ticks
        // for quoted symbols too.
        let quoted = session_capabilities(TapeKind::Quotes, None, true, 0, true);
        assert!(quoted.ohlcv_history);
        assert!(quoted.history_paging);

        // And paging follows the bridge, not the venue: the same exchange
        // contract behind a bridge too old to read its socket offers no button.
        assert!(!session_capabilities(TapeKind::Trades, Some(10), true, 0, false).history_paging);
    }

    #[tokio::test]
    async fn a_quote_only_venue_withdraws_what_it_cannot_back() {
        // The Tickmill US500 case end to end: the bridge declares a venue that
        // prints nothing, and the chart's volume-based affordances have to go
        // dark before the user clicks one — while quotes still chart.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19173".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("US500", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        // Until a bridge says otherwise, the provider's optimistic answer holds.
        assert!(
            feed.capabilities.borrow().traded_volume,
            "the terminal usually streams a real tape; only this session says otherwise"
        );

        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19173").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("could not reach the feed listener");

        // Real US500 shape: quotes only, last "0.00", volume 0, flags BID|ASK,
        // and no book_levels at all.
        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"US500\",\"broker_symbol\":\"US500\",\"digits\":2,",
            "\"server_utc_offset_s\":10800,\"tape\":\"quotes\"}\n",
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1000,\"bid\":\"7447.81\",\"ask\":\"7448.11\",\"last\":\"0.00\",\"volume\":0,\"flags\":6}\n",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1001,\"bid\":\"7447.82\",\"ask\":\"7448.12\",\"last\":\"0.00\",\"volume\":0,\"flags\":6}\n",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // The quote charts: seq 1 has no tick-rule context, seq 2 is a buy at
        // the mid, one unit.
        let event = tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
            .await
            .expect("timed out waiting for the live print")
            .expect("feed closed");
        let FeedEvent::Live(trade) = event else {
            panic!("expected a live synthetic print");
        };
        assert_eq!(trade.agg_id, 2);
        assert_eq!(
            trade.price,
            rust_decimal::Decimal::from_str_exact("7447.97").unwrap()
        );
        assert_eq!(trade.quantity, rust_decimal::Decimal::ONE);

        // And the capability narrowed, so the toolbar's volume/dollar bars,
        // bubbles and heatmap disable themselves this frame.
        let caps = *feed.capabilities.borrow();
        assert!(!caps.traded_volume, "nothing here was ever traded");
        assert!(!caps.book_capture, "this symbol publishes no book");
    }

    /// One NDJSON line the feed wrote back to the fake bridge.
    async fn read_request(sock: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt as _;
        let mut line = Vec::new();
        let mut byte = [0_u8; 1];
        loop {
            let read = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut byte))
                .await
                .expect("timed out waiting for the feed's request")
                .expect("socket error reading the request");
            assert_eq!(read, 1, "the feed closed the socket mid-request");
            if byte[0] == b'\n' {
                return String::from_utf8(line).expect("the feed writes UTF-8");
            }
            line.push(byte[0]);
        }
    }

    #[tokio::test]
    async fn load_older_pages_back_from_the_chart_s_oldest_trade() {
        // End to end at the app layer: the click leaves as a request carrying
        // the chart's own floor, the block comes back as a prepend, and the
        // floor moves so the *next* click reaches further rather than asking
        // for the same window again.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19193".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                match tokio::net::TcpStream::connect("127.0.0.1:19193").await {
                    Ok(s) => return s,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
            panic!("could not reach the feed listener");
        }

        fn tick(seq: u64, time_ms: i64, last: &str) -> String {
            format!(
                "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\
                 \"ask\":\"0\",\"last\":\"{last}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":0,\"history_paging\":true}\n",
        );

        // Offset 0 keeps the arithmetic out of the way: this test is about the
        // cursor, and `bridge_server.rs` already pins the clock conversion.
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\",\"count_hint\":3}\n");
        script.push_str(&tick(1, 5_000, "100"));
        script.push_str(&tick(2, 5_001, "101"));
        script.push_str(&tick(3, 5_002, "102"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        let Some(FeedEvent::HistoryPrepended(opening)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the opening block")
        else {
            panic!("expected the opening block as a prepend");
        };
        assert_eq!(opening.len(), 2, "the first tick has no side context");
        let capabilities = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Connected)
        })
        .await;
        assert_eq!(capabilities, FeedNotice::Connected);
        assert!(
            feed.capabilities.borrow().history_paging,
            "the session declared it, so the button is live"
        );

        // The trader clicks. The request must name 5_001 — the oldest trade the
        // chart actually holds — not 5_000, whose tick the mapper dropped.
        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        let request = read_request(&mut sock).await;
        assert_eq!(
            request, "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":5001}",
            "the cursor is the chart's oldest trade, not its oldest tick"
        );

        // The bridge answers with a window that overlaps the cursor's own
        // millisecond — `copy_ticks_range` takes whole seconds, so this is the
        // ordinary case, not a broken bridge.
        let mut script = String::from("{\"type\":\"history_start\",\"count_hint\":3}\n");
        script.push_str(&tick(4, 4_000, "98"));
        script.push_str(&tick(5, 4_001, "99"));
        script.push_str(&tick(6, 5_001, "101"));
        script.push_str("{\"type\":\"history_end\",\"exhausted\":false}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        let Some(FeedEvent::HistoryPrepended(page)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the page")
        else {
            panic!("expected the page as a prepend");
        };
        assert_eq!(
            page.len(),
            1,
            "one tick had no side context and one repeated the cursor's millisecond"
        );
        assert_eq!(page[0].timestamp_ms, 4_001);

        // Clicking again reaches past the page, not back into it.
        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        let request = read_request(&mut sock).await;
        assert_eq!(
            request, "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":4001}",
            "the floor moved with the page"
        );

        // This time the terminal reports it has nothing older. The button must
        // stop offering what nothing can back — the same rule the heatmap and
        // the volume affordances follow.
        assert!(feed.capabilities.borrow().history_paging);
        sock.write_all(
            b"{\"type\":\"history_start\"}\n{\"type\":\"history_end\",\"exhausted\":true}\n",
        )
        .await
        .unwrap();
        sock.flush().await.unwrap();
        let Some(FeedEvent::HistoryPrepended(last)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the last page")
        else {
            panic!("an empty page is still a page");
        };
        assert!(last.is_empty());
        for _ in 0..50 {
            if !feed.capabilities.borrow().history_paging {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            !feed.capabilities.borrow().history_paging,
            "the end of the tape withdraws the button"
        );
    }

    #[tokio::test]
    async fn every_load_older_is_answered_exactly_once() {
        // The chart *counts* loads: `Tab::request_older_history` begins one for
        // every command it queues, and only a `HistoryPrepended` ends one. A
        // click the pager drops — because one page is already being walked —
        // must therefore still be answered, or the count never returns to zero
        // and "loading history…" stays on the chart for the rest of the
        // session.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19195".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                match tokio::net::TcpStream::connect("127.0.0.1:19195").await {
                    Ok(s) => return s,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
            panic!("could not reach the feed listener");
        }

        fn tick(seq: u64, time_ms: i64, last: &str) -> String {
            format!(
                "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\
                 \"ask\":\"0\",\"last\":\"{last}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":0,\"history_paging\":true}\n",
        );

        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\"}\n");
        script.push_str(&tick(1, 5_000, "100"));
        script.push_str(&tick(2, 5_001, "101"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
        let Some(FeedEvent::HistoryPrepended(_)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the opening block")
        else {
            panic!("expected the opening block");
        };

        // Two clicks, back to back. The first goes out; the second cannot,
        // because the bridge has not answered yet.
        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        let _first = read_request(&mut sock).await;
        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();

        // The dropped click is answered from the app layer, immediately.
        let Some(FeedEvent::HistoryPrepended(dropped)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("the dropped click was never answered: the spinner would never stop")
        else {
            panic!("expected an empty prepend for the dropped click");
        };
        assert!(dropped.is_empty());

        // And the real page still arrives, so the count reaches zero rather
        // than going negative or stalling at one.
        let mut script = String::from("{\"type\":\"history_start\"}\n");
        script.push_str(&tick(3, 4_000, "99"));
        script.push_str(&tick(4, 4_001, "98"));
        script.push_str("{\"type\":\"history_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
        let Some(FeedEvent::HistoryPrepended(page)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the page")
        else {
            panic!("expected the page");
        };
        assert_eq!(page.len(), 1, "the page's first print has no predecessor");
    }

    #[tokio::test]
    async fn a_page_that_maps_to_nothing_still_moves_the_next_request() {
        // Pre-open on WIN$N is thousands of quote-only ticks: the bridge walks
        // hours and the mapper produces no trades at all. Paging from the
        // oldest *trade* would re-request the identical window on every click
        // and the trader could never get past it, so the bridge reports how far
        // it searched and the cursor follows that.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19196".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                match tokio::net::TcpStream::connect("127.0.0.1:19196").await {
                    Ok(s) => return s,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
            panic!("could not reach the feed listener");
        }

        fn tick(seq: u64, time_ms: i64, last: &str) -> String {
            format!(
                "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\
                 \"ask\":\"0\",\"last\":\"{last}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":0,\"history_paging\":true}\n",
        );

        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\"}\n");
        script.push_str(&tick(1, 9_000, "100"));
        script.push_str(&tick(2, 9_001, "101"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
        let Some(FeedEvent::HistoryPrepended(_)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the opening block")
        else {
            panic!("expected the opening block");
        };

        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        assert_eq!(
            read_request(&mut sock).await,
            "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":9001}"
        );

        // An empty page — the bridge searched back to 3 000 and mapped nothing.
        sock.write_all(
            b"{\"type\":\"history_start\"}\n\
              {\"type\":\"history_end\",\"exhausted\":false,\"scanned_to_ms\":3000}\n",
        )
        .await
        .unwrap();
        sock.flush().await.unwrap();
        let Some(FeedEvent::HistoryPrepended(empty)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the empty page")
        else {
            panic!("expected the empty page");
        };
        assert!(empty.is_empty());

        // The next click must reach past the searched stretch, not into it.
        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        assert_eq!(
            read_request(&mut sock).await,
            "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":3000}",
            "an empty page that searched hours must still advance the cursor"
        );
    }

    #[tokio::test]
    async fn load_older_answers_even_when_no_bridge_can_serve_it() {
        // The chart starts a loading indicator the moment it asks, and only a
        // prepend stops it. Nothing is connected here, so the reply has to come
        // from the app layer or the spinner runs forever.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19194".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        assert!(
            !feed.capabilities.borrow().history_paging,
            "nothing has connected, so nothing may promise history"
        );

        feed.commands
            .send(FeedCommand::LoadOlder { count: 2_000 })
            .await
            .unwrap();
        let Some(FeedEvent::HistoryPrepended(empty)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out: the request went unanswered")
        else {
            panic!("expected an empty prepend");
        };
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn reconnect_history_overlap_is_dropped_not_double_counted() {
        // The bridge re-sends its recent-history window on every session; the
        // overlap with trades already forwarded must not inflate the bars.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19172".to_string(),
            side_source: Mt5SideSource::TickRule,
            // These tests are the bridge; a second one racing them would make
            // the assertions depend on whether python happens to be installed.
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                match tokio::net::TcpStream::connect("127.0.0.1:19172").await {
                    Ok(s) => return s,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
            panic!("could not reach the feed listener");
        }

        fn tick(seq: u64, time_ms: i64, last: &str) -> String {
            format!(
                "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\
                 \"ask\":\"0\",\"last\":\"{last}\",\"volume\":1,\"flags\":1080}}\n"
            )
        }
        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"server_utc_offset_s\":-10800}\n",
        );

        // Session 1: history (1000, 1001) then a live tick at 1002.
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\",\"count_hint\":2}\n");
        script.push_str(&tick(1, 1000, "100"));
        script.push_str(&tick(2, 1001, "101"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        script.push_str(&tick(3, 1002, "100"));
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        let Some(FeedEvent::HistoryPrepended(history)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for history")
        else {
            panic!("expected the bridge history as a prepend");
        };
        assert_eq!(history.len(), 1);
        let Some(FeedEvent::Live(live)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the live trade")
        else {
            panic!("expected the live trade");
        };
        assert_eq!(live.timestamp_ms, 1002 + 10_800_000);
        let connected = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Connected)
        })
        .await;
        assert_eq!(connected, FeedNotice::Connected);

        // Session 2 (reconnect): the re-sent window overlaps everything the
        // UI already has, plus one genuinely new tick at 1003.
        drop(sock);
        let reconnecting = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Reconnecting { headline } if headline.contains("disconnected"))
        })
        .await;
        assert!(matches!(reconnecting, FeedNotice::Reconnecting { .. }));
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str("{\"type\":\"backfill_start\",\"count_hint\":4}\n");
        script.push_str(&tick(1, 1000, "100"));
        script.push_str(&tick(2, 1001, "101"));
        script.push_str(&tick(3, 1002, "100"));
        script.push_str(&tick(4, 1003, "102"));
        script.push_str("{\"type\":\"backfill_end\"}\n");
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // Only the unseen tail arrives; the overlap is dropped, not replayed.
        let Some(FeedEvent::Live(fresh)) =
            tokio::time::timeout(Duration::from_secs(5), feed.events.recv())
                .await
                .expect("timed out waiting for the post-reconnect trade")
        else {
            panic!("expected the unseen tail as a live trade");
        };
        assert_eq!(fresh.timestamp_ms, 1003 + 10_800_000);
        assert_eq!(fresh.agg_id, 4);
        let connected_again = notice_matching(&mut feed.notices, |notice| {
            matches!(notice, FeedNotice::Connected)
        })
        .await;
        assert_eq!(connected_again, FeedNotice::Connected);
    }

    #[tokio::test]
    async fn the_candle_capability_rises_only_when_a_block_is_in_hand() {
        // The regression this exists for: MetaTrader used to publish
        // ohlcv_history=true from the moment it spawned. A consumer asked on
        // the first frame, got the honest empty answer, cached it — and then
        // saw no rising edge for the rest of the session, because the flag had
        // been true all along. The block arrived and was held forever.
        //
        // So the flag means "a block is in hand", and the sequence below is
        // exactly the one that used to fail: ask early, get nothing, watch the
        // flag rise when the block lands, ask again, get the bars.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19181".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };
        assert!(
            !feed.capabilities.borrow().ohlcv_history,
            "nothing has been pushed yet, so nothing may be advertised"
        );

        // Ask before anything exists, as a pane's first frame does.
        feed.commands
            .send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                slice_ms: None,
                before_ms: None,
            })
            .await
            .expect("the feed is listening");
        let early = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::OhlcvHistory { bars, .. }) => return bars,
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("the early request must still be answered");
        assert!(early.is_empty(), "there was nothing to answer with");

        let mut sock = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect("127.0.0.1:19181").await {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut sock = sock.expect("could not reach the feed listener");

        let script = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"rates\":true,\"server_utc_offset_s\":-10800}
",
            "{\"type\":\"rates_start\",\"interval_ms\":60000,\"count_hint\":2}
",
            "{\"type\":\"rate\",\"bars\":[",
            "[1784824260000,\"177790\",\"177850\",\"177780\",\"177800\",\"10\"],",
            "[1784824320000,\"177800\",\"177860\",\"177790\",\"177850\",\"20\"]]}
",
            "{\"type\":\"rates_end\"}
",
            // Two live ticks after the block. The feed drains one channel in
            // order, so a trade arriving proves the candles ahead of it were
            // already absorbed — no sleep, no guess about timing.
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1784824400000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"177800\",\"volume\":1,\"flags\":1080}
",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1784824400001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"177805\",\"volume\":1,\"flags\":1080}
",
        );
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();

        // Wait for the trade that follows the block: it is the ordering proof
        // that the candles have been absorbed and held.
        let live = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::Live(trade)) => return trade,
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("timed out waiting for the trade after the candle block");
        assert_eq!(live.agg_id, 2);

        // The edge a consumer watches for: false while the early request was
        // answered empty, true now that the block is held. Without it there is
        // no second chance — nothing else tells anyone the answer changed.
        assert!(
            feed.capabilities.borrow().ohlcv_history,
            "the block is in hand, so the capability must have risen"
        );

        // Ask again on the strength of that edge — and ask for slices, which
        // a bridge serving from a block it already holds declines: there is
        // no venue round trip here for slicing to shorten.
        feed.commands
            .send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                slice_ms: Some(crate::feed::OHLCV_SLICE_SPAN_MS),
                before_ms: None,
            })
            .await
            .expect("the feed is listening");

        let reply = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::OhlcvHistory {
                        interval_ms,
                        bars,
                        slice,
                    }) => {
                        return (interval_ms, bars, slice);
                    }
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("no reply: a pane would hang here");

        let (interval_ms, bars, slice) = reply;
        assert_eq!(
            slice,
            crate::feed::OhlcvSlice::Last { complete: true },
            "a held block is answered once, whatever slicing was asked for"
        );
        assert_eq!(interval_ms, 60_000);
        assert_eq!(bars.len(), 2);
        // Converted out of the terminal's server clock, like every other MT5
        // timestamp.
        assert_eq!(bars[0].open_time, 1_784_824_260_000 + 10_800_000);
        assert_eq!(bars[0].close_time, bars[0].open_time + 59_999);
        assert_eq!(bars[1].open_time, bars[0].open_time + 60_000);
        assert_eq!(bars[0].volume(), rust_decimal::Decimal::from(10));
        assert_eq!(
            bars[0].delta(),
            rust_decimal::Decimal::ZERO,
            "no aggressor split exists in a MetaTrader candle"
        );
    }

    #[tokio::test]
    async fn a_session_without_candles_still_answers_the_request() {
        // The Expert Advisor's case. Nothing is coming, and the request must
        // still resolve — an unanswered one is a pane that spins forever.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19182".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        feed.commands
            .send(FeedCommand::FetchOhlcv {
                span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                slice_ms: None,
                before_ms: None,
            })
            .await
            .expect("the feed is listening");

        let reply = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.events.recv().await {
                    Some(FeedEvent::OhlcvHistory { bars, .. }) => return bars,
                    Some(_) => {}
                    None => panic!("feed closed"),
                }
            }
        })
        .await
        .expect("no reply even with no bridge: a pane would hang here");
        assert!(
            reply.is_empty(),
            "nothing was pushed, so nothing is claimed"
        );
    }

    #[tokio::test]
    async fn an_empty_first_block_does_not_bury_the_one_that_follows() {
        // The latch bug. `ohlcv_history` only ever rises, so a first block that
        // arrived EMPTY — a cold terminal, a paging failure — used to be the
        // last word: the consumer cached that emptiness, watched for a rising
        // edge that could never come again, and the full block from the next
        // routine reconnect was held for the life of the process behind a pane
        // that had stopped asking.
        //
        // The generation is what has no ceiling. This also covers the empty
        // rates_end path, which until now no test exercised at all.
        let settings = MetaTraderSettings {
            listen_addr: "127.0.0.1:19183".to_string(),
            side_source: Mt5SideSource::TickRule,
            bridge_autostart: false,
            ..MetaTraderSettings::default()
        };
        let mut feed = spawn("WIN$N", &settings);
        let Some(FeedEvent::Backfilled(_)) = feed.events.recv().await else {
            panic!("expected the immediate empty backfill");
        };

        async fn connect() -> tokio::net::TcpStream {
            for _ in 0..50 {
                if let Ok(sock) = tokio::net::TcpStream::connect("127.0.0.1:19183").await {
                    return sock;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("could not reach the feed listener");
        }

        const HELLO: &str = concat!(
            "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",",
            "\"symbol\":\"WIN$N\",\"broker_symbol\":\"WINQ26\",\"digits\":0,",
            "\"rates\":true,\"server_utc_offset_s\":-10800}
",
        );
        // A tick after each block: the feed drains one channel in order, so a
        // trade arriving proves what preceded it was absorbed.
        const TICKS: &str = concat!(
            "{\"type\":\"tick\",\"seq\":1,\"time_ms\":1784824400000,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"177800\",\"volume\":1,\"flags\":1080}
",
            "{\"type\":\"tick\",\"seq\":2,\"time_ms\":1784824400001,\"bid\":\"0\",\"ask\":\"0\",\"last\":\"177805\",\"volume\":1,\"flags\":1080}
",
        );

        async fn next_live(feed: &mut FeedHandle) {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match feed.events.recv().await {
                        Some(FeedEvent::Live(_)) => return,
                        Some(_) => {}
                        None => panic!("feed closed"),
                    }
                }
            })
            .await
            .expect("timed out waiting for the ordering tick");
        }

        async fn ask(feed: &mut FeedHandle) -> Vec<quantick_engine::Bar> {
            feed.commands
                .send(FeedCommand::FetchOhlcv {
                    span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
                    slice_ms: None,
                    before_ms: None,
                })
                .await
                .expect("the feed is listening");
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match feed.events.recv().await {
                        Some(FeedEvent::OhlcvHistory { bars, .. }) => return bars,
                        Some(_) => {}
                        None => panic!("feed closed"),
                    }
                }
            })
            .await
            .expect("every request must be answered")
        }

        // Session 1: the bridge promises candles and delivers an empty block —
        // exactly what a cold terminal or a failed paging walk produces.
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str(
            "{\"type\":\"rates_start\",\"interval_ms\":60000,\"count_hint\":0}
",
        );
        script.push_str(
            "{\"type\":\"rates_end\"}
",
        );
        script.push_str(TICKS);
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
        next_live(&mut feed).await;

        let first_generation = feed.capabilities.borrow().ohlcv_generation;
        assert!(
            feed.capabilities.borrow().ohlcv_history,
            "a block arrived, even an empty one"
        );
        assert_eq!(first_generation, 1, "the answer changed once");
        assert!(
            ask(&mut feed).await.is_empty(),
            "the block really was empty"
        );

        // Session 2: the routine reconnect, this time with real candles.
        //
        // Wait for the feed to notice session 1 died before dialing again. A
        // reconnect that beats the teardown is refused as busy — correctly, and
        // that is a different test's subject.
        drop(sock);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                match feed.notices.recv().await {
                    Some(FeedNotice::Reconnecting { .. }) => return,
                    Some(_) => {}
                    None => panic!("notice channel closed"),
                }
            }
        })
        .await
        .expect("the feed must report the lost session");
        let mut sock = connect().await;
        let mut script = String::from(HELLO);
        script.push_str(
            "{\"type\":\"rates_start\",\"interval_ms\":60000,\"count_hint\":2}
",
        );
        script.push_str(
            "{\"type\":\"rate\",\"bars\":[[1784824260000,\"177790\",\"177850\",\"177780\",\"177800\",\"10\"],             [1784824320000,\"177800\",\"177860\",\"177790\",\"177850\",\"20\"]]}
",
        );
        script.push_str(
            "{\"type\":\"rates_end\"}
",
        );
        script.push_str(TICKS);
        sock.write_all(script.as_bytes()).await.unwrap();
        sock.flush().await.unwrap();
        next_live(&mut feed).await;

        // `ohlcv_history` was already true and stayed true — there is no second
        // rising edge to see. The generation is the only thing that moved, and
        // it is what tells a consumer holding an empty block to ask again.
        assert!(feed.capabilities.borrow().ohlcv_history);
        assert!(
            feed.capabilities.borrow().ohlcv_generation > first_generation,
            "the answer changed again; without this the full block is unreachable"
        );
        assert_eq!(ask(&mut feed).await.len(), 2, "and now it is the real one");
    }
}
