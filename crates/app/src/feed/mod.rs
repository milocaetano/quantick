//! Bridges an async market-data feed to the synchronous egui UI.
//!
//! A feed runs on a background thread and pushes [`FeedEvent`]s onto a channel
//! the UI drains each frame via `try_recv` — no async on the UI thread. The UI
//! can send [`FeedCommand`]s back (e.g. "load older history"), serviced between
//! live trades.
//!
//! Which backend runs is chosen at [`spawn`] time from a [`FeedSource`], so the
//! UI is provider-agnostic: it drains the same [`FeedHandle`] regardless of
//! where the trades come from. [`binance`] streams public aggTrades directly;
//! [`hyperliquid`] streams public perpetual trades and complete L2 images;
//! [`metatrader`] listens for the local QuantickBridge EA (see `bridge/mt5/`);
//! [`replay`] plays a recorded session back through the very same channel, which
//! is what lets market replay reuse the whole chart untouched.

pub mod binance;
pub mod hyperliquid;
pub mod metatrader;
pub mod mt5_bridge;
pub mod replay;

use tokio::sync::{mpsc, watch};

use quantick_engine::{Bar, Trade};
// The provider-neutral depth type, from the crate that defines it. Three
// feeds publish on this channel now; routing the shared type through one
// venue's re-export is the name the next feed author would copy.
pub use quantick_orderbook::DepthEvent;

use crate::config::{FeedCapabilities, ProviderKind};

pub use replay::{ReplayControl, ReplayLink, ReplayOptions, ReplayRequest};

/// Default number of recent trades to backfill so the chart opens populated,
/// when `QUANTICK_BACKFILL` is unset. One Binance REST page.
pub const DEFAULT_BACKFILL_TARGET: usize = 1000;

/// How far back a time pane asks for candle history: 90 days, i.e. "at least
/// three months".
///
/// Trade backfill and candle history answer different questions and are sized
/// differently on purpose. The tape is a recent window — what is happening now,
/// in full detail. Candles are the context around it, and a quarter is the
/// shortest span in which a daily or weekly chart says anything at all.
///
/// Only the tests reach for this today: the providers that answer the request
/// are built, and the pane that makes it is not. Delete this attribute when the
/// time pane starts asking.
#[allow(dead_code)]
pub const TIME_HISTORY_SPAN_MS: i64 = 90 * 24 * 60 * 60 * 1_000;

/// Wall-clock epoch milliseconds, for the one thing a feed legitimately needs
/// a clock for: deciding what "the last ninety days" means.
///
/// The determinism rule lives in the engine, which is *told* what time it is by
/// the trades it receives. A feed is the boundary where real time enters, and a
/// candle request has to name a span in it. Nothing derived from this reaches a
/// bar builder.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// The one interval every provider delivers candle history in: one minute.
///
/// A single base series, resampled locally to whatever the pane shows, is what
/// makes this work across four very different transports. The venues disagree
/// about paging, and MetaTrader cannot be *asked* for anything at all (its
/// bridge only pushes), so "every provider answers the same one request" is
/// the only contract all of them can keep. It also makes switching a pane's
/// interval free — no refetch, just a different fold over bars already held.
pub const OHLCV_BASE_INTERVAL_MS: i64 = 60_000;

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
    /// Several live trades received or released together.
    ///
    /// A replay at 50× can release hundreds of prints between two frames, and a
    /// per-trade channel message for each is pure overhead on both ends. The UI
    /// treats a batch exactly as the trades in it, in order.
    LiveBatch(Vec<Trade>),
    /// Discard everything loaded and start over from an empty chart.
    ///
    /// Sent when a source rewinds — seeking a replay backwards, for instance.
    /// Bars that were already closed cannot be un-closed, so the honest answer
    /// is to rebuild from the new position rather than patch the series.
    Reset,
    /// Venue-native candle history, answering exactly one
    /// [`FeedCommand::FetchOhlcv`].
    ///
    /// Empty means the request finished with nothing: the venue has no history
    /// for this symbol, the provider does not serve candles, or the fetch
    /// failed. As with [`HistoryPrepended`](Self::HistoryPrepended), the reply
    /// itself is the signal that loading ended — a provider that stayed silent
    /// would strand a spinner forever.
    ///
    /// Every provider sends this; nothing reads the bars yet, because the pane
    /// that draws them is not built. Delete the attribute when it is.
    #[allow(dead_code)]
    OhlcvHistory {
        /// The interval each bar covers. Always
        /// [`OHLCV_BASE_INTERVAL_MS`] today, and tagged rather than assumed:
        /// a consumer resampling these must never have to guess what it is
        /// resampling *from*, and a venue that one day serves a different base
        /// would otherwise be an invisible change.
        interval_ms: i64,
        /// Closed candles, ascending by `open_time`, deduplicated.
        ///
        /// These are venue candles, not engine bars replayed from trades: see
        /// the mapping rules on [`FeedCommand::FetchOhlcv`] for what that costs
        /// in fidelity.
        bars: Vec<Bar>,
    },
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
    /// Drive playback of a recorded session. Ignored by live feeds.
    Replay(ReplayControl),
    /// Fetch venue-native candle history covering `span_ms` back from now.
    ///
    /// Answered by **exactly one** [`FeedEvent::OhlcvHistory`], by every
    /// provider, always — empty when the provider serves no candles, when the
    /// venue has none for this symbol, or when the fetch failed. A provider
    /// that answered only on success would leave the pane's loading indicator
    /// spinning on the one case a user most needs explained.
    ///
    /// # What these bars are, and are not
    ///
    /// A venue candle is not an engine bar replayed from trades, and the
    /// difference is recorded rather than smoothed over:
    ///
    /// - `open_time` is the bucket start and `close_time` the bucket end minus
    ///   one millisecond — the interval the candle *covers*. An engine bar
    ///   instead stamps its first and last trade, so its times sit strictly
    ///   inside its bucket. Anything joining the two series has to reconcile
    ///   that seam explicitly.
    /// - Intervals the venue reports as empty are dropped, never emitted, so
    ///   the series follows the engine's empty-interval rule: a gap is the
    ///   honest record that nothing traded, and a carried-forward price would
    ///   be a fabricated one.
    /// - `trade_count` is the venue's own count where it publishes one, and 0
    ///   where it does not — 0 meaning "not reported", not "no trades", since
    ///   a candle with no trades is not emitted at all.
    /// - Only Binance publishes an aggressor split. Elsewhere `buy_volume` and
    ///   `sell_volume` each carry half the candle's volume, which keeps
    ///   [`Bar::volume`] exact and makes [`Bar::delta`] identically zero — read
    ///   as "not measured", never as "measured and found balanced".
    ///
    /// Every provider answers this; only the tests send it so far, because the
    /// pane that would is not built. Delete the attribute when it is.
    #[allow(dead_code)]
    FetchOhlcv {
        /// How far back to reach, in milliseconds. See
        /// [`TIME_HISTORY_SPAN_MS`].
        span_ms: i64,
    },
}

