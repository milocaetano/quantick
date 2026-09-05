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

use crate::canvas_layout::{
    self, LayoutPreset, MAX_CANVAS_PANES, MAX_CONTEXT_PANES, PaneIdAllocator, PaneKind,
};
use crate::chart_layers::{ChartLayer, LayerBlock};
use crate::config::{AppConfig, FeedCapabilities};
use crate::loading::{LoadingTask, LoadingTracker};
use crate::metrics;
use crate::orderflow_view::OrderflowView;
use crate::pane::{
    ChartPane, DEFAULT_PANE_FRACTION, DrawingDrag, PaneIndex, PaneSide, SharedPick,
    clamp_pane_fraction,
};
use crate::paper_trading::PaperTrading;
use crate::state::BarSpec;
use crate::style::ChartStyle;
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
mod strategies;

pub use canvas::CanvasChrome;
pub use history::OlderCandles;

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
    /// Put the context column away, or bring it back.
    ///
    /// **The one collapse path.** The divider drag, the rail, the View menu,
    /// `Ctrl+0` and `layout.pane.collapse` all arrive here. Before this
    /// existed the only way a *trader* could collapse the column was a mouse
    /// drag, while an assistant had a named call for it — the second operator
    /// holding a capability the first could not reach from the keyboard,
    /// which is the rule inverted.
    ///
    /// `split_fraction` is deliberately untouched: it is the width the column
    /// springs back to, and spending it here would hand back a different chart
    /// from the one that was put away.
    ///
    /// Returns whether anything changed.
    pub fn set_context_collapsed(&mut self, collapsed: bool) -> bool {
        if self.context_collapsed == collapsed {
            return false;
        }
        self.context_collapsed = collapsed;
        true
    }

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
        self.focus = PaneSide::Time(to_slot);
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
            notice_since_ms: metrics::wall_clock_ms(),
            feed_attached_ms: metrics::wall_clock_ms(),
            connection_since_ms: metrics::wall_clock_ms(),
            resume_floor_ms: None,
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

    /// The canvas width the last drawn frame used, for callers that have to
    /// reason in pixels without a frame in hand — the control plane's resize,
    /// which must honour the same floor a drag does.
    ///
    /// Zero before the first frame, which callers read as "no opinion" rather
    /// than as a canvas of no width.
    #[must_use]
    pub fn last_canvas_width(&self) -> f32 {
        self.last_canvas_width
    }

    /// Whether any context chart is actually on screen.
    ///
    /// The layout has to *hold* one, the tab has to have *built* one, and the
    /// column must not be collapsed. Three conditions that chrome kept
    /// re-deriving one variant at a time — and got wrong twice: the legend
    /// gate matched `TimeAndFlow` alone, so the stacked charts drew no legend
    /// and a collapsed column drew one over the flow chart against a stale
    /// rect.
    #[must_use]
    pub fn shows_context_charts(&self) -> bool {
        self.layout.shows_time() && self.has_time_pane() && !self.context_collapsed
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
        // A collapsed column is not on screen, and focus on a pane nobody can
        // see is worse than useless: `paper_hud_here` goes false for the one
        // pane that *is* drawn, so order entry, the ladder and the trade HUD
        // all go dead on the heatmap until the trader expands again.
        if !self.has_time_pane() || !self.layout.shows_time() {
            return PaneSide::Flow;
        }
        if !self.layout.shows_flow() {
            // A layout with no flow pane has nothing to fall back *to*: the
            // context chart is the only pane drawn, and the collapse flag
            // names a column this layout does not carve. Read before the flag,
            // or `Ctrl+0` on the Timeframe layout sent focus to a pane nobody
            // draws — order entry, the ladder and the trade HUD all dead on
            // the one chart on screen, with no rail to click to undo it.
            return PaneSide::Time(0);
        }
        if self.context_collapsed {
            return PaneSide::Flow;
        }
        // A slot the stack no longer shows — the focused pane was the bottom
        // of a three-pane layout and the trader switched to two — falls back
        // to the top context chart, never to a pane that is not drawn.
        match self.focus {
            PaneSide::Time(slot) if slot >= self.context_panes_shown() => PaneSide::Time(0),
            focus => focus,
        }
    }

    /// How many context panes the layout draws — bounded by how many exist,
    /// for the frame between asking for a layout and its panes being built.
    pub fn context_panes_shown(&self) -> usize {
        self.layout
            .kinds()
            .iter()
            .filter(|kind| **kind == PaneKind::Time)
            .count()
            .min(self.time_panes.len())
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    pub fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get(slot).unwrap_or(&self.flow_pane),
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
            | ChartLayer::PointerPrice
            | ChartLayer::PointerTime
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
        for (slot, time) in self.time_panes.iter_mut().enumerate() {
            time.content_editing = target
                .filter(|(side, _)| *side == PaneSide::Time(slot))
                .map(|(_, index)| index);
        }
    }

    pub fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time(slot) => self.time_panes.get_mut(slot).unwrap_or(&mut self.flow_pane),
            PaneSide::Flow => &mut self.flow_pane,
        }
    }

    /// Every side this tab can address, flow first — the order
    /// [`Self::panes`] walks. The one place "which panes exist" is spelled
    /// out for the chrome, so a surface that asks each pane a question walks
    /// the stack rather than the two sides the split used to have.
    pub fn sides(&self) -> impl Iterator<Item = PaneSide> + '_ {
        (0..self.pane_count()).map(PaneSide::from_index)
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
        self.sides()
            .find(|side| *side != focused && self.pane(*side).drawings.selected().is_some())
            .unwrap_or(focused)
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
        std::iter::once((&self.flow_pane, PaneSide::Flow)).chain(
            self.time_panes
                .iter()
                .enumerate()
                .map(|(slot, time)| (time, PaneSide::Time(slot))),
        )
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    pub fn panes_with_sides_mut(&mut self) -> impl Iterator<Item = (&mut ChartPane, PaneSide)> {
        let Self {
            flow_pane,
            time_panes,
            ..
        } = self;
        std::iter::once((flow_pane, PaneSide::Flow)).chain(
            time_panes
                .iter_mut()
                .enumerate()
                .map(|(slot, pane)| (pane, PaneSide::Time(slot))),
        )
    }

    /// See [`Self::panes_with_sides_mut`], without the addresses.
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
        // A layout chosen from the picker shows the panes its thumbnail drew.
        // Leaving the column collapsed meant the cell lit, two panes were
        // promised, and an 8 px rail arrived instead. Before the early return,
        // because picking the arrangement that is *already* selected is
        // exactly how a trader asks for the charts they can see promised in a
        // lit cell — and a return above this line answered that with the rail.
        self.context_collapsed = false;
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
            CanvasLayout::Time => PaneSide::Time(0),
            // The split reveals whichever pane the previous layout was not
            // showing: the time pane coming from Single, the flow pane coming
            // from Time.
            CanvasLayout::TimeAndFlow | CanvasLayout::TimeTimeAndFlow => match previous {
                CanvasLayout::Single => PaneSide::Time(0),
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
        // The slot this pane will take is the next free one in the stack.
        let interval_ms = self
            .context_opening_intervals_ms
            .get(self.time_panes.len())
            .copied()
            .unwrap_or(self.time_pane_opening_interval_ms);
        let mut pane = ChartPane::time(ids.alloc(), interval_ms);
        pane.legend_collapsed = self.time_pane_opening_legend_collapsed;
        pane.layout = self
            .context_opening_layouts
            .get(self.time_panes.len())
            .copied()
            .flatten()
            .map(crate::layouts::LayoutId);
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

    /// The layout each pane opens on, from a saved workspace: the flow pane's
    /// now, each context pane's when it is built.
    ///
    /// `context` is the wire form — [`crate::ui_state::LAYOUT_UNRECORDED`]
    /// for a slot the file did not state — and it is read back into an
    /// `Option` here, at the one place the file's shape meets the pane's.
    ///
    /// **Ordering.** This names what a pane *will* show; it does not move a
    /// pane that is already showing something. A caller that reaches a tab
    /// whose stack is standing must follow with
    /// `QuantickApp::reload_layouts`, which clears every pane's set and seeds
    /// it again from the layout named here — otherwise a seeded pane keeps
    /// the old layout's indicators, drawings and header label under the new
    /// layout's id, and the next edit is written into the wrong entries.
    /// Both callers do (`restore_workspace` and the bundle import, each
    /// through `reload_cockpit_stores`).
    pub fn set_opening_layouts(&mut self, flow: Option<u64>, context: &[u64]) {
        self.flow_pane.layout = flow.map(crate::layouts::LayoutId);
        self.context_opening_layouts = context
            .iter()
            .map(|id| (*id != crate::ui_state::LAYOUT_UNRECORDED).then_some(*id))
            .take(MAX_CONTEXT_PANES)
            .collect();
        // A context pane already built takes its name now: an import lands on
        // a tab whose stack is standing, and the stash alone would only reach
        // the panes still to come.
        for (slot, pane) in self.time_panes.iter_mut().enumerate() {
            if let Some(id) = context
                .get(slot)
                .copied()
                .filter(|id| *id != crate::ui_state::LAYOUT_UNRECORDED)
            {
                pane.layout = Some(crate::layouts::LayoutId(id));
            }
        }
    }

    /// Name the layout a context pane will open on when the stack builds it.
    ///
    /// Only for a pane that is not standing yet — a canvas change lands its
    /// panes a frame after the layout that asked for them. A pane already on
    /// the canvas is moved by `QuantickApp::switch_pane_layout`, which
    /// carries its indicators and its drawings across too; writing
    /// `pane.layout` under a standing pane would leave the field disagreeing
    /// with what that chart is showing.
    pub fn set_opening_layout(&mut self, side: PaneSide, id: crate::layouts::LayoutId) {
        // The flow pane is built with the tab and is never pending, so the
        // only address that can be waiting is a context slot.
        let PaneSide::Time(slot) = side else {
            debug_assert!(false, "the flow pane is never waiting to be built");
            return;
        };
        if slot >= MAX_CONTEXT_PANES {
            return;
        }
        if self.context_opening_layouts.len() <= slot {
            self.context_opening_layouts.resize(slot + 1, None);
        }
        self.context_opening_layouts[slot] = Some(id.0);
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
        context_intervals_ms: &[i64],
        legends: LegendFold,
    ) {
        // The top chart's interval is also the one every slot past the list
        // opens on, which is what a one-chart file has always meant.
        if let Some(ms) = context_intervals_ms.first() {
            self.time_pane_opening_interval_ms = *ms;
        }
        self.context_opening_intervals_ms = context_intervals_ms
            .iter()
            .copied()
            .take(MAX_CONTEXT_PANES)
            .collect();
        self.set_layout(layout);
        // *After* `set_layout`, for the reason the focus below is: a switch
        // opens the column it just revealed, which is right for a menu click
        // and wrong for a restore, where the saved state is the answer.
        // Assigned before, a workspace saved with its charts put away reopened
        // with them out — and the next `capture_arrangement` wrote that over
        // the trader's choice.
        self.context_collapsed = context_collapsed;
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
                    let _ = self.paper.account_mut().apply_strategy_command(command);
                }
                self.drop_overlay_gestures();
            }
        }
    }
}

crate::hooks::declare_hooks!["QUANTICK_PANE_COLLAPSED"];

#[cfg(test)]
mod tests;
