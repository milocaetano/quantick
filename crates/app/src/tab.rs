//! One open market, owned wholesale.
//!
//! §11 of `docs/ux/ui-design-model.md` draws the line: a tab owns its feed
//! connection and channels, its panes and their drawings and indicator slots,
//! its replay link, its notices and its loading state. Nothing market-scoped
//! lives outside one. What stays in the window around it is chrome that is
//! single-instance by nature — one menu bar, one toolbox, one dock, one
//! appearance, one status line — plus the indicator *persistence* layer, which
//! describes a workspace rather than a market.
//!
//! Tabs multiply markets; the split inside a tab ([`crate::pane`]) multiplies
//! views of one. The two are orthogonal, and a tab carries its own layout.

use eframe::egui;
use smallvec::SmallVec;
use tokio::sync::{mpsc, watch};

use quantick_feed_binance::depth::DepthEvent;

use crate::config::{AppConfig, FeedCapabilities};
use crate::feed::{
    self, FeedCommand, FeedConnectionState, FeedEvent, FeedHandle, FeedNotice, ReplayLink,
};
use crate::loading::{LoadingTask, LoadingTracker};
use crate::metrics;
use crate::orderflow_view::OrderflowView;
use crate::pane::{
    CANVAS_DIVIDER_HANDLE_PX, ChartPane, DEFAULT_PANE_FRACTION, DrawingDrag, PaneChrome, PaneSide,
    SharedEdit, SharedInteraction, SharedPick, clamp_pane_fraction, split_canvas, split_time_pane,
};
use crate::paper_trading::PaperTrading;
use crate::state::{BarKind, BarSpec};
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;
use std::path::PathBuf;

/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
pub const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
const BOOK_DRAIN_BUDGET: usize = 2_048;
/// Thickness of the rule marking the focused pane (§11: an accent under the
/// pane's top edge, never a box drawn around market data).
const FOCUS_RULE_PX: f32 = 1.0;

/// How many charts a tab's canvas shows for its market (§11), and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasLayout {
    /// The flow pane alone — quantick's default and its identity.
    #[default]
    Single,
    /// The time pane alone: a full-window timeframe chart, header included,
    /// with no split. The flow pane keeps being fed off screen, exactly as
    /// the time pane does while Single is showing.
    Time,
    /// Time pane left, flow pane right, on a draggable divider.
    TimeAndFlow,
}

impl CanvasLayout {
    /// Whether this layout draws the time pane at all.
    #[must_use]
    pub fn shows_time(self) -> bool {
        matches!(self, CanvasLayout::Time | CanvasLayout::TimeAndFlow)
    }

    /// Whether this layout draws the flow pane at all.
    #[must_use]
    pub fn shows_flow(self) -> bool {
        matches!(self, CanvasLayout::Single | CanvasLayout::TimeAndFlow)
    }
}

impl From<crate::config::DeclaredLayout> for CanvasLayout {
    fn from(declared: crate::config::DeclaredLayout) -> Self {
        match declared {
            crate::config::DeclaredLayout::Flow => CanvasLayout::Single,
            crate::config::DeclaredLayout::Time => CanvasLayout::Time,
            crate::config::DeclaredLayout::TimeAndFlow => CanvasLayout::TimeAndFlow,
        }
    }
}

/// The way back, for the saved workspace ([`crate::ui_state`]), which has to
/// write a layout out in the vocabulary a config reads.
///
/// A canvas layout a config should not name would have to answer for itself
/// here — which is the point of keeping the two enums apart, and the reason
/// this conversion is total today and may not always be.
impl From<CanvasLayout> for crate::config::DeclaredLayout {
    fn from(layout: CanvasLayout) -> Self {
        match layout {
            CanvasLayout::Single => crate::config::DeclaredLayout::Flow,
            CanvasLayout::Time => crate::config::DeclaredLayout::Time,
            CanvasLayout::TimeAndFlow => crate::config::DeclaredLayout::TimeAndFlow,
        }
    }
}

/// The window chrome a tab's canvas borrows for one frame. The tab completes
/// it with its own symbol to make the [`PaneChrome`] its panes read.
pub struct CanvasChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a crate::drawings::presets::PresetStore,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// What the running source can produce, for the layer menu's disabled
    /// entries. Resolved once by the app rather than per pane, per entry.
    pub capabilities: FeedCapabilities,
    /// Where a pane's layer menu leaves the switches it does not own.
    pub layers: &'a mut crate::chart_layers::LayerActions,
}

/// Drop venue bars that overlap the trade-derived series.
///
/// The two series meet at a seam, and the composed chart is only searchable if
/// `open_time` never decreases across it. A venue candle covering the same
/// window as the first engine bar would sit *after* it in time while sitting
/// before it in slot order, so every venue bucket from that one on is dropped:
/// what the app cut from prints is the better record of that window anyway.
///
/// With no engine bars yet the whole prefix stands — there is nothing to
/// overlap.
fn trim_to_seam(
    mut folded: Vec<quantick_engine::Bar>,
    first_engine_bar: Option<&quantick_engine::Bar>,
    partial: Option<&quantick_engine::Bar>,
    interval_ms: i64,
) -> Vec<quantick_engine::Bar> {
    let Some(first) = first_engine_bar.or(partial) else {
        return folded;
    };
    // Buckets, not stamps. A venue candle's `open_time` is its bucket start; an
    // engine bar's is its *first trade*, which sits strictly inside the bucket.
    // Comparing the two raw would keep the venue candle covering the same
    // window and put a later-closing bar in an earlier slot.
    let seam = crate::resample::bucket_start(first.open_time, interval_ms);
    folded.retain(|bar| bar.open_time < seam);
    folded
}

/// One open market. See the module docs for what does and does not live here.
/// One frame's answer, for both panes, to "what shared mark of the *other*
/// pane is the pointer over?".
///
/// A pair rather than a per-pane field because the question can only be asked
/// while both panes are in hand, and it is asked once for the frame.
#[derive(Debug, Clone, Copy, Default)]
struct SharedPicks {
    time: Option<SharedPick>,
    flow: Option<SharedPick>,
}

impl SharedPicks {
    const fn for_side(self, side: PaneSide) -> Option<SharedPick> {
        match side {
            PaneSide::Time => self.time,
            PaneSide::Flow => self.flow,
        }
    }
}

pub struct Tab {
    /// Stable for as long as the tab is open, and never reused. The indicator
    /// state file names one of these (see `QuantickApp::persisted_tab`), and
    /// per-tab chrome persistence (§14, `ui-state.toml`) would key off it too.
    pub id: u64,

    // Feed & asset selection, driven by the configuration. `feed_id`/`symbol`
    // are what the selectors show (the desired selection); `active` is what the
    // running feed thread is actually streaming. When they diverge, the feed is
    // respawned. Nothing here is hard-coded — it all comes from `config`.
    pub feed_id: String,
    pub symbol: String,
    pub active: (String, String),

    pub events: mpsc::Receiver<FeedEvent>,
    pub book_events: mpsc::Receiver<DepthEvent>,
    /// Connection trouble the feed wants the user to know about.
    pub notices: mpsc::Receiver<FeedNotice>,
    /// The newest notice, held until the feed says it is over. A feed that
    /// blocks once and then goes quiet has to keep saying so — the chart it
    /// left empty will not.
    pub notice: FeedNotice,
    /// State reported by the live trade transport, independent from how often
    /// that market prints and from the last observed arrival latency.
    pub feed_connection: FeedConnectionState,
    /// What the running feed can really do, read fresh every frame. The feed
    /// narrows it once a session tells it what the symbol actually offers.
    pub feed_capabilities: watch::Receiver<FeedCapabilities>,
    pub commands: mpsc::Sender<FeedCommand>,

    // Market Replay. `replay` is `Some` exactly while a recorded session is
    // this tab's source; it is the one flag the rest of the UI checks, so
    // replay never grows a second copy of "which mode are we in". Per tab: a
    // recording plays in the tab that opened it while the others keep
    // streaming, on their own feed threads and their own clocks.
    pub replay: Option<ReplayLink>,

    // How many older trades to pull per "load older" click, and how many
    // trades have been backfilled in total (for the readout).
    pub history_step: usize,
    pub history_trades: usize,
    // Every wait currently in flight in this tab, drawn by one overlay (see
    // crate::loading) while the tab is on screen.
    pub loading: LoadingTracker,

    pub book_capture_epoch: u64,
    pub book_channel_closed_reported: bool,

    /// Exchange-to-UI delay measured when the newest live trade arrived.
    /// Stable while the tape is quiet: market inactivity is not transport lag.
    pub latest_trade_latency_ms: Option<i64>,
    /// Timestamp of the newest live trade (epoch ms), for the tape-age
    /// readout. The latency above is an observation frozen at arrival; this
    /// is what wall clock is compared against every frame.
    pub latest_trade_ms: Option<i64>,
    pub live_trades: u64,

