//! Hyperliquid backend for the provider-neutral app feed bridge.
//!
//! A reconnecting WebSocket starts with the venue's recovery batch and then
//! carries factual aggressor-side prints. Preserving each venue batch through
//! the app bridge avoids per-trade channel overhead. A separately cancellable
//! `l2Book` connection publishes the visible 20-level book through the same
//! neutral [`DepthEvent`] channel Binance and MetaTrader use.

use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tracing::{info, warn};

use quantick_engine::Trade;
use quantick_feed_hyperliquid::{
    Backoff, CANDLE_INTERVAL_1M, HYPERLIQUID_WS_URL, ONE_MINUTE_MS, TradeMapper,
    depth::{DepthEvent, HYPERLIQUID_LEVELS_PER_SIDE, run_depth_with_reconnect},
    fetch_candle_history, run_trades_with_reconnect,
};

use super::{FeedCommand, FeedEvent, FeedHandle, FeedNotice, connection_notice};
use crate::config::ProviderKind;

const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;
const FEED_EVENT_CHANNEL_CAPACITY: usize = 4_096;
const TRADE_BATCH_CHANNEL_CAPACITY: usize = 1_024;
const COMMAND_CHANNEL_CAPACITY: usize = 16;
const NOTICE_CHANNEL_CAPACITY: usize = 32;
const FEED_RUNTIME_WORKERS: usize = 2;
const STARTUP_RECOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const TRADE_RECONNECT_SEED: u64 = 0x4859_5045_525F_5452;
const DEPTH_RECONNECT_SEED: u64 = 0x4859_5045_525F_4C32;

