//! Binance backend for the feed bridge.
//!
//! A background thread runs a tokio runtime that first backfills recent history
//! over REST, then streams live trades (with reconnect + gap detection). Both
//! flow to the UI as [`FeedEvent`]s; the UI's [`FeedCommand`]s (e.g. "load older
//! history") are serviced between live trades.

use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::{error, info, warn};

use quantick_engine::Trade;
use quantick_feed_binance::{
    BINANCE_WS_BASE, Backoff, BinanceHttp, BinanceKlineHttp, KLINE_INTERVAL_1M, ONE_MINUTE_MS,
    agg_trade_url, backfill, backfill_before,
    depth::{
        BinanceDepthHttp, DepthEvent, DepthSessionConfig, MAX_DEPTH_LIMIT, run_depth_with_reconnect,
    },
    fetch_history, run_with_reconnect,
};

use super::{
    FeedCommand, FeedEvent, FeedHandle, FeedNotice, connection_notice, initial_backfill_target,
};
use crate::config::ProviderKind;

/// Default number of REST depth levels requested per side.
const DEFAULT_BOOK_DEPTH: u16 = 1_000;

/// Depth events are independent from the established trade channel.
const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;
const NOTICE_CHANNEL_CAPACITY: usize = 32;

/// Start the Binance feed for `symbol` on a background thread.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let symbol = symbol.to_string();
    std::thread::Builder::new()
        .name("quantick-feed".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build feed runtime");
            runtime.block_on(feed_task(symbol, tx, book_tx, notice_tx, cmd_rx));
        })
        .expect("spawn feed thread");
    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        // Every aggTrade is an execution carrying its real size, and the venue
        // answers the same for every symbol it lists — nothing to narrow later.
        capabilities: super::fixed_capabilities(ProviderKind::Binance.capabilities()),
        commands: cmd_tx,
        replay: None,
    }
}

