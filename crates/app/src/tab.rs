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

use crate::canvas_layout::{
    self, LayoutPreset, MAX_CANVAS_PANES, MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind,
};
use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::config::{AppConfig, FeedCapabilities};
use crate::feed::{
    self, FeedCommand, FeedConnectionState, FeedEvent, FeedHandle, FeedLatency, FeedNotice,
    ReplayLink,
};
use crate::loading::{LoadingTask, LoadingTracker};
use crate::metrics;
use crate::orderflow_view::OrderflowView;
use crate::pane::{
    CANVAS_DIVIDER_HANDLE_PX, ChartPane, DEFAULT_PANE_FRACTION, DrawingDrag, PaneChrome, PaneIndex,
    PaneSide, SharedEdit, SharedInteraction, SharedPick, clamp_pane_fraction, split_time_pane,
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
/// Width of the grip bar on a collapsed column's rail.
const RAIL_GRIP_WIDTH_PX: f32 = 2.0;
/// Height of that grip bar. Long enough to read as a handle at a glance,
/// short enough that it is a mark on the rail rather than the rail itself.
const RAIL_GRIP_HEIGHT_PX: f32 = 24.0;

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
    /// Two time panes stacked in the left column, flow pane right.
    TimeTimeAndFlow,
}

impl CanvasLayout {
    /// The registry entry this layout is a name for.
    ///
    /// The one place a variant is turned into panes. Everything that wants to
    /// know what a layout *holds* reads the table through here rather than
    /// matching on the variant, so an arrangement added to the registry does
    /// not have to be taught to every caller one at a time.
    #[must_use]
    pub fn preset(self) -> &'static LayoutPreset {
        let id = match self {
            CanvasLayout::Single => "flow",
            CanvasLayout::Time => "time",
            CanvasLayout::TimeAndFlow => "time+flow",
            CanvasLayout::TimeTimeAndFlow => "time+time+flow",
        };
        canvas_layout::preset(id).expect("every canvas layout names a registered preset")
    }

    /// The layout a registry entry names, if the canvas can draw it.
    ///
    /// The inverse of [`Self::preset`], and deliberately partial: the registry
    /// is allowed to describe an arrangement the canvas has not learned to
    /// draw yet, and answering `None` is how that stays visible instead of
    /// being approximated into the nearest layout that happens to exist.
    #[must_use]
    pub fn from_preset(preset: &LayoutPreset) -> Option<Self> {
        match preset.id {
            "flow" => Some(CanvasLayout::Single),
            "time" => Some(CanvasLayout::Time),
            "time+flow" => Some(CanvasLayout::TimeAndFlow),
            "time+time+flow" => Some(CanvasLayout::TimeTimeAndFlow),
            _ => None,
        }
    }

    /// The panes this layout draws, left to right.
    #[must_use]
    pub fn kinds(self) -> &'static [PaneKind] {
        self.preset().kinds
    }

    /// Whether this layout draws the time pane at all.
    #[must_use]
    pub fn shows_time(self) -> bool {
        self.kinds().contains(&PaneKind::Time)
    }

    /// Whether this layout draws the flow pane at all.
    #[must_use]
    pub fn shows_flow(self) -> bool {
        self.kinds().contains(&PaneKind::Flow)
    }
}

impl From<crate::config::DeclaredLayout> for CanvasLayout {
    fn from(declared: crate::config::DeclaredLayout) -> Self {
        match declared {
            crate::config::DeclaredLayout::Flow => CanvasLayout::Single,
            crate::config::DeclaredLayout::Time => CanvasLayout::Time,
            crate::config::DeclaredLayout::TimeAndFlow => CanvasLayout::TimeAndFlow,
            crate::config::DeclaredLayout::TimeTimeAndFlow => CanvasLayout::TimeTimeAndFlow,
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
            CanvasLayout::TimeTimeAndFlow => crate::config::DeclaredLayout::TimeTimeAndFlow,
        }
    }
}

/// The window chrome a tab's canvas borrows for one frame. The tab completes
/// it with its own symbol to make the [`PaneChrome`] its panes read.
pub struct CanvasChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a crate::drawings::presets::PresetStore,
    /// See [`PaneChrome::begin_text_edit`].
    pub begin_text_edit: &'a mut bool,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// What the running source can produce, for the layer menu's disabled
    /// entries. Resolved once by the app rather than per pane, per entry.
    pub capabilities: FeedCapabilities,
    /// Whether the source infers the aggressor side (see
    /// [`PaneChrome::side_inferred`]). Resolved once, like `capabilities`.
    pub side_inferred: bool,
    /// The footprint layer's signal tunables (see [`PaneChrome::footprint`]).
    pub footprint: &'a mut crate::footprint_config::FootprintConfig,
    /// Where a pane's layer menu leaves the switches it does not own.
    pub layers: &'a mut crate::chart_layers::LayerActions,
}

/// Put an older slice of venue candles in front of the ones already held,
/// keeping the base ascending by `open_time` and free of duplicates.
///
/// The fast path is the one progressive loading actually produces: the slice
/// is strictly older than everything held, so it is spliced in front and the
/// order is already right. The merge below exists for the case the port
/// permits but no provider aims for — a window that overlaps what is held,
/// through a venue re-reporting a bucket at a boundary. There the candle
/// already on screen wins: it is the one the trader has been reading, and a
/// bar that redraws itself for no visible reason is worse than a bar fetched
/// a second apart from an identical twin.
fn merge_older_candles(base: &mut Vec<quantick_engine::Bar>, older: Vec<quantick_engine::Bar>) {
    let disjoint = match (older.last(), base.first()) {
        (Some(newest_incoming), Some(oldest_held)) => {
            newest_incoming.open_time < oldest_held.open_time
        }
        _ => true,
    };
    if disjoint {
        base.splice(0..0, older);
        return;
    }
    let mut merged: std::collections::BTreeMap<i64, quantick_engine::Bar> =
        older.into_iter().map(|bar| (bar.open_time, bar)).collect();
    for bar in base.drain(..) {
        merged.insert(bar.open_time, bar);
    }
    *base = merged.into_values().collect();
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

/// Whether the chart can reach further back for venue candles, and when it
/// cannot, why — see [`Tab::older_candles`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OlderCandles {
    /// Another span can be asked for.
    Available,
    /// This feed publishes no candle history at all.
    FeedServesNone,
    /// Nothing on this chart is cut by time, so no venue candle was ever
    /// wanted — the prefix follows what a pane *shows*, not which pane it is.
    NoChartCutByTime,
    /// A request is out; the answer is what to wait for.
    Fetching,
    /// The opening span has not landed yet. There is nothing to reach back
    /// *from* until it does.
    NotArrivedYet,
    /// A reach-back came back complete with nothing older in it. That is the
    /// venue's record, or the provider's, and it is the one reason here that
    /// had to be learned by asking.
    RecordStartsHere,
}

impl OlderCandles {
    /// Whether the control is live.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }

    /// What to tell the trader hovering a control this state disabled.
    /// `None` when it is not disabled.
    #[must_use]
    pub const fn why_not(self) -> Option<&'static str> {
        match self {
            Self::Available => None,
            Self::FeedServesNone => Some("this feed publishes no candle history"),
            Self::NoChartCutByTime => {
                Some("no chart here is cut by time, so there are no venue candles to extend")
            }
            Self::Fetching => Some("a request is already out; this is what it is fetching"),
            Self::NotArrivedYet => Some("the first span has not arrived yet"),
            Self::RecordStartsHere => Some("this is as far back as the venue's record goes"),
        }
    }
}

/// One open market. See the module docs for what does and does not live here.
/// One frame's answer, for both panes, to "what shared mark of the *other*
/// pane is the pointer over?".
///
/// A pair rather than a per-pane field because the question can only be asked
/// while both panes are in hand, and it is asked once for the frame.
#[derive(Debug, Clone, Default)]
struct SharedPicks {
    /// One entry per pane, in [`PaneIndex`] order: `0` is the flow pane, `1..`
    /// the context stack. A pair would only answer for two panes, and the
    /// question is asked of however many the layout holds.
    by_pane: SmallVec<[Option<SharedPick>; MAX_CANVAS_PANES]>,
}

impl SharedPicks {
    fn for_pane(&self, pane: PaneIndex) -> Option<SharedPick> {
        self.by_pane.get(pane).copied().flatten()
    }
}