/// Start the selected Hyperliquid perpetual on a background runtime.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(FEED_EVENT_CHANNEL_CAPACITY);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
    let (notice_tx, notice_rx) = mpsc::channel(NOTICE_CHANNEL_CAPACITY);
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    let symbol = symbol.to_owned();
    std::thread::Builder::new()
        .name("quantick-hyperliquid-feed".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(FEED_RUNTIME_WORKERS)
                .enable_all()
                .build()
                .expect("build Hyperliquid feed runtime");
            runtime.block_on(feed_task(symbol, tx, book_tx, notice_tx, cmd_rx));
        })
        .expect("spawn Hyperliquid feed thread");

    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: notice_rx,
        capabilities: super::fixed_capabilities(ProviderKind::Hyperliquid.capabilities()),
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
    let symbol = symbol.to_uppercase();
    let mapper = TradeMapper::new(&symbol);
    let (live_tx, mut live_rx) = mpsc::channel::<Vec<Trade>>(TRADE_BATCH_CHANNEL_CAPACITY);
    let stream_symbol = symbol.clone();
    let trade_backoff = Backoff::for_feed(TRADE_RECONNECT_SEED);
    let (connected_tx, mut connected_rx) = watch::channel(false);
    let reconnect = tokio::spawn(async move {
        run_trades_with_reconnect(
            HYPERLIQUID_WS_URL,
            &stream_symbol,
            &live_tx,
            &connected_tx,
            mapper,
            trade_backoff,
        )
        .await;
    });
    let mut book_capture: Option<BookCaptureTask> = None;
    // Candle history runs off this loop: see `spawn_ohlcv` for what awaiting it
    // in a command arm used to cost the live trade stream.
    let (ohlcv_tx, mut ohlcv_rx) =
        mpsc::channel::<(Vec<quantick_engine::Bar>, crate::feed::OhlcvSlice)>(1);
    let mut ohlcv_task: Option<JoinHandle<()>> = None;
    let mut ever_connected = false;
    let mut recovery_pending = true;
    let recovery_timeout = tokio::time::sleep(STARTUP_RECOVERY_TIMEOUT);
    tokio::pin!(recovery_timeout);

    loop {
        tokio::select! {
            Some((bars, slice)) = ohlcv_rx.recv() => {
                // Only the closing slice frees the slot: a run still walking
                // backwards through the span is one fetch, however many
                // replies it makes.
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
            maybe_batch = live_rx.recv() => {
                match maybe_batch {
                    Some(trades) => {
                        let is_recovery = recovery_pending;
                        let count = trades.len();
                        let event = classify_trade_batch(trades, &mut recovery_pending);
                        if is_recovery {
                            info!(
                                target: "quantick::app",
                                provider = "hyperliquid",
                                symbol,
                                count,
                                "initial Hyperliquid websocket recovery ready"
                            );
                        }
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            changed = connected_rx.changed() => {
                if changed.is_err() {
                    break;
                }
                let notice = connection_notice(
                    *connected_rx.borrow_and_update(),
                    &mut ever_connected,
                    "Hyperliquid",
                );
                if notice_tx.send(notice).await.is_err() {
                    break;
                }
            }
            () = &mut recovery_timeout, if recovery_pending => {
                recovery_pending = false;
                warn!(
                    target: "quantick::app",
                    provider = "hyperliquid",
                    symbol,
                    timeout_ms = STARTUP_RECOVERY_TIMEOUT.as_millis() as u64,
                    action = "continue_live",
                    "initial Hyperliquid websocket recovery timed out"
                );
                if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
                    break;
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(FeedCommand::FetchOhlcv {
                        span_ms,
                        slice_ms,
                        before_ms,
                    }) => {
                        if ohlcv_task.as_ref().is_some_and(|task| !task.is_finished()) {
                            // One fetch at a time; the in-flight one answers.
                            warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "HYPERLIQUID_OHLCV_ALREADY_RUNNING",
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
                            // `load_older` never returns silence either.
                            // `Refused` rather than a short answer: nothing was
                            // fetched because nobody looked, which is not a
                            // statement about the venue's record.
                            if ohlcv_tx
                                .send((Vec::new(), crate::feed::OhlcvSlice::Refused))
                                .await
                                .is_err()
                            {
                                break; // UI gone
                            }
                        } else {
                            ohlcv_task = Some(spawn_ohlcv(
                                symbol.clone(),
                                span_ms,
                                slice_ms,
                                before_ms,
                                ohlcv_tx.clone(),
                            ));
                        }
                    }
                    Some(FeedCommand::LoadOlder { .. }) => {
                        // `recentTrades` is not pageable. Always acknowledge the
                        // request so a stale UI command cannot leave a spinner.
                        warn!(
                            target: "quantick::app",
                            provider = "hyperliquid",
                            symbol,
                            action = "report_no_history_paging",
                            "older Hyperliquid public trades are unavailable"
                        );
                        if tx.send(FeedEvent::HistoryPrepended(Vec::new())).await.is_err() {
                            break;
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
                                    provider = "hyperliquid",
                                    symbol,
                                    initial_generation,
                                    action = "keep_running",
                                    "book capture is already running"
                                );
                            } else {
                                stop_book_capture(
                                    &mut book_capture,
                                    &symbol,
                                    "finished_before_enable",
                                )
                                .await;
                                book_capture = Some(start_book_capture(
                                    &symbol,
                                    initial_generation,
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
                            &book_tx,
                        ));
                    }
                    Some(FeedCommand::Replay(_)) => {}
                    None => break,
                }
            }
        }
    }
    reconnect.abort();
    let _ = reconnect.await;
    // The candle socket is its own connection; close it with the feed rather
    // than leaving it to time out against a consumer that is gone.
    if let Some(task) = ohlcv_task {
        task.abort();
    }
    stop_book_capture(&mut book_capture, &symbol, "feed_dropped").await;
}

fn classify_trade_batch(trades: Vec<Trade>, recovery_pending: &mut bool) -> FeedEvent {
    if std::mem::take(recovery_pending) {
        FeedEvent::Backfilled(trades)
    } else {
        FeedEvent::LiveBatch(trades)
    }
}

struct BookCaptureTask {
    initial_generation: u64,
    handle: JoinHandle<()>,
}

fn start_book_capture(
    symbol: &str,
    initial_generation: u64,
    events: &mpsc::Sender<DepthEvent>,
) -> BookCaptureTask {
    let symbol = symbol.to_owned();
    let events = events.clone();
    let backoff = Backoff::for_feed(DEPTH_RECONNECT_SEED ^ initial_generation);
    info!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "book_capture_started",
        provider = "hyperliquid",
        symbol,
        initial_generation,
        coverage_levels_per_side = HYPERLIQUID_LEVELS_PER_SIDE,
        action = "start",
        "starting Hyperliquid L2 capture"
    );
    let task_symbol = symbol.clone();
    let handle = tokio::spawn(async move {
        run_depth_with_reconnect(
            HYPERLIQUID_WS_URL,
            &task_symbol,
            &events,
            initial_generation,
            backoff,
        )
        .await;
    });
    BookCaptureTask {
        initial_generation,
        handle,
    }
}

/// Fetch `span_ms` of one-minute candles on a task of its own, delivering the
/// bars back to the feed loop through `reply`.
///
/// Off the loop deliberately. The fetch already opens its own socket, so it
/// never competed with the trade stream for bandwidth — but awaiting it in a
/// command arm stopped the loop from polling `live_rx`, which against a stalled
/// venue meant minutes of unread trades, a full channel, and a websocket read
/// loop stalled behind it.
///
/// Every outcome answers, including the ones that failed: the pane's loading
/// indicator keys on the reply, and a venue that refused is a reason to show an
/// empty pane, not a reason to leave one spinning. What went wrong is in the
/// log, where it can carry the detail a chart cannot.
fn spawn_ohlcv(
    symbol: String,
    span_ms: i64,
    slice_ms: Option<i64>,
    before_ms: Option<i64>,
    reply: mpsc::Sender<(Vec<quantick_engine::Bar>, crate::feed::OhlcvSlice)>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let symbol = symbol.as_str();
        // See the Binance twin: the live edge, or the instant a *load older*
        // wants the reply to end at.
        let now_ms = before_ms.unwrap_or_else(crate::metrics::wall_clock_ms);
        let windows = crate::feed::ohlcv_plan::plan(now_ms, span_ms, slice_ms);
        info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HYPERLIQUID_OHLCV_PLAN",
            symbol,
            requested_span_ms = span_ms,
            slice_ms = slice_ms.unwrap_or(0),
            windows = windows.len(),
            "candle history planned"
        );
        // Short or failed windows accumulate into the answer's completeness:
        // the trader is told the span is short if *any* part of it came up
        // short, never just the last window fetched.
        let mut complete = true;
        for window in windows {
            let bars = match fetch_candle_history(
                HYPERLIQUID_WS_URL,
                symbol,
                CANDLE_INTERVAL_1M,
                ONE_MINUTE_MS,
                window.from_ms,
                window.to_ms,
            )
            .await
            {
                Ok(history) => {
                    if !history.complete {
                        warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "HYPERLIQUID_OHLCV_PARTIAL",
                            symbol,
                            requested_span_ms = span_ms,
                            from_ms = window.from_ms,
                            to_ms = window.to_ms,
                            bars = history.bars.len(),
                            action = "answer_partial",
                            "candle history is short of the requested window"
                        );
                    }
                    complete &= history.complete;
                    history.bars
                }
                Err(error) => {
                    warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "HYPERLIQUID_OHLCV_FAILED",
                        symbol,
                        requested_span_ms = span_ms,
                        from_ms = window.from_ms,
                        to_ms = window.to_ms,
                        %error,
                        action = "answer_empty",
                        "could not fetch candle history"
                    );
                    // A failed window is the clearest "not the whole span"
                    // there is — and a reason to keep going, not to stop: the
                    // windows are independent, and one refused socket says
                    // nothing about the next.
                    complete = false;
                    Vec::new()
                }
            };
            let slice = if window.last {
                crate::feed::OhlcvSlice::Last { complete }
            } else {
                crate::feed::OhlcvSlice::More
            };
            // A closed channel means the feed loop is gone, which is not this
            // task's problem to report: it is already being reported there.
            // Stop fetching, though — nothing is listening for the rest.
            if reply.send((bars, slice)).await.is_err() {
                return;
            }
        }
    })
}

