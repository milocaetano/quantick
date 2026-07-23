//! Bridges the async Binance feed to the synchronous egui UI.
//!
//! A background thread runs a tokio runtime that first backfills recent history
//! over REST, then streams live trades (with reconnect + gap detection). Both
//! are pushed onto a channel the UI drains each frame via `try_recv` — no async
//! on the UI thread.

use tokio::sync::mpsc;
use tracing::{error, info};

use quantick_engine::Trade;
use quantick_feed_binance::{
    BINANCE_WS_BASE, Backoff, BinanceHttp, agg_trade_url, backfill, run_with_reconnect,
};

/// How many recent trades to backfill so the chart opens populated.
pub const BACKFILL_TARGET: usize = 500;

/// A message from the feed thread to the UI, tagged by source so the chart can
/// label backfilled vs live data honestly.
pub enum FeedEvent {
    /// The whole backfilled history, delivered as one batch.
    Backfilled(Vec<Trade>),
    /// One live trade.
    Live(Trade),
}

/// Start the feed for `symbol` on a background thread, returning the receiver
/// the UI drains. Dropping the receiver stops the feed.
#[must_use]
pub fn spawn(symbol: &str) -> mpsc::Receiver<FeedEvent> {
    let (tx, rx) = mpsc::channel(4096);
    let symbol = symbol.to_string();
    std::thread::Builder::new()
        .name("quantick-feed".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("build feed runtime");
            runtime.block_on(feed_task(symbol, tx));
        })
        .expect("spawn feed thread");
    rx
}

async fn feed_task(symbol: String, tx: mpsc::Sender<FeedEvent>) {
    // 1. Backfill recent history so the chart opens populated.
    let http = BinanceHttp::new();
    match backfill(&http, &symbol, BACKFILL_TARGET).await {
        Ok(trades) => {
            info!(target: "quantick::app", symbol, count = trades.len(), "backfill ready");
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
    //    speaks Trade; a small forwarder tags each as a live FeedEvent.
    let (live_tx, mut live_rx) = mpsc::channel::<Trade>(4096);
    let url = agg_trade_url(BINANCE_WS_BASE, &symbol);
    // Fixed seed: a single desktop client, so jitter needs no cross-client
    // decorrelation, and a fixed seed keeps behaviour reproducible.
    let backoff = Backoff::for_feed(0x9E37_79B9_7F4A_7C15);
    let reconnect = tokio::spawn(async move { run_with_reconnect(&url, &live_tx, backoff).await });

    while let Some(trade) = live_rx.recv().await {
        if tx.send(FeedEvent::Live(trade)).await.is_err() {
            break; // UI gone
        }
    }
    reconnect.abort();
}
