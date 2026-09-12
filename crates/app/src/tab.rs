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

/// Only the `#[cfg(test)]` geometry fields below name a `Rect`; the canvas
/// painter that used to need this here now lives in [`canvas`].
#[cfg(test)]
use eframe::egui;
use smallvec::SmallVec;
use tokio::sync::{mpsc, watch};

use quantick_feed_binance::depth::DepthEvent;

/// Only the tests below allocate pane ids directly; the panes a layout
/// change adds now take theirs in [`layout`].
#[cfg(test)]
use crate::canvas_layout::PaneIdAllocator;
use crate::canvas_layout::{self, LayoutPreset, MAX_CANVAS_PANES, MAX_CONTEXT_PANES, PaneKind};
use crate::config::{AppConfig, FeedCapabilities};
use crate::loading::{LoadingTask, LoadingTracker};
use crate::metrics;
use crate::pane::{ChartPane, DEFAULT_PANE_FRACTION, DrawingDrag, PaneIndex, PaneSide, SharedPick};
use crate::paper_trading::PaperTrading;
use crate::state::BarSpec;
use quantick_feed::history_reach::{self, Campaign, HistoryReach};
use quantick_feed::stall::{self};
use quantick_feed::{
    FeedCommand, FeedConnectionState, FeedEvent, FeedGap, FeedHandle, FeedLatency, FeedNotice,
    ReplayLink,
};
use std::path::PathBuf;

mod canvas;
mod feed;
mod history;
mod layout;
mod panes;
mod strategies;

pub use canvas::CanvasChrome;
pub use history::OlderCandles;

/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
pub const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
/// The live envelope owns the figure, because the book queue is sized from it.
const BOOK_DRAIN_BUDGET: usize = crate::live_envelope::BURST_DEPTH_UPDATES_PER_FRAME;
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

/// How long the outcome of a *load older* press stays on screen.
///
/// Long enough to read one short line without hunting for it, short enough
/// that it is gone before the trader's next decision. It leaves on its own
/// because it is a remark and not a fault: nothing here needs acknowledging,
/// and a card waiting to be dismissed over a live chart interrupts more than
/// the silence it replaced.
pub const HISTORY_NOTE_LINGER: std::time::Duration = std::time::Duration::from_secs(6);