async fn stop_book_capture(task: &mut Option<BookCaptureTask>, symbol: &str, reason: &'static str) {
    let Some(task) = task.take() else {
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
        provider = "hyperliquid",
        symbol,
        initial_generation,
        reason,
        outcome,
        action = "stop",
        "Hyperliquid book capture stopped"
    );
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;

    fn trade(id: u64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: i64::try_from(id).expect("test id fits in i64"),
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: quantick_engine::Side::Buy,
        }
    }

    #[test]
    fn first_websocket_batch_is_recovery_then_batches_stay_live() {
        let mut recovery_pending = true;
        let first = classify_trade_batch(vec![trade(1), trade(2)], &mut recovery_pending);
        assert!(matches!(first, FeedEvent::Backfilled(trades) if trades.len() == 2));
        assert!(!recovery_pending);

        let second = classify_trade_batch(vec![trade(3)], &mut recovery_pending);
        assert!(matches!(second, FeedEvent::LiveBatch(trades) if trades.len() == 1));
    }

    #[test]
    fn batch_after_recovery_timeout_is_live() {
        let mut recovery_pending = false;
        let event = classify_trade_batch(vec![trade(1)], &mut recovery_pending);
        assert!(matches!(event, FeedEvent::LiveBatch(trades) if trades.len() == 1));
    }
}
