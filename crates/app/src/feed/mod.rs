//! Bridges an async market-data feed to the synchronous egui UI.
//!
//! A feed runs on a background thread and pushes [`FeedEvent`]s onto a channel
//! the UI drains each frame via `try_recv` — no async on the UI thread. The UI
//! can send [`FeedCommand`]s back (e.g. "load older history"), serviced between
//! live trades.
//!
//! Which backend runs is chosen at [`spawn`] time from a [`ProviderKind`], so
//! the UI is provider-agnostic: it drains the same [`FeedHandle`] regardless of
//! exchange. [`binance`] streams public aggTrades directly; [`metatrader`]
//! listens for the local QuantickBridge EA (see `bridge/mt5/`).

pub mod binance;
pub mod metatrader;

use tokio::sync::mpsc;

use quantick_engine::Trade;
pub use quantick_feed_binance::depth::DepthEvent;

use crate::config::ProviderKind;

/// Default number of recent trades to backfill so the chart opens populated,
/// when `QUANTICK_BACKFILL` is unset. One Binance REST page.
pub const DEFAULT_BACKFILL_TARGET: usize = 1000;

/// A message from the feed thread to the UI, tagged by source so the chart can
/// label backfilled vs live data honestly.
pub enum FeedEvent {
    /// The whole backfilled history, delivered as one batch.
    Backfilled(Vec<Trade>),
    /// Older history pulled on demand, to prepend in front of what's loaded.
    /// Empty when the request finished with nothing to prepend (no older
    /// history, or the fetch failed) — the reply itself is the signal that
    /// loading ended.
    HistoryPrepended(Vec<Trade>),
    /// One live trade.
    Live(Trade),
}

/// A command from the UI to the feed thread.
pub enum FeedCommand {
    /// Fetch `count` more trades older than the earliest one loaded.
    LoadOlder { count: usize },
    /// Enable or disable synchronized order-book capture.
    SetBookCapture {
        /// Whether capture should be running.
        enabled: bool,
        /// First generation assigned to a newly started capture.
        initial_generation: u64,
    },
    /// Discard any running capture and start a fresh generation.
    RestartBookCapture {
        /// First generation assigned to the replacement capture.
        initial_generation: u64,
    },
}

/// The UI's handle on a running feed: events to drain, commands to send.
pub struct FeedHandle {
    /// Feed → UI: backfill, prepended history and live trades.
    pub events: mpsc::Receiver<FeedEvent>,
    /// Synchronized order-book snapshots, updates and lifecycle status.
    ///
    /// Depth is isolated from the established trade/bar channel so it can be
    /// stopped, restarted or backpressured independently.
    pub book_events: mpsc::Receiver<DepthEvent>,
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

/// Start the feed for `provider`/`symbol` on a background thread, returning the
/// handle the UI drains and sends commands through. Dropping the handle stops
/// the feed. Provider-specific settings come from `config`.
///
/// This is the whole "provider → backend" dispatch: one place, mirroring the
/// [`ProviderKind`] variants. Adding a provider is a new arm here plus its
/// module.
#[must_use]
pub fn spawn(
    provider: ProviderKind,
    symbol: &str,
    config: &crate::config::AppConfig,
) -> FeedHandle {
    match provider {
        ProviderKind::Binance => binance::spawn(symbol),
        ProviderKind::MetaTrader => metatrader::spawn(symbol, &config.metatrader),
    }
}