/// Which of a restored tab's panes open with their indicator legend folded.
///
/// A named pair rather than two positional bools: `restore_canvas(.., true,
/// false)` at the call site says nothing about which chart is which, and the
/// two panes are exactly the thing a reader would have to guess.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegendFold {
    /// The flow pane's legend.
    pub flow: bool,
    /// The time pane's legend, when the tab has one.
    pub time: bool,
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
    /// Where this feed's delay is being spent, read fresh every frame.
    ///
    /// A reading rather than an event, so a frame that skipped three samples
    /// sees the newest one instead of a queue. `None` on a provider that
    /// cannot cut its own chain, and until the first sample arrives.
    pub feed_latency: watch::Receiver<Option<FeedLatency>>,
    /// A latency split forced by `QUANTICK_FAKE_LATENCY_SPLIT`, for a scripted
    /// run that has to photograph the readout without a slow venue.
    ///
    /// Overrides the feed's own reading when set, and set only from the hook —
    /// a live session leaves it `None` and reads the real one.
    pub forced_latency: Option<FeedLatency>,
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
    /// The context charts beside it, top to bottom in the left column.
    ///
    /// Built the first time a layout that shows one is picked, and kept for as
    /// long as the tab lives — switching to a layout that hides them only
    /// stops them being drawn, and must not throw away their indicators and
    /// drawings.
    ///
    /// While one exists it is fed every trade the flow pane is fed, on screen
    /// or not, which is what keeps them in step. The cost is the market's
    /// trades retained once per pane: one tape, N `ChartState`s, and still
    /// only one bar-building path. `MAX_CONTEXT_PANES` is what bounds that
    /// cost.
    pub time_panes: SmallVec<[ChartPane; MAX_CONTEXT_PANES]>,
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
    /// Whether a fetch is out. One at a time — the *closing* reply is what
    /// clears it, and every provider always sends one. A progressive fetch
    /// stays pending across all of its slices: it is one request throughout,
    /// and a chart already showing the most recent week has not finished
    /// loading.
    ohlcv_pending: bool,
    /// Whether the slices still arriving belong to a request whose answer is
    /// no longer wanted.
    ///
    /// A push feed can store a fresh block partway through a progressive run,
    /// and that discards the base being built (see [`Self::poll_ohlcv_capability`]).
    /// The slices already in flight know nothing about it, and folding them
    /// onto an empty base would build a history missing exactly the newest
    /// part — the slices that had already been thrown away. So they are
    /// dropped, and the closing one re-opens the door for a fresh request.
    ohlcv_stale: bool,
    /// The oldest candle held when the in-flight request went out, for a
    /// request that was reaching *back* past it — `None` for the opening one.
    ///
    /// Kept so the closing reply can answer the only question the trader has
    /// after clicking "older": did that get me anything? A reply is not empty
    /// when the venue has nothing older — a provider serving from a block it
    /// already holds honestly re-sends the same candles — so the answer is
    /// "did the oldest bar move", which is a fact rather than an inference.
    ohlcv_reaching_back: Option<i64>,
    /// Whether the last *load older* came back with nothing older than what
    /// was already held: the venue's record starts here, or the provider's
    /// reach does.
    ///
    /// Latched rather than recomputed, because the only way to know is to
    /// have asked. Cleared by a change of market, and by the opening request
    /// of a new one — never by time passing, which cannot make a venue's
    /// record deeper.
    ohlcv_older_exhausted: bool,
    /// Whether the next candle request asks for slices (View → progressive
    /// venue history). Mirrored from the app each frame rather than read from
    /// it, because the tab is what phrases the request and a tab in a test has
    /// no app around it.
    ///
    /// Only read when a request is *sent*: flipping the switch mid-run never
    /// reshapes an answer already being fetched, which is the honest
    /// behaviour — the venue was asked one way and is answering that way.
    pub progressive_history: bool,
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
    /// The interval the time pane opens on when it is first built. The
    /// header's default unless the feed declared one (`default_bars` with a
    /// time-showing `default_layout`); once the pane exists, its own header
    /// owns the interval and this is never read again.
    time_pane_opening_interval_ms: i64,
    /// Whether the time pane opens with its indicator legend folded, for the
    /// same reason the interval above is stashed: a restored workspace names
    /// the fold a frame before the pane it belongs to exists.
    time_pane_opening_legend_collapsed: bool,
    /// Set when the split is asked for and the time pane does not exist yet;
    /// drained by [`Self::apply_pending_layout`] on the following frame.
    pending_context_panes: usize,
    /// Which panes this tab's canvas shows. In-session only for now: per-tab
    /// chrome persistence is the open question §14 leaves to `ui-state.toml`,
    /// and this field with `split_fraction` and `focus` is what it would
    /// write.
    pub layout: CanvasLayout,
    /// The context column's share of the canvas width while it is shown.
    ///
    /// Kept while the column is collapsed, which is what it springs back to.
    /// One number and one flag rather than two numbers: a separate "restore"
    /// field would be a second opinion about the same width.
    pub split_fraction: f32,
    /// Whether the context column is collapsed to its rail.
    pub context_collapsed: bool,
    /// The pane the chrome speaks for while this tab is active: status bar,
    /// indicator targeting and the keyboard's drawing grammar (§11).
    /// Meaningless while the canvas is Single — read it through
    /// [`Self::focused_side`], never directly.
    pub focus: PaneSide,

    #[cfg(test)]
    time_header_chips: [egui::Rect; crate::time_header::PRESETS.len()],
    #[cfg(test)]
    canvas_divider: Option<egui::Rect>,
    #[cfg(test)]
    collapsed_rail: Option<egui::Rect>,
}