async fn feed_task(
    symbol: String,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    notice_tx: mpsc::Sender<FeedNotice>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    let http = BinanceHttp::new();
    // Candles come off a different endpoint with a different paging rule, so
    // they get their own client rather than overloading the aggTrade one.
    let klines = BinanceKlineHttp::new();
    let target = initial_backfill_target();

    // 1. Backfill recent history so the chart opens populated. Remember the
    //    earliest agg_id so we can page further back on demand.
    let mut earliest_id: Option<u64> = None;
    match backfill(&http, &symbol, target).await {
        Ok(trades) => {
            earliest_id = trades.first().map(|t| t.agg_id);
            info!(target: "quantick::app", symbol, count = trades.len(), target, "backfill ready");
            if tx.send(FeedEvent::Backfilled(trades)).await.is_err() {
                return; // UI gone
            }
        }
        Err(e) => {
            error!(target: "quantick::app", symbol, %e, "backfill failed; continuing to live only");
            // Still mark an empty boundary so the UI knows backfill is done.
            if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
                return;
            }
        }
    }

    // 2. Stream live trades on top, reconnecting as needed. run_with_reconnect
    //    speaks Trade; the loop below tags each as a live FeedEvent and, in the
    //    same select, services UI commands between trades.
    let (live_tx, mut live_rx) = mpsc::channel::<Trade>(4096);
    let url = agg_trade_url(BINANCE_WS_BASE, &symbol);
    // Fixed seed: a single desktop client, so jitter needs no cross-client
    // decorrelation, and a fixed seed keeps behaviour reproducible.
    let backoff = Backoff::for_feed(0x9E37_79B9_7F4A_7C15);
    let (connected_tx, mut connected_rx) = watch::channel(false);
    let reconnect = tokio::spawn(async move {
        run_with_reconnect(&url, &live_tx, &connected_tx, backoff).await;
    });
    let snapshot_limit = initial_book_depth();
    let mut book_capture: Option<BookCaptureTask> = None;
    let mut ever_connected = false;
    // Candle history runs off this loop, not inside it. A week is ~11
    // sequential pages and a trader paging back through a quarter asks for
    // thirteen such runs — seconds on a good day, far longer against a venue
    // that is throttling — and awaiting that in a command arm stops `live_rx`
    // from being polled for the duration: the trade channel fills, the
    // websocket read loop behind it stalls, and pongs stop going out. The task
    // sends its result back here and the loop keeps turning meanwhile.
    let (ohlcv_tx, mut ohlcv_rx) =
        mpsc::channel::<(Vec<quantick_engine::Bar>, crate::feed::OhlcvSlice)>(1);
    let mut ohlcv_task: Option<JoinHandle<()>> = None;

    loop {
        tokio::select! {
            Some((bars, slice)) = ohlcv_rx.recv() => {
                // Only the closing slice frees the slot. A run still walking
                // backwards through the span is one fetch, however many
                // replies it makes, and letting a second one start beside it
                // would spend the same rate budget twice.
                if slice.is_last() {
                    ohlcv_task = None;
                }
                if tx
                    .send(FeedEvent::OhlcvHistory {
                        interval_ms: ONE_MINUTE_MS,
                        bars,
                        slice,
                    })
                    .await
                    .is_err()
                {
                    break; // UI gone
                }
            }
            maybe_trade = live_rx.recv() => {
                match maybe_trade {
                    Some(trade) => {
                        if tx.send(FeedEvent::Live(trade)).await.is_err() {
                            break; // UI gone
                        }
                    }
                    None => break, // stream ended
                }
            }
            changed = connected_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let notice = connection_notice(
                    *connected_rx.borrow_and_update(),
                    &mut ever_connected,
                    "Binance",
                );
                if notice_tx.send(notice).await.is_err() {
                    break;
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(FeedCommand::LoadOlder { count }) => {
                        earliest_id = load_older(&http, &symbol, earliest_id, count, &tx).await;
                        if tx.is_closed() {
                            break; // UI gone
                        }
                    }
                    Some(FeedCommand::FetchOhlcv {
                        span_ms,
                        slice_ms,
                        before_ms,
                    }) => {
                        if ohlcv_task.as_ref().is_some_and(|task| !task.is_finished()) {
                            // One fetch at a time: the venue's rate budget is
                            // shared, and the in-flight one already answers.
                            warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "BINANCE_OHLCV_ALREADY_RUNNING",
                                symbol,
                                requested_span_ms = span_ms,
                                requested_before_ms = before_ms.unwrap_or(0),
                                action = "answer_empty_and_let_the_running_one_finish",
                                "a candle fetch is already in flight; this one is refused, not queued"
                            );
                            // Refused, but *answered*. The caller marked itself
                            // pending and put its spinner up before this command
                            // left, so a silent drop leaves that spinner turning
                            // for the rest of the session and the reach-back
                            // button disabled behind it — the same reason
                            // `load_older` never returns silence either. Empty
                            // and known-short, because that is exactly what this
                            // is: nothing fetched, and not because the venue had
                            // nothing.
                            if ohlcv_tx
                                .send((Vec::new(), crate::feed::OhlcvSlice::Last { complete: false }))
                                .await
                                .is_err()
                            {
                                break; // UI gone
                            }
                        } else {
                            ohlcv_task = Some(spawn_ohlcv(
                                klines.clone(),
                                symbol.clone(),
                                span_ms,
                                slice_ms,
                                before_ms,
                                ohlcv_tx.clone(),
                            ));
                        }
                    }
                    Some(FeedCommand::SetBookCapture {
                        enabled,
                        initial_generation,
                    }) => {
                        if enabled {
                            if book_capture
                                .as_ref()
                                .is_some_and(|task| !task.handle.is_finished())
                            {
                                info!(
                                    target: "quantick::app",
                                    schema_version = 1_u8,
                                    event_code = "book_capture_enable_ignored",
                                    provider = "binance",
                                    symbol = symbol.as_str(),
                                    initial_generation,
                                    snapshot_limit,
                                    action = "keep_running",
                                    "book capture is already running"
                                );
                            } else {
                                // Reap a task that ended by itself before
                                // replacing it.
                                stop_book_capture(
                                    &mut book_capture,
                                    &symbol,
                                    "finished_before_enable",
                                )
                                .await;
                                book_capture = Some(start_book_capture(
                                    &symbol,
                                    initial_generation,
                                    snapshot_limit,
                                    &book_tx,
                                ));
                            }
                        } else {
                            stop_book_capture(&mut book_capture, &symbol, "disabled").await;
                        }
                    }
                    Some(FeedCommand::RestartBookCapture { initial_generation }) => {
                        stop_book_capture(&mut book_capture, &symbol, "restart").await;
                        book_capture = Some(start_book_capture(
                            &symbol,
                            initial_generation,
                            snapshot_limit,
                            &book_tx,
                        ));
                    }
                    // Transport commands belong to a recorded session; a live
                    // venue has no playhead to move. Ignored rather than
                    // refused — the UI only shows the transport while a replay
                    // is the source.
                    Some(FeedCommand::Replay(_)) => {}
                    None => break, // UI dropped the command sender: it's gone
                }
            }
        }
    }
    reconnect.abort();
    let _ = reconnect.await;
    // A candle fetch can be 130 requests deep when the feed goes away; nobody
    // is left to read its answer.
    if let Some(task) = ohlcv_task {
        task.abort();
    }
    stop_book_capture(&mut book_capture, &symbol, "feed_dropped").await;
}

