//! Bridges the async Binance feed to the synchronous egui UI.
//!
//! A background thread runs a tokio runtime that first backfills recent history
//! over REST, then streams live trades (with reconnect + gap detection). Both
//! are pushed onto a channel the UI drains each frame via `try_recv` — no async
//! on the UI thread. The UI can also send commands back (e.g. "load older
//! history"), which the feed thread services between live trades.

use tokio::sync::mpsc;
use tracing::{error, info, warn};

use quantick_engine::Trade;
use quantick_feed_binance::{
    BINANCE_WS_BASE, Backoff, BinanceHttp, agg_trade_url, backfill, backfill_before,
    run_with_reconnect,
};

/// Default number of recent trades to backfill so the chart opens populated,
/// when `QUANTICK_BACKFILL` is unset. One REST page.
pub const DEFAULT_BACKFILL_TARGET: usize = 1000;

/// A message from the feed thread to the UI, tagged by source so the chart can
/// label backfilled vs live data honestly.
pub enum FeedEvent {
    /// The whole backfilled history, delivered as one batch.
    Backfilled(Vec<Trade>),
    /// Older history pulled on demand, to prepend in front of what's loaded.
    HistoryPrepended(Vec<Trade>),
    /// One live trade.
    Live(Trade),
}

/// A command from the UI to the feed thread.
pub enum FeedCommand {
    /// Fetch `count` more trades older than the earliest one loaded.
    LoadOlder { count: usize },
}

/// The UI's handle on a running feed: events to drain, commands to send.
pub struct FeedHandle {
    /// Feed → UI: backfill, prepended history and live trades.
    pub events: mpsc::Receiver<FeedEvent>,
    /// UI → feed: on-demand history loading.
    pub commands: mpsc::Sender<FeedCommand>,
}

/// The initial backfill depth: `QUANTICK_BACKFILL` if it parses to a positive
/// integer, else [`DEFAULT_BACKFILL_TARGET`].
#[must_use]
pub fn initial_backfill_target() -> usize {
    std::env::var("QUANTICK_BACKFILL")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(DEFAULT_BACKFILL_TARGET)
}

/// Start the feed for `symbol` on a background thread, returning the handle the
/// UI drains and sends commands through. Dropping the handle stops the feed.
#[must_use]
pub fn spawn(symbol: &str) -> FeedHandle {
    let (tx, rx) = mpsc::channel(4096);
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
            runtime.block_on(feed_task(symbol, tx, cmd_rx));
        })
        .expect("spawn feed thread");
    FeedHandle {
        events: rx,
        commands: cmd_tx,
    }
}

async fn feed_task(
    symbol: String,
    tx: mpsc::Sender<FeedEvent>,
    mut cmd_rx: mpsc::Receiver<FeedCommand>,
) {
    let http = BinanceHttp::new();
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
    let reconnect = tokio::spawn(async move { run_with_reconnect(&url, &live_tx, backoff).await });

    loop {
        tokio::select! {
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
            maybe_cmd = cmd_rx.recv() => {
                match maybe_cmd {
                    Some(FeedCommand::LoadOlder { count }) => {
                        earliest_id = load_older(&http, &symbol, earliest_id, count, &tx).await;
                        if tx.is_closed() {
                            break; // UI gone
                        }
                    }
                    None => break, // UI dropped the command sender: it's gone
                }
            }
        }
    }
    reconnect.abort();
}

/// Fetch `count` trades older than `earliest`, send them to the UI, and return
/// the new earliest agg_id (unchanged if nothing older was available or a fetch
/// failed). `None` earliest means there is no history to page back from.
async fn load_older(
    http: &BinanceHttp,
    symbol: &str,
    earliest: Option<u64>,
    count: usize,
    tx: &mpsc::Sender<FeedEvent>,
) -> Option<u64> {
    let Some(before) = earliest else {
        warn!(target: "quantick::app", "load older ignored: no history to page back from");
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
            earliest
        }
        Err(e) => {
            error!(target: "quantick::app", symbol, %e, "load older failed");
            earliest
        }
    }
}
