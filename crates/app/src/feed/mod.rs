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
pub mod ohlcv_plan;
pub mod replay;
pub mod stall;

use tokio::sync::{mpsc, watch};

use quantick_engine::{Bar, Trade};
// The provider-neutral depth type, from the crate that defines it. Three
// feeds publish on this channel now; routing the shared type through one
// venue's re-export is the name the next feed author would copy.
pub use quantick_orderbook::DepthEvent;

use crate::config::{FeedCapabilities, ProviderKind};

pub use metatrader::forced_latency_split;
pub use replay::{ReplayControl, ReplayLink, ReplayOptions, ReplayRequest};

/// Default number of recent trades to backfill so the chart opens populated,
/// when `QUANTICK_BACKFILL` is unset. One Binance REST page.
pub const DEFAULT_BACKFILL_TARGET: usize = 1000;

/// How far back a time pane asks for candle history in **one** request: seven
/// days.
///
/// Trade backfill and candle history answer different questions and are sized
/// differently on purpose. The tape is a recent window — what is happening now,
/// in full detail. Candles are the context around it.
///
/// This used to be ninety days, on the reasoning that a quarter is the
/// shortest span a weekly chart says anything in. That reasoning was about the
/// *deepest* chart a trader might open, and it was charged to every chart they
/// actually open: a quarter of one-minute candles is over a hundred sequential
/// venue pages, and the trader waits through all of them at every launch to
/// read the last hour. A week is the span a session starts from — a few
/// seconds of fetching, and the intraday context already there.
///
/// The quarter is not gone, it is asked for: each *load older* request reaches
/// another span of this size further back and prepends it (see
/// `Tab::request_older_ohlcv_history`), so the deep chart is thirteen clicks,
/// paid by the trader who wants it rather than by everyone who does not.
pub const TIME_HISTORY_SPAN_MS: i64 = 7 * 24 * 60 * 60 * 1_000;

/// The one interval every provider delivers candle history in: one minute.
///
/// A single base series, resampled locally to whatever the pane shows, is what
/// makes this work across four very different transports. The venues disagree
/// about paging, and MetaTrader cannot be *asked* for anything at all (its
/// bridge only pushes), so "every provider answers the same one request" is
/// the only contract all of them can keep. It also makes switching a pane's
/// interval free — no refetch, just a different fold over bars already held.
pub const OHLCV_BASE_INTERVAL_MS: i64 = 60_000;

/// How much of the span one progressive slice covers: two days.
///
/// Chosen against [`TIME_HISTORY_SPAN_MS`], not in isolation — and re-derived
/// when that span became a week. Left at a week it would have been dead
/// policy: a slice as wide as the span is a single window
/// ([`ohlcv_plan::plan`]), so every request would go straight back to the
/// all-at-once wait progressive loading exists to remove.
///
/// The width is a trade between two costs pulling opposite ways, and a week is
/// short enough that the second one now dominates:
///
/// - **Time to the first readable frame.** Four windows means the newest two
///   days land in roughly a quarter of the venue round trips the span costs.
///   That is the number the trader feels — how long the chart stays empty.
/// - **Refold work per arrival.** Every slice runs `refold_history_prefix`, a
///   fold over the *whole* accumulated base plus an indicator rebuild per
///   pane. That is bounded per request by [`ohlcv_plan::MAX_SLICES`], but not
///   across requests: a trader paging back thirteen spans to the old quarter
///   pays it once per slice, against a base growing toward six figures. At one
///   slice a day that is 91 full refolds for the same final chart; at two days
///   it is 52, and at a week it would be 13 with no progressive painting at
///   all.
///
/// Two days keeps the opening week painting in four steps while cutting the
/// deep-chart refold bill by nearly half. Narrower paints marginally sooner
/// and multiplies that bill; wider approaches the all-at-once wait again.
pub const OHLCV_SLICE_SPAN_MS: i64 = 2 * 24 * 60 * 60 * 1_000;

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
    /// Older trades the chart never asked for: the rest of the opening session,
    /// arriving behind the slice the chart first painted.
    ///
    /// Prepended exactly like [`HistoryPrepended`](Self::HistoryPrepended) and
    /// deliberately **not** it, because that event is a *reply*. Arriving as
    /// one, an opening slice would stop the loading indicator a `+ older`
    /// press raised and hand the trader's own history campaign a page it did
    /// not fetch — the run would spend its budget on work it never asked for
    /// and could declare itself finished on tape it did not pull.
    ///
    /// A feed that fills a chart in behind the trader must therefore be able
    /// to say so, and this is that word.
    OpeningPrepended(Vec<Trade>),
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
    /// Venue-native candle history, answering one
    /// [`FeedCommand::FetchOhlcv`] — in one reply, or in a run of them from
    /// the newest part of the span backwards.
    ///
    /// Empty means the request finished with nothing: the venue has no history
    /// for this symbol, the provider does not serve candles, or the fetch
    /// failed. As with [`HistoryPrepended`](Self::HistoryPrepended), the
    /// closing reply is the signal that loading ended — a provider that stayed
    /// silent would strand a spinner forever, and [`OhlcvSlice`] is what says
    /// which reply is the closing one.
    ///
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
        /// in fidelity. In a sliced answer these are the candles of *this*
        /// slice's window only, and every later slice is strictly older.
        bars: Vec<Bar>,
        /// Where this reply sits in the run answering the request.
        slice: OhlcvSlice,
    },
}