/// A running depth capture plus the epoch assigned by its controller.
struct BookCaptureTask {
    initial_generation: u64,
    handle: JoinHandle<()>,
}

/// Start one independently cancellable depth reconnect loop.
fn start_book_capture(
    symbol: &str,
    initial_generation: u64,
    snapshot_limit: u16,
    events: &mpsc::Sender<DepthEvent>,
) -> BookCaptureTask {
    let symbol = symbol.to_string();
    let events = events.clone();
    let source = BinanceDepthHttp::new();
    let config = DepthSessionConfig {
        snapshot_limit,
        initial_generation,
        ..DepthSessionConfig::default()
    };
    // Keep retry jitter reproducible while giving replacement generations a
    // different deterministic sequence.
    let backoff = Backoff::for_feed(0xD1B5_4A32_D192_ED03 ^ initial_generation);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_started",
        provider = "binance",
        symbol = symbol.as_str(),
        initial_generation,
        snapshot_limit,
        action = "start",
        "starting synchronized Binance book capture"
    );
    let handle = tokio::spawn(async move {
        run_depth_with_reconnect(BINANCE_WS_BASE, &symbol, &source, &events, config, backoff).await;
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "book_capture_task_finished",
            provider = "binance",
            symbol = symbol.as_str(),
            initial_generation,
            snapshot_limit,
            action = "stop",
            "Binance book capture task finished"
        );
    });
    BookCaptureTask {
        initial_generation,
        handle,
    }
}

/// Abort and reap the active depth task, if any.
async fn stop_book_capture(task: &mut Option<BookCaptureTask>, symbol: &str, reason: &'static str) {
    let Some(task) = task.take() else {
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "book_capture_stop_ignored",
            provider = "binance",
            symbol,
            reason,
            action = "already_stopped",
            "book capture stop requested with no active task"
        );
        return;
    };
    let initial_generation = task.initial_generation;
    task.handle.abort();
    let join_result = task.handle.await;
    let outcome = if join_result
        .as_ref()
        .is_err_and(tokio::task::JoinError::is_cancelled)
    {
        "cancelled"
    } else if join_result.is_ok() {
        "finished"
    } else {
        "join_error"
    };
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_stopped",
        provider = "binance",
        symbol,
        initial_generation,
        reason,
        outcome,
        action = "stop",
        "Binance book capture stopped"
    );
}

/// Initial REST depth level count, configurable through
/// `QUANTICK_BOOK_DEPTH`.
fn initial_book_depth() -> u16 {
    parse_book_depth(std::env::var("QUANTICK_BOOK_DEPTH").ok().as_deref())
}

fn parse_book_depth(raw: Option<&str>) -> u16 {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.clamp(1, usize::from(MAX_DEPTH_LIMIT)) as u16)
        .unwrap_or(DEFAULT_BOOK_DEPTH)
}