    /// Paper trading for this market: the deterministic simulator plus its
    /// journal, chart layer, dock tab and report.
    ///
    /// Per tab, because a simulated position belongs to a tape. Two tabs on
    /// two markets hold two independent positions, and a position can never
    /// be marked against prints it was not opened against — the invariant
    /// [`PaperTrading::on_timeline_reset`] protects when one tab switches
    /// symbol is the same one tab-scoping protects between tabs.
    pub paper: PaperTrading,

    /// quantick's own chart, and the only one in the default layout.
    pub flow_pane: ChartPane,
    /// The context chart beside it (§11), built the first time the split is
    /// shown and kept for as long as the tab lives — switching back to Single
    /// hides it, and must not throw away its indicators and drawings.
    ///
    /// While it exists it is fed every trade the flow pane is fed, on screen
    /// or not, which is what keeps the two in step. The cost is the market's
    /// trades retained twice: one tape, two `ChartState`s, and still only one
    /// bar-building path.
    pub time_pane: Option<ChartPane>,
    /// `SYMBOL · venue`, as the strip shows it — see [`Self::chip_label`].
    chip_label: String,
    /// The venue's own 1-minute candles for this market, fetched once and
    /// folded locally to whatever interval the time pane shows.
    ///
    /// `None` until a reply lands; `Some(empty)` after one that carried
    /// nothing, which is what keeps a failed or unsupported fetch from being
    /// retried every frame. Held by the tab rather than the pane because it is
    /// the *market's* history: changing the pane's interval refolds it, and
    /// only a change of market throws it away.
    ohlcv_base: Option<Vec<quantick_engine::Bar>>,
    /// Whether a fetch is out. One at a time — the reply is what clears it,
    /// and every provider always sends one.
    ohlcv_pending: bool,
    /// The candle generation this tab has already acted on.
    ///
    /// A pull feed leaves it at zero forever — it answers whenever asked, so
    /// nothing changes behind us. A push feed moves it every time it stores a
    /// block, including a replacement for one already delivered, and that is
    /// the only signal saying "the answer changed, ask again". A rising
    /// *capability* edge cannot say it: the flag rises once and stays, so a
    /// block arriving after an empty answer would sit unread.
    ohlcv_generation: u64,
    /// What `ohlcv_history` said last frame, so the rising edge can be seen.
    ///
    /// MetaTrader narrows its capabilities when the bridge says hello, which
    /// happens *after* the pane may already have asked and been answered
    /// `nothing_held`. The edge is what asks again once the answer can be a
    /// real one.
    ohlcv_capable: bool,
    /// The id the time pane takes when this tab first shows the split.
    time_pane_id: u64,
    /// The interval the time pane opens on when it is first built. The
    /// header's default unless the feed declared one (`default_bars` with a
    /// time-showing `default_layout`); once the pane exists, its own header
    /// owns the interval and this is never read again.
    time_pane_opening_interval_ms: i64,
    /// Set when the split is asked for and the time pane does not exist yet;
    /// drained by [`Self::apply_pending_layout`] on the following frame.
    pending_time_pane: bool,
    /// Which panes this tab's canvas shows. In-session only for now: per-tab
    /// chrome persistence is the open question §14 leaves to `ui-state.toml`,
    /// and this field with `split_fraction` and `focus` is what it would
    /// write.
    pub layout: CanvasLayout,
    /// The time pane's share of the canvas width while the split is shown.
    pub split_fraction: f32,
    /// The pane the chrome speaks for while this tab is active: status bar,
    /// indicator targeting and the keyboard's drawing grammar (§11).
    /// Meaningless while the canvas is Single — read it through
    /// [`Self::focused_side`], never directly.
    pub focus: PaneSide,

    #[cfg(test)]
    time_header_chips: [egui::Rect; crate::time_header::PRESETS.len()],
    #[cfg(test)]
    canvas_divider: Option<egui::Rect>,
}

impl Tab {
    /// A tab on `feed_id`/`symbol`, already streaming through `feed`, showing
    /// bar `spec`.
    ///
    /// `id` and `pane_ids` must be unique among the open tabs: pane ids
    /// namespace egui interaction state, so two tabs sharing them would share
    /// a drag.
    #[must_use]
    pub fn new(
        id: u64,
        pane_ids: (u64, u64),
        feed_id: String,
        symbol: String,
        spec: BarSpec,
        feed: FeedHandle,
        trades_dir: PathBuf,
    ) -> Self {
        let mut loading = LoadingTracker::new();
        // The feed starts backfilling the moment it is spawned, so the tab
        // opens with that one load already in flight.
        loading.begin(LoadingTask::History);
        Self {
            id,
            active: (feed_id.clone(), symbol.clone()),
            feed_id,
            events: feed.events,
            book_events: feed.book_events,
            notices: feed.notices,
            notice: FeedNotice::Clear,
            feed_connection: FeedConnectionState::Connecting,
            feed_capabilities: feed.capabilities,
            commands: feed.commands,
            replay: feed.replay,
            history_step: 2000,
            history_trades: 0,
            loading,
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            latest_trade_latency_ms: None,
            latest_trade_ms: None,
            live_trades: 0,
            paper: PaperTrading::with_trades_dir(trades_dir),
            flow_pane: ChartPane::flow(pane_ids.0, spec, symbol.clone()),
            chip_label: String::new(),
            ohlcv_base: None,
            ohlcv_pending: false,
            ohlcv_generation: 0,
            ohlcv_capable: false,
            time_pane: None,
            time_pane_id: pane_ids.1,
            time_pane_opening_interval_ms: crate::time_header::DEFAULT_INTERVAL_MS,
            pending_time_pane: false,
            layout: CanvasLayout::Single,
            split_fraction: DEFAULT_PANE_FRACTION,
            focus: PaneSide::Flow,
            symbol,
            #[cfg(test)]
            time_header_chips: [egui::Rect::NOTHING; crate::time_header::PRESETS.len()],
            #[cfg(test)]
            canvas_divider: None,
        }
    }

    /// Take over a freshly spawned feed: channels, capabilities, commands and
    /// replay link in one move.
    ///
    /// The old handle goes with the old feed thread, which stops when its
    /// receivers drop. The old feed's trouble is not the new feed's, so the
    /// notice and the transport state start clean — switching away from a
    /// blocked source must not leave its instruction on screen.
    fn attach(&mut self, handle: FeedHandle) {
        // The old market's candles describe the old market. Any reply still in
        // flight belongs to a channel that is about to be dropped, so the wait
        // restarts rather than draining to zero on an answer that never comes.
        self.ohlcv_base = None;
        self.ohlcv_pending = false;
        self.ohlcv_capable = false;
        self.loading.set_active(LoadingTask::VenueHistory, false);
        for pane in std::iter::once(&mut self.flow_pane).chain(self.time_pane.as_mut()) {
            pane.install_history_prefix(Vec::new());
        }
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;
    }

    /// Drop the transient pointer state of every pane's overlay, for a change
    /// that re-cuts the bars under it — a spec switch, a source reset.
    ///
    /// The *objects* survive: their anchors carry market time, so they are
    /// re-expressed against the new series rather than discarded (`ChartPane::
    /// reanchor_drawings`). A drawing belongs to the trader who placed it and
    /// leaves when they delete it, not when the chart is re-cut underneath.
    ///
    /// What cannot survive is a gesture in flight: a half-finished drag is
    /// holding pixel coordinates of bars that no longer exist there.
    fn drop_overlay_gestures(&mut self) {
        for pane in self.panes_mut() {
            pane.drawing_hover = None;
            pane.drawing_press_position = None;
            pane.drawing_press_started_empty = false;
            pane.drawing_drag = DrawingDrag::None;
        }
    }

    /// Where the timeframe chips landed, in `crate::time_header::PRESETS` order.
    #[cfg(test)]
    pub(crate) fn time_header_chip(&self, index: usize) -> Option<egui::Rect> {
        self.time_header_chips.get(index).copied()
    }

    /// Where the canvas divider landed, while the split is shown.
    #[cfg(test)]
    pub(crate) fn canvas_divider_rect(&self) -> Option<egui::Rect> {
        self.canvas_divider
    }

    /// Forget which candle generation was acted on, so the next poll treats
    /// the feed's as new — what a reconnect storing a fresh block does.
    #[cfg(test)]
    pub fn forget_ohlcv_generation_for_test(&mut self) {
        self.ohlcv_generation = u64::MAX;
    }

    /// Swap in a feed the test drives, through the same path a respawn takes.
    #[cfg(test)]
    pub fn attach_for_test(&mut self, handle: FeedHandle) {
        self.attach(handle);
    }