/// Where one [`FeedEvent::OhlcvHistory`] sits in the run of replies answering
/// a single [`FeedCommand::FetchOhlcv`].
///
/// The whole point of the enum is that "how complete is the span?" is a
/// question only the closing reply can answer. An intermediate slice knows
/// nothing about the windows still being fetched, so it is given no field in
/// which to claim otherwise — a `complete` flag on every reply would be a
/// number that means something different depending on which reply carried it,
/// which is exactly the kind of quiet lie the data-honesty rule exists to
/// prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhlcvSlice {
    /// An older slice of the same request follows. The wait is not over and
    /// the loading indicator stays up.
    More,
    /// The request was **refused before it was served**: a fetch for this
    /// market was already in flight and the provider serves one at a time.
    ///
    /// A closing reply, because the caller raised a spinner before the command
    /// left and a silent drop would leave it turning forever. But a distinct
    /// one, because "nobody looked" is not "the venue came up short": answered
    /// as `Last { complete: false }` the tab warns that a venue stopped short
    /// when none did, refolds the whole accumulated base over an empty vector,
    /// and consumes the reach-back measurement for a request that was never
    /// made. This variant ends the wait and changes nothing else.
    Refused,
    /// The last reply for this request — and the only one a provider that
    /// serves the whole span at once ever sends.
    Last {
        /// Whether the run as a whole covered the span that was asked for.
        ///
        /// False means the answer is short and *known* to be short: a venue
        /// that stopped answering partway, a bridge whose paging failed after
        /// some pages had landed, a block clipped to a cap. It does not mean
        /// the same thing as a short series — an instrument younger than the
        /// span genuinely has fewer candles, and that answer is complete.
        ///
        /// Carried rather than inferred from the bar count, which cannot tell
        /// those two apart, and cannot tell either from a quiet market. In a
        /// sliced run it is the conjunction over every window: one window that
        /// came up short makes the whole answer short.
        complete: bool,
    },
}