/// What the last *load older* press had to say, and when it said it.
///
/// Raised only when the press left the chart where it was, or stopped short of
/// the reach it promised. A press that landed what it promised raises nothing:
/// the bars are the acknowledgement, and a sentence after every success is
/// noise a trader learns to stop reading.
#[derive(Debug, Clone, Copy)]
struct HistoryNote {
    /// Borrowed, never owned: every sentence is a fixed one belonging to
    /// [`quantick_feed::history_reach`], so the outcome and the run that produced it
    /// cannot drift into two different accounts of the same press.
    text: &'static str,
    raised_at: std::time::Instant,
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
    /// Wall clock when [`Self::notice`] last changed.
    ///
    /// A feed cannot report that nothing is happening, because nothing
    /// happening produces no event; the only way to tell a step in progress
    /// from a step that has stopped progressing is how long it has stood.
    /// Read by [`Self::stall_at`].
    pub notice_since_ms: i64,
    /// Wall clock when the running feed session was attached — the floor for
    /// judging a first connection that never lands.
    pub feed_attached_ms: i64,
    /// Wall clock when [`Self::feed_connection`] last changed.
    ///
    /// The reconnect budget is measured from here rather than from the notice,
    /// because a supervisor that alternates two lines — `Lost`, then `Waiting`,
    /// then `Lost` — changes the notice every few seconds while the transport
    /// stays exactly as broken as it was. Anchored on the notice, that pair
    /// re-stamped the clock forever and the budget never ran out, which is the
    /// failure this whole module exists to end.
    pub connection_since_ms: i64,
    /// After a reconnect that kept the timeline, the market time the chart had
    /// already reached.
    ///
    /// Every print at or before it belongs to the window the new session
    /// replays and is dropped rather than counted twice; the first print past
    /// it clears the floor and decides whether a gap has to be marked. `None`
    /// on a session that started from an empty chart, which has nothing to
    /// overlap with. See [`quantick_feed::past_resume_floor`].
    pub resume_floor_ms: Option<i64>,
    /// How many feed sessions this tab has taken over since it opened: one
    /// more on every attach — a reconnect, a reload, a market switch, a replay
    /// opened or closed. Read by `feed.status` so a client that lost the
    /// answer to `feed.reconnect` or `feed.reload` can see whether the tab
    /// really took a new session, which no other field says.
    feed_generation: u64,
    /// A stall forced by `QUANTICK_FEED_STALL`, for a scripted run that has to
    /// photograph the recovery controls without breaking a real feed.
    ///
    /// Overrides the judgement when set, and set only from the hook — a live
    /// session leaves it `None` and reads the real one, exactly as
    /// [`Self::forced_latency`] does.
    pub forced_stall: Option<stall::ForcedStall>,
    /// A silence asked for by `QUANTICK_FEED_GAP`, still waiting for bars to
    /// land it between. Taken once the chart has some, and cleared, so the
    /// hook marks one gap rather than one per frame.
    pub pending_demo_gap_ms: Option<i64>,
    /// Silences in this tab's tape that no print covers, in market time,
    /// oldest first and bounded by [`MAX_REMEMBERED_GAPS`].
    ///
    /// Written only by a reconnect that kept the timeline. A reload has no
    /// gaps to record: it throws the timeline away, so there is no seam.
    pub feed_gaps: Vec<FeedGap>,
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
    /// How far one press of "load older" reaches: one page, or back past the
    /// market's last close with a lead into the session before it.
    ///
    /// A tab-level copy of the window's standing choice, pushed on change the
    /// way `progressive_history` is — the reach is a habit, not a per-market
    /// setting, and a trader who picked it once must not have to pick it again
    /// in the next tab.
    pub history_reach: HistoryReach,
    /// Minutes of traded time one press of [`HistoryReach::Span`] pulls,
    /// mirrored from the window so every tab presses the way the trader said.
    pub history_reach_span_minutes: u32,
    /// Slices of the opening session still to arrive, while one is filling in
    /// behind the chart. `None` when nothing is filling.
    ///
    /// Cleared by every way a fill can *end*, not only by the last slice
    /// saying zero. A bridge that dies mid-fill sends no final slice, and a
    /// count frozen at twelve would go on telling an operator the chart was
    /// still arriving for the life of the tab — which is the one question this
    /// field exists to answer.
    opening_slices_remaining: Option<u64>,
    /// The run of requests a reach beyond one page started, or `None` when
    /// nothing is paging.
    ///
    /// One per tab, because the transports serve one request at a time: the
    /// reply is what sends the next request, so this is a state machine and
    /// never a loop. See [`quantick_feed::history_reach`].
    campaign: Option<Campaign>,
    /// What the last *load older* press had to say, while it is still on
    /// screen. See [`HistoryNote`].
    history_note: Option<HistoryNote>,
    /// Whether a pane that is *not* cut by time may carry the venue's own
    /// candles in front of its bars.
    ///
    /// Off by default, so a tick chart opens exactly as it always has: with
    /// the prints this session saw and nothing invented in front of them. On,
    /// the venue's 1-minute candles are installed unfolded — real candles,
    /// counted apart from built bars on the status bar — which is the only way
    /// a chart cut by trades can show yesterday at all. A minute is not a tick
    /// bar and never becomes one; the two simply sit side by side, named.
    ///
    /// **What the trader is accepting by switching it on**, said here because
    /// it is the honest cost and the View menu's hover says it too: the series
    /// an indicator is computed over is the prefix plus the pane's own bars
    /// (`ChartPane::closed_bars`). On a time pane the prefix is folded to that
    /// pane's interval, so the series stays one population; here it does not,
    /// and an average running across the seam averages minute candles with
    /// bars cut by trades. The alternative — hiding the prefix from the
    /// indicators of a chart cut by trades and not of one cut by time — would
    /// make the same prefix mean two things, which is the worse dishonesty.
    pub venue_lead_in: bool,
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

    /// Alarm sounds this tab's armed instances asked for, oldest first,
    /// drained by the app once per frame.
    ///
    /// A queue rather than a direct call to the platform: the judgement is
    /// made deep in the per-trade sweep, where a tab must stay testable
    /// without a build machine making noise, and where blocking on an
    /// operating-system call would be on the tape's path. The app plays
    /// them through the [`AlertSink`](crate::audio::AlertSink) port.
    pub pending_alarm_sounds: Vec<crate::audio::Cue>,