/// Where a feed's trades come from. One variant per backend, mirroring the
/// [`spawn`] dispatch.
pub enum FeedSource {
    /// A live venue, selected from the configuration.
    Live {
        /// Which backend streams it.
        provider: ProviderKind,
        /// The instrument to stream.
        symbol: String,
    },
    /// A recorded session, played back from disk.
    Replay(Box<replay::ReplayRequest>),
}

/// Provider-neutral state of the live trade transport.
///
/// This is deliberately independent from trade arrival latency: a quiet market
/// says nothing about whether a socket is connected, and a latency observation
/// made before a disconnect must not keep the status bar green.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedConnectionState {
    /// The selected feed has not established its first session yet.
    Connecting,
    /// A previously established session ended and its reconnect loop is active.
    Reconnecting,
    /// The provider confirmed that the live trade transport is established.
    Connected,
}

/// What a feed wants the person watching the chart to know.
///
/// A feed that cannot deliver trades knows *why* — the terminal is closed, a
/// package is missing, the contract does not exist. Before this channel that
/// reason only ever reached a log file, and the chart stayed blank with a
/// faint "connecting" dot: honest, but useless to anyone not reading stderr.
///
/// [`Connected`](Self::Connected) and [`Reconnecting`](Self::Reconnecting) are
/// explicit transport transitions. The other variants are presentation only:
/// [`Working`](Self::Working) is the feed's own business, still in progress;
/// [`Attention`](Self::Attention) needs a human, and always carries the one
/// next step in plain words. Keeping those roles separate prevents unrelated
/// supervisor output from changing a healthy connection's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedNotice {
    /// The live trade transport is established.
    ///
    /// This is a control signal rather than a card: the app clears any
    /// connection-progress notice and marks the provider-neutral transport
    /// connected without waiting for a trade to infer it.
    Connected,
    /// A previously established live transport ended and is reconnecting.
    ///
    /// Like [`Connected`](Self::Connected), this explicitly drives the
    /// provider-neutral connection state. The headline can also explain an
    /// empty chart while the reconnect loop is working.
    Reconnecting {
        /// One line, e.g. `Hyperliquid disconnected — reconnecting`.
        headline: String,
    },
    /// Whatever was being reported is over; the chart speaks for itself now.
    Clear,
    /// A step is under way and needs nobody: connecting, waiting, retrying.
    Working {
        /// One line, e.g. `starting the MetaTrader bridge`.
        headline: String,
    },
    /// Something needs the person watching — a closed terminal, a missing
    /// package, a contract this account cannot see.
    Attention {
        /// What went wrong, in the user's terms.
        headline: String,
        /// The single next step to fix it.
        next_step: String,
    },
}