impl OhlcvSlice {
    /// Whether this reply closes its request.
    #[must_use]
    pub fn is_last(self) -> bool {
        matches!(self, Self::Last { .. } | Self::Refused)
    }
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
    /// Answered by **exactly one closing** [`FeedEvent::OhlcvHistory`] — one
    /// tagged [`OhlcvSlice::Last`] — by every provider, always, and optionally
    /// preceded by [`OhlcvSlice::More`] slices running from the newest part of
    /// the span backwards. Empty when the provider serves no candles, when the
    /// venue has none for this symbol, or when the fetch failed. A provider
    /// that answered only on success would leave the pane's loading indicator
    /// spinning on the one case a user most needs explained.
    ///
    /// A provider is free to ignore `slice_ms` and answer once: MetaTrader and
    /// market replay both serve from a block already in hand, where slicing
    /// would add replies without shortening any wait. Slicing is worth doing
    /// exactly where the span costs sequential venue round trips.
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
    FetchOhlcv {
        /// How far back to reach, in milliseconds. See
        /// [`TIME_HISTORY_SPAN_MS`].
        span_ms: i64,
        /// The newest millisecond this request covers, inclusive. `None` means
        /// *now* — the opening request, reaching back from the live edge.
        ///
        /// `Some` is how the chart reaches further into the past than one
        /// request goes: the caller passes the millisecond just before the
        /// oldest candle it already holds, and the reply is a span of that
        /// size older still, to prepend. The two are the same request with a
        /// different right-hand edge, so a provider that honours the span
        /// honours this for free — it is the `now_ms` handed to
        /// [`ohlcv_plan::plan`].
        ///
        /// A provider serving from a block it already holds (MetaTrader,
        /// market replay) reaches as far back as that block does and no
        /// further, whatever is asked; it answers with what it has rather than
        /// with silence, and says so in its log.
        before_ms: Option<i64>,
        /// How much of the span one reply should cover, newest first.
        ///
        /// `None` asks for the whole span in a single reply — what every
        /// provider did before progressive loading existed, and what the
        /// trader gets back by turning the option off. `Some` is a request,
        /// not an instruction: [`ohlcv_plan::plan`] decides the actual windows
        /// and caps how many there are, and a provider serving from a block it
        /// already holds answers once regardless. See [`OHLCV_SLICE_SPAN_MS`].
        slice_ms: Option<i64>,
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

/// A stretch of market time no print covers, left by a reconnect that kept the
/// chart's timeline instead of rebuilding it.
///
/// [`Tab::reconnect_feed`](crate::tab::Tab::reconnect_feed) exists so a feed
/// that hiccuped costs the trader nothing: the bars, drawings, indicators,
/// armed strategies and any open paper position all survive the new session.
/// What cannot survive is the market that traded while nobody was listening,
/// and the honesty rule is explicit about it — inferred or incomplete data is
/// labelled as such, never silently patched. So the hole is recorded here and
/// drawn on the chart at the seam it falls in, rather than closed by butting
/// the two halves of the session against each other as if nothing happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedGap {
    /// Timestamp of the last print before the silence.
    pub from_ms: i64,
    /// Timestamp of the first print after it.
    pub to_ms: i64,
}

impl FeedGap {
    /// How long nothing was heard, in milliseconds.
    #[must_use]
    pub fn duration_ms(self) -> i64 {
        self.to_ms.saturating_sub(self.from_ms).max(0)
    }
}

/// A gap asked for by `QUANTICK_FEED_GAP`, in milliseconds of silence.
///
/// The seam a reconnect leaves is drawn only after a feed has dropped and come
/// back with real market time missing in between, which a scripted run cannot
/// arrange: it would have to break a live venue mid-capture and wait. So the
/// hook asks for one, and the tab records it through
/// [`Tab::record_gap`](crate::tab::Tab::record_gap) — the same function the
/// real path calls, at a market time taken from bars the chart really built,
/// so the mark lands where a real silence of that length would have put it.
///
/// Unset or unparseable means no demo gap, and a value under
/// [`MIN_MARKED_GAP_MS`] is refused rather than rounded up: a hook must not
/// photograph a mark the application would not have drawn.
#[must_use]
pub fn demo_gap_ms() -> Option<i64> {
    let requested: i64 = std::env::var("QUANTICK_FEED_GAP").ok()?.parse().ok()?;
    (requested >= MIN_MARKED_GAP_MS).then_some(requested)
}

/// The shortest silence worth marking, in milliseconds.
///
/// A reconnect always costs *something* — spawning a thread, opening a socket,
/// the bridge's own handshake — and a mark for every one of them would be
/// noise on the chart that taught the trader to stop reading marks. Five
/// seconds is longer than the recovery path takes when it works and shorter
/// than any silence a trader would want unlabelled.
pub const MIN_MARKED_GAP_MS: i64 = 5_000;

/// How many gaps one tab remembers.
///
/// Bounded because nothing else bounds it: a session left reconnecting on a
/// dead terminal could otherwise grow this list for hours. The newest are the
/// ones on screen, so the oldest is what falls off.
pub const MAX_REMEMBERED_GAPS: usize = 32;

/// The trades in `batch` the chart has not already seen, given the market time
/// it had already reached.
///
/// Every feed session replays a recent window when it connects — that is what
/// makes a reconnect useful — so resuming onto a timeline that was kept means
/// the overlap arrives a second time. Ids cannot separate the two: the
/// MetaTrader bridge restarts its synthetic ids on every session, and the only
/// key stable across sessions is time. So the rule `quantick-feed-mt5` already
/// applies *within* a session is applied here across one, and for every
/// provider: strictly newer than what the chart holds, or it is overlap.
///
/// A print sharing the floor's own millisecond is dropped with the rest.
/// Losing one same-millisecond tick to a reconnect is honest; silently
/// inflating a bar with a print it already counted is not.
///
/// `batch` must be ordered by timestamp, which every feed's batches are.
#[must_use]
pub fn past_resume_floor(batch: &[Trade], floor_ms: i64) -> &[Trade] {
    &batch[batch.partition_point(|trade| trade.timestamp_ms <= floor_ms)..]
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

/// Where a feed's delay is being spent, as far as the provider can tell.
///
/// The chart has always been able to say *how* late a print was — its timestamp
/// against the local clock, one number. That number cannot be acted on: a tape
/// running seconds behind looks the same whether the venue's own adapter got it
/// late, the transport carried it late, or the chart drained it late, and those
/// have different fixes. This is that number cut into the pieces the provider
/// can actually measure.
///
/// Every split is optional, and the whole struct is absent for a provider that
/// cannot cut its own chain. That is deliberate: an invented zero would read as
/// "this hop is instant", which is a measurement nobody took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedLatency {
    /// Newest print: venue stamp to the provider reading it off the wire.
    ///
    /// Measured at the provider, not at the chart. The chart keeps its own
    /// end-to-end figure (`Tab::trade_arrival_ms`), taken when the print is
    /// drained into a frame, and the *difference* between the two is what
    /// quantick's own queueing and drawing cost. Two measurements of two
    /// different things, and every surface that shows both says which is which.
    pub arrival_lag_ms: i64,
    /// Venue stamp to the source handing the print over — everything upstream
    /// of quantick.
    pub source_lag_ms: Option<i64>,
    /// Worst `source_lag_ms` over the sample.
    ///
    /// The only peak here, because it is the only one a provider can take
    /// without a clock: both stamps come from the source, per print. A peak on
    /// the arrival or wire figures would need the reader's clock applied to a
    /// print that arrived earlier, which measures that print's *age* rather
    /// than its delay — and on a quiet tape that is the sampling interval,
    /// reported as latency.
    pub source_lag_peak_ms: Option<i64>,
    /// The source handing it over to quantick reading it: the wire.
    pub transport_lag_ms: Option<i64>,
    /// The provider's own name for the hop that owns most of the delay.
    ///
    /// A borrowed name rather than a shared enum, because the chains differ:
    /// MetaTrader's runs venue → terminal → bridge → socket, a web-socket
    /// venue's does not have a terminal at all, and a recorded session has no
    /// chain to speak of. One enum covering all of them would either lose the
    /// detail that makes the reading actionable or grow a variant per provider,
    /// and this readout exists to be acted on.
    pub hop: Option<&'static str>,
    /// How many live prints the sample covers.
    pub prints: u32,
}

/// The latency channel of a feed that cannot split its own chain.
///
/// The sender is dropped immediately, so the receiver serves `None` forever and
/// the chart shows the end-to-end figure with no breakdown beside it.
#[must_use]
pub fn unsplit_latency() -> watch::Receiver<Option<FeedLatency>> {
    watch::channel(None).1
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
    /// Where this feed's delay is being spent, when the provider can tell.
    ///
    /// A `watch` rather than an event, because it is a *current reading* and
    /// not a thing that happened: a consumer that missed three samples wants
    /// the newest one, never the backlog. Its own channel for the same reason
    /// notices have one — a tape that has stopped arriving is exactly when the
    /// question "who is late" is being asked, and the trade channel is silent.
    ///
    /// `None` until the provider has something measured to say, and on every
    /// provider that cannot cut its own chain.
    pub latency: watch::Receiver<Option<FeedLatency>>,
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
    fn a_provider_that_cannot_split_publishes_nothing_forever() {
        // The other half of the port, exercised by a second implementation:
        // three of the four providers in this repo publish exactly this, and a
        // receiver whose sender is already gone has to keep answering `None`
        // rather than closing or panicking on the frame that reads it.
        let unsplit = unsplit_latency();
        assert_eq!(*unsplit.borrow(), None);
        assert_eq!(*unsplit.borrow(), None, "and on every frame after");
    }

    #[test]
    fn a_provider_that_can_split_publishes_its_newest_reading() {
        // A `watch`, not a queue: a consumer that missed three samples must
        // read the newest one, never a backlog of readings that are no longer
        // true. This is the behaviour the status bar and the health view both
        // depend on, and the reason the port is a watch at all.
        let (tx, rx) = watch::channel::<Option<FeedLatency>>(None);
        let reading = |ms: i64| FeedLatency {
            arrival_lag_ms: ms,
            source_lag_ms: Some(ms - 100),
            source_lag_peak_ms: Some(ms - 100),
            transport_lag_ms: Some(100),
            hop: Some("bridge"),
            prints: 64,
        };
        tx.send_replace(Some(reading(9_000)));
        tx.send_replace(Some(reading(300)));
        assert_eq!(
            rx.borrow().map(|split| split.arrival_lag_ms),
            Some(300),
            "the newest reading, not the first"
        );

        // A feed that stops leaves its last reading standing rather than
        // closing the channel out from under a frame that is mid-draw.
        drop(tx);
        assert_eq!(rx.borrow().map(|split| split.arrival_lag_ms), Some(300));
    }

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

    /// A batch at three timestamps, so the split can be read off the result.
    fn batch(stamps: &[i64]) -> Vec<Trade> {
        stamps
            .iter()
            .map(|&timestamp_ms| Trade {
                timestamp_ms,
                price: rust_decimal::Decimal::ONE,
                quantity: rust_decimal::Decimal::ONE,
                side: quantick_engine::Side::Buy,
                agg_id: 0,
            })
            .collect()
    }

    #[test]
    fn a_resumed_session_keeps_only_what_the_chart_has_not_seen() {
        let replayed = batch(&[10, 20, 30, 40]);
        let stamps = |kept: &[Trade]| kept.iter().map(|t| t.timestamp_ms).collect::<Vec<_>>();

        assert_eq!(
            stamps(past_resume_floor(&replayed, 20)),
            vec![30, 40],
            "everything at or before the floor is the window being replayed"
        );
        assert_eq!(
            stamps(past_resume_floor(&replayed, 5)),
            vec![10, 20, 30, 40],
            "a floor older than the batch keeps all of it"
        );
        assert!(
            past_resume_floor(&replayed, 40).is_empty(),
            "a batch entirely inside the overlap contributes nothing"
        );
    }

    /// The same-millisecond rule, stated in `past_resume_floor`'s own words:
    /// a print sharing the floor's millisecond is dropped rather than counted
    /// twice into a bar.
    #[test]
    fn a_print_on_the_floors_own_millisecond_is_overlap() {
        let replayed = batch(&[100, 100, 101]);
        assert_eq!(
            past_resume_floor(&replayed, 100)
                .iter()
                .map(|t| t.timestamp_ms)
                .collect::<Vec<_>>(),
            vec![101]
        );
    }

    #[test]
    fn a_gap_measures_the_silence_and_never_goes_backwards() {
        assert_eq!(
            FeedGap {
                from_ms: 1_000,
                to_ms: 61_000
            }
            .duration_ms(),
            60_000
        );
        assert_eq!(
            FeedGap {
                from_ms: 5_000,
                to_ms: 1_000
            }
            .duration_ms(),
            0,
            "a venue clock that stepped back is not a negative silence"
        );
    }
}