    /// Paper trading for this market: the deterministic simulator plus its
    /// journal, chart layer, dock tab and report.
    ///
    /// Per tab, because a simulated position belongs to a tape. Two tabs on
    /// two markets hold two independent positions, and a position can never
    /// be marked against prints it was not opened against — the invariant
    /// Which pane the order-entry gesture belonged to last frame.
    ///
    /// Only consulted while a paper drag is in progress: the pointer may
    /// wander out of the pane that owns the grabbed line — or into another
    /// one — and the price under the drag must keep being read against the
    /// scale the gesture started on. Between gestures the pointer decides
    /// afresh every frame, so this never pins anything.
    paper_drag_pane: Option<PaneIndex>,

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
    /// The interval each context pane opens on, by slot, when a restored
    /// workspace recorded more than one. A slot past the end of this list
    /// opens on `time_pane_opening_interval_ms`, which is what every tab did
    /// while the stack held one chart.
    context_opening_intervals_ms: SmallVec<[i64; MAX_CONTEXT_PANES]>,
    /// The layout each context pane opens on, by slot, from a restored
    /// workspace. A slot past the end takes what a fresh pane takes: the
    /// focused pane's layout.
    context_opening_layouts: SmallVec<[Option<u64>; MAX_CONTEXT_PANES]>,
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
    /// The canvas width the last drawn frame used. See
    /// [`Self::last_canvas_width`].
    last_canvas_width: f32,
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
            notice_since_ms: metrics::wall_clock_ms(),
            feed_attached_ms: metrics::wall_clock_ms(),
            connection_since_ms: metrics::wall_clock_ms(),
            resume_floor_ms: None,
            feed_generation: 0,
            forced_stall: stall::ForcedStall::from_env(),
            pending_demo_gap_ms: quantick_feed::demo_gap_ms(),
            feed_gaps: Vec::new(),
            feed_connection: FeedConnectionState::Connecting,
            feed_capabilities: feed.capabilities,
            feed_latency: feed.latency,
            forced_latency: quantick_feed::forced_latency_split(),
            commands: feed.commands,
            replay: feed.replay,
            history_step: 2000,
            history_trades: 0,
            history_reach: HistoryReach::default(),
            opening_slices_remaining: None,
            // Overwritten by `drain_tabs` on the first frame from the
            // window's own value; this is only what a tab holds before that.
            history_reach_span_minutes: (history_reach::DEFAULT_REACH_SPAN_MS / 60_000) as u32,
            campaign: None,
            history_note: None,
            venue_lead_in: false,
            loading,
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            latest_trade_latency_ms: None,
            latest_trade_ms: None,
            live_trades: 0,
            pending_alarm_sounds: Vec::new(),
            paper: PaperTrading::with_trades_dir(trades_dir),
            paper_drag_pane: None,
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
            context_opening_intervals_ms: SmallVec::new(),
            context_opening_layouts: SmallVec::new(),
            time_pane_opening_legend_collapsed: false,
            pending_context_panes: 0,
            layout: CanvasLayout::Single,
            split_fraction: DEFAULT_PANE_FRACTION,
            context_collapsed: std::env::var("QUANTICK_PANE_COLLAPSED")
                .is_ok_and(|value| value == "1"),
            last_canvas_width: 0.0,
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
        self.attach_with(handle, false);
    }

    /// Take over a freshly spawned feed for the *same* market, keeping every
    /// record built from the old session: candles, history prefixes and how far
    /// back this tab has already reached.
    ///
    /// This is the half of a reconnect that makes it worth having beside a
    /// reload. Switching markets must forget all of it — the old market's
    /// candles describe the old market — but a socket that dropped and came
    /// back is still the same instrument, and refetching a week of history
    /// because a bridge hiccuped is the wait the trader was trying to escape.
    fn attach_resuming(&mut self, handle: FeedHandle) {
        self.attach_with(handle, true);
    }

    /// The shared body. `keep_timeline` decides only what is *forgotten*;
    /// everything tied to the handle itself is replaced either way, because an
    /// in-flight reply belongs to a channel that is about to be dropped.
    fn attach_with(&mut self, handle: FeedHandle, keep_timeline: bool) {
        if !keep_timeline {
            // The old market's candles describe the old market.
            self.ohlcv_base = None;
            // A different market has a different record. Whatever this tab
            // learned about how far back the last one reached says nothing
            // here.
            self.ohlcv_reaching_back = None;
            self.ohlcv_older_exhausted = false;
            self.ohlcv_capable = false;
            for pane in self.panes_mut() {
                pane.install_history_prefix(Vec::new());
            }
            // A seam and a resume floor both belong to the timeline being
            // thrown away with them. Left standing across a market switch, the
            // floor filters the *new* market's prints against the old one's
            // clock and writes a fabricated gap on a chart that never
            // reconnected.
            self.feed_gaps.clear();
            self.resume_floor_ms = None;
        } else {
            // A kept timeline needs no refill, so nothing restarts the history
            // wait — but a request the old session never answered would spin
            // its spinner for the rest of the session. Every outstanding one
            // belonged to the channel about to be dropped.
            self.loading.set_active(LoadingTask::History, false);
        }
        // Any reply still in flight belongs to a channel that is about to be
        // dropped, so the wait restarts rather than draining to zero on an
        // answer that never comes.
        self.ohlcv_pending = false;
        // The channel carrying any in-flight slices is dropped with the old
        // handle, so nothing survives to be dropped as stale.
        self.ohlcv_stale = false;
        // The run belonged to the old session's tape; its reply is on a channel
        // about to be dropped, and whatever it had to say was about a record
        // this tab no longer shows.
        self.abandon_history_run();
        self.loading.set_active(LoadingTask::VenueHistory, false);
        self.feed_generation = self.feed_generation.saturating_add(1);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.feed_latency = handle.latency;
        self.notice = FeedNotice::Clear;
        self.notice_since_ms = metrics::wall_clock_ms();
        self.feed_attached_ms = self.notice_since_ms;
        self.connection_since_ms = self.notice_since_ms;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        // The journal records where a session's trades came from; the
        // attached handle is the single truth for that.
        self.paper
            .account_mut()
            .set_session_source(if self.replay.is_some() {
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
            pane.gestures.hover = None;
            pane.gestures.press_position = None;
            pane.gestures.press_started_empty = false;
            pane.gestures.drag = DrawingDrag::None;
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

    /// How many feed sessions this tab has taken over; see the field.
    pub fn feed_generation(&self) -> u64 {
        self.feed_generation
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
}

crate::hooks::declare_hooks!["QUANTICK_PANE_COLLAPSED"];

#[cfg(test)]
mod tests;