/// Fetch `span_ms` of one-minute candles on a task of its own, delivering the
/// bars back to the feed loop through `reply` — in one message, or in a run of
/// them from the newest window backwards when `slice_ms` asks for slices.
///
/// Off the loop deliberately: this is the one command whose work is measured in
/// seconds rather than milliseconds, and the loop it was called from is the one
/// draining live trades. See where it is spawned for what awaiting it inline
/// used to cost.
///
/// A short answer is still an answer: the pane's loading indicator keys on the
/// closing reply, so a fetch that ran out of rate budget resolves it with what
/// it collected rather than leaving the pane waiting on a venue that has
/// stopped talking. That is also why a failed window does not abandon the run:
/// the remaining windows are still attempted and the closing slice reports the
/// whole answer as short. How much arrived is in the `BINANCE_OHLCV_*` log
/// events.
fn spawn_ohlcv(
    klines: BinanceKlineHttp,
    symbol: String,
    span_ms: i64,
    slice_ms: Option<i64>,
    before_ms: Option<i64>,
    reply: mpsc::Sender<(Vec<quantick_engine::Bar>, crate::feed::OhlcvSlice)>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The right-hand edge of everything this request covers: the live edge
        // for the opening fetch, and the millisecond before the oldest candle
        // held for a *load older*. The plan needs no other change to reach
        // further back — it always cut a span ending at a caller-supplied
        // instant, and the wall clock was only ever the instant that mattered.
        let now_ms = before_ms.unwrap_or_else(crate::metrics::wall_clock_ms);
        let windows = crate::feed::ohlcv_plan::plan(now_ms, span_ms, slice_ms);
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BINANCE_OHLCV_PLAN",
            symbol,
            requested_span_ms = span_ms,
            slice_ms = slice_ms.unwrap_or(0),
            windows = windows.len(),
            "candle history planned"
        );
        // Short windows accumulate into the answer's completeness: the trader
        // is told the span is short if *any* part of it came up short, never
        // just the last one fetched.
        let mut complete = true;
        for window in windows {
            let history = fetch_history(
                &klines,
                &symbol,
                KLINE_INTERVAL_1M,
                ONE_MINUTE_MS,
                window.from_ms,
                window.to_ms,
            )
            .await;
            complete &= history.complete;
            if !history.complete {
                warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "BINANCE_OHLCV_PARTIAL",
                    symbol,
                    requested_span_ms = span_ms,
                    from_ms = window.from_ms,
                    to_ms = window.to_ms,
                    bars = history.bars.len(),
                    action = "answer_partial",
                    "candle history is short of the requested window"
                );
            }
            let slice = if window.last {
                crate::feed::OhlcvSlice::Last { complete }
            } else {
                crate::feed::OhlcvSlice::More
            };
            // A closed channel means the feed loop is gone, which is not this
            // task's problem to report: it is already being reported there.
            // Stop fetching, though — nothing is listening for the rest.
            if reply.send((history.bars, slice)).await.is_err() {
                return;
            }
        }
    })
}

/// Fetch `count` trades older than `earliest`, send them to the UI, and return
/// the new earliest agg_id (unchanged if nothing older was available or a fetch
/// failed). `None` earliest means there is no history to page back from.
///
/// Every call answers the UI with exactly one [`FeedEvent::HistoryPrepended`] —
/// empty when nothing older exists or the fetch failed — mirroring how a failed
/// initial backfill still sends an empty [`FeedEvent::Backfilled`]. The UI keys
/// its loading indicator on that reply, so a silent no-answer would leave a
/// spinner running forever.
async fn load_older(
    http: &BinanceHttp,
    symbol: &str,
    earliest: Option<u64>,
    count: usize,
    tx: &mpsc::Sender<FeedEvent>,
) -> Option<u64> {
    let Some(before) = earliest else {
        warn!(target: "quantick::app", "load older ignored: no history to page back from");
        let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
        return None;
    };
    match backfill_before(http, symbol, before, count).await {
        Ok(older) if !older.is_empty() => {
            let new_earliest = older.first().map(|t| t.agg_id);
            info!(target: "quantick::app", symbol, count = older.len(), "older history ready");
            if tx.send(FeedEvent::HistoryPrepended(older)).await.is_err() {
                return earliest; // UI gone; caller notices via tx.is_closed()
            }
            new_earliest
        }
        Ok(_) => {
            info!(target: "quantick::app", symbol, "no older history available");
            let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
            earliest
        }
        Err(e) => {
            error!(target: "quantick::app", symbol, %e, "load older failed");
            let _ = tx.send(FeedEvent::HistoryPrepended(Vec::new())).await;
            earliest
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_depth_defaults_and_clamps_the_environment_value() {
        assert_eq!(parse_book_depth(None), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("")), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("invalid")), DEFAULT_BOOK_DEPTH);
        assert_eq!(parse_book_depth(Some("0")), 1);
        assert_eq!(parse_book_depth(Some("2500")), 2_500);
        assert_eq!(parse_book_depth(Some("999999")), MAX_DEPTH_LIMIT);
    }
}