/// The notice channel of a feed that has nothing to say: closed at birth, so
/// the UI reads it exactly like a feed that simply never reported trouble.
#[must_use]
pub fn silent_notices() -> mpsc::Receiver<FeedNotice> {
    let (_tx, rx) = mpsc::channel(1);
    rx
}

/// The capability channel of a feed whose answer is known at spawn and never
/// changes. The sender is dropped immediately; a `watch` receiver keeps serving
/// the value it was born with.
#[must_use]
pub fn fixed_capabilities(capabilities: FeedCapabilities) -> watch::Receiver<FeedCapabilities> {
    watch::channel(capabilities).1
}

impl FeedNotice {
    /// Shorthand for an explicit reconnecting transport transition.
    #[must_use]
    pub fn reconnecting(headline: impl Into<String>) -> Self {
        Self::Reconnecting {
            headline: headline.into(),
        }
    }

    /// Shorthand for a step in progress.
    #[must_use]
    pub fn working(headline: impl Into<String>) -> Self {
        Self::Working {
            headline: headline.into(),
        }
    }

    /// Shorthand for something that needs a human.
    #[must_use]
    pub fn attention(headline: impl Into<String>, next_step: impl Into<String>) -> Self {
        Self::Attention {
            headline: headline.into(),
            next_step: next_step.into(),
        }
    }
}

/// Translate the boolean lifecycle emitted by a provider reconnect loop into
/// the app's notice protocol. `ever_connected` distinguishes first-connect
/// retries from a real reconnect without consulting trade arrival.
pub(super) fn connection_notice(
    connected: bool,
    ever_connected: &mut bool,
    provider: &str,
) -> FeedNotice {
    if connected {
        *ever_connected = true;
        FeedNotice::Connected
    } else if *ever_connected {
        FeedNotice::reconnecting(format!("{provider} disconnected — reconnecting"))
    } else {
        FeedNotice::working(format!("connecting to {provider}"))
    }
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
    /// Feed → UI: what the person watching should know about the connection.
    ///
    /// Its own channel rather than a [`FeedEvent`] variant: connection trouble
    /// is not market data, and a blocked feed must be able to say so while the
    /// trade channel sits silent — which is exactly when it matters.
    pub notices: mpsc::Receiver<FeedNotice>,
    /// What this running feed can really do, narrowed as it finds out.
    ///
    /// A provider answers for itself at spawn time, but some answers only exist
    /// once a session starts: whether *this* MetaTrader symbol has a book, or a
    /// tape at all, arrives with the bridge's hello. The UI reads the current
    /// value every frame — exactly as it read the static provider answer
    /// before — so an affordance withdraws itself the moment the truth is known.
    pub capabilities: watch::Receiver<FeedCapabilities>,
    /// UI → feed: on-demand history loading.
    pub commands: mpsc::Sender<FeedCommand>,
    /// Present only while a recorded session is playing: what the transport bar
    /// reads to draw itself. `None` means this is a live feed, which is the one
    /// check the UI needs to tell the two modes apart.
    pub replay: Option<ReplayLink>,
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

/// Start the feed for `source` on a background thread, returning the handle the
/// UI drains and sends commands through. Dropping the handle stops the feed.
/// Provider-specific settings come from `config`.
///
/// This is the whole "source → backend" dispatch: one place, mirroring the
/// [`FeedSource`] variants. Adding a source is a new arm here plus its module.
#[must_use]
pub fn spawn(source: FeedSource, config: &crate::config::AppConfig) -> FeedHandle {
    match source {
        FeedSource::Live { provider, symbol } => match provider {
            ProviderKind::Binance => binance::spawn(&symbol),
            ProviderKind::Hyperliquid => hyperliquid::spawn(&symbol),
            ProviderKind::MetaTrader => metatrader::spawn(&symbol, &config.metatrader),
        },
        FeedSource::Replay(request) => replay::spawn(*request),
    }
}

/// Start a live feed for `provider`/`symbol`. A shorthand for the common case.
#[must_use]
pub fn spawn_live(
    provider: ProviderKind,
    symbol: &str,
    config: &crate::config::AppConfig,
) -> FeedHandle {
    spawn(
        FeedSource::Live {
            provider,
            symbol: symbol.to_string(),
        },
        config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_loop_lifecycle_uses_explicit_transport_notices() {
        let mut ever_connected = false;

        assert!(matches!(
            connection_notice(false, &mut ever_connected, "Binance"),
            FeedNotice::Working { headline } if headline == "connecting to Binance"
        ));
        assert_eq!(
            connection_notice(true, &mut ever_connected, "Binance"),
            FeedNotice::Connected
        );
        assert!(matches!(
            connection_notice(false, &mut ever_connected, "Binance"),
            FeedNotice::Reconnecting { headline }
                if headline == "Binance disconnected — reconnecting"
        ));
    }
}
