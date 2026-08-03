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
    clamp_pane_fraction, split_canvas, split_time_pane,
};
use crate::state::{BarKind, BarSpec};
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;

/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
pub const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
const BOOK_DRAIN_BUDGET: usize = 2_048;
/// Thickness of the rule marking the focused pane (§11: an accent under the
/// pane's top edge, never a box drawn around market data).
const FOCUS_RULE_PX: f32 = 1.0;

/// How many charts a tab's canvas shows for its market (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasLayout {
    /// The flow pane alone — quantick's default and its identity.
    #[default]
    Single,
    /// Time pane left, flow pane right, on a draggable divider.
    TimeAndFlow,
}

/// The window chrome a tab's canvas borrows for one frame. The tab completes
/// it with its own symbol to make the [`PaneChrome`] its panes read.
pub struct CanvasChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a crate::drawings::presets::PresetStore,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
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
    /// What `ohlcv_history` said last frame, so the rising edge can be seen.
    ///
    /// MetaTrader narrows its capabilities when the bridge says hello, which
    /// happens *after* the pane may already have asked and been answered
    /// `nothing_held`. The edge is what asks again once the answer can be a
    /// real one.
    ohlcv_capable: bool,
    /// The id the time pane takes when this tab first shows the split.
    time_pane_id: u64,
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
            flow_pane: ChartPane::flow(pane_ids.0, spec, symbol.clone()),
            chip_label: String::new(),
            ohlcv_base: None,
            ohlcv_pending: false,
            ohlcv_capable: false,
            time_pane: None,
            time_pane_id: pane_ids.1,
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
        self.loading.restart(LoadingTask::VenueHistory);
        self.loading.end(LoadingTask::VenueHistory);
        if let Some(pane) = self.time_pane.as_mut() {
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

    /// Clear the bar-anchored overlay on `side`, reporting whether anything
    /// was actually lost.
    ///
    /// Bar indices are only meaningful for the market and spec that made them,
    /// so a source or aggregation rebuild has to drop the marks rather than
    /// silently reattach them to different data. The window turns the answer
    /// into the toast that says so and the tool reset that follows — the marks
    /// are the tab's, the chrome around them is not.
    fn clear_overlay(&mut self, side: PaneSide) -> bool {
        let pane = self.pane_mut(side);
        let had_drawings = !pane.drawings.items().is_empty();
        pane.drawings.clear();
        pane.drawing_hover = None;
        pane.drawing_press_position = None;
        pane.drawing_press_started_empty = false;
        pane.drawing_drag = DrawingDrag::None;
        had_drawings
    }

    /// Every pane's overlay at once, for a change that invalidates them all —
    /// a feed switch or a source reset re-cuts both charts.
    fn clear_overlays(&mut self) -> bool {
        let flow = self.clear_overlay(PaneSide::Flow);
        let time = self.time_pane.is_some() && self.clear_overlay(PaneSide::Time);
        flow || time
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

    /// Swap in a feed the test drives, through the same path a respawn takes.
    #[cfg(test)]
    pub fn attach_for_test(&mut self, handle: FeedHandle) {
        self.attach(handle);
    }

    /// Ask the venue for its candle history, if there is anything to ask.
    ///
    /// Gated on the capability, never on the provider: a feed that serves no
    /// candles, and a recording — which is a fixed span of prints with no
    /// venue behind it — are both simply not asked. One request at a time, and
    /// a base already held is not re-fetched: changing the pane's interval is
    /// a different fold over the same bars.
    fn request_ohlcv_history(&mut self, config: &AppConfig) {
        if self.time_pane.is_none()
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
            // A dropped request is not worth a retry queue: the capability
            // check runs every frame and will ask again next time.
            Err(error) => tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_REQUEST_DROPPED",
                reason = %error,
                "candle-history request not queued"
            ),
        }
    }

    /// Watch the capability for the false→true edge and ask when it lands.
    ///
    /// Called every frame: the check is two bools and an `Option` when there
    /// is nothing to do.
    pub fn poll_ohlcv_capability(&mut self, config: &AppConfig) {
        let capable = self.capabilities(config).ohlcv_history;
        let rising = capable && !self.ohlcv_capable;
        self.ohlcv_capable = capable;
        if rising {
            // A session that narrowed *into* serving candles may have answered
            // an earlier request with nothing; that answer described a feed
            // that did not know itself yet.
            if self.ohlcv_base.as_ref().is_some_and(Vec::is_empty) {
                self.ohlcv_base = None;
            }
            self.request_ohlcv_history(config);
        }
    }

    /// Take a candle-history reply, and put it in front of the time pane.
    ///
    /// An empty reply is a complete answer — the venue has none, the provider
    /// serves none, or the fetch failed — and is recorded as such so the tab
    /// stops asking. Either way the wait ends here: a provider that answered
    /// only on success would strand the spinner on the one case that most
    /// needs explaining.
    fn take_ohlcv_history(&mut self, interval_ms: i64, bars: Vec<quantick_engine::Bar>) {
        self.ohlcv_pending = false;
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

    /// Rebuild the time pane's prefix from the base at the pane's interval.
    ///
    /// Free of the venue: a chip click lands here, not on the network.
    pub fn refold_history_prefix(&mut self) {
        let Some(base) = self.ohlcv_base.as_ref() else {
            return;
        };
        let Some(pane) = self.time_pane.as_mut() else {
            return;
        };
        // A pane not cutting by time has no interval to fold to, and a
        // sub-minute one has no whole number of venue candles in it: both get
        // no prefix, which is the honest answer rather than an invented one.
        let interval = pane.state.spec().time_interval_ms().unwrap_or_default();
        let folded = crate::resample::fold(base, interval);
        let prefix = trim_to_seam(
            folded,
            pane.state.bars().first(),
            pane.state.partial(),
            interval,
        );
        pane.install_history_prefix(prefix);
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
    /// make; a Single canvas is the flow pane by definition, whatever the last
    /// split left `focus` set to.
    pub fn focused_side(&self) -> PaneSide {
        match (self.layout, self.time_pane.is_some()) {
            (CanvasLayout::TimeAndFlow, true) => self.focus,
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

    /// Show or hide the context chart (§11).
    ///
    /// The first Time + Flow builds the pane and seeds it from the trades the
    /// flow pane already holds, so it opens showing the same market rather
    /// than an empty chart waiting for the next print. Going back to Single
    /// only stops drawing it: its indicators, drawings and bars survive, and
    /// it keeps being fed, so re-showing it never has to catch up.
    pub fn set_layout(&mut self, layout: CanvasLayout) {
        self.layout = layout;
        if layout == CanvasLayout::TimeAndFlow && self.time_pane.is_none() {
            // Seeding replays every retained trade, which on a deep history
            // holds the render thread long enough to notice. Armed here and
            // done on the next frame, exactly as a bar-spec change is: the
            // frame carrying the menu click paints the loading overlay first,
            // so the wait reads as the chart working rather than the app
            // hanging.
            self.pending_time_pane = true;
            self.loading.begin(LoadingTask::BarRebuild);
        }
        if layout == CanvasLayout::Single {
            self.focus = PaneSide::Flow;
        }
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
        let mut pane = ChartPane::time(self.time_pane_id, crate::time_header::DEFAULT_INTERVAL_MS);
        pane.seed_from(
            self.flow_pane.state.trades(),
            self.flow_pane.state.backfill_trade_count(),
        );
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
    pub fn maybe_switch_feed(&mut self, config: &AppConfig) -> bool {
        // A replay owns the chart until it is closed. The selectors are not
        // drawn while it plays, so nothing can diverge here — but a stale
        // selection must not respawn a live feed underneath the recording.
        if self.replay.is_some() {
            return false;
        }
        if self.active == (self.feed_id.clone(), self.symbol.clone()) {
            return false;
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
            return false;
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
        let cleared = self.clear_overlays();
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.loading.restart(LoadingTask::History);
        self.latest_trade_latency_ms = None;
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);

        self.active = (self.feed_id.clone(), self.symbol.clone());
        self.refresh_chip_label(config);
        self.ensure_book_capture(config);
        self.apply_feed_bubble_preset_after_switch(config, &previous_feed);
        cleared
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

    /// Let every pane's selectors settle, then mirror the result onto the
    /// rebuild indicator: it is up while *any* pane has a rebuild pending.
    pub fn apply_spec_changes(&mut self) -> bool {
        let mut cleared = self.apply_spec_change(PaneSide::Flow);
        if self.time_pane.is_some() {
            cleared |= self.apply_spec_change(PaneSide::Time);
        }
        let rebuilding = self.flow_pane.pending_spec.is_some()
            || self
                .time_pane
                .as_ref()
                .is_some_and(|pane| pane.pending_spec.is_some());
        self.loading.set_active(LoadingTask::BarRebuild, rebuilding);
        cleared
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
    /// the flow pane and the time pane's own header governs the time pane
    /// (§11), so a timeframe change must not rebuild the chart beside it.
    fn apply_spec_change(&mut self, side: PaneSide) -> bool {
        let desired = self.pane(side).current_spec();
        let pane = self.pane_mut(side);
        if desired == *pane.state.spec() {
            // Selection and chart agree — nothing is pending any more (a feed
            // switch or reset may have rebuilt the state under a pending spec).
            pane.pending_spec = None;
            return false;
        }
        match pane.pending_spec.take() {
            // The frame that changed the selector: arm the indicator, paint.
            None => {
                pane.pending_spec = Some(desired);
                false
            }
            // Still moving: wait for the selector to settle for a frame.
            Some(pending) if pending != desired => {
                pane.pending_spec = Some(desired);
                false
            }
            // Settled since last frame: do the rebuild.
            Some(_) => {
                // Where the user is looking, in market time — the one thing a
                // rebuild preserves. The new series cuts the same trades into
                // a different number of bars, so the old right-edge *index*
                // may not exist in it at all: keeping it would leave the
                // window past the end of the data, drawing nothing.
                let anchor = pane.right_edge_time();
                pane.state.set_spec(desired);
                pane.send_indicator_rebuild();
                // The venue prefix folds to the new interval before the view
                // is reanchored: the market time the user was looking at has
                // to resolve against the series they will be looking at.
                if side == PaneSide::Time {
                    self.refold_history_prefix();
                }
                let pane = self.pane_mut(side);
                let slot = anchor.and_then(|ms| pane.slot_at_time(ms));
                let slots = pane.slots();
                pane.viewport.reanchor(slot, slots);
                self.clear_overlay(side)
            }
        }
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// observed arrival latency and live-trade counts for the metrics.
    pub fn drain_feed(&mut self) -> bool {
        self.drain_feed_with_clock(metrics::wall_clock_ms)
    }

    /// Clock-injected drain used to prove that one UI cycle is one observation.
    pub fn drain_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) -> bool {
        let mut cleared = false;
        let mut live = false;
        let mut received_at_ms = None;
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    self.loading.end(LoadingTask::History);
                    self.history_trades += trades.len();
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
                Ok(FeedEvent::Reset) => cleared |= self.reset_market_state(),
                Ok(FeedEvent::OhlcvHistory { interval_ms, bars }) => {
                    self.take_ohlcv_history(interval_ms, bars);
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
        cleared
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
        for pane in self.panes_mut() {
            pane.ingest_live_trade(trade);
        }
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    pub fn reset_market_state(&mut self) -> bool {
        for pane in self.panes_mut() {
            pane.reset_series();
            // Indicators follow the chart into the empty state; the refill's
            // Backfilled event replays them (replay seek funnels through here,
            // so seeking inherits correct indicator behavior for free).
            pane.send_indicator_rebuild();
            pane.last_lane_divider_x = None;
        }
        let cleared = self.clear_overlays();
        self.history_trades = 0;
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.loading.restart(LoadingTask::History);
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
        cleared
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
    pub fn open_replay(&mut self, config: &AppConfig, request: crate::feed::ReplayRequest) -> bool {
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
        self.reset_market_state()
    }

    /// Leave replay and put the live feed back.
    pub fn close_replay(&mut self, config: &AppConfig) -> bool {
        if self.replay.take().is_none() {
            return false;
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
            return self.reset_market_state();
        };
        let handle = feed::spawn_live(provider, &self.symbol, config);
        self.attach(handle);
        self.reset_market_state()
    }

    /// Start the current feed over, from the card that asked the user to fix
    /// something.
    ///
    /// The same respawn a feed switch performs, minus the switch: after the
    /// terminal is opened or the package installed, the way back has to be one
    /// click, not a restart of quantick. A replay owns the chart while it
    /// plays and has nothing to retry.
    pub fn restart_feed(&mut self, config: &AppConfig) -> bool {
        if self.replay.is_some() {
            return false;
        }
        let Some(provider) = config.provider_of(&self.feed_id) else {
            return false;
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
        let cleared = self.reset_market_state();
        // The live market is back and it can stream depth again; start
        // recording immediately rather than waiting for the map to be opened.
        self.ensure_book_capture(config);
        cleared
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
        let split = self.layout == CanvasLayout::TimeAndFlow && self.time_pane.is_some();
        // Unsplit, the flow pane is handed the whole canvas and the rest of
        // this reduces to nothing: no divider, no header, no focus rule.
        let (time_area, divider, flow_area) = if split {
            let areas = split_canvas(area, self.split_fraction);
            (Some(areas.time), Some(areas.divider), areas.flow)
        } else {
            (None, None, area)
        };

        let time_chart = time_area.map(|time_area| {
            // Focus before input, so the click that focuses a pane is also the
            // click that pane goes on to handle.
            self.focus_from_pointer(ui, time_area, flow_area);
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

        {
            let Self {
                flow_pane,
                time_pane,
                symbol,
                ..
            } = self;
            let mut chrome = PaneChrome {
                toolrail: chrome.toolrail,
                presets: chrome.presets,
                style: chrome.style,
                tz: chrome.tz,
                symbol,
            };
            // Time pane first, then flow. Both take the same two steps in the
            // same order — which is what keeps the split honest: the second
            // pane cannot drift from the first, and one pane is this same
            // loop with one entry in it.
            let time = time_chart.and_then(|chart| Some((time_pane.as_mut()?, chart)));
            for (pane, rect) in time
                .into_iter()
                .chain(std::iter::once((&mut *flow_pane, flow_area)))
            {
                pane.handle_navigation(ui, rect, &mut chrome);
                pane.draw_chart(ui.painter(), rect, &chrome);
            }
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
        let handle = ui.interact(
            divider.expand2(egui::vec2(CANVAS_DIVIDER_HANDLE_PX, 0.0)),
            egui::Id::new("canvas_divider"),
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