    /// Whether any pane on this tab cuts bars by a foldable time interval —
    /// the gate for venue candle history. Capability-shaped, like every other
    /// gate in the app (audit S1): the prefix belongs to what a pane *shows*
    /// (`BarSpec::Time` at a whole number of venue candles), never to which
    /// pane object it is. `bars → time` on the flow pane earns the same 90
    /// days the split's time pane gets.
    fn any_pane_wants_venue_history(&self) -> bool {
        std::iter::once(&self.flow_pane)
            .chain(self.time_pane.as_ref())
            .any(|pane| {
                pane.state
                    .spec()
                    .time_interval_ms()
                    .is_some_and(crate::resample::is_foldable)
            })
    }

    /// Ask the venue for its candle history, if there is anything to ask.
    ///
    /// Gated on the capability, never on the provider: a feed that serves no
    /// candles, and a recording — which is a fixed span of prints with no
    /// venue behind it — are both simply not asked. One request at a time, and
    /// a base already held is not re-fetched: changing a pane's interval is
    /// a different fold over the same bars.
    fn request_ohlcv_history(&mut self, config: &AppConfig) {
        if !self.any_pane_wants_venue_history()
            || self.replay.is_some()
            || self.ohlcv_pending
            || self.ohlcv_base.is_some()
            || !self.capabilities(config).ohlcv_history
        {
            return;
        }
        let command = FeedCommand::FetchOhlcv {
            span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
        };
        match self.commands.try_send(command) {
            Ok(()) => {
                self.ohlcv_pending = true;
                self.loading.begin(LoadingTask::VenueHistory);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_REQUESTED",
                    tab = self.id,
                    symbol = %self.symbol,
                    span_ms = crate::feed::TIME_HISTORY_SPAN_MS,
                    action = "await_single_reply",
                    "asked the venue for candle history"
                );
            }
            // A full channel is a busy frame, and the every-frame poll asks
            // again. A closed one is a feed thread that is gone, which is
            // worth saying out loud: nothing will answer, ever.
            Err(mpsc::error::TrySendError::Full(_)) => tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_REQUEST_BACKPRESSURE",
                tab = self.id,
                action = "retry_next_frame",
                "candle-history request not queued; channel full"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REQUEST_CHANNEL_CLOSED",
                tab = self.id,
                symbol = %self.symbol,
                action = "no_history_until_feed_restart",
                "candle-history request cannot be sent; the feed is gone"
            ),
        }
    }

    /// Watch the capability for the false→true edge and ask when it lands.
    ///
    /// Called every frame: the check is two bools and an `Option` when there
    /// is nothing to do.
    pub fn poll_ohlcv_capability(&mut self, config: &AppConfig) {
        let capabilities = self.capabilities(config);
        let capable = capabilities.ohlcv_history;
        let rising = capable && !self.ohlcv_capable;
        self.ohlcv_capable = capable;
        if capabilities.ohlcv_generation != self.ohlcv_generation {
            // The venue re-answered. A reconnect can carry a longer block than
            // the one held, or a corrected one, so what is held goes whether or
            // not it had bars in it — the guard below then lets a fresh request
            // through, and the reply reinstalls the prefix by the same path the
            // first one took.
            self.ohlcv_generation = capabilities.ohlcv_generation;
            self.ohlcv_base = None;
        }
        if rising {
            // A session that narrowed *into* serving candles may have answered
            // an earlier request with nothing; that answer described a feed
            // that did not know itself yet. Only the edge clears it — a
            // steady-state empty base is a real "the venue has none".
            if self.ohlcv_base.as_ref().is_some_and(Vec::is_empty) {
                self.ohlcv_base = None;
            }
        }
        // Unconditional: every guard inside makes this a no-op once the
        // request is out or answered, and asking here is what actually retries
        // a request the command channel refused. The feed ignores a duplicate
        // while one is in flight, and `ohlcv_pending` means we never send one.
        self.request_ohlcv_history(config);
    }

    /// Take a candle-history reply, and put it in front of the time pane.
    ///
    /// An empty reply is a complete answer — the venue has none, the provider
    /// serves none, or the fetch failed — and is recorded as such so the tab
    /// stops asking. Either way the wait ends here: a provider that answered
    /// only on success would strand the spinner on the one case that most
    /// needs explaining.
    fn take_ohlcv_history(
        &mut self,
        interval_ms: i64,
        bars: Vec<quantick_engine::Bar>,
        complete: bool,
    ) {
        self.ohlcv_pending = false;
        if !complete {
            // Known-short, not merely short: a venue that stopped answering
            // partway, or a block clipped to a cap. An instrument younger than
            // the span is a *complete* answer with fewer bars, which is why
            // this is carried rather than guessed from the count.
            //
            // Said in the log today. §11 records where it will be said on
            // screen — beside the three-way bar count — and the badge itself
            // is deferred rather than half-built.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_INCOMPLETE",
                tab = self.id,
                symbol = %self.symbol,
                interval_ms,
                bars = bars.len(),
                action = "install_what_arrived",
                "the venue's candle history stopped short of the span asked for"
            );
        }
        self.loading.end(LoadingTask::VenueHistory);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "OHLCV_RECEIVED",
            tab = self.id,
            symbol = %self.symbol,
            interval_ms,
            bars = bars.len(),
            action = if bars.is_empty() { "no_prefix" } else { "install_prefix" },
            "candle history arrived"
        );
        if interval_ms != crate::feed::OHLCV_BASE_INTERVAL_MS && !bars.is_empty() {
            // The event tags its own interval so a consumer never has to
            // guess; a base this fold was not written for is refused rather
            // than folded wrongly.
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_UNEXPECTED_BASE",
                interval_ms,
                expected_ms = crate::feed::OHLCV_BASE_INTERVAL_MS,
                action = "no_prefix",
                "candle history arrived at an interval the pane cannot fold from"
            );
            self.ohlcv_base = Some(Vec::new());
            return;
        }
        self.ohlcv_base = Some(bars);
        self.refold_history_prefix();
    }

    /// Rebuild every pane's prefix from the base at that pane's interval.
    ///
    /// Free of the venue: a chip click lands here, not on the network. One
    /// base, one fold per time-cutting pane — the flow pane showing time
    /// bars folds exactly as the split's time pane does (audit S1).
    /// Reports whether any prefix actually changed — an installed prefix
    /// rebuilds the indicators, so the caller can skip sending a second one.
    pub fn refold_history_prefix(&mut self) -> bool {
        let Some(base) = self.ohlcv_base.as_ref() else {
            return false;
        };
        let mut changed = false;
        for pane in std::iter::once(&mut self.flow_pane).chain(self.time_pane.as_mut()) {
            // A pane not cutting by time has no interval to fold to, and a
            // sub-minute one has no whole number of venue candles in it: both
            // get no prefix, which is the honest answer rather than an
            // invented one.
            let prefix = match pane.state.spec().time_interval_ms() {
                Some(interval) => {
                    let folded = crate::resample::fold(base, interval);
                    trim_to_seam(
                        folded,
                        pane.state.bars().first(),
                        pane.state.partial(),
                        interval,
                    )
                }
                None => Vec::new(),
            };
            changed |= pane.install_history_prefix(prefix);
        }
        changed
    }

    /// Whether this tab has trouble worth showing on its chip while it sits in
    /// the background (§11: honesty at a glance).
    ///
    /// A recording never reports transport trouble — it has no transport — and
    /// "still connecting" is not trouble either. This is a feed that had a
    /// connection and lost it, or one asking the user to fix something.
    #[must_use]
    pub fn needs_attention(&self) -> bool {
        self.replay.is_none()
            && (self.feed_connection == FeedConnectionState::Reconnecting
                || matches!(self.notice, FeedNotice::Attention { .. }))
    }

    /// The pane the chrome speaks for. Only a split canvas has a choice to
    /// make: a single-pane layout *is* its one visible pane, whatever the
    /// last split left `focus` set to. Time falls back to the flow pane for
    /// the frame between asking for the layout and the pane being built.
    pub fn focused_side(&self) -> PaneSide {
        match (self.layout, self.time_pane.is_some()) {
            (CanvasLayout::TimeAndFlow, true) => self.focus,
            (CanvasLayout::Time, true) => PaneSide::Time,
            _ => PaneSide::Flow,
        }
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    pub fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time => self.time_pane.as_ref().unwrap_or(&self.flow_pane),
            PaneSide::Flow => &self.flow_pane,
        }
    }

    /// See [`Self::pane`].
    pub fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time => self.time_pane.as_mut().unwrap_or(&mut self.flow_pane),
            PaneSide::Flow => &mut self.flow_pane,
        }
    }

    /// The pane every chrome surface reads from — see [`Self::focused_side`].
    pub fn focused_pane(&self) -> &ChartPane {
        self.pane(self.focused_side())
    }

    /// See [`Self::focused_pane`].
    pub fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.focused_side())
    }

    /// The pane holding the drawing selection — which is not always the
    /// focused one.
    ///
    /// A shared mark can be taken from either chart it appears on, and it
    /// stays in the store of the pane it was drawn on. So the inspector, the
    /// keyboard and the object manager follow the *object*, not the pane the
    /// pointer happens to be over: selecting a level on the time pane and
    /// pressing Delete has to delete that level, wherever it lives.
    ///
    /// Exactly one pane holds a selection at a time
    /// ([`Self::apply_shared_interactions`] drops the other's), so this asks
    /// the focused pane first and takes the answer it finds.
    pub fn drawing_side(&self) -> PaneSide {
        let focused = self.focused_side();
        if self.pane(focused).drawings.selected().is_some() || self.time_pane.is_none() {
            return focused;
        }
        let other = focused.other();
        if self.pane(other).drawings.selected().is_some() {
            return other;
        }
        focused
    }

    /// The pane every drawing surface reads from — see [`Self::drawing_side`].
    pub fn drawing_pane(&self) -> &ChartPane {
        self.pane(self.drawing_side())
    }

    /// See [`Self::drawing_pane`].
    pub fn drawing_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.drawing_side())
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut ChartPane> {
        std::iter::once(&mut self.flow_pane).chain(self.time_pane.as_mut())
    }

    /// The flow pane's tape.
    ///
    /// The flow pane is built with one ([`ChartPane::flow`]) and never gives it
    /// up; the `Option` on the pane exists so a *time* pane can go without a
    /// book worker, not because this one can be missing.
    pub fn tape(&self) -> &OrderflowView {
        self.flow_pane
            .orderflow
            .as_ref()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// See [`Self::tape`].
    pub fn tape_mut(&mut self) -> &mut OrderflowView {
        self.flow_pane
            .orderflow
            .as_mut()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// Switch which panes the canvas shows (§11).
    ///
    /// The first layout that needs the time pane builds it and seeds it from
    /// the trades the flow pane already holds, so it opens showing the same
    /// market rather than an empty chart waiting for the next print. Leaving
    /// a layout only stops drawing its pane: indicators, drawings and bars
    /// survive, and the pane keeps being fed, so re-showing it never has to
    /// catch up.
    ///
    /// Focus follows what the switch reveals: the pane that just appeared is
    /// the one the user asked to work on, so the chrome speaks for it without
    /// a second click. A switch that reveals nothing new (Time + Flow from
    /// Time + Flow) keeps the focus where it was.
    pub fn set_layout(&mut self, layout: CanvasLayout) {
        let previous = self.layout;
        if layout == previous {
            return;
        }
        self.layout = layout;
        if layout.shows_time() && self.time_pane.is_none() {
            // Seeding replays every retained trade, which on a deep history
            // holds the render thread long enough to notice. Armed here and
            // done on the next frame, exactly as a bar-spec change is: the
            // frame carrying the menu click paints the loading overlay first,
            // so the wait reads as the chart working rather than the app
            // hanging.
            self.pending_time_pane = true;
            self.loading.begin(LoadingTask::BarRebuild);
        }
        self.focus = match layout {
            CanvasLayout::Single => PaneSide::Flow,
            CanvasLayout::Time => PaneSide::Time,
            // The split reveals whichever pane the previous layout was not
            // showing: the time pane coming from Single, the flow pane coming
            // from Time.
            CanvasLayout::TimeAndFlow => match previous {
                CanvasLayout::Single => PaneSide::Time,
                CanvasLayout::Time => PaneSide::Flow,
                CanvasLayout::TimeAndFlow => self.focus,
            },
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANVAS_LAYOUT",
            layout = ?layout,
            time_pane_bars = self.time_pane.as_ref().map(|pane| pane.state.bars().len()),
            action = if self.pending_time_pane {
                "build_time_pane_next_frame"
            } else {
                "relayout_canvas"
            },
            "canvas layout changed"
        );
    }

    /// Build the time pane the last layout change asked for, if one is due.
    ///
    /// Runs at the top of the frame after the click, so the overlay armed by
    /// [`Self::set_layout`] has already been painted once.
    pub fn apply_pending_layout(&mut self, config: &AppConfig) {
        if !self.pending_time_pane {
            return;
        }
        self.pending_time_pane = false;
        let mut pane = ChartPane::time(self.time_pane_id, self.time_pane_opening_interval_ms);
        pane.seed_from(
            self.flow_pane.state.trades(),
            self.flow_pane.state.backfill_trade_count(),
        );
        // The pane opens looking like the one it splits away from: a user who
        // switched the crosshair off is not asking for it back by opening a
        // second view of the same market.
        pane.hidden_layers = self.flow_pane.hidden_layers.clone();
        self.time_pane = Some(pane);
        self.loading.end(LoadingTask::BarRebuild);
        // The pane exists now, so there is something for a prefix to go in
        // front of. A base already held (a layout toggled off and on) is
        // folded rather than re-fetched; otherwise this is the first moment
        // asking for one means anything.
        if self.ohlcv_base.is_some() {
            self.refold_history_prefix();
        } else {
            self.request_ohlcv_history(config);
        }
    }

    /// The display name of the currently selected feed, or its id as a
    /// fallback.
    pub fn feed_display_name<'a>(&'a self, config: &'a AppConfig) -> &'a str {
        config.feed_name(&self.feed_id)
    }

    /// This tab's chip label, `SYMBOL · venue`.
    ///
    /// Composed when the market changes rather than every frame: the strip
    /// redraws at frame rate and the string only moves when the selection
    /// does. [`Self::refresh_chip_label`] is what keeps the two in step.
    #[must_use]
    pub fn chip_label(&self) -> &str {
        &self.chip_label
    }

    /// Recompose the chip label after a write to `feed_id` or `symbol`.
    pub fn refresh_chip_label(&mut self, config: &AppConfig) {
        let venue = config.feed_name(&self.feed_id);
        self.chip_label.clear();
        self.chip_label.push_str(&self.symbol);
        self.chip_label.push_str(" · ");
        self.chip_label.push_str(venue);
    }

    /// Keep `symbol` valid for the selected feed: if the feed changed and no
    /// longer offers the current symbol, fall back to its first symbol.
    pub fn ensure_symbol_valid(&mut self, config: &AppConfig) {
        if let Some(symbol) = config.resolve_symbol(&self.feed_id, &self.symbol) {
            self.symbol = symbol;
        }
    }

    /// What the selected feed's backend can do.
    ///
    /// A feed missing from the config can do nothing — the selection is snapped
    /// back on the next switch, and until then no affordance may promise data
    /// nothing is streaming.
    pub fn capabilities(&self, config: &AppConfig) -> FeedCapabilities {
        // A feed missing from the config resolves to no provider, so nothing is
        // streaming and nothing may be promised.
        if config.provider_of(&self.feed_id).is_none() && self.replay.is_none() {
            return FeedCapabilities::none();
        }
        // Otherwise the running feed answers for itself. Each source declares
        // what it is — a recording has trades and no depth, a bridge session
        // knows whether its symbol has a book or a tape — and every affordance
        // already asks the capability rather than the provider name, so they
        // enable and disable themselves from this one value.
        *self.feed_capabilities.borrow()
    }

    /// Ask the feed thread to fetch and prepend `history_step` older trades.
    /// Non-blocking: if a request is already queued, this frame's click is
    /// dropped rather than piling up commands.
    pub fn request_older_history(&mut self) {
        match self.commands.try_send(FeedCommand::LoadOlder {
            count: self.history_step.max(1),
        }) {
            Ok(()) => {
                self.loading.begin(LoadingTask::History);
                tracing::info!(
                    target: "quantick::app",
                    count = self.history_step,
                    "requested older history"
                );
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(target: "quantick::app", "older-history request already pending");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(target: "quantick::app", "feed command channel closed");
            }
        }
    }

    /// Allocate a capture generation well above all reconnect generations from
    /// the previous UI capture epoch.
    pub fn next_book_generation(&mut self) -> u64 {
        self.book_capture_epoch = self.book_capture_epoch.saturating_add(1);
        self.book_capture_epoch
            .saturating_mul(BOOK_GENERATION_STRIDE)
    }

    /// Keep the recorder running for any feed that can stream depth.
    ///
    /// Capture is a data concern: it starts with the feed and stops only when
    /// the market itself changes (feed/symbol switch, or a replay taking the
    /// chart over). Showing and hiding the map never reaches this far, which is
    /// what lets a hidden heatmap come back with its history intact.
    ///
    /// Recording with nobody watching stays inside the retention budget the
    /// heatmap already had — `retention_ms` (30 min by default) bounded by
    /// `max_history_runs` / `max_history_bytes` — so the ceiling is the same
    /// one an open map pays for, not a new one.
    ///
    /// Idempotent and cheap, so the frame loop can call it as a heartbeat on
    /// top of the lifecycle calls: already recording costs one bool read, and
    /// a replay costs one more `Option` check.
    pub fn ensure_book_capture(&mut self, config: &AppConfig) {
        if self.tape().enabled() || !self.capabilities(config).book_capture {
            return;
        }
        self.request_book_capture(config, true);
    }

    /// Start or stop the independent depth pipeline without touching aggTrades
    /// or candle construction. UI state changes only if the command is queued.
    pub fn request_book_capture(&mut self, config: &AppConfig, enabled: bool) {
        if !self.capabilities(config).book_capture {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROVIDER_UNSUPPORTED",
                feed = self.feed_id.as_str(),
                symbol = self.symbol.as_str(),
                enabled,
                action = "leave_capture_disabled",
                "selected provider has no order-book pipeline"
            );
            return;
        }

        let generation = self.next_book_generation();
        let command = FeedCommand::SetBookCapture {
            enabled,
            initial_generation: generation,
        };
        match self.commands.try_send(command) {
            Ok(()) => self.tape_mut().set_enabled(enabled, generation),
            Err(mpsc::error::TrySendError::Full(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_BACKPRESSURE",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "retry_on_next_frame",
                "book capture command channel is full"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_COMMAND_CHANNEL_CLOSED",
                symbol = self.symbol.as_str(),
                enabled,
                generation,
                action = "keep_current_capture_state",
                "book capture command channel is closed"
            ),
        }
    }

    /// Restart capture after a semantic configuration change such as base
    /// price grouping. The view commits its staged reset only after this
    /// command is accepted, preserving current history on backpressure.
    pub fn restart_book_capture(&mut self) {
        if !self.tape().enabled() {
            return;
        }
        let generation = self.next_book_generation();
        match self.commands.try_send(FeedCommand::RestartBookCapture {
            initial_generation: generation,
        }) {
            Ok(()) => self.tape_mut().accept_capture_grouping_restart(generation),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.tape_mut()
                    .reject_capture_grouping_restart("command_channel_full");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_BACKPRESSURE",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is full"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.tape_mut()
                    .reject_capture_grouping_restart("command_channel_closed");
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_RESTART_CHANNEL_CLOSED",
                    symbol = self.symbol.as_str(),
                    generation,
                    action = "keep_existing_capture",
                    "book restart command channel is closed"
                );
            }
        }
    }

    /// Respawn the feed and reset the chart when the selected feed or symbol
    /// differs from what is currently streaming. A no-op otherwise.
    pub fn maybe_switch_feed(&mut self, config: &AppConfig) {
        // A replay owns the chart until it is closed. The selectors are not
        // drawn while it plays, so nothing can diverge here — but a stale
        // selection must not respawn a live feed underneath the recording.
        if self.replay.is_some() {
            return;
        }
        if self.active == (self.feed_id.clone(), self.symbol.clone()) {
            return;
        }
        let previous_feed = self.active.0.clone();
        let Some(provider) = config.provider_of(&self.feed_id) else {
            tracing::warn!(
                target: "quantick::app",
                feed = %self.feed_id,
                "selected feed is not in the config; ignoring switch"
            );
            // Snap the selection back to what is actually running.
            (self.feed_id, self.symbol) = self.active.clone();
            return;
        };
        // The recorder follows the feed, not the toggle: a market that can
        // stream depth is recorded from the moment it starts streaming. Kept
        // for the log line below; the start itself goes through
        // [`Self::ensure_book_capture`], the one place that decides it.
        let resume_book_capture = provider.capabilities().book_capture;

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_SWITCH",
            feed = %self.feed_id,
            symbol = %self.symbol,
            provider = ?provider,
            resume_book_capture,
            action = "reset_market_state",
            "switching feed/symbol; resetting chart"
        );

        // Dropping the old handle stops the old feed thread. The new feed starts
        // with a fresh backfill in flight.
        let handle = feed::spawn_live(provider, &self.symbol, config);
        self.attach(handle);

        // Rebuild every pane from scratch for the new stream, each keeping its
        // own bar spec. Retained trades from the old symbol must not leak in.
        for pane in self.panes_mut() {
            pane.reset_series();
        }
        self.drop_overlay_gestures();
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.loading.restart(LoadingTask::History);
        self.latest_trade_latency_ms = None;
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        // The simulated position cannot follow this tab onto another market:
        // flatten at the old symbol's last mark, labeled and journaled (the
        // journal still targets the old symbol — it only follows `self.symbol`
        // at the top of the next drain).
        self.paper.on_timeline_reset();

        self.active = (self.feed_id.clone(), self.symbol.clone());
        self.refresh_chip_label(config);
        self.ensure_book_capture(config);
        self.apply_feed_bubble_preset_after_switch(config, &previous_feed);
    }

    /// Apply the arrived-at feed's declared preset — only when the switch
    /// actually crossed feeds. A symbol hop inside one feed keeps the user's
    /// panel tweaks: the declared look belongs to the feed, not the symbol.
    pub fn apply_feed_bubble_preset_after_switch(
        &mut self,
        config: &AppConfig,
        previous_feed: &str,
    ) {
        if previous_feed == self.feed_id {
            return;
        }
        self.apply_feed_bubble_preset(config);
    }

    /// Apply the bubble preset the current feed declares, if it declares one.
    ///
    /// A feed with no `bubble_preset` changes nothing: the panel keeps the look
    /// the user last chose. An unknown name is reported and ignored — the
    /// presets file is user-edited, and a typo there must not silently restyle
    /// the chart.
    pub fn apply_feed_bubble_preset(&mut self, config: &AppConfig) {
        let Some(name) = config
            .feed(&self.feed_id)
            .and_then(|feed| feed.bubble_preset.clone())
        else {
            return;
        };
        let applied = self.tape_mut().apply_preset(&name);
        if applied {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FEED_BUBBLE_PRESET",
                feed = %self.feed_id,
                preset = name.as_str(),
                action = "apply_preset",
                "feed declares a bubble preset; applied"
            );
        } else {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FEED_BUBBLE_PRESET_UNKNOWN",
                feed = %self.feed_id,
                preset = name.as_str(),
                action = "keep_current_look",
                "feed declares a bubble preset that is not in the presets file; ignoring"
            );
        }
    }

    /// Open wearing the layout the feed declares, if it declares one.
    ///
    /// Startup-scoped, like `default_feed`: it decides what a tab on this
    /// feed *opens* showing, and never touches a layout the user has since
    /// chosen — the callers are tab creation, nothing else. A feed with no
    /// `default_layout` changes nothing, so the factory default stays the
    /// flow pane (the decision on record in the UX audit §3).
    pub fn apply_feed_declared_layout(&mut self, config: &AppConfig) {
        let Some(declared) = config
            .feed(&self.feed_id)
            .and_then(|feed| feed.default_layout)
        else {
            return;
        };
        let layout = CanvasLayout::from(declared);
        // A declared time-bar spec names the interval the declared layout's
        // time pane opens on; the pane's own header takes over from there.
        if layout.shows_time()
            && let Some(BarSpec::Time(ms)) = config.startup_spec_for(&self.feed_id)
        {
            self.time_pane_opening_interval_ms = ms;
        }
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_DECLARED_LAYOUT",
            feed = %self.feed_id,
            layout = ?layout,
            action = "open_declared_layout",
            "feed declares an opening layout; applied"
        );
        self.set_layout(layout);
        // Opening is not switching. [`Self::set_layout`] focuses whatever the
        // switch revealed, which is right for a menu click — the trader asked
        // for that pane. On a tab that has never been on screen there was no
        // gesture and nothing was revealed, and leaving the focus there put a
        // fresh window's BARS group and status line on the *context* chart:
        // the first thing a trader touched changed the timeframe pane instead
        // of the flow chart quantick exists to show. So the flow pane takes
        // the focus whenever this layout draws it.
        self.focus = if layout.shows_flow() {
            PaneSide::Flow
        } else {
            PaneSide::Time
        };
    }

    /// Put this tab's canvas back the way a saved workspace recorded it: the
    /// layout, the divider, the focused pane, and the interval the time pane
    /// opens on.
    ///
    /// One method rather than four public fields, because the order matters
    /// and only the tab knows it. The opening interval has to be set *before*
    /// [`Self::set_layout`] arms the time pane, or the pane is built on the
    /// header default and the saved interval lands one frame too late. The
    /// focus is applied *after*, because `set_layout` moves it to whatever the
    /// switch reveals — right for a menu click, wrong for a restore, where the
    /// saved focus is the answer.
    ///
    /// Startup-scoped, like [`Self::apply_feed_declared_layout`]: the only
    /// caller is the app restoring a workspace into a tab it has just opened.
    pub fn restore_canvas(
        &mut self,
        layout: CanvasLayout,
        split_fraction: Option<f32>,
        focus: Option<PaneSide>,
        time_interval_ms: Option<i64>,
    ) {
        if let Some(ms) = time_interval_ms {
            self.time_pane_opening_interval_ms = ms;
        }
        self.set_layout(layout);
        if let Some(fraction) = split_fraction {
            self.split_fraction = clamp_pane_fraction(fraction);
        }
        if let Some(side) = focus {
            self.focus = side;
        }
    }

    /// Let every pane's selectors settle, then mirror the result onto the
    /// rebuild indicator: it is up while *any* pane has a rebuild pending.
    pub fn apply_spec_changes(&mut self) {
        self.apply_spec_change(PaneSide::Flow);
        if self.time_pane.is_some() {
            self.apply_spec_change(PaneSide::Time);
        }
        let rebuilding = self.flow_pane.pending_spec.is_some()
            || self
                .time_pane
                .as_ref()
                .is_some_and(|pane| pane.pending_spec.is_some());
        self.loading.set_active(LoadingTask::BarRebuild, rebuilding);
    }

    /// Apply one pane's bar-type/parameter change, a frame after its selectors
    /// settle.
    ///
    /// Switching the spec replays every retained trade synchronously, which
    /// can hold this thread long enough to notice on a deep history. Deferring
    /// the rebuild by one frame lets the frame that carries the change paint
    /// the loading overlay first, so the wait reads as the chart working
    /// rather than the app hanging. A selector still moving (a dragged
    /// parameter) keeps pushing the pending spec forward, which also debounces
    /// the rebuild to one per gesture.
    ///
    /// The two panes run this independently: the toolbar's BARS group governs
    /// the focused pane and the time pane's own header governs the time pane
    /// (§11), so a change to one pane must not rebuild the chart beside it.
    fn apply_spec_change(&mut self, side: PaneSide) {
        let desired = self.pane(side).current_spec();
        let pane = self.pane_mut(side);
        if desired == *pane.state.spec() {
            // Selection and chart agree — nothing is pending any more (a feed
            // switch or reset may have rebuilt the state under a pending spec).
            pane.pending_spec = None;
            return;
        }
        match pane.pending_spec.take() {
            // The frame that changed the selector: arm the indicator, paint.
            None => pane.pending_spec = Some(desired),
            // Still moving: wait for the selector to settle for a frame.
            Some(pending) if pending != desired => pane.pending_spec = Some(desired),
            // Settled since last frame: do the rebuild.
            Some(_) => {
                // Where the user is looking, in market time — the one thing a
                // rebuild preserves. The new series cuts the same trades into
                // a different number of bars, so the old right-edge *index*
                // may not exist in it at all: keeping it would leave the
                // window past the end of the data, drawing nothing.
                let anchor = pane.right_edge_time();
                // The series the drawings are still anchored to, captured
                // before it is replaced: their bar indices are meaningless in
                // the new cut and have to be re-derived from the market time
                // each anchor carries.
                let old_slots = pane.slots();
                pane.state.set_spec(desired);
                // The venue prefix folds to the new interval before the view
                // is reanchored: the market time the user was looking at has
                // to resolve against the series they will be looking at.
                // Either pane: `bars → time` on the flow pane is exactly the
                // spec change this refold exists for (audit S1).
                //
                // Installing a prefix rebuilds the indicators over the whole
                // composed series, so the plain rebuild is only sent when the
                // refold did not — two rebuilds of ~130k bars per settled drag
                // frame, with no coalescing in the worker, is the cost of
                // sending both.
                let refolded = self.refold_history_prefix();
                if !refolded {
                    self.pane_mut(side).send_indicator_rebuild();
                }
                let pane = self.pane_mut(side);
                let slot = anchor.and_then(|ms| pane.slot_at_time(ms));
                let slots = pane.slots();
                pane.viewport.reanchor(slot, slots);
                // The marks follow the view: same market time, this pane's
                // new bar space. Nothing is lost, so there is nothing to
                // announce.
                pane.reanchor_drawings(old_slots);
                self.drop_overlay_gestures();
            }
        }
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// observed arrival latency and live-trade counts for the metrics.
    pub fn drain_feed(&mut self) {
        self.drain_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected drain used to prove that one UI cycle is one observation.
    pub fn drain_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
        // The journal follows this tab's symbol; synced before the drain so a
        // new feed's first trades are never attributed to the old symbol.
        // Every tab drains every frame, so every journal tracks its own market
        // whether or not that tab is the one on screen.
        let Self { paper, symbol, .. } = self;
        paper.set_symbol(symbol);
        let mut live = false;
        let mut received_at_ms = None;
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    self.loading.end(LoadingTask::History);
                    self.history_trades += trades.len();
                    // History only seeds the simulator's mark — filling
                    // against the past would be look-ahead.
                    if let Some(last) = trades.last() {
                        self.paper.seed(last);
                    }
                    // One tape, every pane: the split multiplies views of the
                    // market, never the stream behind them.
                    for pane in self.panes_mut() {
                        pane.ingest_backfill(&trades);
                    }
                }
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    // The reply — even an empty one — answers exactly one
                    // pending load; the indicator survives until the last one.
                    self.loading.end(LoadingTask::History);
                    self.history_trades += trades.len();
                    // Each pane cuts the older trades into its own bars, so
                    // each shifts its own anchors by its own count.
                    for pane in self.panes_mut() {
                        pane.prepend_history(&trades);
                    }
                    // The first engine bar just moved backwards in time, and
                    // the prefix was trimmed against where it used to be. Any
                    // venue candle now covering a re-cut minute has to go.
                    self.refold_history_prefix();
                }
                Ok(FeedEvent::Live(trade)) => {
                    let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                    self.ingest_live_trade_at(&trade, received_at_ms);
                    live = true;
                }
                Ok(FeedEvent::LiveBatch(trades)) => {
                    if !trades.is_empty() {
                        let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                        for trade in &trades {
                            self.ingest_live_trade_at(trade, received_at_ms);
                        }
                        live = true;
                    }
                }
                Ok(FeedEvent::Reset) => self.reset_market_state(),
                Ok(FeedEvent::OhlcvHistory {
                    interval_ms,
                    bars,
                    complete,
                }) => {
                    self.take_ohlcv_history(interval_ms, bars, complete);
                }
                Err(_) => break,
            }
        }
        // One forming-bar update per pane for the whole drain, however many
        // prints arrived: only its latest value is ever read.
        if live {
            for pane in self.panes_mut() {
                pane.publish_partial();
            }
        }
        // A reset left the marks waiting for bars to anchor to, and this is
        // the drain that may have just delivered them. One flag test per pane
        // when nothing is owed.
        for pane in self.panes_mut() {
            pane.settle_pending_reanchor();
        }
    }

    /// Take the newest feed notice, if the feed sent any this frame.
    ///
    /// Level-triggered rather than queued: only the latest state matters, and
    /// a burst of bridge output must not queue up cards to show one by one.
    /// A closed channel (a feed with nothing to report) simply yields nothing.
    pub fn drain_notices(&mut self) {
        while let Ok(notice) = self.notices.try_recv() {
            match notice {
                FeedNotice::Connected => {
                    self.feed_connection = FeedConnectionState::Connected;
                    self.notice = FeedNotice::Clear;
                }
                FeedNotice::Reconnecting { .. } => {
                    self.feed_connection = FeedConnectionState::Reconnecting;
                    self.notice = notice;
                }
                FeedNotice::Working { .. } | FeedNotice::Attention { .. } => self.notice = notice,
                FeedNotice::Clear => self.notice = FeedNotice::Clear,
            }
        }
    }

    /// Deterministic half of live ingestion: `received_at_ms` is the UI's epoch
    /// observation time, supplied explicitly so tests never wait on a clock.
    ///
    /// The transport observation is the window's; what the trade does to the
    /// bars, the tape and the indicators is the pane's.
    pub fn ingest_live_trade_at(&mut self, trade: &quantick_engine::Trade, received_at_ms: i64) {
        self.latest_trade_latency_ms =
            metrics::feed_lag_ms(received_at_ms, Some(trade.timestamp_ms));
        self.latest_trade_ms = Some(trade.timestamp_ms);
        self.live_trades += 1;
        // The simulator taps the same per-trade point the bar engine does, so
        // paper trading works identically on a live feed and a replay — and on
        // a tab the user is not looking at, whose position keeps marking
        // against its own tape.
        self.paper.on_trade(trade);
        for pane in self.panes_mut() {
            pane.ingest_live_trade(trade);
        }
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    pub fn reset_market_state(&mut self) {
        for pane in self.panes_mut() {
            pane.reset_series();
            // Indicators follow the chart into the empty state; the refill's
            // Backfilled event replays them (replay seek funnels through here,
            // so seeking inherits correct indicator behavior for free).
            pane.send_indicator_rebuild();
            pane.last_lane_divider_x = None;
        }
        self.drop_overlay_gestures();
        self.history_trades = 0;
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.loading.restart(LoadingTask::History);
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        // The simulator flattens at its last mark and says so — a position
        // cannot honestly survive into a rebuilt timeline.
        self.paper.on_timeline_reset();
    }

    /// Everything this tab must settle before it is dropped.
    ///
    /// Feeds and workers need nothing: their loops end when the channels go
    /// with the tab. The simulator does — an open position is state the user
    /// created, and the honesty contract says it ends in an explicit, labeled,
    /// journaled flatten, never by silently vanishing with its window.
    pub fn close(&mut self) {
        self.paper.on_timeline_reset();
    }

    /// Drain a bounded number of synchronized depth events. The separate
    /// channel and budget ensure heatmap work cannot block candle ingestion.
    pub fn drain_book_feed(&mut self) {
        self.drain_book_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected depth drain; a burst handled by one UI frame has one
    /// observation time, matching the trade-side metric and avoiding O(n)
    /// system-clock reads.
    pub fn drain_book_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
        let mut received_at_ms = None;
        for _ in 0..BOOK_DRAIN_BUDGET {
            match self.book_events.try_recv() {
                Ok(event) => {
                    let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                    self.tape_mut().handle_depth_event_at(event, received_at_ms);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.tape().enabled() && !self.book_channel_closed_reported {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "HEATMAP_EVENT_CHANNEL_CLOSED",
                            symbol = self.symbol.as_str(),
                            action = "retain_last_book_and_wait_for_feed_switch",
                            "depth event channel closed"
                        );
                        self.book_channel_closed_reported = true;
                    }
                    break;
                }
            }
        }
    }

    /// Delay observed when the newest live trade reached the UI.
    ///
    /// `None` while a session is replaying: those prints are as old as the day
    /// they were recorded, so their original arrival latency is unavailable.
    ///
    /// This figure freezes between prints — it is an observation, not a
    /// measurement of now. [`Self::tape_age_ms`] is the one that ages.
    pub fn trade_arrival_ms(&self) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        self.latest_trade_latency_ms
    }

    /// How old the newest event on the tape is, right now.
    ///
    /// Deterministic half: the caller supplies wall clock. Takes the newer of
    /// the trade stream and the book, so a symbol with depth but a thin tape
    /// is not called stale while its book is live. `None` while replaying, and
    /// before anything has arrived — nothing to be stale about yet.
    pub fn tape_age_at(&self, now_ms: i64) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        let newest = match (self.latest_trade_ms, self.tape().last_event_ms()) {
            (Some(trade), Some(book)) => Some(trade.max(book)),
            (trade, book) => trade.or(book),
        }?;
        Some(now_ms.saturating_sub(newest).max(0))
    }

    /// Data-honesty label for how each print is known, or `None` when the venue
    /// reports true trades and true sides (§8 — the status bar's middle
    /// section). The label shares its row with the machinery readouts, so it
    /// stays short and the full story lives in the hover.
    ///
    /// A venue that prints nothing at all takes precedence over how sides were
    /// decided: on a quote-driven feed *every* print is derived, and saying so
    /// is the more important disclosure. Without it a chart of one-unit prints
    /// reads as a market where every trade happened to be the same size.
    pub fn side_note(&self, config: &AppConfig) -> Option<(String, Option<String>)> {
        if let Some(link) = &self.replay {
            Some((
                match link.session.header.side_source.as_deref() {
                    Some(source) => format!("side: {source}"),
                    None => "side: not recorded".to_owned(),
                },
                None,
            ))
        } else if config.provider_of(&self.active.0).is_some()
            && !self.capabilities(config).traded_volume
        {
            Some((
                "prints: quote-derived".to_owned(),
                Some(
                    "this venue quotes prices but prints no trades: every candle is built \
                     from one synthetic print per tick, at the mid of bid and ask, carrying \
                     one unit — never a traded size"
                        .to_owned(),
                ),
            ))
        } else {
            // The running feed, not the still-uncommitted selection.
            config
                .side_note(&self.active.0)
                .map(|note| (note.to_owned(), None))
        }
    }

    /// Make a recorded session the chart's source, replacing whatever feed is
    /// running. The live selection is untouched, so closing the replay comes
    /// back to exactly the feed and symbol that were streaming before.
    pub fn open_replay(&mut self, config: &AppConfig, request: crate::feed::ReplayRequest) {
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_OPENED",
            session = %request.session.label(),
            file = %request.session.path.display(),
            trades = request.session.trades.len(),
            speed = request.options.speed,
            action = "replace_feed_source",
            "opening a recorded session"
        );

        let handle = feed::spawn(feed::FeedSource::Replay(Box::new(request)), config);
        self.attach(handle);

        if let Some(link) = &self.replay {
            self.symbol = link.symbol().to_string();
        }
        self.refresh_chip_label(config);
        // Depth is not in a recording; the toggle is disabled by capability,
        // and the view must not keep drawing a book from the live feed.
        let generation = self.next_book_generation();
        self.tape_mut().set_enabled(false, generation);
        self.reset_market_state();
    }

    /// Leave replay and put the live feed back.
    pub fn close_replay(&mut self, config: &AppConfig) {
        if self.replay.take().is_none() {
            return;
        }
        let (feed_id, symbol) = self.active.clone();
        self.feed_id = feed_id;
        self.symbol = symbol;
        self.refresh_chip_label(config);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_CLOSED",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_live_feed",
            "leaving market replay"
        );

        let Some(provider) = config.provider_of(&self.feed_id) else {
            // The configuration changed under us; there is nothing to go back
            // to, so the chart stays as it is rather than dying.
            self.reset_market_state();
            return;
        };
        let handle = feed::spawn_live(provider, &self.symbol, config);
        self.attach(handle);
        self.reset_market_state();
    }

    /// Start the current feed over, from the card that asked the user to fix
    /// something.
    ///
    /// The same respawn a feed switch performs, minus the switch: after the
    /// terminal is opened or the package installed, the way back has to be one
    /// click, not a restart of quantick. A replay owns the chart while it
    /// plays and has nothing to retry.
    pub fn restart_feed(&mut self, config: &AppConfig) {
        if self.replay.is_some() {
            return;
        }
        let Some(provider) = config.provider_of(&self.feed_id) else {
            return;
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_RESTARTED_BY_USER",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_feed",
            "restarting the feed from the notice card"
        );
        let handle = feed::spawn_live(provider, &self.symbol, config);
        self.attach(handle);
        self.reset_market_state();
        // The live market is back and it can stream depth again; start
        // recording immediately rather than waiting for the map to be opened.
        self.ensure_book_capture(config);
    }
    /// Lay the canvas out and run every visible pane through it (§11).
    ///
    /// Single is one pane over the whole area — the same code path the split
    /// takes, with one pane in it, so the default layout can never drift from
    /// the split one.
    pub fn draw_canvas(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        chrome: &mut CanvasChrome<'_>,
    ) {
        let show_time = self.layout.shows_time() && self.time_pane.is_some();
        // The flow pane also stands in for a time pane still being built, so
        // the frame between asking for the Time layout and the pane existing
        // shows the market rather than nothing.
        let show_flow = self.layout.shows_flow() || !show_time;
        let split = show_time && show_flow;
        // One visible pane is handed the whole canvas and the rest of this
        // reduces to nothing: no divider, no focus rule — though a lone time
        // pane keeps its header.
        let (time_area, divider, flow_area) = if split {
            let areas = split_canvas(area, self.split_fraction);
            (Some(areas.time), Some(areas.divider), areas.flow)
        } else if show_time {
            (Some(area), None, area)
        } else {
            (None, None, area)
        };

        let time_chart = time_area.map(|time_area| {
            // Focus before input, so the click that focuses a pane is also the
            // click that pane goes on to handle. Only a split has focus to
            // move: a single visible pane is the focused one by definition.
            if split {
                self.focus_from_pointer(ui, time_area, flow_area);
            }
            let areas = split_time_pane(time_area);
            // The time pane's own timeframe selector (§11): its BARS group,
            // beside the toolbar's, which keeps governing the flow pane.
            let mut interval_ms = self.pane(PaneSide::Time).time_interval_ms;
            let header_layout = crate::time_header::draw(ui, areas.header, &mut interval_ms);
            #[cfg(test)]
            {
                self.time_header_chips = header_layout.chips();
            }
            if header_layout.changed {
                let pane = self.pane_mut(PaneSide::Time);
                pane.kind = BarKind::Time;
                pane.time_interval_ms = interval_ms;
            }
            areas.chart
        });

        // Which shared mark the pointer is over, on each pane, against the
        // other pane's store. Answered here because answering it needs both
        // panes at once, and the loop below holds them one at a time.
        let picks = self.shared_picks(ui);

        let mut edits: SmallVec<[(PaneSide, SharedInteraction); 2]> = SmallVec::new();
        {
            let focused = self.focused_side();
            let Self {
                flow_pane,
                time_pane,
                symbol,
                paper,
                ..
            } = self;
            let mut chrome = PaneChrome {
                toolrail: chrome.toolrail,
                presets: chrome.presets,
                style: chrome.style,
                tz: chrome.tz,
                symbol,
                paper,
                paper_owns_input: false,
                shared_pick: None,
                shared: SharedInteraction::default(),
                capabilities: chrome.capabilities,
                layers: chrome.layers,
            };
            // Time pane first, then flow. Both take the same two steps in the
            // same order — which is what keeps the split honest: the second
            // pane cannot drift from the first, and one pane is this same
            // loop with one entry in it.
            let time =
                time_chart.and_then(|chart| Some((time_pane.as_mut()?, chart, PaneSide::Time)));
            let flow = show_flow.then_some((&mut *flow_pane, flow_area, PaneSide::Flow));
            for (pane, rect, side) in time.into_iter().chain(flow) {
                // Order entry follows the focused pane (§11): both charts are
                // trading surfaces — a level is as true on the time pane as on
                // the flow pane — and focus lands on the press that acts, so
                // the first click already trades where the accent rule is.
                // Unsplit, the flow pane is the only pane and nothing changes.
                chrome.paper_owns_input = side == focused;
                chrome.shared_pick = picks.for_side(side);
                chrome.shared = SharedInteraction::default();
                pane.handle_navigation(ui, rect, &mut chrome);
                pane.draw_chart(ui.painter(), rect, &mut chrome);
                // Whatever this pane did to the other's marks travels out of
                // the loop: the store it belongs to is the pane that is not
                // borrowed right now.
                if chrome.shared != SharedInteraction::default() {
                    edits.push((side, chrome.shared));
                }
            }
        }
        self.apply_shared_interactions(&edits);

        // Drawings marked "show on all charts" cross here, after both panes
        // have drawn and cached their projections. It happens outside the
        // loop above because each pane paints the *other* pane's marks, and
        // that needs both panes borrowed at once — immutably, which is also
        // the guarantee that a foreign mark can only be looked at.
        self.paint_shared_drawings(ui.painter());

        // The position HUD rides the pane that owns order entry (the focused
        // one). It draws here, after the pane loop, because its buttons need
        // the paper host mutably — inside the loop that borrow is pinned
        // behind the shared chrome.
        if let Some((rect, scale)) = self.focused_pane().paper_hud_anchor() {
            crate::paper_hud::draw(ui.ctx(), rect, &mut self.paper, &scale);
        }

        let (Some(time_area), Some(divider)) = (time_area, divider) else {
            return;
        };
        self.draw_canvas_divider(ui, divider, area.width());
        // §11: a 1 px accent under the focused pane's top edge — no border
        // boxes around market data.
        let focused = match self.focused_side() {
            PaneSide::Time => time_area,
            PaneSide::Flow => flow_area,
        };
        ui.painter().line_segment(
            [
                egui::pos2(focused.left(), focused.top() + FOCUS_RULE_PX / 2.0),
                egui::pos2(focused.right(), focused.top() + FOCUS_RULE_PX / 2.0),
            ],
            egui::Stroke::new(FOCUS_RULE_PX, theme::ACCENT),
        );
    }

    /// What the pointer is over, on each pane, among the *other* pane's
    /// shared marks.
    ///
    /// Resolved with both panes in hand and before either is borrowed for its
    /// input pass, which is the only moment a pane can be asked about marks it
    /// does not hold. Nothing to answer on an unsplit tab: one pane has no
    /// other pane to mirror.
    fn shared_picks(&self, ui: &egui::Ui) -> SharedPicks {
        let Some(time_pane) = self.time_pane.as_ref() else {
            return SharedPicks::default();
        };
        let Some(position) = ui.input(|input| input.pointer.latest_pos()) else {
            return SharedPicks::default();
        };
        let pick = |pane: &ChartPane, source: &ChartPane| {
            // Only the pane the pointer is actually over is asked. Besides
            // halving the work, it is what stops a horizontal line — which
            // spans a whole chart — from reporting a hit on the pane beside
            // the one the pointer is in, at the same height.
            if !pane
                .last_chart_area
                .is_some_and(|chart| chart.contains(position))
            {
                return None;
            }
            pane.shared_pick(source, position)
                .map(|(index, anchor)| SharedPick {
                    index,
                    anchor,
                    locked: source.drawings.items()[index].locked,
                })
        };
        SharedPicks {
            time: pick(time_pane, &self.flow_pane),
            flow: pick(&self.flow_pane, time_pane),
        }
    }

    /// Land what each pane did to the other's marks on the store that holds
    /// them.
    ///
    /// The gesture brackets travel with the edits so a whole drag started on
    /// the mirror lands as one undo entry on the owning store — the same
    /// coalescing the object gets on its own chart, because it is the same
    /// gesture on the same object. A selection taken on one pane is dropped on
    /// the other, so the tab never holds two.
    fn apply_shared_interactions(&mut self, edits: &[(PaneSide, SharedInteraction)]) {
        for (side, interaction) in edits {
            let owner = side.other();
            if interaction.begin_gesture {
                self.pane_mut(owner).drawings.begin_gesture();
            }
            if let Some(edit) = interaction.edit {
                if matches!(edit, SharedEdit::Select(_)) {
                    self.pane_mut(*side).drawings.select(None);
                }
                self.pane_mut(owner).apply_shared_edit(edit);
            }
            if interaction.commit_gesture {
                self.pane_mut(owner).drawings.commit_gesture();
            }
        }
    }

    /// Cross-pane drawings: each pane paints the shared marks of the other.
    ///
    /// Nothing happens on an unsplit tab — one pane has no other pane to
    /// borrow marks from, and the drawing is already on the only chart there
    /// is. Scope stops here, at the tab: the panes below hold one symbol on
    /// one feed, which is what makes a price level mean the same thing on
    /// both (`docs/ux/drawing-tools-2026-08.md` §D7).
    fn paint_shared_drawings(&self, painter: &egui::Painter) {
        let Some(time_pane) = self.time_pane.as_ref() else {
            return;
        };
        let flow_pane = &self.flow_pane;
        flow_pane.paint_shared_from(painter, time_pane);
        time_pane.paint_shared_from(painter, flow_pane);
    }

    /// Clicking a pane focuses it (§11). Read from the raw pointer press
    /// rather than a widget response, so the press that starts a pan or picks
    /// up a drawing focuses the pane it landed in on that same frame.
    fn focus_from_pointer(&mut self, ui: &egui::Ui, time_area: egui::Rect, flow_area: egui::Rect) {
        let pressed = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        let Some(position) = pressed else { return };
        // A press egui routed to another layer belongs to whatever floats
        // there — the toast, the object manager, the inspector — not to the
        // pane it happens to cover. Taking those as pane clicks made the
        // toast's Undo act on the chart the button floated over rather than
        // the one it was raised for.
        if ui.ctx().layer_id_at(position) != Some(ui.layer_id()) {
            return;
        }
        if time_area.contains(position) {
            self.focus = PaneSide::Time;
        } else if flow_area.contains(position) {
            self.focus = PaneSide::Flow;
        }
    }

    /// The divider between the panes, as a resize handle.
    ///
    /// Registered after both panes so it takes the drag that would otherwise
    /// pan the chart behind its grab area, exactly as the live lane's own
    /// divider does inside a pane.
    fn draw_canvas_divider(&mut self, ui: &egui::Ui, divider: egui::Rect, canvas_width: f32) {
        #[cfg(test)]
        {
            self.canvas_divider = Some(divider);
        }
        ui.painter()
            .rect_filled(divider, egui::Rounding::ZERO, theme::BORDER);
        // Namespaced by tab for the same reason a pane namespaces its own ids
        // (see [`crate::pane`]): egui keeps drag state per id, so one shared
        // id would let a drag started on this tab's divider carry on into the
        // next tab's the moment Ctrl+Tab switches under a held button.
        let handle = ui.interact(
            divider.expand2(egui::vec2(CANVAS_DIVIDER_HANDLE_PX, 0.0)),
            egui::Id::new(("canvas_divider", self.id)),
            egui::Sense::drag(),
        );
        if handle.hovered() || handle.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if handle.dragged() && canvas_width > 0.0 {
            let moved = self.split_fraction + handle.drag_delta().x / canvas_width;
            self.split_fraction = clamp_pane_fraction(moved);
        }
    }
}
