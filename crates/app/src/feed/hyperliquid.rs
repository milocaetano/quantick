//! Hyperliquid backend for the provider-neutral app feed bridge.
//!
//! Startup reads the venue's short `recentTrades` recovery window, then a
//! reconnecting WebSocket carries factual aggressor-side prints. A separately
//! cancellable `l2Book` connection publishes the visible 20-level book through
//! the same neutral [`DepthEvent`] channel Binance and MetaTrader use.

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{error, info, warn};

use quantick_engine::Trade;
use quantick_feed_hyperliquid::{
    Backoff, HYPERLIQUID_WS_URL, HyperliquidHttp, TradeMapper,
    depth::{DepthEvent, HYPERLIQUID_LEVELS_PER_SIDE, run_depth_with_reconnect},
    fetch_recent_trades, run_trades_with_reconnect,
};

use super::{FeedCommand, FeedEvent, FeedHandle};
use crate::config::ProviderKind;

const BOOK_EVENT_CHANNEL_CAPACITY: usize = 8_192;
const TRADE_EVENT_CHANNEL_CAPACITY: usize = 4_096;
const COMMAND_CHANNEL_CAPACITY: usize = 16;
const FEED_RUNTIME_WORKERS: usize = 2;
const TRADE_RECONNECT_SEED: u64 = 0x4859_5045_525F_5452;
const DEPTH_RECONNECT_SEED: u64 = 0x4859_5045_525F_4C32;

/// Start the selected Hyperliquid perpetual on a background runtime.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(TRADE_EVENT_CHANNEL_CAPACITY);
    let (book_tx, book_rx) = mpsc::channel(BOOK_EVENT_CHANNEL_CAPACITY);
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
            runtime.block_on(feed_task(symbol, tx, book_tx, cmd_rx));
        })
        .expect("spawn Hyperliquid feed thread");

    FeedHandle {
        events: rx,
        book_events: book_rx,
        notices: super::silent_notices(),
        capabilities: super::fixed_capabilities(ProviderKind::Hyperliquid.capabilities()),
        commands: cmd_tx,
        replay: None,
    }
}

async fn feed_task(
    symbol: String,
    tx: mpsc::Sender<FeedEvent>,
    book_tx: mpsc::Sender<DepthEvent>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    let symbol = symbol.to_uppercase();
    let http = HyperliquidHttp::new();
    let mut mapper = TradeMapper::new(&symbol);

    match fetch_recent_trades(&http, &symbol).await {
        Ok(raw) => {
            let batch = mapper.map_batch(raw);
            log_mapping_ledger(&symbol, "startup", &batch);
            info!(
                target: "quantick::app",
                provider = "hyperliquid",
                symbol,
                count = batch.trades.len(),
                "recent Hyperliquid trades ready"
            );
            if tx.send(FeedEvent::Backfilled(batch.trades)).await.is_err() {
                return;
            }
        }
        Err(error) => {
            error!(
                target: "quantick::app",
                provider = "hyperliquid",
                symbol,
                %error,
                "recent-trades fetch failed; continuing with live data"
            );
            if tx.send(FeedEvent::Backfilled(Vec::new())).await.is_err() {
                return;
            }
        }
    }

    let (live_tx, mut live_rx) = mpsc::channel::<Trade>(TRADE_EVENT_CHANNEL_CAPACITY);
    let stream_symbol = symbol.clone();
    let trade_backoff = Backoff::for_feed(TRADE_RECONNECT_SEED);
    let reconnect = tokio::spawn(async move {
        run_trades_with_reconnect(
            HYPERLIQUID_WS_URL,
            &stream_symbol,
            &live_tx,
            mapper,
            trade_backoff,
        )
        .await;
    });
    let mut book_capture: Option<BookCaptureTask> = None;

    loop {
        tokio::select! {
            maybe_trade = live_rx.recv() => {
                match maybe_trade {
                    Some(trade) => {
                        if tx.send(FeedEvent::Live(trade)).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
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
    stop_book_capture(&mut book_capture, &symbol, "feed_dropped").await;
}

fn log_mapping_ledger(
    symbol: &str,
    stage: &'static str,
    batch: &quantick_feed_hyperliquid::MappedBatch,
) {
    if batch.errors.is_empty() && batch.stale == 0 {
        return;
    }
    warn!(
        target: "quantick::app",
        provider = "hyperliquid",
        symbol,
        stage,
        accepted = batch.trades.len(),
        duplicates = batch.duplicates,
        stale = batch.stale,
        malformed = batch.errors.len(),
        first_error = batch.errors.first().map(ToString::to_string),
        action = "skip_invalid_rows",
        "Hyperliquid trade mapping was partially usable"
    );
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