impl Tab {
    /// Move the context pane at `from` to `to`, keeping the rest in order.
    ///
    /// **The one reposition path.** The View menu, the keyboard and the
    /// control plane all arrive here, so none of them can grow its own idea of
    /// what moving a pane does; a drag gesture, when it lands, is sugar over
    /// this call rather than a second implementation of it.
    ///
    /// Addresses are [`PaneIndex`]es, so `0` names the flow pane. The flow
    /// pane does not move: it is the protagonist and its column is the one
    /// thing every preset agrees on. Refused rather than clamped — a caller
    /// that asked to move the heatmap meant something this cannot do, and
    /// quietly moving a different pane would be worse than saying no.
    ///
    /// Returns whether anything moved.
    pub fn move_context_pane(&mut self, from: PaneIndex, to: PaneIndex) -> bool {
        let (Some(from_slot), Some(to_slot)) = (from.checked_sub(1), to.checked_sub(1)) else {
            return false;
        };
        if from_slot >= self.time_panes.len() || to_slot >= self.time_panes.len() {
            return false;
        }
        if from_slot == to_slot {
            return false;
        }
        let pane = self.time_panes.remove(from_slot);
        self.time_panes.insert(to_slot, pane);
        // Focus follows the pane the trader just moved, so the next command
        // lands on the chart they were working with rather than on whichever
        // one slid into its place.
        self.focus = PaneSide::Time;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUT_PANE_MOVED",
            tab_id = self.id,
            from = from,
            to = to,
            "a context pane was moved within the stack"
        );
        true
    }

    /// How many panes this tab holds, drawn or not.
    #[must_use]
    pub fn pane_count(&self) -> PaneIndex {
        1 + self.time_panes.len()
    }

    /// The pane at `index`: `0` is the flow pane, `1..` the context stack.
    #[must_use]
    pub fn pane_at(&self, index: PaneIndex) -> Option<&ChartPane> {
        match index {
            0 => Some(&self.flow_pane),
            other => self.time_panes.get(other - 1),
        }
    }

    /// The pane at `index`, mutably.
    pub fn pane_at_mut(&mut self, index: PaneIndex) -> Option<&mut ChartPane> {
        match index {
            0 => Some(&mut self.flow_pane),
            other => self.time_panes.get_mut(other - 1),
        }
    }

    /// The first context pane, if this tab has built one.
    ///
    /// Most of the chrome speaks about *the* context chart because most
    /// layouts show one. The ones that show more reach for `time_panes`
    /// directly; this is the convenience, not the truth.
    #[must_use]
    pub fn time_pane(&self) -> Option<&ChartPane> {
        self.time_panes.first()
    }

    /// The first context pane, mutably.
    pub fn time_pane_mut(&mut self) -> Option<&mut ChartPane> {
        self.time_panes.first_mut()
    }

    /// Whether this tab has built any context pane at all.
    #[must_use]
    pub fn has_time_pane(&self) -> bool {
        !self.time_panes.is_empty()
    }

    /// A tab on `feed_id`/`symbol`, already streaming through `feed`, showing
    /// bar `spec`.
    ///
    /// `id` and `flow_pane_id` must be unique among the open tabs: pane ids
    /// namespace egui interaction state, so two tabs sharing them would share
    /// a drag. Context panes take their ids from the window's allocator as
    /// they are built, rather than a tab reserving one it may never use.
    #[must_use]
    pub fn new(
        id: u64,
        flow_pane_id: u64,
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
            feed_latency: feed.latency,
            forced_latency: crate::feed::forced_latency_split(),
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
            flow_pane: ChartPane::flow(flow_pane_id, spec, symbol.clone()),
            chip_label: String::new(),
            ohlcv_base: None,
            ohlcv_pending: false,
            ohlcv_stale: false,
            ohlcv_reaching_back: None,
            ohlcv_older_exhausted: false,
            progressive_history: true,
            ohlcv_generation: 0,
            ohlcv_capable: false,
            time_panes: SmallVec::new(),
            time_pane_opening_interval_ms: crate::time_header::DEFAULT_INTERVAL_MS,
            time_pane_opening_legend_collapsed: false,
            pending_context_panes: 0,
            layout: CanvasLayout::Single,
            split_fraction: DEFAULT_PANE_FRACTION,
            context_collapsed: std::env::var("QUANTICK_PANE_COLLAPSED")
                .is_ok_and(|value| value == "1"),
            focus: PaneSide::Flow,
            symbol,
            #[cfg(test)]
            time_header_chips: [egui::Rect::NOTHING; crate::time_header::PRESETS.len()],
            #[cfg(test)]
            canvas_divider: None,
            #[cfg(test)]
            collapsed_rail: None,
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
        // The channel carrying any in-flight slices is dropped with the old
        // handle, so nothing survives to be dropped as stale.
        self.ohlcv_stale = false;
        // A different market has a different record. Whatever this tab
        // learned about how far back the last one reached says nothing here.
        self.ohlcv_reaching_back = None;
        self.ohlcv_older_exhausted = false;
        self.ohlcv_capable = false;
        self.loading.set_active(LoadingTask::VenueHistory, false);
        for pane in self.panes_mut() {
            pane.install_history_prefix(Vec::new());
        }
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.feed_latency = handle.latency;
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        // The journal records where a session's trades came from; the
        // attached handle is the single truth for that.
        self.paper.set_session_source(if self.replay.is_some() {
            quantick_sim::history::SessionSource::Replay
        } else {
            quantick_sim::history::SessionSource::Live
        });
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

    /// Publish a latency reading as the attached feed would, for a test that
    /// needs the tab to have read one.
    #[cfg(test)]
    pub fn publish_latency_for_test(&mut self, split: Option<FeedLatency>) {
        let (tx, rx) = watch::channel(split);
        self.feed_latency = rx;
        // The sender is dropped on purpose: a `watch` receiver keeps serving
        // the value it was born with, which is what a test wants and what
        // `unsplit_latency` relies on in production.
        drop(tx);
    }

    /// Whether any pane on this tab cuts bars by a foldable time interval —
    /// the gate for venue candle history. Capability-shaped, like every other
    /// gate in the app (audit S1): the prefix belongs to what a pane *shows*
    /// (`BarSpec::Time` at a whole number of venue candles), never to which
    /// pane object it is. `bars → time` on the flow pane earns the same span
    /// the split's time pane gets.
    fn any_pane_wants_venue_history(&self) -> bool {
        std::iter::once(&self.flow_pane)
            .chain(self.time_pane())
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
        let progressive = self.progressive_history;
        if !self.any_pane_wants_venue_history()
            || self.replay.is_some()
            || self.ohlcv_pending
            || self.ohlcv_base.is_some()
            || !self.capabilities(config).ohlcv_history
        {
            return;
        }
        let slice_ms = progressive.then_some(crate::feed::OHLCV_SLICE_SPAN_MS);
        let command = FeedCommand::FetchOhlcv {
            span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
            slice_ms,
            // The opening request: back from the live edge. Reaching further
            // than one span is `request_older_ohlcv_history`'s job.
            before_ms: None,
        };
        match self.commands.try_send(command) {
            Ok(()) => {
                self.ohlcv_pending = true;
                self.ohlcv_reaching_back = None;
                self.ohlcv_older_exhausted = false;
                self.loading.begin(LoadingTask::VenueHistory);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_REQUESTED",
                    tab = self.id,
                    symbol = %self.symbol,
                    span_ms = crate::feed::TIME_HISTORY_SPAN_MS,
                    slice_ms = slice_ms.unwrap_or(0),
                    action = if progressive { "await_slices" } else { "await_single_reply" },
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
            // And with it, everything learned by reaching back through it. The
            // oldest bucket a request was measured against is gone, so a reply
            // still in flight must not be compared to it; and "the record
            // starts here" was a fact about a block this generation replaces.
            // `attach` clears both for the same reason on a change of market.
            self.ohlcv_reaching_back = None;
            self.ohlcv_older_exhausted = false;
            // Slices of the discarded answer may still be on their way. They
            // describe a base that no longer exists, so they are dropped
            // rather than folded onto nothing.
            self.ohlcv_stale = self.ohlcv_pending;
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
    /// stops asking. Either way the wait ends on the *closing* slice: a
    /// provider that answered only on success would strand the spinner on the
    /// one case that most needs explaining, and one that ended it on the first
    /// of thirteen slices would claim a quarter of history had arrived when a
    /// week of it had.
    ///
    /// Progressive slices run newest-first, so each one goes in *front* of the
    /// base already held. The prefix is rebuilt after every slice, which is
    /// the whole point — the chart grows leftwards while the rest is still
    /// being fetched, and nothing the trader is looking at moves under them
    /// (`ChartPane::install_history_prefix` shifts the viewport and every
    /// bar-anchored drawing by the same amount the prefix grew).
    fn take_ohlcv_history(
        &mut self,
        interval_ms: i64,
        bars: Vec<quantick_engine::Bar>,
        slice: crate::feed::OhlcvSlice,
    ) {
        let last = slice.is_last();
        if self.ohlcv_stale {
            // An answer the tab already threw away (see `poll_ohlcv_capability`).
            // The closing slice is what re-opens the door to asking again.
            if last {
                self.ohlcv_stale = false;
                self.ohlcv_pending = false;
                // Whatever this answer was measured against no longer exists.
                self.ohlcv_reaching_back = None;
                self.loading.end(LoadingTask::VenueHistory);
            }
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_SLICE_DISCARDED",
                tab = self.id,
                bars = bars.len(),
                last,
                action = "await_fresh_request",
                "dropped a slice of a candle answer that was superseded"
            );
            return;
        }
        if !last {
            self.take_ohlcv_slice(interval_ms, bars);
            return;
        }
        self.ohlcv_pending = false;
        if slice == crate::feed::OhlcvSlice::Refused {
            // Nobody looked, so nothing is known. End the wait — the spinner
            // was raised before the command left — and touch nothing else: no
            // short-answer warning about a venue that never answered, no
            // refold of the whole base over an empty vector, and above all no
            // verdict on a reach-back that was never served.
            self.ohlcv_reaching_back = None;
            self.loading.end(LoadingTask::VenueHistory);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_REFUSED",
                tab = self.id,
                symbol = %self.symbol,
                action = "await_the_running_fetch",
                "the provider was already fetching; this request was not served"
            );
            return;
        }
        let complete = matches!(slice, crate::feed::OhlcvSlice::Last { complete } if complete);
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
        let usable = self.take_ohlcv_slice(interval_ms, bars);
        // A *load older* that closed: did the oldest candle actually move? A
        // reply that re-sends what is already held is not an empty reply, so
        // the bar count cannot answer this and the oldest bucket can. Latched
        // here rather than asked again, because the only way to find out was
        // to ask.
        if let Some(was_oldest) = self.ohlcv_reaching_back.take() {
            let now_oldest = self.oldest_venue_candle_ms();
            let moved = now_oldest.is_some_and(|oldest| oldest < was_oldest);
            // Only a *complete* answer teaches anything about where the record
            // starts. A run that came up short — a venue that stopped
            // answering, a socket that failed — brought nothing older for a
            // reason that has nothing to do with the venue's depth, and
            // latching on it would retire the button for the session and tell
            // the trader their history starts here. Short means try again.
            // Complete *and* usable. A run that came up short brought nothing
            // older for a reason unrelated to the venue's depth, and a reply
            // the pane had to refuse is not an answer about depth at all.
            self.ohlcv_older_exhausted = complete && usable && !moved;
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "OHLCV_OLDER_SETTLED",
                tab = self.id,
                symbol = %self.symbol,
                was_oldest_ms = was_oldest,
                now_oldest_ms = now_oldest.unwrap_or(0),
                complete,
                usable,
                moved,
                exhausted = self.ohlcv_older_exhausted,
                action = if self.ohlcv_older_exhausted {
                    "stop_offering_older"
                } else if moved {
                    "older_available"
                } else {
                    "no_evidence_try_again"
                },
                "a request for older candles settled"
            );
        }
    }

    /// Merge one slice into the base and rebuild the prefix from it.
    ///
    /// Reports whether the slice was *usable* — an interval this pane can fold
    /// from. A refused slice is not an answer about the venue's depth, and the
    /// exhaustion latch upstream must not read it as one.
    ///
    /// Shared by the closing reply and every slice before it: whether an
    /// answer arrived whole or in thirteen pieces changes when the wait ends,
    /// never how the candles are installed. Merging rather than replacing is
    /// what makes that true — a run's last slice is the *oldest* week of the
    /// span, and assigning it over the base would throw away the twelve that
    /// had already been drawn.
    fn take_ohlcv_slice(&mut self, interval_ms: i64, bars: Vec<quantick_engine::Bar>) -> bool {
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
                action = if self.ohlcv_base.is_some() {
                    "refuse_slice_keep_prefix"
                } else {
                    "no_prefix"
                },
                "candle history arrived at an interval the pane cannot fold from"
            );
            // Refuse the slice, never the history. Recording an empty base is
            // right for the *opening* answer — it is how "this venue serves
            // nothing this pane can fold" is remembered — but a reach-back
            // reply at a bad interval arrives on top of a week (or a quarter)
            // the trader has already paged in, and throwing that away would
            // cost them everything they waited for over one unusable slice.
            if self.ohlcv_base.is_none() {
                self.ohlcv_base = Some(Vec::new());
            }
            return false;
        }
        let base = self.ohlcv_base.get_or_insert_with(Vec::new);
        if base.is_empty() {
            *base = bars;
        } else if !bars.is_empty() {
            merge_older_candles(base, bars);
        }
        self.refold_history_prefix();
        true
    }

    /// Hand the tab one candle-history reply directly, as the feed drain does.
    ///
    /// The `QUANTICK_VENUE_HISTORY_DEMO` hook's one door in. It goes through
    /// the same function a real reply does — a hook that installed a prefix by
    /// another route would photograph a state the app cannot actually be in.
    /// The pending flags are set first because that is what a request having
    /// gone out looks like, and an unclosed run is precisely the frame the
    /// `partial` variant exists to reach.
    pub fn deliver_ohlcv_slice(
        &mut self,
        interval_ms: i64,
        bars: Vec<quantick_engine::Bar>,
        slice: crate::feed::OhlcvSlice,
    ) {
        if !self.ohlcv_pending {
            self.ohlcv_pending = true;
            self.loading.begin(LoadingTask::VenueHistory);
        }
        self.take_ohlcv_history(interval_ms, bars, slice);
    }

    /// How many venue candles this tab holds, at the base interval. Zero on a
    /// feed that serves none, and before the first reply lands.
    #[must_use]
    pub fn venue_candles_held(&self) -> usize {
        self.ohlcv_base.as_ref().map_or(0, Vec::len)
    }

    /// The oldest venue candle held, by bucket start.
    fn oldest_venue_candle_ms(&self) -> Option<i64> {
        self.ohlcv_base.as_ref()?.first().map(|bar| bar.open_time)
    }

    /// Whether asking for older candles could get the trader anything — and
    /// when it could not, *which* of the reasons it is.
    ///
    /// A bool would be enough to grey the control out and is not enough to
    /// say why, and "why" is the whole of the disabled tooltip. Under the
    /// data-honesty rule a control that offers three possible reasons and is
    /// disabled for a fourth is telling the trader something untrue: on a tick
    /// chart the answer is not "the venue's record starts here", it is that
    /// nothing on this chart is cut by time, so no venue candle was ever
    /// wanted. One enum, so the reason shown is the reason.
    #[must_use]
    pub fn older_candles(&self, capabilities: FeedCapabilities) -> OlderCandles {
        if !capabilities.ohlcv_history {
            OlderCandles::FeedServesNone
        } else if !self.any_pane_wants_venue_history() {
            OlderCandles::NoChartCutByTime
        } else if self.ohlcv_pending {
            OlderCandles::Fetching
        } else if self.oldest_venue_candle_ms().is_none() {
            OlderCandles::NotArrivedYet
        } else if self.ohlcv_older_exhausted {
            OlderCandles::RecordStartsHere
        } else {
            OlderCandles::Available
        }
    }

    /// Shorthand for the one caller that only needs the yes/no.
    #[must_use]
    pub fn can_load_older_candles(&self, capabilities: FeedCapabilities) -> bool {
        self.older_candles(capabilities).is_available()
    }

    /// Reach one more [`crate::feed::TIME_HISTORY_SPAN_MS`] into the past and
    /// prepend what comes back.
    ///
    /// The whole reason a chart opens on a week rather than a quarter: the
    /// deep history is available, it is simply asked for by the trader who
    /// wants it. Each call is the same request the opening one was, with its
    /// right-hand edge moved to just before the oldest candle held — so the
    /// windows never overlap and the merge on the way back in has nothing to
    /// reconcile.
    ///
    /// Reports whether a request actually went out, so a caller can tell "the
    /// venue is fetching" from "there was nothing to ask for".
    pub fn request_older_ohlcv_history(&mut self, capabilities: FeedCapabilities) -> bool {
        let oldest = self.oldest_venue_candle_ms();
        if !self.can_load_older_candles(capabilities) || oldest.is_none() {
            tracing::debug!(
                target: "quantick::app",
                event_code = "OHLCV_OLDER_DECLINED",
                tab = self.id,
                pending = self.ohlcv_pending,
                exhausted = self.ohlcv_older_exhausted,
                held = oldest.is_some(),
                "nothing older to ask for"
            );
            return false;
        }
        // Read once above and unwrapped here: `can_load_older_candles` already
        // required it, and a second read that could disagree with the guard is
        // exactly the kind of duplicate this branch is trying not to leave.
        let oldest = oldest.expect("the guard above required a candle held");
        // One millisecond before the oldest bucket start held. The plan's
        // windows are closed at both ends, so anything else would re-fetch the
        // candle already on screen.
        let before_ms = oldest.saturating_sub(1);
        let slice_ms = self
            .progressive_history
            .then_some(crate::feed::OHLCV_SLICE_SPAN_MS);
        let command = FeedCommand::FetchOhlcv {
            span_ms: crate::feed::TIME_HISTORY_SPAN_MS,
            slice_ms,
            before_ms: Some(before_ms),
        };
        match self.commands.try_send(command) {
            Ok(()) => {
                self.ohlcv_pending = true;
                self.ohlcv_reaching_back = Some(oldest);
                self.loading.begin(LoadingTask::VenueHistory);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_OLDER_REQUESTED",
                    tab = self.id,
                    symbol = %self.symbol,
                    span_ms = crate::feed::TIME_HISTORY_SPAN_MS,
                    before_ms,
                    slice_ms = slice_ms.unwrap_or(0),
                    action = "await_prepend",
                    "asked the venue for another span of older candles"
                );
                true
            }
            // Same two cases the opening request distinguishes, and for the
            // same reasons: a busy frame is worth a retry, a dead feed is
            // worth saying out loud.
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(
                    target: "quantick::app",
                    event_code = "OHLCV_OLDER_BACKPRESSURE",
                    tab = self.id,
                    action = "retry_on_next_click",
                    "older-candle request not queued; channel full"
                );
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "OHLCV_OLDER_CHANNEL_CLOSED",
                    tab = self.id,
                    symbol = %self.symbol,
                    action = "no_history_until_feed_restart",
                    "older-candle request cannot be sent; the feed is gone"
                );
                false
            }
        }
    }

    /// Rebuild every pane's prefix from the base at that pane's interval.
    ///
    /// Free of the venue: a chip click lands here, not on the network. One
    /// base, one fold per time-cutting pane — the flow pane showing time
    /// bars folds exactly as the split's time pane does (audit S1).
    /// Reports whether any prefix actually changed — an installed prefix
    /// rebuilds the indicators, so the caller can skip sending a second one.
    pub fn refold_history_prefix(&mut self) -> bool {
        let Self {
            ohlcv_base,
            flow_pane,
            time_panes,
            ..
        } = self;
        let Some(base) = ohlcv_base.as_ref() else {
            return false;
        };
        let mut changed = false;
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
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
        // Read from what the layout *holds*, never from which variant it is.
        // Matching one variant is how the three-pane canvas shipped with dead
        // focus: a click set `self.focus` and every reader threw it away,
        // because `TimeTimeAndFlow` was not in the arm.
        if !self.has_time_pane() || !self.layout.shows_time() {
            return PaneSide::Flow;
        }
        if !self.layout.shows_flow() {
            return PaneSide::Time;
        }
        self.focus
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    pub fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time => self.time_pane().unwrap_or(&self.flow_pane),
            PaneSide::Flow => &self.flow_pane,
        }
    }

    /// Whether a LAYERS lamp reads as on, and what blocks it if anything.
    ///
    /// One reading, two callers: the toolbar model built each frame and the
    /// semantic scene an operator captures on demand. A lamp that told the
    /// trader one thing and an assistant another would be worse than no scene
    /// at all, so neither side gets its own copy of the question — both come
    /// here, and here asks [`ChartPane::layer_switched_on`] and
    /// [`ChartPane::layer_blocked`], which already resolve every layer to the
    /// one field and the one gate that own it.
    ///
    /// The reading is the *switch*, not what the source lets through it: a
    /// lamp lit from `layer_visible` reads dark while book capture is starting
    /// and forever on a source with no book, so the trader presses an unlit
    /// button and switches the layer they wanted off. The block beside it is
    /// what says the source cannot fill it.
    pub(crate) fn layer_toggle_state(
        &self,
        layer: ChartLayer,
        style: &ChartStyle,
        capabilities: FeedCapabilities,
    ) -> (bool, Option<LayerBlock>) {
        let pane = self.pane(self.layer_toggle_side(layer));
        (
            pane.layer_switched_on(layer, style),
            pane.layer_blocked(layer, capabilities),
        )
    }

    /// Which pane a LAYERS button speaks for.
    ///
    /// The footprint folds the pane's *own* retained trades, so its lamp
    /// answers for the pane with focus: one lit from the flow pane while the
    /// time pane has focus would report a layer the trader is not looking at.
    /// The other three read the tape, and only the flow pane has one.
    fn layer_toggle_side(&self, layer: ChartLayer) -> PaneSide {
        match layer {
            // Read off the tape, and only the flow pane has one. A time pane
            // asked about these answers for machinery it does not own, which
            // is what `ChartPane::layer_blocked` says in words.
            ChartLayer::TapeChart
            | ChartLayer::TapeHeatmap
            | ChartLayer::TapeBubbles
            | ChartLayer::Heatmap
            | ChartLayer::Bubbles
            | ChartLayer::LiveStrip
            | ChartLayer::LaneMarks
            | ChartLayer::FlowLegend
            | ChartLayer::BookStatus
            | ChartLayer::DepthGaps => PaneSide::Flow,
            // The pane's own: the footprint folds the pane's retained trades,
            // the rest are that canvas's chrome and its objects. A lamp lit
            // from the flow pane while the time pane has focus would report a
            // layer the trader is not looking at.
            ChartLayer::Footprint
            | ChartLayer::Grid
            | ChartLayer::LastPrice
            | ChartLayer::BackfillDivider
            | ChartLayer::SeamDivider
            | ChartLayer::Crosshair
            | ChartLayer::PaperTrading
            | ChartLayer::TradePaint
            | ChartLayer::Drawings => self.focused_side(),
        }
    }

    /// See [`Self::pane`].
    /// Point exactly one of this tab's panes at the object whose content is
    /// being typed off-canvas, and clear the other.
    ///
    /// Both are written every time on purpose: the flag suppresses an
    /// object's own painting, so a pane left holding a stale index keeps a
    /// note invisible for the rest of the session. The tab is what knows
    /// whether a time pane exists at all, which is why the loop lives here
    /// rather than in the host.
    pub fn set_content_editing(&mut self, target: Option<(PaneSide, usize)>) {
        self.flow_pane.content_editing = target
            .filter(|(side, _)| *side == PaneSide::Flow)
            .map(|(_, index)| index);
        if let Some(time) = self.time_pane_mut() {
            time.content_editing = target
                .filter(|(side, _)| *side == PaneSide::Time)
                .map(|(_, index)| index);
        }
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time => self.time_panes.first_mut().unwrap_or(&mut self.flow_pane),
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
        if self.pane(focused).drawings.selected().is_some() || self.time_panes.is_empty() {
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

    /// Every pane holding this market's bars, on screen or not, each beside
    /// the side it answers to — flow first, so a walk of one tab is stable and
    /// a capture stays diffable against the one before it.
    ///
    /// An iterator and not a `Vec`: this is walked per frame by the control
    /// plane's journal comparison, which must not touch the allocator on a
    /// quiet frame.
    pub fn panes(&self) -> impl Iterator<Item = (&ChartPane, PaneSide)> {
        std::iter::once((&self.flow_pane, PaneSide::Flow))
            .chain(self.time_panes.iter().map(|time| (time, PaneSide::Time)))
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    pub fn panes_mut(&mut self) -> impl Iterator<Item = &mut ChartPane> {
        // Destructured rather than borrowed field by field: the flow pane and
        // the context stack are two disjoint parts of `self`, and the compiler
        // only knows that when it is told in one pattern.
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once(flow_pane).chain(time_panes.iter_mut())
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
        let wanted = layout
            .kinds()
            .iter()
            .filter(|kind| matches!(kind, PaneKind::Time))
            .count();
        // Set, never raised: switching to a three-pane layout and back before
        // the next frame used to leave the count where the wider layout put
        // it, and the tab then built a pane no layout had asked for — seeded
        // from the whole retained tape and fed every trade thereafter.
        self.pending_context_panes = wanted.saturating_sub(self.time_panes.len());
        if wanted > self.time_panes.len() {
            // Seeding replays every retained trade, which on a deep history
            // holds the render thread long enough to notice. Armed here and
            // done on the next frame, exactly as a bar-spec change is: the
            // frame carrying the menu click paints the loading overlay first,
            // so the wait reads as the chart working rather than the app
            // hanging.
            self.loading.begin(LoadingTask::BarRebuild);
        }
        self.focus = match layout {
            CanvasLayout::Single => PaneSide::Flow,
            CanvasLayout::Time => PaneSide::Time,
            // The split reveals whichever pane the previous layout was not
            // showing: the time pane coming from Single, the flow pane coming
            // from Time.
            CanvasLayout::TimeAndFlow | CanvasLayout::TimeTimeAndFlow => match previous {
                CanvasLayout::Single => PaneSide::Time,
                CanvasLayout::Time => PaneSide::Flow,
                CanvasLayout::TimeAndFlow | CanvasLayout::TimeTimeAndFlow => self.focus,
            },
        };
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANVAS_LAYOUT",
            layout = ?layout,
            time_pane_bars = self.time_pane().map(|pane| pane.state.bars().len()),
            action = if self.pending_context_panes > 0 {
                "build_time_pane_next_frame"
            } else {
                "relayout_canvas"
            },
            "canvas layout changed"
        );
    }

    /// Build the context panes the last layout change asked for, if any are
    /// due.
    ///
    /// Runs at the top of the frame after the click, so the overlay armed by
    /// [`Self::set_layout`] has already been painted once. `ids` is the
    /// window's allocator rather than the tab's: pane ids namespace egui
    /// interaction state across the whole window, so a tab may not mint its
    /// own.
    pub fn apply_pending_layout(
        &mut self,
        config: &AppConfig,
        style: &ChartStyle,
        ids: &mut PaneIdAllocator,
    ) {
        if self.pending_context_panes == 0 {
            return;
        }
        self.pending_context_panes -= 1;
        let mut pane = ChartPane::time(ids.alloc(), self.time_pane_opening_interval_ms);
        pane.legend_collapsed = self.time_pane_opening_legend_collapsed;
        pane.seed_from(
            self.flow_pane.state.trades(),
            self.flow_pane.state.backfill_trade_count(),
        );
        // The pane opens looking like the one it splits away from: a user who
        // switched the crosshair off is not asking for it back by opening a
        // second view of the same market. Orientation is part of that look —
        // an upside-down market does not turn back over by being given a
        // second view, and the boot's QUANTICK_INVERTED hook fires before
        // this pane exists at all.
        // Copying the *switches* rather than a list of field names: an earlier
        // version cloned `hidden_layers` alone, which left the footprint
        // behind — a per-pane field of its own — so the split opened with the
        // ladder on in the flow pane and off in the time pane, contradicting
        // the paragraph above and darkening the toolbar's footprint lamp the
        // moment the trader clicked into the left chart. `apply_layer_states`
        // drops whatever this pane does not draw, which is the whole of what
        // §11 asks for, and it covers `hidden_layers` in passing.
        //
        // Every layer but one. `Drawings` resolves to `DrawingStore`, whose
        // setter records an undo entry — right for a click, wrong for a pane
        // being born: seeded through it, a time pane holding zero objects
        // opens with a non-empty history, and the trader's first Ctrl+Z there
        // un-hides drawings rather than doing nothing. It is seeded through
        // the store's own opening setter instead.
        let mut states = self.flow_pane.layer_states(style);
        states.remove(&ChartLayer::Drawings);
        pane.apply_layer_states(&states);
        pane.drawings
            .open_all_hidden(self.flow_pane.drawings.all_hidden());
        pane.price_view
            .set_inverted(self.flow_pane.price_view.is_inverted());
        self.time_panes.push(pane);
        // One pane per frame, for the reason the first one waits a frame at
        // all: seeding replays every retained trade, and building three at
        // once would hold the render thread for three times as long. The
        // overlay stays up until the last one lands.
        if self.pending_context_panes == 0 {
            self.loading.end(LoadingTask::BarRebuild);
        }
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

    /// Where this feed's delay is being spent, as far as the provider can tell.
    ///
    /// `None` while replaying: a recording's prints are as old as the day they
    /// were captured and the playback clock decides when they appear, so there
    /// is no delay here for any hop to own — the same reason
    /// [`trade_arrival_ms`](Self::trade_arrival_ms) reports nothing there.
    #[must_use]
    pub fn feed_latency(&self) -> Option<FeedLatency> {
        if self.replay.is_some() {
            return None;
        }
        self.forced_latency.or(*self.feed_latency.borrow())
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
        let (previous_feed, previous_symbol) = self.active.clone();
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
        // The marks stay — only the trader deletes a drawing — but they were
        // drawn on the instrument that just left. Time survives a symbol
        // switch and price does not, so without this a BTC level would paint
        // at full strength over an index chart, at a number that means
        // nothing there (§D7b).
        for pane in self.panes_mut() {
            pane.drawings.mark_market_changed();
            // The regions those instances watched belong to the market that
            // just left; a bot must never fire on a level from another
            // instrument. Disarmed by name, never silently dropped. Their
            // pending entries need no sweep of ours: the paper reset a few
            // lines down cancels every order with the honest `reset` label.
            let _ = pane
                .strategies
                .disarm_all(quantick_strategy::DisarmReason::MarketChanged);
            let _ = pane.take_strategy_bars();
        }
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
        self.apply_feed_bubble_preset_after_switch(config, &previous_feed, &previous_symbol);
    }

    /// Apply the arrived-at declared preset — when the switch crossed feeds,
    /// or when it crossed symbols whose declared looks differ. A symbol hop
    /// between two symbols that declare nothing of their own keeps the user's
    /// panel tweaks, exactly as before per-symbol declarations existed: the
    /// declared look belongs to the feed, and to the symbols that state one.
    ///
    /// One asymmetry is deliberate: hopping *off* a declared symbol onto one
    /// that declares nothing (on a feed that also declares nothing) keeps the
    /// look just left on screen — nothing remembers what the panel wore
    /// before the declaration applied, and inventing a "previous look" store
    /// for that one hop would be a second owner for the panel's state.
    pub fn apply_feed_bubble_preset_after_switch(
        &mut self,
        config: &AppConfig,
        previous_feed: &str,
        previous_symbol: &str,
    ) {
        if previous_feed == self.feed_id {
            let feed = config.feed(&self.feed_id);
            let arrived = feed.and_then(|feed| feed.bubble_preset_for(&self.symbol));
            let left = feed.and_then(|feed| feed.bubble_preset_for(previous_symbol));
            if arrived.is_none() || arrived == left {
                return;
            }
        }
        self.apply_feed_bubble_preset(config);
    }

    /// Apply the bubble preset declared for the current feed and symbol, if
    /// one is declared ([`FeedConfig::bubble_preset_for`]'s ladder: the
    /// symbol's own entry first, the feed-wide declaration behind it).
    ///
    /// A feed declaring nothing changes nothing: the panel keeps the look the
    /// user last chose. An unknown name is reported and ignored — the presets
    /// file is user-edited, and a typo there must not silently restyle the
    /// chart.
    pub fn apply_feed_bubble_preset(&mut self, config: &AppConfig) {
        let Some(name) = config
            .feed(&self.feed_id)
            .and_then(|feed| feed.bubble_preset_for(&self.symbol))
            .map(str::to_owned)
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
                symbol = %self.symbol,
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
                symbol = %self.symbol,
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
        context_collapsed: bool,
        focus: Option<PaneSide>,
        time_interval_ms: Option<i64>,
        legends: LegendFold,
    ) {
        self.context_collapsed = context_collapsed;
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
        self.flow_pane.legend_collapsed = legends.flow;
        // The time pane may not exist yet. `set_layout` only *arms* it
        // (`pending_time_pane`); `apply_pending_layout` builds it a frame
        // later, which is the very reason the opening interval above is
        // stashed rather than assigned. Writing the fold here alone would
        // write it to `None` and the pane would open expanded — and the next
        // `capture_arrangement` would then persist that `false` over the
        // trader's choice.
        self.time_pane_opening_legend_collapsed = legends.time;
        if let Some(time) = self.time_pane_mut() {
            time.legend_collapsed = legends.time;
        }
    }

    /// Let every pane's selectors settle, then mirror the result onto the
    /// rebuild indicator: it is up while *any* pane has a rebuild pending.
    pub fn apply_spec_changes(&mut self) {
        // Every pane, by address. Settling "the flow pane and the time pane"
        // left the second stacked chart's selector armed for ever: its header
        // chip lit, its interval changed, and its bars never rebuilt.
        for pane in 0..self.pane_count() {
            self.apply_spec_change_at(pane);
        }
        let rebuilding = self
            .panes()
            .any(|(pane, _side)| pane.pending_spec.is_some());
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
    fn apply_spec_change_at(&mut self, index: PaneIndex) {
        let Some(desired) = self.pane_at(index).map(ChartPane::current_spec) else {
            return;
        };
        let Some(pane) = self.pane_at_mut(index) else {
            return;
        };
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
                pane.set_spec(desired);
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
                if !refolded && let Some(pane) = self.pane_at_mut(index) {
                    pane.send_indicator_rebuild();
                }
                let Some(pane) = self.pane_at_mut(index) else {
                    return;
                };
                let slot = anchor.and_then(|ms| pane.slot_at_time(ms));
                let slots = pane.slots();
                pane.viewport.reanchor(slot, slots);
                // The marks follow the view: same market time, this pane's
                // new bar space. Nothing is lost, so there is nothing to
                // announce.
                pane.reanchor_drawings(old_slots);
                // The strategies do not follow: the body average that
                // defines a force bar means something else under another
                // bar spec, so the instances disarm and say why. The tape
                // itself continues, so any pending bot entry is swept here
                // and now — through the same funnel manual orders use.
                let cleanup = pane
                    .strategies
                    .disarm_all(quantick_strategy::DisarmReason::BarSpecChanged);
                let _ = pane.take_strategy_bars();
                for command in cleanup {
                    let _ = self.paper.apply_strategy_command(command);
                }
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
                    slice,
                }) => {
                    self.take_ohlcv_history(interval_ms, bars, slice);
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
        self.run_strategies();
    }

    /// Evaluate the armed instances against the bars that just closed and
    /// route the simulator's answers back to them.
    ///
    /// Runs inside the same sweep that ingested the trades, so the slots
    /// queued with each bar and the drawings' anchors are read against one
    /// cut of the series. Order per pane: the prints' events first (they
    /// resolve earlier operations), then closed bars oldest-first, each
    /// instance's commands applied immediately so its own acknowledgement
    /// reaches it before anything else does. Cost is zero on a chart with
    /// no instances — the pane queues no bars and the paper host buffers no
    /// events.
    fn run_strategies(&mut self) {
        let print_events = self.paper.drain_bot_events();
        let Self {
            paper,
            flow_pane,
            time_panes,
            ..
        } = self;
        let mut watching = 0;
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
            if pane.strategies.is_empty() {
                continue;
            }
            // The batch to each instance, applying what it answers with —
            // the self-protection close after a dropped bracket. Applying
            // a returned command emits no events of its own (kernel
            // contract), so one echo settles it.
            if !print_events.is_empty() {
                for index in 0..pane.strategies.instances.len() {
                    let responses = pane.strategies.instances[index]
                        .armed
                        .on_sim_events(&print_events);
                    for command in responses {
                        let events = paper.apply_strategy_command(command);
                        let _ = pane.strategies.instances[index]
                            .armed
                            .on_sim_events(&events);
                    }
                }
            }
            let bars = pane.take_strategy_bars();
            if !bars.is_empty() {
                // An instance whose drawing was deleted from a surface that
                // could not remove it directly dies here, in the sweep.
                let alive: Vec<crate::drawings::DrawingId> = pane
                    .strategies
                    .instances
                    .iter()
                    .map(|instance| instance.drawing)
                    .filter(|id| pane.drawings.index_of(*id).is_some())
                    .collect();
                let orphan_cleanup = pane.strategies.drop_orphans(|id| alive.contains(&id));
                for command in orphan_cleanup {
                    // A dead drawing's bot must not leave its entry resting
                    // with no badge over it; swept through the same funnel.
                    let _ = paper.apply_strategy_command(command);
                }
            }
            for (bar, slot) in &bars {
                for index in 0..pane.strategies.instances.len() {
                    let drawing = pane.strategies.instances[index].drawing;
                    // A region that cannot honestly be tested (hidden, off
                    // its series, another market) holds fire but never
                    // starves the ruler: the trigger's contract is every
                    // closed bar, so the gates shut instead of the feed.
                    let (region, active) = pane.strategy_region(drawing, *slot).unwrap_or((
                        quantick_strategy::Region::new(
                            rust_decimal::Decimal::ZERO,
                            rust_decimal::Decimal::ZERO,
                        ),
                        false,
                    ));
                    let flat = paper.is_flat();
                    let commands = pane.strategies.instances[index]
                        .armed
                        .on_closed_bar(bar, &region, active, flat);
                    for command in commands {
                        let events = paper.apply_strategy_command(command);
                        let _ = pane.strategies.instances[index]
                            .armed
                            .on_sim_events(&events);
                    }
                }
            }
            watching += pane.strategies.watching();
        }
        paper.set_bot_listening(watching > 0);
    }

    /// Apply the cleanup commands the panes' drawing menus queued this
    /// frame — a disarm or removal over a resting retest limit cancels the
    /// order. Runs on the UI frame that clicked, not on the next print: a
    /// cancel that waits for the market to move may lose the race to it.
    pub fn apply_strategy_cleanup(&mut self) {
        let Self {
            paper,
            flow_pane,
            time_panes,
            ..
        } = self;
        for pane in std::iter::once(flow_pane).chain(time_panes.iter_mut()) {
            for command in pane.take_strategy_cleanup() {
                let _ = paper.apply_strategy_command(command);
            }
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
            // Judgements armed on the old timeline do not carry into the
            // rebuilt one — the same honesty rule the simulator's flatten
            // follows, with the reason on the badge. No cleanup commands
            // come back under this reason: `paper.on_timeline_reset` below
            // sweeps every order with the honest `reset` label.
            let _ = pane
                .strategies
                .disarm_all(quantick_strategy::DisarmReason::TimelineReset);
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
        // Never the forced split's figure. This is a *measurement* — it reaches
        // the log as APP_HIGH_TRADE_LAG, the control feed scope and the health
        // view, none of which carry a marker saying a capture run invented it,
        // and inferred data that is not labelled as such is the one thing this
        // repo does not ship. The hook drives the readout through the latency
        // port instead, where every consumer already knows the figure is the
        // feed's own and not the chart's.
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

        // A position on the tape belongs to the session that is ending:
        // flatten it while the journal still carries that session's
        // source. attach() flips the source to the new feed's, and a
        // flatten after it would file the old session's trade under the
        // new session's name — a live trade laundered into the practice
        // record. (reset_market_state below flattens again: a no-op.)
        self.paper.on_timeline_reset();
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
        // Same ordering rule as open_replay: the flatten of a replay
        // position must journal under the replay source, before attach()
        // flips the journal back to live — or a practice trade counts in
        // the real track record.
        self.paper.on_timeline_reset();
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
        // How many context charts this layout asks for, and how many the tab
        // has actually built. The lower of the two is what gets drawn: a
        // layout may name a pane the tab is still building, and half a canvas
        // is better than a frame of nothing.
        let context_wanted = self
            .layout
            .kinds()
            .iter()
            .filter(|kind| matches!(kind, PaneKind::Time))
            .count();
        let context_shown = context_wanted.min(self.time_panes.len());
        let show_time = context_shown > 0;
        // The flow pane also stands in for a time pane still being built, so
        // the frame between asking for the Time layout and the pane existing
        // shows the market rather than nothing.
        let show_flow = self.layout.shows_flow() || !show_time;
        let split = show_time && show_flow;
        // One visible pane is handed the whole canvas and the rest of this
        // reduces to nothing: no divider, no focus rule — though a lone time
        // pane keeps its header.
        let (time_area, divider, flow_area) = if split {
            let width = if self.context_collapsed {
                canvas_layout::PaneWidth::Collapsed {
                    restore: self.split_fraction,
                }
            } else {
                canvas_layout::PaneWidth::Manual(self.split_fraction)
            };
            let row = canvas_layout::split_row(area, &[width, canvas_layout::PaneWidth::Auto]);
            (Some(row.panes[0]), Some(row.dividers[0]), row.panes[1])
        } else if show_time {
            (Some(area), None, area)
        } else {
            (None, None, area)
        };
        // A collapsed column paints a rail, not charts: eight pixels is a
        // handle, not a chart, and laying one out there would draw a price
        // axis and nothing else.
        let collapsed_rail = (split && self.context_collapsed)
            .then_some(time_area)
            .flatten();
        let time_area = if collapsed_rail.is_some() {
            None
        } else {
            time_area
        };

        // The context column, carved into one band per chart it shows, top to
        // bottom. Each band spends its own header strip and hands back the
        // chart rect below it.
        let mut context_charts: SmallVec<[egui::Rect; MAX_CONTEXT_PANES]> = SmallVec::new();
        if let Some(column) = time_area {
            // Focus before input, so the click that focuses a pane is also the
            // click that pane goes on to handle. Only a split has focus to
            // move: a single visible pane is the focused one by definition.
            if split {
                self.focus_from_pointer(ui, column, flow_area);
            }
            let heights: SmallVec<[canvas_layout::PaneWidth; MAX_CONTEXT_PANES]> =
                SmallVec::from_elem(canvas_layout::PaneWidth::Auto, context_shown);
            let bands = canvas_layout::split_column(column, &heights);
            for (slot, band) in bands.panes.iter().enumerate().take(context_shown) {
                let areas = split_time_pane(*band);
                // Each context chart carries its own timeframe selector (§11):
                // its BARS group, beside the toolbar's, which keeps governing
                // the flow pane.
                let mut interval_ms = self.time_panes[slot].time_interval_ms;
                let header_layout = crate::time_header::draw(
                    ui,
                    areas.header,
                    &mut interval_ms,
                    self.time_panes[slot].id,
                );
                #[cfg(test)]
                if slot == 0 {
                    self.time_header_chips = header_layout.chips();
                }
                if header_layout.changed {
                    let pane = &mut self.time_panes[slot];
                    pane.kind = BarKind::Time;
                    pane.time_interval_ms = interval_ms;
                }
                context_charts.push(areas.chart);
            }
        }

        // Which shared mark the pointer is over, on each pane, against the
        // other pane's store. Answered here because answering it needs both
        // panes at once, and the loop below holds them one at a time.
        let picks = self.shared_picks(ui);

        let mut edits: SmallVec<[(PaneIndex, SharedInteraction); MAX_CANVAS_PANES]> =
            SmallVec::new();
        {
            // Focus as an address, so the loop below compares like with
            // like however many panes it walks.
            let focused = match self.focused_side() {
                PaneSide::Flow => 0,
                PaneSide::Time => 1,
            };
            let Self {
                flow_pane,
                time_panes,
                symbol,
                paper,
                ..
            } = self;
            // The time pane has no tape of its own (§11), so its footprint
            // rows adopt the flow pane's capture bucket — the instrument's
            // grid is a fact about the market, not about which pane shows it.
            //
            // Which is why there is no longer a gate here. This used to run
            // only while the time pane's *footprint layer* was visible, and
            // that contradicted the sentence above it: the ladders have a
            // second consumer now, and a fixed-range volume profile folds them
            // with the layer hidden. So the same profile, on the same market,
            // read at the flow pane's bucket on one chart and at the default
            // on the other — a hundredfold difference in row height on WDO,
            // which paints as a slab beside a wash. Two surfaces that are the
            // same thing have to behave the same way.
            //
            // Unconditional is also cheap: `set_footprint_group` returns
            // immediately when the bucket has not changed, which is every
            // frame but the one after a market switch.
            if let (Some(time), Some(base)) = (
                time_panes.first_mut(),
                flow_pane
                    .orderflow
                    .as_ref()
                    .map(|tape| tape.base_capture_grouping()),
            ) {
                time.state.set_footprint_group(base);
            }
            let mut chrome = PaneChrome {
                toolrail: chrome.toolrail,
                presets: chrome.presets,
                begin_text_edit: chrome.begin_text_edit,
                style: chrome.style,
                tz: chrome.tz,
                symbol,
                paper,
                paper_owns_input: false,
                shared_pick: None,
                shared: SharedInteraction::default(),
                capabilities: chrome.capabilities,
                side_inferred: chrome.side_inferred,
                footprint: chrome.footprint,
                layers: chrome.layers,
            };
            // Time pane first, then flow. Both take the same two steps in the
            // same order — which is what keeps the split honest: the second
            // pane cannot drift from the first, and one pane is this same
            // loop with one entry in it.
            // Context panes carry addresses `1..`, the flow pane `0` — the
            // order `Tab::pane_at` uses, never the order they sit in.
            let context = time_panes
                .iter_mut()
                .zip(context_charts.iter().copied())
                .enumerate()
                .map(|(slot, (pane, chart))| (pane, chart, slot + 1));
            let flow = show_flow.then_some((&mut *flow_pane, flow_area, 0 as PaneIndex));
            for (pane, rect, side) in context.chain(flow) {
                // Order entry follows the focused pane (§11): both charts are
                // trading surfaces — a level is as true on the time pane as on
                // the flow pane — and focus lands on the press that acts, so
                // the first click already trades where the accent rule is.
                // Unsplit, the flow pane is the only pane and nothing changes.
                chrome.paper_owns_input = side == focused;
                chrome.shared_pick = picks.for_pane(side);
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

        if let Some(rail) = collapsed_rail {
            self.draw_collapsed_rail(ui, rail);
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
        let count = self.pane_count();
        let mut picks = SharedPicks {
            by_pane: SmallVec::from_elem(None, count),
        };
        if count < 2 {
            // One pane has no other pane to mirror.
            return picks;
        }
        let Some(position) = ui.input(|input| input.pointer.latest_pos()) else {
            return picks;
        };

        for viewer in 0..count {
            let Some(pane) = self.pane_at(viewer) else {
                continue;
            };
            // Only the pane the pointer is actually over is asked. Besides
            // saving the work, it is what stops a horizontal line — which
            // spans a whole chart — from reporting a hit on the pane beside
            // the one the pointer is in, at the same height.
            if !pane
                .last_chart_area
                .is_some_and(|chart| chart.contains(position))
            {
                continue;
            }
            // The mark may belong to any other pane. First owner in address
            // order wins, which is stable frame to frame: a pick that
            // depended on iteration luck would move an object between charts
            // between frames.
            for owner in (0..count).filter(|owner| *owner != viewer) {
                let Some(source) = self.pane_at(owner) else {
                    continue;
                };
                if let Some((index, anchor)) = pane.shared_pick(source, position) {
                    picks.by_pane[viewer] = Some(SharedPick {
                        owner,
                        index,
                        anchor,
                        locked: source.drawings.items()[index].locked,
                    });
                    break;
                }
            }
        }
        picks
    }

    /// Land what each pane did to the other's marks on the store that holds
    /// them.
    ///
    /// The gesture brackets travel with the edits so a whole drag started on
    /// the mirror lands as one undo entry on the owning store — the same
    /// coalescing the object gets on its own chart, because it is the same
    /// gesture on the same object. A selection taken on one pane is dropped on
    /// the other, so the tab never holds two.
    fn apply_shared_interactions(&mut self, edits: &[(PaneIndex, SharedInteraction)]) {
        for (actor, interaction) in edits {
            // No owner means no mark was ever taken hold of, so there is
            // nothing to land. Refused rather than guessed: landing it on a
            // neighbour chosen by arithmetic would move an object the trader
            // drew onto a chart they were not working on.
            let Some(owner) = interaction.owner else {
                continue;
            };
            if self.pane_at(owner).is_none() {
                continue;
            }
            if interaction.begin_gesture {
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .drawings
                    .begin_gesture();
            }
            if let Some(edit) = interaction.edit {
                if matches!(edit, SharedEdit::Select(_))
                    && let Some(pane) = self.pane_at_mut(*actor)
                {
                    // A selection taken on a mirror is dropped on the pane
                    // that took it, so the tab never holds two.
                    pane.drawings.select(None);
                }
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .apply_shared_edit(edit);
            }
            if interaction.commit_gesture {
                self.pane_at_mut(owner)
                    .expect("owner checked above")
                    .drawings
                    .commit_gesture();
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
        let count = self.pane_count();
        if count < 2 {
            return;
        }
        for viewer in 0..count {
            let Some(pane) = self.pane_at(viewer) else {
                continue;
            };
            for owner in (0..count).filter(|owner| *owner != viewer) {
                if let Some(source) = self.pane_at(owner) {
                    pane.paint_shared_from(painter, source);
                }
            }
        }
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

    /// The collapsed context column: a rail with a grip, and the way back.
    ///
    /// A pane dragged to nothing has to leave something behind. Blender's
    /// manual puts the rule plainly — a hidden region leaves a little arrow to
    /// click — and the vertical axis in this app already refuses zero for the
    /// same reason (`indicators::COLLAPSED_PANE_HEIGHT_PX`). Eight pixels of a
    /// 1920 px canvas is four tenths of one percent: near enough to the "size
    /// zero" a trader asks for, and not so near that the chart is gone for
    /// good.
    ///
    /// The paint is 8 px wide; the *hit* area is 24 px, reaching into the
    /// chart beside it where it costs nothing but the pointer's first few
    /// pixels. A rail that photographed well but could not be hit would be a
    /// picture of an affordance rather than one.
    fn draw_collapsed_rail(&mut self, ui: &egui::Ui, rail: egui::Rect) {
        #[cfg(test)]
        {
            self.collapsed_rail = Some(rail);
        }
        let painter = ui.painter();
        painter.rect_filled(rail, egui::Rounding::ZERO, theme::CHROME);
        // The inner edge, so the rail reads as chrome against the chart rather
        // than as a stripe the chart happens to start after.
        painter.line_segment(
            [
                egui::pos2(rail.right(), rail.top()),
                egui::pos2(rail.right(), rail.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );

        let hit = egui::Rect::from_min_max(
            rail.min,
            egui::pos2(
                rail.left() + canvas_layout::COLLAPSED_HIT_PX.max(rail.width()),
                rail.bottom(),
            ),
        );
        let response = ui
            .interact(
                hit,
                egui::Id::new(("collapsed_context_rail", self.id)),
                egui::Sense::click(),
            )
            .on_hover_text("show the timeframe charts again");
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // The grip: a short bar at the rail's middle, in the colour a reader
        // already knows means "chrome you can take hold of".
        let grip_colour = if response.hovered() {
            theme::ACCENT
        } else {
            theme::TEXT_MUTED
        };
        let grip = egui::Rect::from_center_size(
            rail.center(),
            egui::vec2(RAIL_GRIP_WIDTH_PX, RAIL_GRIP_HEIGHT_PX),
        );
        ui.painter()
            .rect_filled(grip, egui::Rounding::same(1.0), grip_colour);

        if response.clicked() {
            self.context_collapsed = false;
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
            // In pixels, because the gesture is in pixels and the floor is
            // too. `split_fraction` carries the *asked-for* width rather than
            // a floored one, so a hand that keeps pushing left keeps
            // travelling: the splitter floors what it draws, and this is what
            // makes "drag past the floor to dismiss it" a gesture a hand can
            // finish rather than one that needs a single impossible frame.
            let wanted_px = self.split_fraction * canvas_width + handle.drag_delta().x;
            if wanted_px < canvas_layout::COLLAPSE_AT_PX {
                // Dismissed, not squeezed. `split_fraction` is left where it
                // was, so the rail springs back to the width the trader chose
                // rather than to a default that would discard it.
                self.context_collapsed = true;
            } else {
                self.context_collapsed = false;
                self.split_fraction = clamp_pane_fraction(wanted_px / canvas_width);
            }
        }
    }
}

#[cfg(test)]
mod shared_routing_tests {
    use super::*;
    use crate::feed;
    use crate::state::BarSpec;
    use tokio::sync::mpsc;

    /// Hands each test tab its own trades directory. A tab opens a
    /// paper-trading ledger, and two tabs pointed at one folder read each
    /// other's trades — which shows up as unrelated ledger tests failing.
    static NEXT_TEST_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// A tab with `context` context panes stacked beside its flow pane.
    fn tab_with_context_panes(context: usize) -> Tab {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let mut tab = Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            // Its own directory, never the shared temp root: a tab opens a
            // paper-trading ledger, and pointing every test tab at one folder
            // makes them read each other's trades.
            std::env::temp_dir().join(format!(
                "quantick-tab-test-{context}-{}",
                NEXT_TEST_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )),
        );
        for slot in 0..context {
            tab.time_panes.push(ChartPane::time(
                100 + slot as u64,
                crate::time_header::DEFAULT_INTERVAL_MS,
            ));
        }
        tab
    }

    /// Collapsing must not spend the width it collapses.
    ///
    /// `split_fraction` is the trader's own sizing and the only thing that can
    /// restore the column. A collapse that overwrote it — with a rail's width,
    /// or with a default — would hand back a different chart from the one that
    /// was put away.
    #[test]
    fn collapsing_a_column_keeps_the_width_it_springs_back_to() {
        let mut tab = tab_with_context_panes(1);
        tab.split_fraction = 0.42;

        tab.context_collapsed = true;
        assert_eq!(
            tab.split_fraction, 0.42,
            "the collapse spent the width it was supposed to remember"
        );

        // And again: a second collapse must not overwrite it either.
        tab.context_collapsed = true;
        assert_eq!(tab.split_fraction, 0.42);

        tab.context_collapsed = false;
        assert_eq!(
            tab.split_fraction, 0.42,
            "the column came back at a width the trader never chose"
        );
    }

    /// The flow pane is address `0` and the context stack follows it, whatever
    /// order they are drawn in. A reader who took this for a left-to-right
    /// order would mirror every edit, so it is pinned.
    #[test]
    fn panes_are_addressed_flow_first_then_the_context_stack() {
        let tab = tab_with_context_panes(2);
        assert_eq!(tab.pane_count(), 3);
        assert_eq!(
            tab.pane_at(0).map(|pane| pane.id),
            Some(tab.flow_pane.id),
            "address 0 is the flow pane"
        );
        assert_eq!(tab.pane_at(1).map(|pane| pane.id), Some(100));
        assert_eq!(tab.pane_at(2).map(|pane| pane.id), Some(101));
        assert!(tab.pane_at(3).is_none(), "there is no fourth pane");
    }

    /// A shared mark belongs to the pane whose store holds it, and an edit
    /// made on a mirror has to land *there* — not on "the other pane".
    ///
    /// With two panes those two phrases mean the same thing, which is why the
    /// routing this replaced (`side.other()`) was correct and why nothing
    /// caught it losing that meaning. With a stack beside the flow pane there
    /// is more than one other pane. The owner named here is address 2, which
    /// is neither the actor nor the actor's single counterpart, so this fails
    /// against the arithmetic it replaced rather than merely passing beside
    /// it.
    #[test]
    fn a_shared_gesture_opens_on_the_pane_the_interaction_names() {
        let mut tab = tab_with_context_panes(2);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: Some(2),
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);

        assert!(
            tab.pane_at(2)
                .expect("the named pane exists")
                .drawings
                .in_gesture(),
            "the gesture must open on the pane the interaction named"
        );
        for bystander in [0usize, 1] {
            assert!(
                !tab.pane_at(bystander)
                    .expect("pane exists")
                    .drawings
                    .in_gesture(),
                "pane {bystander} took a gesture it was never named for"
            );
        }
    }

    /// An interaction with no owner is refused rather than guessed. Landing it
    /// on a neighbour chosen by arithmetic would move an object the trader
    /// drew onto a chart they were not working on.
    #[test]
    fn an_unowned_interaction_lands_nowhere() {
        let mut tab = tab_with_context_panes(2);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: None,
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);
        for pane in 0..tab.pane_count() {
            assert!(
                !tab.pane_at(pane)
                    .expect("pane exists")
                    .drawings
                    .in_gesture(),
                "pane {pane} opened a gesture for an interaction that named no owner"
            );
        }
    }

    /// An owner address past the end of the stack is ignored, not panicked on:
    /// a saved workspace or a control-plane call can name a pane that has
    /// since gone.
    #[test]
    fn an_owner_that_no_longer_exists_is_ignored() {
        let mut tab = tab_with_context_panes(1);
        tab.apply_shared_interactions(&[(
            0,
            SharedInteraction {
                owner: Some(7),
                edit: None,
                begin_gesture: true,
                commit_gesture: false,
            },
        )]);
        for pane in 0..tab.pane_count() {
            assert!(
                !tab.pane_at(pane)
                    .expect("pane exists")
                    .drawings
                    .in_gesture()
            );
        }
    }
}

#[cfg(test)]
mod move_pane_tests {
    use super::*;
    use crate::feed;
    use crate::state::BarSpec;
    use tokio::sync::mpsc;

    static NEXT_DIR: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);

    fn tab_with(context: usize) -> Tab {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let mut tab = Tab::new(
            0,
            0,
            "binance".to_owned(),
            "BTCUSDT".to_owned(),
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(
                    crate::config::ProviderKind::Binance.capabilities(),
                ),
                latency: feed::unsplit_latency(),
                commands: cmd_tx,
                replay: None,
            },
            std::env::temp_dir().join(format!(
                "quantick-move-test-{}",
                NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            )),
        );
        for slot in 0..context {
            tab.time_panes.push(ChartPane::time(
                200 + slot as u64,
                crate::time_header::DEFAULT_INTERVAL_MS,
            ));
        }
        tab
    }

    /// The order the stack draws in is the order it holds, so moving a chart
    /// moves the pane rather than swapping what is inside two of them: the
    /// drawings, indicators and bars travel with the chart the trader moved.
    #[test]
    fn moving_a_chart_carries_the_pane_rather_than_its_contents() {
        let mut tab = tab_with(3);
        let ids: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(ids, vec![200, 201, 202]);

        assert!(
            tab.move_context_pane(3, 1),
            "the bottom chart moves to the top"
        );
        let after: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(
            after,
            vec![202, 200, 201],
            "the pane moved and the others closed up behind it"
        );
    }

    #[test]
    fn moving_a_chart_one_slot_swaps_it_with_its_neighbour() {
        let mut tab = tab_with(2);
        assert!(tab.move_context_pane(1, 2));
        let after: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert_eq!(after, vec![201, 200]);
    }

    /// The flow pane is address `0` and does not move: its column is the one
    /// thing every preset agrees on, and a caller that asked to move the
    /// heatmap meant something this cannot do.
    #[test]
    fn the_flow_pane_refuses_to_move() {
        let mut tab = tab_with(2);
        let before: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        assert!(!tab.move_context_pane(0, 1), "address 0 is the flow pane");
        assert!(!tab.move_context_pane(1, 0), "and it is not a destination");
        assert_eq!(
            tab.time_panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            before,
            "a refused move must leave the stack exactly as it was"
        );
    }

    /// An address past the end is refused rather than clamped. A control-plane
    /// call or a stale menu naming a chart that has gone means something this
    /// cannot do, and moving a different chart would be worse than saying no.
    #[test]
    fn an_address_the_stack_does_not_have_is_refused() {
        let mut tab = tab_with(2);
        let before: Vec<u64> = tab.time_panes.iter().map(|pane| pane.id).collect();
        for (from, to) in [(1_usize, 9_usize), (9, 1), (7, 8)] {
            assert!(
                !tab.move_context_pane(from, to),
                "moving {from} to {to} names a chart that is not there"
            );
        }
        assert_eq!(
            tab.time_panes
                .iter()
                .map(|pane| pane.id)
                .collect::<Vec<_>>(),
            before
        );
    }

    /// Moving a chart onto itself changes nothing, and says so. A caller that
    /// retried a dropped call needs "nothing happened" to be distinguishable
    /// from "it worked".
    #[test]
    fn moving_a_chart_onto_itself_reports_no_change() {
        let mut tab = tab_with(2);
        assert!(!tab.move_context_pane(1, 1));
        assert!(!tab.move_context_pane(2, 2));
    }

    /// A single context chart has nowhere to go.
    #[test]
    fn a_lone_context_chart_cannot_be_reordered() {
        let mut tab = tab_with(1);
        assert!(!tab.move_context_pane(1, 1));
        assert!(!tab.move_context_pane(1, 2));
        assert_eq!(tab.time_panes.len(), 1);
    }
}
