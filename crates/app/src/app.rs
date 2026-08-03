//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.
//!
//! What is on the canvas lives one layer down, in [`crate::pane`]: the bar
//! series, the viewport, the drawings and the indicator slots belong to a
//! [`ChartPane`], not to the window. The market feeding that pane — channels,
//! connection state, notices, history counters — is still owned here.

use std::time::{Duration, Instant};

use eframe::egui;
use tokio::sync::{mpsc, watch};

use quantick_feed_binance::depth::DepthEvent;

use crate::candle_view::draw_style_window;
use crate::chart::PriceScale;
use crate::config::{AppConfig, FeedCapabilities};
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{
    self, DeleteOutcome, MAX_DRAWING_FILL_ALPHA, MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX,
};
use crate::feed::{
    self, FeedCommand, FeedConnectionState, FeedEvent, FeedHandle, FeedNotice, ReplayLink,
};
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::library::ScriptLibrary;
use crate::indicators::state_file::{self, SavedIndicator, SavedInput, SavedKind};
use crate::loading::{self, LoadingTask, LoadingTracker};
use crate::metrics::{self, FrameStats};
use crate::notice_card;
use crate::orderflow_view::OrderflowView;
use crate::pane::{self, ChartPane, DRAWING_ANCHOR_RADIUS_PX, DrawingDrag, PaneChrome};
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::{BarKind, BarSpec};
use crate::statusbar;
use crate::style::{CandlePreset, ChartStyle};
use crate::theme;
use crate::time_header;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail};

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;
/// Id of the pane the chart opens with. The ids are what keep two panes'
/// gestures apart (see [`crate::pane`]); the tab strip will add more again.
const FLOW_PANE_ID: u64 = 0;
/// See [`FLOW_PANE_ID`].
const TIME_PANE_ID: u64 = 1;
/// Width of the draggable divider between the two panes, in pixels.
const CANVAS_DIVIDER_PX: f32 = 4.0;
/// Half-width of the divider's grab area, which reaches a little into both
/// panes so the handle is catchable without widening the rule itself.
const CANVAS_DIVIDER_HANDLE_PX: f32 = 5.0;
/// Neither pane may be squeezed below this share of the canvas (§11).
const MIN_PANE_FRACTION: f32 = 0.25;
/// Where the divider sits when the split is first shown (§11).
const DEFAULT_PANE_FRACTION: f32 = 0.5;
/// Thickness of the rule marking the focused pane (§11: an accent under the
/// pane's top edge, never a box drawn around market data).
const FOCUS_RULE_PX: f32 = 1.0;
/// Initial position of the selected-drawing inspector.
const DRAWING_INSPECTOR_DEFAULT_POSITION: egui::Pos2 = egui::pos2(90.0, 120.0);
/// Length of the EMA the toolbar's hardcoded M1 entry adds (the settings UI
/// generated from `InputSpec` replaces this in M4).
const DEFAULT_EMA_LEN: usize = 9;
/// How often the hot-reload poll checks script files for changes.
const SCRIPT_RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(1_000);
/// How long after the last indicator change the state file is written.
const INDICATOR_STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(1_000);
/// Inspector width bounds (UX spec: resizable between 300 and 440 px).
const INSPECTOR_MIN_WIDTH_PX: f32 = 300.0;
/// See [`INSPECTOR_MIN_WIDTH_PX`].
const INSPECTOR_MAX_WIDTH_PX: f32 = 440.0;
/// Default inspector width for the shipped tools (the spec reserves 360 px
/// for the Fib level editor).
const INSPECTOR_DEFAULT_WIDTH_PX: f32 = 320.0;
/// Default inspector width for tools that mount a level editor tab.
const INSPECTOR_LEVELS_WIDTH_PX: f32 = 360.0;
/// Gap kept between the inspector and the selected object's bounding box.
const INSPECTOR_OBJECT_GAP_PX: f32 = 12.0;
/// Assumed inspector height for placement before its first frame reports one.
const INSPECTOR_FALLBACK_HEIGHT_PX: f32 = 280.0;
/// Dragging the price field across the whole visible range takes this many
/// steps, whatever the symbol's price magnitude.
const PRICE_DRAG_STEPS: f64 = 200.0;
/// DragValue speed of bar-index coordinates, in bars per drag point.
const BAR_DRAG_SPEED: f64 = 0.25;
/// Where the object manager first opens: under the toolbox's home corner.
const DRAWING_MANAGER_DEFAULT_POSITION: egui::Pos2 = egui::pos2(70.0, 140.0);
/// How long the delete toast keeps its Undo affordance on screen (UX spec).
const TOAST_UNDO_MS: u64 = 8_000;
/// Horizontal offset of a duplicated drawing, so the copy is visibly a copy.
const DUPLICATE_OFFSET_BARS: f32 = 2.0;
/// Vertical clearance between the toast and the bottom chrome.
const TOAST_BOTTOM_MARGIN_PX: f32 = 44.0;

/// Transient confirmation of a destructive drawing command, with its escape
/// hatch. Undo works from the button for [`TOAST_UNDO_MS`] and from Ctrl+Z
/// for as long as the history holds.
#[derive(Debug)]
struct DrawingToast {
    message: &'static str,
    shown_at: Instant,
    /// Whether the toast offers Undo. A delete does; the honest clear after
    /// a bar rebuild does not — its history is gone with the drawings, and
    /// a dead Undo button would lie.
    offers_undo: bool,
}

/// Which inspector tab is open. Tabs exist per capability: every tool gets
/// Style and Coordinates; a tool that brings its own tab (the Fib level
/// editor) mounts it as Extra without the central code knowing its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum InspectorTab {
    #[default]
    Style,
    Extra,
    Coordinates,
}

/// What the inspector body asked for this frame. The caller owns every
/// mutation, so the pinned panel and the floating window share one rule set.
#[derive(Debug, Default, Clone, Copy)]
struct InspectorActions {
    toggle_hidden: bool,
    toggle_lock: bool,
    toggle_pin: bool,
    delete: bool,
    cancel_delete: bool,
    force_delete: bool,
    close: bool,
    edited: bool,
}

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);
/// Coalesce slider drags into one diagnostic event after the value settles.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);
/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
const BOOK_DRAIN_BUDGET: usize = 2_048;

/// Split the padded plot area into the candle chart, the indicator panes, the
/// optional live strip, the right price gutter and the bottom time strip, so
/// the input handler and the renderer agree on the boundaries.
/// `live_strip_width` of zero means the strip is off and the chart runs
/// straight into the gutter, exactly as it did before the strip existed.
///
/// `pane_count` is the number of *visible* pane indicators: the band they
/// claim is carved here, once, rather than by each caller — a chart rect that
/// two call sites disagree about is two price scales for the same pixels.
pub fn plot_split(area: egui::Rect, live_strip_width: f32, pane_count: usize) -> PlotAreas {
    let plot = area.shrink(16.0);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    let body = egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y));
    let (chart, indicator_panes) = crate::indicators::split_panes(body, pane_count);
    PlotAreas {
        chart,
        indicator_panes,
        live_strip: (strip_width > 0.0).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(split_x, plot.top()),
                egui::pos2(gutter_x, split_y),
            )
        }),
        price_gutter: egui::Rect::from_min_max(
            egui::pos2(gutter_x, plot.top()),
            egui::pos2(plot.right(), split_y),
        ),
        time_strip: egui::Rect::from_min_max(
            egui::pos2(plot.left(), split_y),
            egui::pos2(split_x, plot.bottom()),
        ),
    }
}

/// Split the canvas for the Time + Flow layout: **time pane left, flow pane
/// right**, with the divider's own strip between them (§11).
///
/// `time_fraction` is the time pane's share of the width, clamped so neither
/// pane can be squeezed below [`MIN_PANE_FRACTION`] — a pane too narrow to
/// read is not a layout, it is a lost pane.
pub fn split_canvas(area: egui::Rect, time_fraction: f32) -> (egui::Rect, egui::Rect, egui::Rect) {
    let fraction = clamp_pane_fraction(time_fraction);
    let divider_x = area.left() + area.width() * fraction;
    let half = CANVAS_DIVIDER_PX / 2.0;
    (
        egui::Rect::from_min_max(area.min, egui::pos2(divider_x - half, area.bottom())),
        egui::Rect::from_min_max(
            egui::pos2(divider_x - half, area.top()),
            egui::pos2(divider_x + half, area.bottom()),
        ),
        egui::Rect::from_min_max(egui::pos2(divider_x + half, area.top()), area.max),
    )
}

/// Hold a canvas split inside the 25% minimum each pane is promised (§11).
pub fn clamp_pane_fraction(fraction: f32) -> f32 {
    fraction.clamp(MIN_PANE_FRACTION, 1.0 - MIN_PANE_FRACTION)
}

/// Carve the time pane's header strip off the top of its area (§11); the rest
/// is the chart. The header is a strip rather than an overlay so the selector
/// is never painted across market data.
pub fn split_time_pane(area: egui::Rect) -> (egui::Rect, egui::Rect) {
    let split_y = (area.top() + time_header::HEIGHT_PX).min(area.bottom());
    (
        egui::Rect::from_min_max(area.min, egui::pos2(area.right(), split_y)),
        egui::Rect::from_min_max(egui::pos2(area.left(), split_y), area.max),
    )
}

/// Whether a pointer at `x` is over the live lane rather than the candles.
///
/// The divider itself counts as the lane, so the gesture that resizes it and
/// the gesture that pans the candles can never both fire on the same pixel.
/// Without a lane every pixel belongs to the candles, exactly as before.
pub fn gesture_hits_lane(divider_x: Option<f32>, x: f32) -> bool {
    divider_x.is_some_and(|divider| x >= divider)
}

/// Split the bottom time strip at the lane's divider: the candles' own time
/// axis on the left, the lane's on the right.
///
/// Each pane zooms from the strip under it, which is the only place a zoom
/// gesture can say *which* time axis it means. Without a divider the whole
/// strip belongs to the candles, exactly as it did before the lane had a zoom.
pub fn split_time_strip(
    strip: egui::Rect,
    divider_x: Option<f32>,
) -> (egui::Rect, Option<egui::Rect>) {
    let Some(divider) = divider_x.filter(|x| strip.x_range().contains(*x)) else {
        return (strip, None);
    };
    (
        egui::Rect::from_min_max(strip.min, egui::pos2(divider, strip.bottom())),
        Some(egui::Rect::from_min_max(
            egui::pos2(divider, strip.top()),
            strip.max,
        )),
    )
}

/// How many charts the canvas shows for one market (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasLayout {
    /// The flow pane alone — quantick's default and its identity.
    #[default]
    Single,
    /// Time pane left, flow pane right, on a draggable divider.
    TimeAndFlow,
}

/// Which of the canvas's panes something belongs to.
///
/// Named for where they sit in the split, because that is how the user picks
/// one: the time pane is on the left, the flow pane on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneSide {
    #[default]
    Flow,
    Time,
}

/// An indicator slot together with the pane that owns it.
///
/// Slot ids are allocated per pane, so the id alone stopped identifying an
/// indicator the moment there were two panes: without the side, removing the
/// time pane's slot 0 would drop the flow pane's bookkeeping for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaneSlot {
    side: PaneSide,
    slot: SlotId,
}

/// The interactive regions of the plot, plus the optional live strip.
pub struct PlotAreas {
    /// The candle body, with the indicator pane band already taken out of it.
    /// Every consumer — renderer and input handler alike — reads the chart
    /// rect from here, which is what keeps the price scale a drawing is
    /// placed against identical to the one it is hit-tested against.
    pub chart: egui::Rect,
    /// Stacked indicator panes below the candles, top to bottom. Empty when
    /// no pane indicator is visible.
    pub indicator_panes: Vec<egui::Rect>,
    /// Present only while the strip is shown; sits between `chart` and
    /// `price_gutter` and is not an input region.
    pub live_strip: Option<egui::Rect>,
    pub price_gutter: egui::Rect,
    pub time_strip: egui::Rect,
}

/// Format the forming bar's countdown, e.g. `37/50 ticks`.
///
/// Trailing zeros are trimmed on both figures: a volume bar's accumulator
/// carries the feed's own scale, and `1.20000000/5 vol` reads as noise.
fn fmt_progress(progress: &quantick_engine::BarProgress, unit: &str) -> String {
    format!(
        "{}/{} {unit}",
        progress.done.normalize(),
        progress.target.normalize()
    )
}

/// Format a duration in milliseconds for the lane's time axis, in the unit a
/// human would read it in.
pub fn fmt_window(milliseconds: i64) -> String {
    let milliseconds = milliseconds.max(0);
    if milliseconds < 1_000 {
        format!("{milliseconds} ms")
    } else if milliseconds < 60_000 {
        format!("{:.1} s", milliseconds as f64 / 1_000.0)
    } else {
        format!("{:.1} min", milliseconds as f64 / 60_000.0)
    }
}

/// Format a UTC epoch-millisecond timestamp as `HH:MM:SS` in the display
/// timezone `tz`, for the time axis.
pub fn fmt_time(ms: i64, tz: TzOffset) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// The quantick chart window.
pub struct QuantickApp {
    /// quantick's own chart, and the only one in the default layout.
    flow_pane: ChartPane,
    /// The context chart beside it (§11), built the first time the split is
    /// shown and kept for the session afterwards — switching back to Single
    /// hides it, and must not throw away its indicators and drawings.
    ///
    /// While it exists it is fed every trade the flow pane is fed, on screen
    /// or not, which is what keeps the two in step. The cost is the market's
    /// trades retained twice: one tape, two `ChartState`s, and still only one
    /// bar-building path.
    time_pane: Option<ChartPane>,
    /// Which panes the canvas shows. In-session only for now: persisting
    /// chrome state is the open question §14 leaves to the dock's decision.
    layout: CanvasLayout,
    /// The time pane's share of the canvas width while the split is shown.
    split_fraction: f32,
    /// The pane the chrome speaks for: status bar, indicator targeting and the
    /// keyboard's drawing grammar (§11). Meaningless while the canvas is
    /// Single — read it through [`Self::focused_side`], never directly.
    focus: PaneSide,
    events: mpsc::Receiver<FeedEvent>,
    book_events: mpsc::Receiver<DepthEvent>,
    /// Connection trouble the feed wants the user to know about.
    notices: mpsc::Receiver<FeedNotice>,
    /// The newest notice, held until the feed says it is over. A feed that
    /// blocks once and then goes quiet has to keep saying so — the chart it
    /// left empty will not.
    notice: FeedNotice,
    /// State reported by the live trade transport, independent from how often
    /// that market prints and from the last observed arrival latency.
    feed_connection: FeedConnectionState,
    /// What the running feed can really do, read fresh every frame. The feed
    /// narrows it once a session tells it what the symbol actually offers.
    feed_capabilities: watch::Receiver<FeedCapabilities>,
    commands: mpsc::Sender<FeedCommand>,
    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    script_library: ScriptLibrary,
    /// The open indicator-settings dialog, if any (one at a time).
    indicator_settings: Option<SettingsDialog>,
    /// The pane the open dialog's slot lives on. Held apart from the dialog
    /// so focus moving under it cannot retarget its Apply.
    indicator_settings_side: PaneSide,
    /// File-backed script slots: (slot, library index, last seen mtime) —
    /// what the hot-reload poll walks.
    script_files: Vec<(PaneSlot, usize, std::time::SystemTime)>,
    /// How each live slot restores (the persistence identity per slot).
    ///
    /// Stays beside the library and the state file rather than moving into the
    /// panes with the slots themselves: one file records what the window had
    /// open, so one list records what is in it.
    slot_kinds: Vec<(PaneSlot, SavedKind)>,
    /// Slots restored as hidden, applied when their Rebuilt lands. Flow-pane
    /// only, because restoring is (see [`Self::maintain_indicator_state`]).
    pending_hidden: Vec<SlotId>,
    /// Where the indicator set persists.
    indicator_state_path: std::path::PathBuf,
    /// Set by any add/remove/hide/inputs change; drained by the debounced
    /// save.
    indicator_state_dirty: bool,
    /// When the last indicator change happened (the debounce clock).
    last_indicator_change: Option<Instant>,
    /// Last hot-reload poll instant (the poll runs about once a second;
    /// file metadata every frame would be waste).
    last_script_poll: Instant,
    book_capture_epoch: u64,
    book_channel_closed_reported: bool,

    // Feed & asset selection, driven by the configuration. `feed_id`/`symbol`
    // are what the selectors show (the desired selection); `active` is what the
    // running feed thread is actually streaming. When they diverge, the feed is
    // respawned. Nothing here is hard-coded — it all comes from `config`.
    config: AppConfig,
    feed_id: String,
    symbol: String,
    active: (String, String),

    // Market Replay. `replay` is `Some` exactly while a recorded session is
    // the chart's source; it is the one flag the rest of the UI checks, so
    // replay never grows a second copy of "which mode are we in". The view
    // owns the browser window and the transport bar.
    replay: Option<ReplayLink>,
    replay_view: ReplayView,

    // External chart chrome: the tabbed right dock and the corner-docked
    // drawing toolbox. Neither is painted over the chart canvas.
    dock: Dock,
    toolrail: ToolRail,

    // How many older trades to pull per "load older" click, and how many
    // trades have been backfilled in total (for the readout).
    history_step: usize,
    history_trades: usize,
    // Every wait currently in flight, drawn by one overlay (see
    // crate::loading). History loads are counted — several can be queued and
    // the first reply must not hide the indicator while others are still out;
    // the feed answers every request with exactly one event, so the count
    // always drains back to zero. Replay parsing and book synchronization are
    // level-triggered mirrors of their owners' state.
    loading: LoadingTracker,

    // Delete confirmation for a locked drawing, shown next to the trigger.
    drawing_delete_confirm: bool,
    // Pre-edit copy of the selected drawing while an inspector edit gesture
    // (slider/color/coordinate drag) is in flight; committed as one undo
    // entry once pointer and keyboard let go.
    inspector_edit_baseline: Option<(usize, drawings::Drawing)>,
    drawing_toast: Option<DrawingToast>,
    // Inspector chrome state: open tab, dock pin, whether the user moved the
    // floating window this session (manual position wins over placement),
    // and the selection the last placement was computed for.
    inspector_tab: InspectorTab,
    inspector_pinned: bool,
    inspector_moved: bool,
    inspector_last_selection: Option<usize>,
    drawing_manager_open: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    #[cfg(test)]
    inspector_pin_rect: Option<egui::Rect>,
    #[cfg(test)]
    time_header_chips: [egui::Rect; time_header::PRESETS.len()],
    #[cfg(test)]
    canvas_divider: Option<egui::Rect>,
    #[cfg(test)]
    manager_action_rects: Vec<(usize, &'static str, egui::Rect)>,

    // Candle appearance + whether the style panel is open.
    style: ChartStyle,
    show_style: bool,
    style_revision: u64,
    style_log_pending: bool,
    last_style_change: Option<Instant>,
    // Whether the status bar shows the perf readings (View → perf readings).
    show_perf: bool,

    // Fixed UTC offset the time axis is displayed in (default UTC−03:00).
    tz: TzOffset,

    frames: FrameStats,
    /// CPU time per frame (update + tessellation + paint, no vsync wait), from
    /// eframe. Separates "we are slow" from "we are waiting for the display".
    cpu_frames: FrameStats,
    last_frame: Option<Instant>,
    /// Exchange-to-UI delay measured when the newest live trade arrived.
    /// Stable while the tape is quiet: market inactivity is not transport lag.
    latest_trade_latency_ms: Option<i64>,
    /// Timestamp of the newest live trade (epoch ms), for the tape-age
    /// readout. The latency above is an observation frozen at arrival; this
    /// is what wall clock is compared against every frame.
    latest_trade_ms: Option<i64>,
    live_trades: u64,
    trades_since_summary: u64,
    last_summary: Instant,
}

impl QuantickApp {
    /// Create the app on `config`, opening on `feed_id`/`symbol` (already
    /// streaming through `feed`) and bar `spec`.
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: BarSpec,
        feed: FeedHandle,
    ) -> Self {
        let feed_id = feed_id.into();
        let symbol = symbol.into();

        let mut loading = LoadingTracker::new();
        // The feed starts backfilling the moment it is spawned, so the chart
        // opens with that one load already in flight.
        loading.begin(LoadingTask::History);

        let mut app = Self {
            flow_pane: ChartPane::flow(FLOW_PANE_ID, spec, symbol.clone()),
            time_pane: None,
            layout: CanvasLayout::Single,
            split_fraction: DEFAULT_PANE_FRACTION,
            focus: PaneSide::Flow,
            events: feed.events,
            book_events: feed.book_events,
            notices: feed.notices,
            feed_capabilities: feed.capabilities,
            notice: FeedNotice::Clear,
            feed_connection: FeedConnectionState::Connecting,
            commands: feed.commands,
            script_library: ScriptLibrary::scan(),
            indicator_settings: None,
            indicator_settings_side: PaneSide::Flow,
            script_files: Vec::new(),
            slot_kinds: Vec::new(),
            pending_hidden: Vec::new(),
            indicator_state_path: state_file::default_path(),
            indicator_state_dirty: false,
            last_indicator_change: None,
            last_script_poll: Instant::now(),
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            active: (feed_id.clone(), symbol.clone()),
            replay: feed.replay,
            replay_view: ReplayView::new(),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            config,
            feed_id,
            symbol,
            history_step: 2000,
            history_trades: 0,
            loading,
            drawing_delete_confirm: false,
            inspector_edit_baseline: None,
            drawing_toast: None,
            inspector_tab: InspectorTab::default(),
            inspector_pinned: false,
            inspector_moved: false,
            inspector_last_selection: None,
            drawing_manager_open: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            #[cfg(test)]
            inspector_pin_rect: None,
            #[cfg(test)]
            time_header_chips: [egui::Rect::NOTHING; time_header::PRESETS.len()],
            #[cfg(test)]
            canvas_divider: None,
            #[cfg(test)]
            manager_action_rects: Vec::new(),
            style: ChartStyle::default(),
            show_style: false,
            style_revision: 0,
            style_log_pending: false,
            last_style_change: None,
            show_perf: true,
            tz: TzOffset::default(),
            frames: FrameStats::new(120),
            cpu_frames: FrameStats::new(120),
            last_frame: None,
            latest_trade_latency_ms: None,
            latest_trade_ms: None,
            live_trades: 0,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        };
        // Recording is not a display choice: it starts with the feed, so
        // hiding the map later never leaves a hole in what was captured.
        app.ensure_book_capture();
        // A feed that declares its own look opens wearing it.
        app.apply_feed_bubble_preset();
        // The map itself stays hidden until asked for — a layer nobody
        // requested must cost no projection. Dev/ops can open it without a
        // click; capture is already running either way.
        app.tape_mut()
            .set_depth_visible(std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|v| v == "1"));
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            app.flow_pane.live_strip_visible = true;
        }
        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            app.tape_mut().set_bubbles_enabled(true);
        }
        // Same convenience for indicators: open with the two M1 natives on
        // (EMA overlay + CVD pane), through the same code path the toolbar
        // menu takes, so a scripted validation run needs no clicks.
        if std::env::var("QUANTICK_INDICATORS_AUTOSTART").is_ok_and(|value| value == "1") {
            app.flow_pane.add_indicator(IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            });
            app.flow_pane.add_indicator(IndicatorSource::NativeCvd);
        }
        // Restore the persisted indicator set before any autostart hook:
        // the file is what the user actually had open.
        app.restore_indicator_state();
        // Scripted validation runs can open with library scripts loaded:
        // a comma-separated list of script names, each through the same
        // code path the INDICATORS menu takes.
        if let Ok(names) = std::env::var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match app
                    .script_library
                    .entries()
                    .iter()
                    .position(|entry| entry.name == name)
                {
                    Some(index) => {
                        app.add_script_indicator(index);
                        // An env var is not a user edit. Without this, a
                        // scripted validation run appended its own scripts to
                        // the saved set and they opened by themselves on the
                        // next plain launch — config presence activating
                        // something, which the rules forbid. The natives hook
                        // above never registers a kind, so it is already inert.
                        app.forget_last_indicator_state_change();
                    }
                    None => tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "INDICATOR_SCRIPT_UNKNOWN",
                        script = %name,
                        action = "autostart_entry_skipped",
                        "autostart names a script the library does not have"
                    ),
                }
            }
        }
        // Same convenience for Market Replay: open the folder named by
        // QUANTICK_REPLAY_DIR and play its first session. One env var, the same
        // code path a click takes, so a scripted run and a person get the same
        // behaviour.
        if std::env::var("QUANTICK_REPLAY_AUTOSTART").is_ok_and(|value| value == "1") {
            let speed = std::env::var("QUANTICK_REPLAY_SPEED")
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|speed| *speed > 0.0)
                .unwrap_or(1.0);
            let started = app.replay_view.autostart(speed);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                folder = std::env::var(crate::replay_view::REPLAY_DIR_ENV).unwrap_or_default(),
                speed,
                started,
                action = if started { "load_first_session" } else { "open_browser" },
                "market replay autostart"
            );
        }
        app
    }

    /// The pane the chrome speaks for. Only a split canvas has a choice to
    /// make; a Single canvas is the flow pane by definition, whatever the last
    /// split left `focus` set to.
    fn focused_side(&self) -> PaneSide {
        match (self.layout, self.time_pane.is_some()) {
            (CanvasLayout::TimeAndFlow, true) => self.focus,
            _ => PaneSide::Flow,
        }
    }

    /// The pane on `side`, falling back to the flow pane when the time pane
    /// has never been opened.
    fn pane(&self, side: PaneSide) -> &ChartPane {
        match side {
            PaneSide::Time => self.time_pane.as_ref().unwrap_or(&self.flow_pane),
            PaneSide::Flow => &self.flow_pane,
        }
    }

    /// See [`Self::pane`].
    fn pane_mut(&mut self, side: PaneSide) -> &mut ChartPane {
        match side {
            PaneSide::Time => self.time_pane.as_mut().unwrap_or(&mut self.flow_pane),
            PaneSide::Flow => &mut self.flow_pane,
        }
    }

    /// The pane every chrome surface reads from — see [`Self::focused_side`].
    fn focused_pane(&self) -> &ChartPane {
        self.pane(self.focused_side())
    }

    /// See [`Self::focused_pane`].
    fn focused_pane_mut(&mut self) -> &mut ChartPane {
        self.pane_mut(self.focused_side())
    }

    /// Every pane holding this market's bars, on screen or not. One tape, and
    /// however many charts the layout has ever shown read off it.
    fn panes_mut(&mut self) -> impl Iterator<Item = &mut ChartPane> {
        std::iter::once(&mut self.flow_pane).chain(self.time_pane.as_mut())
    }

    /// The flow pane's tape.
    ///
    /// The flow pane is built with one ([`ChartPane::flow`]) and never gives it
    /// up; the `Option` on the pane exists so a *time* pane can go without a
    /// book worker, not because this one can be missing.
    fn tape(&self) -> &OrderflowView {
        self.flow_pane
            .orderflow
            .as_ref()
            .expect("the flow pane is built with a tape and never drops it")
    }

    /// See [`Self::tape`].
    fn tape_mut(&mut self) -> &mut OrderflowView {
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
    fn set_layout(&mut self, layout: CanvasLayout) {
        self.layout = layout;
        if layout == CanvasLayout::TimeAndFlow && self.time_pane.is_none() {
            let mut pane = ChartPane::time(TIME_PANE_ID, time_header::DEFAULT_INTERVAL_MS);
            pane.seed_from(
                self.flow_pane.state.trades(),
                self.flow_pane.state.backfill_trade_count(),
            );
            self.time_pane = Some(pane);
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
            action = "relayout_canvas",
            "canvas layout changed"
        );
    }

    /// The display name of the currently selected feed, or its id as a fallback.
    fn feed_display_name(&self) -> String {
        self.config
            .feed(&self.feed_id)
            .map_or_else(|| self.feed_id.clone(), |f| f.name.clone())
    }

    /// Keep `symbol` valid for the selected feed: if the feed changed and no
    /// longer offers the current symbol, fall back to its first symbol.
    fn ensure_symbol_valid(&mut self) {
        let valid = self
            .config
            .feed(&self.feed_id)
            .is_some_and(|f| f.symbols.contains(&self.symbol));
        if !valid
            && let Some(first) = self
                .config
                .feed(&self.feed_id)
                .and_then(|f| f.symbols.first())
                .cloned()
        {
            self.symbol = first;
        }
    }

    /// Build the toolbar's model from the app's state, draw it, and carry
    /// out whatever it asked (§6 — the toolbar module owns grouping and the
    /// overflow rule; this method owns the side effects).
    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        // Pre-collect owned option lists so the toolbar's combos don't borrow
        // `self.config` while they mutate `self.feed_id` / `self.symbol`.
        // Providers that aren't streaming yet are labelled "(soon)" so the
        // menu is honest about what actually connects.
        let feeds: Vec<(String, String)> = self
            .config
            .feeds
            .iter()
            .map(|f| {
                let label = if f.provider.is_implemented() {
                    f.name.clone()
                } else {
                    format!("{} (soon)", f.name)
                };
                (f.id.clone(), label)
            })
            .collect();
        let symbols: Vec<String> = self
            .config
            .feed(&self.feed_id)
            .map(|f| f.symbols.clone())
            .unwrap_or_default();
        // During a replay the SOURCE group gives way to what is actually
        // playing: a live venue cannot be picked without leaving the
        // recording first, and a combo that silently did so would throw away
        // the session mid-run.
        let replay = self.replay.as_ref().map(|link| toolbar::ReplaySource {
            label: link.label(),
            hover: format!(
                "Replaying {}\nSide source: {}",
                link.session.path.display(),
                link.session
                    .header
                    .side_source
                    .as_deref()
                    .unwrap_or("not recorded"),
            ),
        });
        let capabilities = self.capabilities();
        let feed_display_name = self.feed_display_name();
        let heatmap_on = self.tape().depth_visible();
        let bubbles_on = self.tape().bubbles_enabled();
        // The focused pane's slots (§11): the menu lists what a command from
        // it would act on, and never the pane beside it.
        let indicators: Vec<toolbar::IndicatorMenuEntry> = self
            .focused_pane()
            .indicators
            .all()
            .iter()
            .map(|view| toolbar::IndicatorMenuEntry {
                slot: view.slot.0,
                label: view.label().to_owned(),
                hidden: view.hidden,
                errored: view.error.is_some(),
                stale: view.stale.is_some(),
            })
            .collect();
        let mut model = toolbar::ToolbarModel {
            feeds,
            feed_id: &mut self.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut self.symbol,
            replay,
            kind: &mut self.flow_pane.kind,
            tick_n: &mut self.flow_pane.tick_n,
            volume_units: &mut self.flow_pane.volume_units,
            dollar_notional: &mut self.flow_pane.dollar_notional,
            time_interval_ms: &mut self.flow_pane.time_interval_ms,
            imbalance_target: &mut self.flow_pane.imbalance_target,
            history_step: &mut self.history_step,
            history_trades: self.history_trades,
            capabilities,
            heatmap_on,
            bubbles_on,
            live_strip_on: self.flow_pane.live_strip_visible,
            dock_visible: self.dock.visible(),
            appearance_open: self.show_style,
            indicators,
            scripts: self
                .script_library
                .entries()
                .iter()
                .map(|entry| entry.name.clone())
                .collect(),
        };
        let actions = toolbar::draw(ctx, &mut model);
        // A newly picked feed may not offer the current symbol. Never during
        // a replay: the recorded instrument belongs to no live feed's menu,
        // and snapping it away would relabel the whole session — the status
        // bar and the logs must keep naming what is actually playing.
        if self.replay.is_none() {
            self.ensure_symbol_valid();
        }
        for action in actions {
            self.apply_toolbar_action(action);
        }
    }

    /// One toolbar side effect. Layer toggles reuse the same code paths the
    /// old checkboxes took, so provider gating and command acknowledgement
    /// rules are unchanged.
    fn apply_toolbar_action(&mut self, action: ToolbarAction) {
        match action {
            ToolbarAction::LoadOlder => self.request_older_history(),
            ToolbarAction::SetHeatmap(shown) => self.tape_mut().set_depth_visible(shown),
            ToolbarAction::SetBubbles(enabled) => self.tape_mut().set_bubbles_enabled(enabled),
            ToolbarAction::SetLiveStrip(shown) => self.flow_pane.live_strip_visible = shown,
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.show_style = !self.show_style,
            // Every indicator command lands on the focused pane (§11), which
            // is the flow pane whenever the canvas is not split.
            ToolbarAction::AddEmaIndicator => {
                self.add_native_indicator(SavedKind::NativeEma);
            }
            ToolbarAction::AddCvdIndicator => {
                self.add_native_indicator(SavedKind::NativeCvd);
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                self.focused_pane_mut()
                    .indicators
                    .toggle_hidden(SlotId(slot));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::RemoveIndicator(slot) => {
                let target = PaneSlot {
                    side: self.focused_side(),
                    slot: SlotId(slot),
                };
                // UI first (the entry vanishes this frame), worker second;
                // events already in flight for the slot are dropped on apply.
                let pane = self.focused_pane_mut();
                pane.indicators.remove(target.slot);
                pane.indicator_worker
                    .send(IndicatorCommand::Remove(target.slot));
                self.slot_kinds.retain(|(owner, _)| *owner != target);
                self.script_files.retain(|(owner, ..)| *owner != target);
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let slot = SlotId(slot);
                let side = self.focused_side();
                if let Some(view) = self
                    .pane(side)
                    .indicators
                    .all()
                    .iter()
                    .find(|v| v.slot == slot)
                {
                    self.indicator_settings = Some(SettingsDialog {
                        slot,
                        title: view.label().to_owned(),
                        draft: view.input_values.clone(),
                    });
                    self.indicator_settings_side = side;
                }
            }
        }
    }

    /// Draw the settings dialog and execute its outcome. Apply goes through
    /// the worker (construct anew, replace, replay) — the same path every
    /// input change takes, UI or not.
    fn draw_indicator_settings(&mut self, ctx: &egui::Context) {
        let side = self.indicator_settings_side;
        let outcome = {
            let Self {
                indicator_settings,
                flow_pane,
                time_pane,
                ..
            } = self;
            let Some(dialog) = indicator_settings.as_mut() else {
                return;
            };
            let pane = match side {
                PaneSide::Time => time_pane.as_ref().unwrap_or(&*flow_pane),
                PaneSide::Flow => &*flow_pane,
            };
            let Some(view) = pane
                .indicators
                .all()
                .iter()
                .find(|view| view.slot == dialog.slot)
            else {
                // The indicator was removed under the dialog.
                *indicator_settings = None;
                return;
            };
            indicator_panel::draw(ctx, dialog, &view.descriptor.inputs)
        };
        match outcome {
            SettingsOutcome::Open => {}
            SettingsOutcome::Cancel => self.indicator_settings = None,
            SettingsOutcome::Apply => {
                let dialog = self.indicator_settings.take().expect("dialog is open");
                // The pane the dialog was opened on, not whichever has focus
                // now: clicking Apply must not retarget the edit.
                self.pane_mut(side)
                    .indicator_worker
                    .send(IndicatorCommand::SetInputs {
                        slot: dialog.slot,
                        values: dialog.draft,
                    });
                self.mark_indicator_state_dirty();
            }
        }
    }

    /// Load a library script behind a fresh slot. A file that no longer
    /// reads or a script that no longer compiles becomes the slot's error —
    /// shown with lines and codes, never silently dropped.
    ///
    /// Returns the slot it claimed, so a caller that needs to address the new
    /// indicator (restoring saved inputs, say) does not have to guess which
    /// one it is.
    fn add_script_indicator(&mut self, index: usize) -> Option<SlotId> {
        let entry = self.script_library.entries().get(index)?;
        let name = entry.name.clone();
        let side = self.focused_side();
        match self.script_library.read(index) {
            Some(Ok(text)) => {
                let slot = self.pane_mut(side).add_indicator(IndicatorSource::Script {
                    name: name.clone(),
                    text,
                });
                let owner = PaneSlot { side, slot };
                // Watch the file so a save reloads it. Registered here, with
                // the add, so the two cannot drift apart.
                if let Some((_, mtime)) = self.script_library.file_info(index) {
                    self.script_files.push((owner, index, mtime));
                }
                self.slot_kinds.push((owner, SavedKind::Script { name }));
                self.mark_indicator_state_dirty();
                Some(slot)
            }
            Some(Err(message)) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %name,
                    error = %message,
                    action = "error_slot_shown",
                    "cannot read an indicator script"
                );
                // A click that produces nothing at all is the failure this
                // function's own doc comment rules out. The compile half of
                // that promise runs worker-side; the read half never leaves
                // the UI thread, so the error slot is built here, from the
                // same two events the worker would have sent.
                let slot = self.flow_pane.indicators.allocate_slot();
                self.flow_pane.indicators.apply(IndicatorEvent::Rebuilt {
                    slot,
                    descriptor: quantick_indicators::IndicatorDescriptor {
                        title: name,
                        short_title: None,
                        overlay: false,
                        plots: Vec::new(),
                        fills: Vec::new(),
                        inputs: Vec::new(),
                    },
                    columns: Vec::new(),
                    inputs: Vec::new(),
                    stale: None,
                });
                self.flow_pane.indicators.apply(IndicatorEvent::Error {
                    slot,
                    error: quantick_indicators::EvalError {
                        bar_index: 0,
                        message,
                    },
                });
                Some(slot)
            }
            None => None,
        }
    }

    /// What the selected feed's backend can do.
    ///
    /// A feed missing from the config can do nothing — the selection is snapped
    /// back on the next switch, and until then no affordance may promise data
    /// nothing is streaming.
    fn capabilities(&self) -> FeedCapabilities {
        // A feed missing from the config resolves to no provider, so nothing is
        // streaming and nothing may be promised.
        if self.config.provider_of(&self.feed_id).is_none() && self.replay.is_none() {
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
    fn request_older_history(&mut self) {
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
    fn next_book_generation(&mut self) -> u64 {
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
    fn ensure_book_capture(&mut self) {
        if self.tape().enabled() || !self.capabilities().book_capture {
            return;
        }
        self.request_book_capture(true);
    }

    /// Start or stop the independent depth pipeline without touching aggTrades
    /// or candle construction. UI state changes only if the command is queued.
    fn request_book_capture(&mut self, enabled: bool) {
        if !self.capabilities().book_capture {
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
    fn restart_book_capture(&mut self) {
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
    fn maybe_switch_feed(&mut self) {
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
        let Some(provider) = self.config.provider_of(&self.feed_id) else {
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
        let handle = feed::spawn_live(provider, &self.symbol, &self.config);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;

        // Rebuild every pane from scratch for the new stream, each keeping its
        // own bar spec. Retained trades from the old symbol must not leak in.
        for pane in self.panes_mut() {
            pane.reset_series();
        }
        self.reset_drawing_overlays();
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.loading.restart(LoadingTask::History);
        self.latest_trade_latency_ms = None;
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);

        self.active = (self.feed_id.clone(), self.symbol.clone());
        self.ensure_book_capture();
        self.apply_feed_bubble_preset_after_switch(&previous_feed);
    }

    /// Apply the arrived-at feed's declared preset — only when the switch
    /// actually crossed feeds. A symbol hop inside one feed keeps the user's
    /// panel tweaks: the declared look belongs to the feed, not the symbol.
    fn apply_feed_bubble_preset_after_switch(&mut self, previous_feed: &str) {
        if previous_feed == self.feed_id {
            return;
        }
        self.apply_feed_bubble_preset();
    }

    /// Apply the bubble preset the current feed declares, if it declares one.
    ///
    /// A feed with no `bubble_preset` changes nothing: the panel keeps the look
    /// the user last chose. An unknown name is reported and ignored — the
    /// presets file is user-edited, and a typo there must not silently restyle
    /// the chart.
    fn apply_feed_bubble_preset(&mut self) {
        let Some(name) = self
            .config
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
    fn apply_spec_changes(&mut self) {
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
    /// the flow pane and the time pane's own header governs the time pane
    /// (§11), so a timeframe change must not rebuild the chart beside it.
    fn apply_spec_change(&mut self, side: PaneSide) {
        let pane = self.pane_mut(side);
        let desired = pane.current_spec();
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
                pane.state.set_spec(desired);
                pane.send_indicator_rebuild();
                let slot = anchor.and_then(|ms| pane.state.slot_at_time(ms));
                let slots = pane.slots();
                pane.viewport.reanchor(slot, slots);
                self.reset_drawing_overlay(side);
            }
        }
    }

    /// Draw the modular candle-appearance panel and debounce its diagnostic
    /// event so dragging a slider cannot flood logs at frame rate.
    fn draw_style_panel(&mut self, ctx: &egui::Context, now: Instant) {
        let response = draw_style_window(ctx, &mut self.show_style, &mut self.style);
        if response.changed {
            self.style_revision = self.style_revision.saturating_add(1);
            self.style_log_pending = true;
            self.last_style_change = Some(now);
        }

        let settled = self
            .last_style_change
            .is_some_and(|changed| now.saturating_duration_since(changed) >= STYLE_LOG_DEBOUNCE);
        if self.style_log_pending && (settled || !self.show_style) {
            self.emit_style_changed(response.applied_preset);
        }
    }

    fn emit_style_changed(&mut self, applied_preset: Option<CandlePreset>) {
        let candles = &self.style.candles;
        let preset = applied_preset
            .or_else(|| CandlePreset::detect(candles))
            .map_or("custom", CandlePreset::log_value);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CANDLE_STYLE_CHANGED",
            revision = self.style_revision,
            preset,
            body_mode = ?candles.body_mode,
            fill_opacity = candles.fill_opacity,
            outline_opacity = candles.outline_opacity,
            outline_width_px = candles.outline_width,
            body_width_fraction = candles.body_width_frac,
            wick_mode = ?candles.wick_color_mode,
            wick_width_px = candles.wick_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            action = "redraw_only",
            "candle appearance changed"
        );
        self.style_log_pending = false;
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// observed arrival latency and live-trade counts for the metrics.
    fn drain_feed(&mut self) {
        self.drain_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected drain used to prove that one UI cycle is one observation.
    fn drain_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
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
                }
                Ok(FeedEvent::LiveBatch(trades)) => {
                    if !trades.is_empty() {
                        let received_at_ms = *received_at_ms.get_or_insert_with(&mut wall_clock_ms);
                        for trade in &trades {
                            self.ingest_live_trade_at(trade, received_at_ms);
                        }
                    }
                }
                Ok(FeedEvent::Reset) => self.reset_market_state(),
                Err(_) => break,
            }
        }
    }

    /// Take the newest feed notice, if the feed sent any this frame.
    ///
    /// Level-triggered rather than queued: only the latest state matters, and
    /// a burst of bridge output must not queue up cards to show one by one.
    /// A closed channel (a feed with nothing to report) simply yields nothing.
    fn drain_notices(&mut self) {
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
    fn ingest_live_trade_at(&mut self, trade: &quantick_engine::Trade, received_at_ms: i64) {
        self.latest_trade_latency_ms =
            metrics::feed_lag_ms(received_at_ms, Some(trade.timestamp_ms));
        self.latest_trade_ms = Some(trade.timestamp_ms);
        self.live_trades += 1;
        self.trades_since_summary += 1;
        for pane in self.panes_mut() {
            pane.ingest_live_trade(trade);
        }
    }

    /// Hot reload: about once a second, compare each file-backed script's
    /// mtime; a changed file is re-read and sent as a Reload — recompiled
    /// and replayed on success, or flagged stale (the last good version
    /// keeps running) on errors. The mtime updates even when the compile
    /// fails, so a broken save does not re-fire every second.
    fn poll_script_files(&mut self) {
        if self.script_files.is_empty()
            || self.last_script_poll.elapsed() < SCRIPT_RELOAD_POLL_INTERVAL
        {
            return;
        }
        self.last_script_poll = Instant::now();
        let mut reloads: Vec<(PaneSlot, String, String)> = Vec::new();
        for (owner, index, seen_mtime) in &mut self.script_files {
            let Some((path, mtime)) = self.script_library.file_info(*index) else {
                continue;
            };
            if mtime == *seen_mtime {
                continue;
            }
            *seen_mtime = mtime;
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    reloads.push((*owner, name, text));
                }
                Err(error) => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNREADABLE",
                    script = %path.display(),
                    error = %error,
                    action = "reload_skipped",
                    "cannot re-read a changed indicator script"
                ),
            }
        }
        for (owner, name, text) in reloads {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_SCRIPT_RELOAD",
                script = %name,
                pane = ?owner.side,
                action = "recompile_and_replay",
                "indicator script changed on disk"
            );
            // To the worker that owns the slot: the same script loaded on both
            // panes is two slots, and a Reload sent to the wrong one addresses
            // whatever indicator happens to share its number there.
            self.pane_mut(owner.side)
                .indicator_worker
                .send(IndicatorCommand::Reload {
                    slot: owner.slot,
                    source: IndicatorSource::Script { name, text },
                });
        }
    }

    /// Add one of the built-in indicators to the focused pane and register how
    /// it restores.
    fn add_native_indicator(&mut self, kind: SavedKind) -> SlotId {
        let side = self.focused_side();
        let source = match kind {
            SavedKind::NativeCvd => IndicatorSource::NativeCvd,
            // Every other kind is a script, which comes through
            // `add_script_indicator`; EMA is the remaining native.
            _ => IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            },
        };
        let slot = self.pane_mut(side).add_indicator(source);
        self.slot_kinds.push((PaneSlot { side, slot }, kind));
        self.mark_indicator_state_dirty();
        slot
    }

    fn mark_indicator_state_dirty(&mut self) {
        self.indicator_state_dirty = true;
        self.last_indicator_change = Some(Instant::now());
    }

    /// Undo the dirty mark an add just set — for indicators an env var asked
    /// for rather than the user. The slot still exists and still works; it
    /// simply does not enter the persisted set.
    fn forget_last_indicator_state_change(&mut self) {
        self.slot_kinds.pop();
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
    }

    /// Rebuild the persisted set at startup, through the same commands the
    /// menu sends (add, then bind saved inputs, then hide once the view
    /// lands). A script the library no longer has is skipped with a log
    /// line, never a phantom entry.
    ///
    /// The flow pane only, and the file holds only its slots — see
    /// [`Self::maintain_indicator_state`] for why.
    fn restore_indicator_state(&mut self) {
        let saved = state_file::load(&self.indicator_state_path);
        for entry in saved {
            let slot = match &entry.kind {
                SavedKind::NativeEma => Some(self.add_native_indicator(SavedKind::NativeEma)),
                SavedKind::NativeCvd => Some(self.add_native_indicator(SavedKind::NativeCvd)),
                SavedKind::Script { name } => {
                    match self
                        .script_library
                        .entries()
                        .iter()
                        .position(|candidate| candidate.name == *name)
                    {
                        // The returned slot, not `slot_kinds.last()`: that
                        // assumed the add always pushes an entry as its last
                        // act, and it does not when the file no longer reads
                        // — the saved inputs would then bind to whatever
                        // indicator happened to be added before this one.
                        Some(index) => self.add_script_indicator(index),
                        None => {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_STATE_SCRIPT_MISSING",
                                script = %name,
                                action = "entry_skipped",
                                "the saved state references a script the library no longer has"
                            );
                            None
                        }
                    }
                }
            };
            let Some(slot) = slot else { continue };
            let values: Vec<_> = entry
                .inputs
                .iter()
                .filter_map(SavedInput::to_value)
                .collect();
            if !values.is_empty() && values.len() == entry.inputs.len() {
                self.flow_pane
                    .indicator_worker
                    .send(IndicatorCommand::SetInputs { slot, values });
            } else if !entry.inputs.is_empty() {
                // One unreadable cell dropped every input of the entry, in
                // silence — a hand-edited or stale file lost the whole
                // parameter set without a word.
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_STATE_INPUTS_DROPPED",
                    kind = ?entry.kind,
                    saved = entry.inputs.len(),
                    readable = values.len(),
                    action = "declared_defaults_used",
                    "saved indicator inputs could not be read; using the declared defaults"
                );
            }
            if entry.hidden {
                self.pending_hidden.push(slot);
            }
        }
        // Restoring is not a change; only user edits dirty the file.
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
    }

    /// Apply restored-hidden flags once their views exist, then write the
    /// state file when a change has settled (debounced off the frame path).
    ///
    /// The file records the **flow pane's** slots only, and the format is
    /// unchanged by the split. The time pane's indicators are in-session
    /// because the split itself is (§14 leaves chrome-state persistence open):
    /// with the canvas opening Single, a restored time-pane entry would have
    /// no pane to land on, and would then be quietly dropped by the next save.
    /// Persisting the layout is what unlocks persisting its indicators, and
    /// the two land together or not at all.
    fn maintain_indicator_state(&mut self) {
        if !self.pending_hidden.is_empty() {
            let existing: Vec<SlotId> = self
                .pending_hidden
                .iter()
                .copied()
                .filter(|slot| {
                    self.flow_pane
                        .indicators
                        .all()
                        .iter()
                        .any(|v| v.slot == *slot)
                })
                .collect();
            for slot in &existing {
                self.flow_pane.indicators.toggle_hidden(*slot);
            }
            self.pending_hidden.retain(|slot| !existing.contains(slot));
        }
        let settled = self
            .last_indicator_change
            .is_some_and(|changed| changed.elapsed() >= INDICATOR_STATE_SAVE_DEBOUNCE);
        if self.indicator_state_dirty && settled {
            self.indicator_state_dirty = false;
            // The change has been written; the clock starts again with the
            // next edit rather than ticking on every frame from here on.
            self.last_indicator_change = None;
            // What is on disk today, so a slot that failed to build does not
            // overwrite its own saved parameters with an empty list.
            let previous = state_file::load(&self.indicator_state_path);
            let saved: Vec<SavedIndicator> = self
                .flow_pane
                .indicators
                .all()
                .iter()
                .filter_map(|view| {
                    let owner = PaneSlot {
                        side: PaneSide::Flow,
                        slot: view.slot,
                    };
                    let kind_ref = self
                        .slot_kinds
                        .iter()
                        .find(|(candidate, _)| *candidate == owner)
                        .map(|(_, kind)| kind)?;
                    let kind = kind_ref.clone();
                    // A slot whose build failed has an empty view: the
                    // worker's error path sends `Rebuilt { inputs: [] }`.
                    // Rewriting its entry from that would erase the user's
                    // saved parameters before they had a chance to fix the
                    // script, so a broken slot keeps what is already on disk.
                    let inputs = if view.error.is_some() {
                        previous
                            .iter()
                            .find(|entry| entry.kind == *kind_ref)
                            .map_or_else(Vec::new, |entry| entry.inputs.clone())
                    } else {
                        view.input_values
                            .iter()
                            .map(SavedInput::from_value)
                            .collect()
                    };
                    Some(SavedIndicator {
                        kind,
                        hidden: view.hidden,
                        inputs,
                    })
                })
                .collect();
            state_file::save(&self.indicator_state_path, &saved);
        }
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    fn reset_market_state(&mut self) {
        for pane in self.panes_mut() {
            pane.reset_series();
            // Indicators follow the chart into the empty state; the refill's
            // Backfilled event replays them (replay seek funnels through here,
            // so seeking inherits correct indicator behavior for free).
            pane.send_indicator_rebuild();
            pane.last_lane_divider_x = None;
        }
        self.reset_drawing_overlays();
        self.history_trades = 0;
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.loading.restart(LoadingTask::History);
        let symbol = self.symbol.clone();
        self.tape_mut().reset_for_symbol(symbol);
    }

    /// Every pane's overlay at once, for a change that invalidates them all —
    /// a feed switch or a source reset re-cuts both charts.
    fn reset_drawing_overlays(&mut self) {
        self.reset_drawing_overlay(PaneSide::Flow);
        if self.time_pane.is_some() {
            self.reset_drawing_overlay(PaneSide::Time);
        }
    }

    /// Bar-index anchors are meaningful only for the market/spec that created
    /// them. Clear them on a source or aggregation rebuild rather than
    /// silently attaching a mark to different market data — and say so.
    ///
    /// Scoped to one pane: the panes cut the same trades into different bars,
    /// so re-cutting one of them leaves the other's anchors exactly as valid
    /// as they were.
    fn reset_drawing_overlay(&mut self, side: PaneSide) {
        let pane = self.pane_mut(side);
        let had_drawings = !pane.drawings.items().is_empty();
        pane.drawings.clear();
        pane.drawing_hover = None;
        pane.drawing_press_position = None;
        pane.drawing_press_started_empty = false;
        pane.drawing_drag = DrawingDrag::None;
        self.toolrail.arm(Tool::Pointer);
        self.drawing_delete_confirm = false;
        self.inspector_edit_baseline = None;
        self.inspector_last_selection = None;
        // The cleared history cannot resurrect anything, so this toast
        // offers no Undo — a dead button would lie. But losing the marks is
        // never silent.
        self.drawing_toast = had_drawings.then(|| DrawingToast {
            message: "Drawings cleared - the bars were rebuilt under them.",
            shown_at: Instant::now(),
            offers_undo: false,
        });
    }

    /// Drain a bounded number of synchronized depth events. The separate
    /// channel and budget ensure heatmap work cannot block candle ingestion.
    fn drain_book_feed(&mut self) {
        self.drain_book_feed_with_clock(metrics::wall_clock_ms);
    }

    /// Clock-injected depth drain; a burst handled by one UI frame has one
    /// observation time, matching the trade-side metric and avoiding O(n)
    /// system-clock reads.
    fn drain_book_feed_with_clock(&mut self, mut wall_clock_ms: impl FnMut() -> i64) {
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
    fn trade_arrival_ms(&self) -> Option<i64> {
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
    fn tape_age_at(&self, now_ms: i64) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        let newest = match (self.latest_trade_ms, self.tape().last_event_ms()) {
            (Some(trade), Some(book)) => Some(trade.max(book)),
            (trade, book) => trade.or(book),
        }?;
        Some(now_ms.saturating_sub(newest).max(0))
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = self.trade_arrival_ms();
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let cpu_avg = self.cpu_frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);
        let book = self.tape_mut().health();
        let book_lag = book.arrival_latency_ms;
        let book_rate = book.depth_updates_since_summary as f64 / elapsed.as_secs_f64();
        let book_queue_len = self.book_events.len();
        let candle_preset =
            CandlePreset::detect(&self.style.candles).map_or("custom", CandlePreset::log_value);

        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "APP_HEALTH_SUMMARY",
            fps = fps as i64,
            frame_avg_ms = avg,
            frame_cpu_ms = cpu_avg,
            frame_worst_ms = worst,
            feed_arrival_ms = lag,
            trades_per_s = rate,
            live_trades = self.live_trades,
            bar_spec = self.flow_pane.state.spec().summary(),
            canvas_layout = ?self.layout,
            time_pane_spec = self.time_pane.as_ref().map(|pane| pane.state.spec().summary()),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_arrival_ms = book_lag,
            book_updates_per_s = book_rate,
            book_updates_total = book.depth_updates,
            book_queue_len,
            book_channel_closed = self.book_channel_closed_reported,
            book_bid_levels = book.bid_levels,
            book_ask_levels = book.ask_levels,
            heatmap_active_levels = book.active_levels,
            heatmap_archived_runs = book.archived_runs,
            aggression_count = book.aggression_count,
            heatmap_history_bytes = book.history_bytes,
            heatmap_cells = book.projection_cells,
            heatmap_aggressions = book.projection_aggressions,
            heatmap_liquidity_events = book.projection_liquidity_events,
            heatmap_effective_grouping = %book.effective_grouping,
            heatmap_effective_grouping_multiple = book.effective_grouping_multiple,
            heatmap_dropped_cells = book.dropped_cells,
            heatmap_dropped_aggressions = book.dropped_aggressions,
            heatmap_dropped_liquidity_events = book.dropped_liquidity_events,
            heatmap_projection_ms = book.projection_ms,
            heatmap_live_ms = book.live_ms,
            heatmap_projection_builds = book.projection_builds,
            heatmap_projection_cache_hits = book.projection_cache_hits,
            heatmap_config_revision = book.config_revision,
            heatmap_snapshots = book.snapshots,
            heatmap_gaps = book.gaps,
            candle_style_revision = self.style_revision,
            candle_preset,
            candle_body_mode = ?self.style.candles.body_mode,
            candle_fill_opacity = self.style.candles.fill_opacity,
            candle_outline_opacity = self.style.candles.outline_opacity,
            candle_outline_width_px = self.style.candles.outline_width,
            chart_background_enabled = self.style.canvas.background_enabled,
            chart_grid_enabled = self.style.canvas.grid_enabled,
            replay_active = self.replay.is_some(),
            replay_speed = self.replay.as_ref().map(|r| r.status.speed()),
            replay_playing = self.replay.as_ref().map(|r| r.status.is_playing()),
            replay_progress = self.replay.as_ref().map(|r| r.status.progress()),
            replay_played = self.replay.as_ref().map(|r| r.status.played()),
            replay_total = self.replay.as_ref().map(|r| r.status.total()),
            action = "observe",
            "application health summary"
        );
        if avg > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_SLOW_FRAMES",
                frame_avg_ms = avg,
                threshold_ms = metrics::SLOW_FRAME_MS,
                heatmap_enabled = book.enabled,
                heatmap_projection_ms = book.projection_ms,
                heatmap_cells = book.projection_cells,
                action = "inspect_render_budget",
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = lag
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "APP_HIGH_TRADE_LAG",
                feed_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                action = "inspect_trade_connection",
                "high feed lag: trades are arriving well behind their timestamps"
            );
        }
        if let Some(l) = book_lag
            && book.enabled
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_HIGH_ARRIVAL",
                symbol = self.symbol.as_str(),
                book_arrival_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = book.status,
                action = "inspect_depth_connection",
                // Arrival, not age: this is how late the newest accepted
                // depth event was when it reached us, an observation frozen
                // at that moment. A book that stops updating keeps its last
                // figure — the tape-age readout is what catches that.
                "order-book events are arriving late"
            );
        }
        if book.dropped_cells > 0
            || book.dropped_aggressions > 0
            || book.dropped_liquidity_events > 0
        {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_PROJECTION_CAPPED",
                symbol = self.symbol.as_str(),
                dropped_cells = book.dropped_cells,
                dropped_aggressions = book.dropped_aggressions,
                dropped_liquidity_events = book.dropped_liquidity_events,
                action = "increase_grouping_or_reduce_retention",
                "heatmap primitive cap was reached"
            );
        }

        self.trades_since_summary = 0;
        self.tape_mut().reset_summary_counters();
        self.last_summary = now;
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
    fn side_note(&self) -> Option<(String, Option<String>)> {
        if let Some(link) = &self.replay {
            Some((
                match link.session.header.side_source.as_deref() {
                    Some(source) => format!("side: {source}"),
                    None => "side: not recorded".to_owned(),
                },
                None,
            ))
        } else if self.config.provider_of(&self.active.0).is_some()
            && !self.capabilities().traded_volume
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
            self.config
                .side_note(&self.active.0)
                .map(|note| (note.to_owned(), None))
        }
    }

    /// Everything the status bar reports this frame.
    ///
    /// Provenance (venue, symbol, transport, side honesty) is the market's and
    /// reads from the window; the content section — spec, bar counts, forming
    /// bar, whether the view follows live — is the *focused pane's* (§11), so
    /// the bar always describes the chart the user is working in.
    fn status_model(&self) -> statusbar::StatusModel {
        let pane = self.focused_pane();
        let bars = pane.state.bars();
        let (backfilled, live) = match pane.state.backfill_boundary() {
            Some(boundary) => (boundary, bars.len().saturating_sub(boundary)),
            None => (0, bars.len()),
        };
        let note = self.side_note();
        statusbar::StatusModel {
            venue: if self.replay.is_some() {
                "recording".to_owned()
            } else {
                self.feed_display_name()
            },
            symbol: self.symbol.clone(),
            replay: self.replay.as_ref().map(|link| statusbar::ReplayFigures {
                speed: link.status.speed(),
                progress: link.status.progress(),
            }),
            connection: self.feed_connection,
            feed_arrival_ms: self.trade_arrival_ms(),
            tape_age_ms: self.tape_age_at(metrics::wall_clock_ms()),
            spec_summary: pane.state.spec().summary(),
            bar_progress: pane
                .state
                .progress()
                .map(|(progress, unit)| fmt_progress(&progress, unit)),
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: note.clone().map(|(label, _)| label),
            side_detail: note.and_then(|(_, detail)| detail),
            follows_live: pane.viewport.follows_live(),
            price_auto: pane.price_view.is_auto(),
            live_trades: self.live_trades,
            fps: self.frames.fps(),
            frame_avg_ms: self.frames.avg_ms(),
            frame_cpu_ms: self.cpu_frames.avg_ms(),
            show_perf: self.show_perf,
        }
    }
}

/// Opens the Market Replay browser (§10).
const REPLAY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
/// Shows/hides the panels dock (§10).
const DOCK_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);
/// Height of the menu bar, in pixels (§5 zone 1).
const MENU_BAR_HEIGHT: f32 = 28.0;

impl QuantickApp {
    /// The window's menu bar (§10): shallow menus for discoverability and
    /// shortcuts, never the only path to anything.
    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&REPLAY_SHORTCUT)) {
            self.replay_view.open_browser();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&DOCK_SHORTCUT)) {
            self.dock.toggle_visible();
        }

        egui::TopBottomPanel::top("menu_bar")
            .exact_height(MENU_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        ui.menu_button("Layout", |ui| {
                            for (layout, label) in [
                                (CanvasLayout::Single, "Single"),
                                (CanvasLayout::TimeAndFlow, "Time + Flow"),
                            ] {
                                if ui.selectable_label(self.layout == layout, label).clicked() {
                                    self.set_layout(layout);
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new("Market Replay…")
                                    .shortcut_text(ui.ctx().format_shortcut(&REPLAY_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.replay_view.open_browser();
                            ui.close_menu();
                        }
                        if self.replay.is_some() && ui.button("Close Replay").clicked() {
                            self.close_replay();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        let panels_label = if self.dock.visible() {
                            "Hide panels"
                        } else {
                            "Show panels"
                        };
                        if ui
                            .add(
                                egui::Button::new(panels_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&DOCK_SHORTCUT)),
                            )
                            .clicked()
                        {
                            self.dock.toggle_visible();
                            ui.close_menu();
                        }
                        let toolbox_label = if self.toolrail.visible() {
                            "Hide drawing toolbox"
                        } else {
                            "Show drawing toolbox"
                        };
                        if ui.button(toolbox_label).clicked() {
                            self.toolrail.toggle_visible();
                            ui.close_menu();
                        }
                        for (tab, label) in [
                            (DockTab::L2, "L2 settings"),
                            (DockTab::Bubbles, "Bubble settings"),
                            (DockTab::Session, "Session"),
                        ] {
                            if ui.button(label).clicked() {
                                self.dock.open_tab(tab);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_perf, "Perf readings")
                            .on_hover_text("fps, frame time and trade count on the status bar");
                        ui.separator();
                        ui.menu_button("Timezone", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for tz in TzOffset::ALL {
                                        if ui.selectable_label(self.tz == tz, tz.label()).clicked()
                                        {
                                            self.tz = tz;
                                            ui.close_menu();
                                        }
                                    }
                                });
                        });
                    });
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Appearance…").clicked() {
                            self.show_style = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Replay file format…").clicked() {
                            self.replay_view.open_format_help();
                            ui.close_menu();
                        }
                    });
                });
            });
    }

    /// One delete command for every trigger (inspector button, keyboard,
    /// manager). A locked object raises the confirmation next to the trigger
    /// instead of deleting; a landed delete raises the Undo toast.
    fn request_delete_selected(&mut self, now: Instant) {
        match self.focused_pane_mut().drawings.delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.drawing_delete_confirm = false;
                self.drawing_toast = Some(DrawingToast {
                    message: "Drawing deleted.",
                    shown_at: now,
                    offers_undo: true,
                });
            }
            DeleteOutcome::NeedsConfirmation => self.drawing_delete_confirm = true,
            DeleteOutcome::NothingSelected => {}
        }
    }

    /// Keyboard grammar for drawings. Any focused widget wins: while an input
    /// owns the keyboard, chart shortcuts stay suspended.
    fn handle_drawing_keys(&mut self, ctx: &egui::Context, now: Instant) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        struct DrawingKeys {
            escape: bool,
            delete: bool,
            backspace: bool,
            undo: bool,
            redo: bool,
            lock: bool,
            hide: bool,
            duplicate: bool,
            nudge_bars: f32,
            nudge_px: f32,
        }
        let keys = ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            // Shift turns a nudge into ten steps (UX spec).
            let step = if shift { 10.0 } else { 1.0 };
            let horizontal = f32::from(input.key_pressed(egui::Key::ArrowRight))
                - f32::from(input.key_pressed(egui::Key::ArrowLeft));
            let vertical = f32::from(input.key_pressed(egui::Key::ArrowUp))
                - f32::from(input.key_pressed(egui::Key::ArrowDown));
            DrawingKeys {
                escape: input.key_pressed(egui::Key::Escape),
                delete: input.key_pressed(egui::Key::Delete),
                backspace: input.key_pressed(egui::Key::Backspace),
                undo: command && !shift && input.key_pressed(egui::Key::Z),
                redo: (command && input.key_pressed(egui::Key::Y))
                    || (command && shift && input.key_pressed(egui::Key::Z)),
                lock: alt && input.key_pressed(egui::Key::L),
                hide: alt && input.key_pressed(egui::Key::H),
                duplicate: command && input.key_pressed(egui::Key::D),
                nudge_bars: horizontal * step,
                nudge_px: vertical * step,
            }
        });
        // The escape stack: pending confirmation → draft → selection →
        // Pointer, one layer per press.
        if keys.escape {
            if self.drawing_delete_confirm {
                self.drawing_delete_confirm = false;
            } else if self.focused_pane().drawings.draft().is_some() {
                self.focused_pane_mut().drawings.cancel_draft();
                self.toolrail.arm(Tool::Pointer);
            } else if self.focused_pane().drawings.selected().is_some() {
                self.focused_pane_mut().drawings.select(None);
            } else {
                self.toolrail.arm(Tool::Pointer);
            }
        }
        if self.focused_pane().drawings.draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                self.focused_pane_mut().drawings.remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(now);
        }
        if keys.undo {
            self.focused_pane_mut().drawings.undo();
        }
        if keys.redo {
            self.focused_pane_mut().drawings.redo();
        }
        if keys.lock
            && let Some(index) = self.focused_pane().drawings.selected()
        {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = self.focused_pane().drawings.selected()
        {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if keys.duplicate {
            self.flow_pane
                .drawings
                .duplicate_selected(DUPLICATE_OFFSET_BARS);
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0)
            && self.focused_pane().drawings.selected().is_some()
        {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of price per
            // vertical step. Each press lands as one undo entry.
            let price_per_px = self.focused_pane().last_auto_range.map_or(0.0, |auto| {
                let (lo, hi) = self.focused_pane().price_view.resolve(auto);
                (hi - lo) / f64::from(self.focused_pane().last_chart_height.max(1.0))
            });
            self.focused_pane_mut().drawings.begin_gesture();
            self.flow_pane
                .drawings
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            self.focused_pane_mut().drawings.commit_gesture();
        }
    }

    /// Commit a pending inspector edit gesture as one undo entry.
    fn commit_inspector_gesture(&mut self) {
        if let Some((index, before)) = self.inspector_edit_baseline.take() {
            self.focused_pane_mut()
                .drawings
                .record_edit_of(index, before);
        }
    }

    /// The delete toast: visible for [`TOAST_UNDO_MS`], with an Undo button
    /// driving the same history as Ctrl+Z.
    fn draw_drawing_toast(&mut self, ctx: &egui::Context, now: Instant) {
        let Some(toast) = &self.drawing_toast else {
            return;
        };
        if now.saturating_duration_since(toast.shown_at) >= Duration::from_millis(TOAST_UNDO_MS) {
            self.drawing_toast = None;
            return;
        }
        let message = toast.message;
        let offers_undo = toast.offers_undo;
        let mut undo_clicked = false;
        egui::Area::new(egui::Id::new("drawing_toast"))
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -TOAST_BOTTOM_MARGIN_PX),
            )
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(theme::TAG_BG)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(6.0_f32)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(message);
                            if offers_undo && ui.button("Undo").clicked() {
                                undo_clicked = true;
                            }
                        });
                    });
            });
        if undo_clicked {
            self.focused_pane_mut().drawings.undo();
            self.drawing_toast = None;
        }
    }

    /// Everything the inspector shows for the selected object, shared by the
    /// floating window and the pinned dock panel. Sections are driven by the
    /// tool's capabilities — an unsupported property is absent, not disabled.
    fn drawing_inspector_body(&mut self, ui: &mut egui::Ui, index: usize) -> InspectorActions {
        let mut actions = InspectorActions::default();
        let drawing = &self.focused_pane().drawings.items()[index];
        let tool = drawing.tool;
        let locked = drawing.locked;
        let hidden = drawing.hidden;
        let show_confirm = self.drawing_delete_confirm && locked;

        // Header: object identity plus the view controls.
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tool.name()).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Close").clicked() {
                    actions.close = true;
                }
                let pin_label = if self.inspector_pinned {
                    "Unpin"
                } else {
                    "Pin"
                };
                let pin = ui
                    .small_button(pin_label)
                    .on_hover_text("Dock the inspector at the side of the chart");
                #[cfg(test)]
                {
                    self.inspector_pin_rect = Some(pin.rect);
                }
                if pin.clicked() {
                    actions.toggle_pin = true;
                }
                let eye_label = if hidden { "Show" } else { "Hide" };
                if ui.small_button(eye_label).clicked() {
                    actions.toggle_hidden = true;
                }
            });
        });

        // The always-visible textual actions (UX spec: never glyph-only,
        // never behind a scroll).
        let intent = drawings::action_bar::draw(ui, locked);
        actions.toggle_lock |= intent.toggle_lock;
        actions.delete |= intent.delete;

        if locked {
            ui.label(
                egui::RichText::new(
                    "Locked - protected from accidental moves. Style stays editable.",
                )
                .small(),
            );
        }
        if hidden {
            ui.label(egui::RichText::new("Hidden - Show brings it back.").small());
        }
        if show_confirm {
            ui.separator();
            ui.label("Delete locked drawing?");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    actions.cancel_delete = true;
                }
                if ui.button("Delete anyway").clicked() {
                    actions.force_delete = true;
                }
            });
        }
        ui.separator();

        if self.inspector_tab == InspectorTab::Extra && tool.extra_tab().is_none() {
            // The previous selection had an extra tab; this tool brings none.
            self.inspector_tab = InspectorTab::Style;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.inspector_tab, InspectorTab::Style, "Style");
            // A tool that brings its own tab (the Fib level editor) mounts it
            // here by name; the central code never learns what is inside.
            if let Some(extra) = tool.extra_tab() {
                ui.selectable_value(&mut self.inspector_tab, InspectorTab::Extra, extra);
            }
            ui.selectable_value(
                &mut self.inspector_tab,
                InspectorTab::Coordinates,
                "Coordinates",
            );
        });
        ui.separator();

        let tab = self.inspector_tab;
        let price_speed = self.focused_pane().last_auto_range.map_or(1.0, |(lo, hi)| {
            ((hi - lo) / PRICE_DRAG_STEPS).abs().max(1e-9)
        });
        let side = self.focused_side();
        let Self {
            flow_pane,
            time_pane,
            drawing_presets,
            ..
        } = self;
        let drawings = match side {
            PaneSide::Time => time_pane
                .as_mut()
                .map_or(&mut flow_pane.drawings, |pane| &mut pane.drawings),
            PaneSide::Flow => &mut flow_pane.drawings,
        };
        let Some(drawing) = drawings.selected_mut() else {
            return actions;
        };
        match tab {
            InspectorTab::Extra => {
                actions.edited |= tool.draw_extra_tab(ui, drawing, drawing_presets);
            }
            InspectorTab::Style => {
                ui.label("Style");
                actions.edited |= ui
                    .color_edit_button_srgba(&mut drawing.style.color)
                    .changed();
                actions.edited |= ui
                    .add(
                        egui::Slider::new(
                            &mut drawing.style.width_px,
                            MIN_DRAWING_WIDTH_PX..=MAX_DRAWING_WIDTH_PX,
                        )
                        .text("line width (px)"),
                    )
                    .changed();
                if tool.supports_fill() {
                    actions.edited |= ui
                        .add(
                            egui::Slider::new(
                                &mut drawing.style.fill_alpha,
                                0..=MAX_DRAWING_FILL_ALPHA,
                            )
                            .text("fill opacity"),
                        )
                        .changed();
                }
            }
            InspectorTab::Coordinates => {
                // Geometry through numbers: bar index and price per anchor,
                // the same canonical coordinates drags write. Locked blocks
                // geometry here exactly as it does on the canvas.
                const ANCHOR_LABELS: [&str; 4] = ["A", "B", "C", "D"];
                ui.add_enabled_ui(!locked, |ui| {
                    for (point_index, point) in drawing.points.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(ANCHOR_LABELS.get(point_index).copied().unwrap_or("?"));
                            actions.edited |= ui
                                .add(
                                    egui::DragValue::new(&mut point.bar)
                                        .speed(BAR_DRAG_SPEED)
                                        .prefix("bar "),
                                )
                                .changed();
                            actions.edited |= ui
                                .add(egui::DragValue::new(&mut point.price).speed(price_speed))
                                .changed();
                        });
                    }
                });
                if locked {
                    ui.label(
                        egui::RichText::new("Unlock the drawing to edit its coordinates.").small(),
                    );
                }
            }
        }
        actions
    }

    /// Apply what the inspector body asked for, with the shared undo
    /// coalescing: the pre-edit object is captured at the first change and
    /// committed once pointer and keyboard let go.
    fn apply_inspector_actions(
        &mut self,
        ctx: &egui::Context,
        actions: InspectorActions,
        index: usize,
        before: drawings::Drawing,
        now: Instant,
    ) {
        if actions.edited && self.inspector_edit_baseline.is_none() {
            self.inspector_edit_baseline = Some((index, before));
        }
        let gesture_settled = ctx.input(|input| !input.pointer.any_down())
            && ctx.memory(|memory| memory.focused().is_none());
        if gesture_settled {
            self.commit_inspector_gesture();
        }
        if actions.toggle_hidden {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if actions.toggle_lock {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_selected_locked(!locked);
            self.drawing_delete_confirm = false;
        }
        if actions.toggle_pin {
            self.inspector_pinned = !self.inspector_pinned;
        }
        if actions.delete {
            self.request_delete_selected(now);
        }
        if actions.cancel_delete {
            self.drawing_delete_confirm = false;
        }
        if actions.force_delete {
            self.drawing_delete_confirm = false;
            if self.focused_pane_mut().drawings.delete_selected(true) == DeleteOutcome::Deleted {
                self.drawing_toast = Some(DrawingToast {
                    message: "Drawing deleted.",
                    shown_at: now,
                    offers_undo: true,
                });
            }
        }
        if actions.close {
            self.focused_pane_mut().drawings.select(None);
            self.drawing_delete_confirm = false;
        }
    }

    /// Where a freshly opened floating inspector should sit: 12 px right of
    /// the object's bounding box, falling back to left, below and above, and
    /// always clamped into the chart pane — which already excludes the price
    /// and time axes, so the popup can never cover either or leave the view.
    fn inspector_target_position(&self, ctx: &egui::Context, index: usize) -> Option<egui::Pos2> {
        let chart = self.focused_pane().last_chart_area?;
        let total = self.focused_pane().slots();
        let (auto_lo, auto_hi) = self.focused_pane().last_auto_range?;
        let (lo, hi) = self.focused_pane().price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.focused_pane().last_chart_top,
            self.focused_pane().last_chart_top + self.focused_pane().last_chart_height,
        );
        let history_right = self
            .focused_pane()
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let drawing = self.focused_pane().drawings.items().get(index)?;
        let points = self
            .flow_pane
            .projected_drawing_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        let bbox = bbox.expand(DRAWING_ANCHOR_RADIUS_PX);
        let size = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .map_or(
                egui::vec2(INSPECTOR_DEFAULT_WIDTH_PX, INSPECTOR_FALLBACK_HEIGHT_PX),
                |rect| rect.size(),
            );
        let gap = INSPECTOR_OBJECT_GAP_PX;
        let candidates = [
            egui::pos2(bbox.right() + gap, bbox.top()),
            egui::pos2(bbox.left() - gap - size.x, bbox.top()),
            egui::pos2(bbox.left(), bbox.bottom() + gap),
            egui::pos2(bbox.left(), bbox.top() - gap - size.y),
        ];
        let fits = |position: egui::Pos2| {
            let rect = egui::Rect::from_min_size(position, size);
            chart.contains_rect(rect) && !rect.intersects(bbox)
        };
        let chosen = candidates
            .into_iter()
            .find(|position| fits(*position))
            .unwrap_or(candidates[0]);
        let max_x = (chart.right() - size.x).max(chart.left());
        let max_y = (chart.bottom() - size.y).max(chart.top());
        Some(egui::pos2(
            chosen.x.clamp(chart.left(), max_x),
            chosen.y.clamp(chart.top(), max_y),
        ))
    }

    /// Shared prologue of both inspector hosts. Returns the selection and its
    /// pre-frame copy, or cleans up when nothing is selected.
    fn inspector_selection(&mut self) -> Option<(usize, drawings::Drawing)> {
        let Some(index) = self.focused_pane().drawings.selected() else {
            self.drawing_delete_confirm = false;
            self.commit_inspector_gesture();
            self.inspector_last_selection = None;
            return None;
        };
        // An edit gesture that outlived its object's selection commits now.
        if self
            .inspector_edit_baseline
            .as_ref()
            .is_some_and(|(baseline_index, _)| *baseline_index != index)
        {
            self.commit_inspector_gesture();
        }
        Some((index, self.focused_pane().drawings.items()[index].clone()))
    }

    /// The pinned inspector: a dock panel at the chart's side. Declared with
    /// the chrome, before the central canvas, so the canvas pays its width.
    fn draw_drawing_inspector_panel(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.inspector_pinned {
            return;
        }
        let Some((index, before)) = self.inspector_selection() else {
            return;
        };
        let mut actions = InspectorActions::default();
        egui::SidePanel::right("drawing_inspector_panel")
            .resizable(true)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .width_range(INSPECTOR_MIN_WIDTH_PX..=INSPECTOR_MAX_WIDTH_PX)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    actions = self.drawing_inspector_body(ui, index);
                });
            });
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// The selected object's floating inspector. Non-modal by contract: it
    /// never captures the whole canvas and never blocks moving the geometry
    /// underneath it. Opens beside the selection; once the user moves it, the
    /// manual position wins for the rest of the session.
    fn draw_drawing_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if self.inspector_pinned {
            // The pinned panel already drew (and cleaned up) this frame.
            return;
        }
        let Some((index, before)) = self.inspector_selection() else {
            return;
        };
        let selection_changed = self.inspector_last_selection != Some(index);
        self.inspector_last_selection = Some(index);
        let mut actions = InspectorActions::default();
        let mut open = true;
        let inspector_interactable = !self.focused_pane().drawing_drag.is_active();
        // The level editor earns the wider default the spec reserves for it.
        let default_width = if before.tool.extra_tab().is_some() {
            INSPECTOR_LEVELS_WIDTH_PX
        } else {
            INSPECTOR_DEFAULT_WIDTH_PX
        };
        let mut window = egui::Window::new(before.tool.settings_title())
            .id(egui::Id::new("drawing_inspector"))
            .open(&mut open)
            .default_pos(DRAWING_INSPECTOR_DEFAULT_POSITION)
            .default_width(default_width)
            .min_width(INSPECTOR_MIN_WIDTH_PX)
            .max_width(INSPECTOR_MAX_WIDTH_PX)
            .collapsible(false)
            .movable(true)
            .interactable(inspector_interactable)
            .resizable(true);
        if selection_changed
            && !self.inspector_moved
            && let Some(position) = self.inspector_target_position(ctx, index)
        {
            window = window.current_pos(position);
        }
        let response = window.show(ctx, |ui| self.drawing_inspector_body(ui, index));
        if let Some(response) = &response {
            if response.response.dragged() {
                self.inspector_moved = true;
            }
            if let Some(inner) = response.inner {
                actions = inner;
            }
        }
        if !open {
            actions.close = true;
        }
        self.apply_inspector_actions(ctx, actions, index, before, now);
    }

    /// The object manager: a non-modal list of every drawing with the named
    /// per-object actions. It sends the same store commands as the inspector
    /// and the keyboard — nothing here re-implements lock or delete rules.
    fn draw_drawing_manager(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.drawing_manager_open {
            return;
        }
        #[cfg(test)]
        self.manager_action_rects.clear();
        let mut open = true;
        let mut select_row: Option<usize> = None;
        let mut eye_row: Option<usize> = None;
        let mut lock_row: Option<usize> = None;
        let mut front_row: Option<usize> = None;
        let mut delete_row: Option<usize> = None;
        let mut show_all = false;
        let mut unlock_all = false;
        egui::Window::new("Drawn objects")
            .id(egui::Id::new("drawing_manager"))
            .open(&mut open)
            .default_pos(DRAWING_MANAGER_DEFAULT_POSITION)
            .default_width(INSPECTOR_DEFAULT_WIDTH_PX)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                let count = self.focused_pane().drawings.items().len();
                if count == 0 {
                    ui.label("No drawings yet.");
                }
                // Walked in reverse: the manager lists top-most first, the
                // same order hit-testing resolves overlap.
                for index in (0..count).rev() {
                    let drawing = &self.focused_pane().drawings.items()[index];
                    let selected = self.focused_pane().drawings.selected() == Some(index);
                    let locked = drawing.locked;
                    let hidden = drawing.hidden;
                    let name = drawing.tool.name();
                    ui.horizontal(|ui| {
                        let mut label = egui::RichText::new(format!("{} {}", name, index + 1));
                        if hidden {
                            label = label.weak();
                        }
                        if ui.selectable_label(selected, label).clicked() {
                            select_row = Some(index);
                        }
                        if locked {
                            ui.label(egui::RichText::new("locked").small());
                        }
                        if hidden {
                            ui.label(egui::RichText::new("hidden").small());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let delete = ui.small_button("Delete");
                            #[cfg(test)]
                            self.manager_action_rects
                                .push((index, "Delete", delete.rect));
                            if delete.clicked() {
                                delete_row = Some(index);
                            }
                            let front = ui.small_button("Front");
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Front", front.rect));
                            if front.clicked() {
                                front_row = Some(index);
                            }
                            let lock = ui.small_button(if locked { "Unlock" } else { "Lock" });
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Lock", lock.rect));
                            if lock.clicked() {
                                lock_row = Some(index);
                            }
                            let eye = ui.small_button(if hidden { "Show" } else { "Hide" });
                            #[cfg(test)]
                            self.manager_action_rects.push((index, "Eye", eye.rect));
                            if eye.clicked() {
                                eye_row = Some(index);
                            }
                        });
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Show all").clicked() {
                        show_all = true;
                    }
                    if ui.button("Unlock all").clicked() {
                        unlock_all = true;
                    }
                });
            });
        self.drawing_manager_open = open;
        if let Some(index) = select_row {
            self.focused_pane_mut().drawings.select(Some(index));
            // Centre the viewport on the object's bar span.
            let slots = self.focused_pane().slots();
            if let Some(chart) = self.focused_pane().last_chart_area {
                let points = &self.focused_pane().drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    self.focused_pane_mut()
                        .viewport
                        .center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        if let Some(index) = eye_row {
            let hidden = self.focused_pane().drawings.items()[index].hidden;
            self.focused_pane_mut()
                .drawings
                .set_hidden_at(index, !hidden);
        }
        if let Some(index) = lock_row {
            let locked = self.focused_pane().drawings.items()[index].locked;
            self.focused_pane_mut()
                .drawings
                .set_locked_at(index, !locked);
        }
        if let Some(index) = front_row {
            self.focused_pane_mut().drawings.bring_to_front(index);
        }
        if let Some(index) = delete_row {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            self.focused_pane_mut().drawings.select(Some(index));
            self.request_delete_selected(now);
        }
        if show_all {
            self.focused_pane_mut().drawings.set_all_hidden(false);
        }
        if unlock_all {
            self.focused_pane_mut().drawings.set_all_locked(false);
        }
    }

    /// Carry out what the replay interface asked for.
    fn apply_replay_action(&mut self, action: ReplayAction) {
        match action {
            ReplayAction::Open(request) => self.open_replay(*request),
            ReplayAction::Close => self.close_replay(),
            ReplayAction::Control(control) => {
                // A dropped transport click is not worth a retry queue: the
                // worker drains commands every 8 ms, so a full channel means
                // the click was already superseded.
                if let Err(e) = self.commands.try_send(FeedCommand::Replay(control)) {
                    tracing::debug!(
                        target: "quantick::app",
                        event_code = "REPLAY_COMMAND_DROPPED",
                        reason = %e,
                        "transport command not queued"
                    );
                }
            }
        }
    }

    /// Make a recorded session the chart's source, replacing whatever feed is
    /// running. The live selection is untouched, so closing the replay comes
    /// back to exactly the feed and symbol that were streaming before.
    fn open_replay(&mut self, request: crate::feed::ReplayRequest) {
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

        let handle = feed::spawn(feed::FeedSource::Replay(Box::new(request)), &self.config);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;

        if let Some(link) = &self.replay {
            self.symbol = link.symbol().to_string();
        }
        // Depth is not in a recording; the toggle is disabled by capability,
        // and the view must not keep drawing a book from the live feed.
        let generation = self.next_book_generation();
        self.tape_mut().set_enabled(false, generation);
        self.reset_market_state();
    }

    /// Leave replay and put the live feed back.
    fn close_replay(&mut self) {
        if self.replay.take().is_none() {
            return;
        }
        let (feed_id, symbol) = self.active.clone();
        self.feed_id = feed_id;
        self.symbol = symbol;
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_CLOSED",
            feed = %self.feed_id,
            symbol = %self.symbol,
            action = "respawn_live_feed",
            "leaving market replay"
        );

        let Some(provider) = self.config.provider_of(&self.feed_id) else {
            // The configuration changed under us; there is nothing to go back
            // to, so the chart stays as it is rather than dying.
            self.reset_market_state();
            return;
        };
        let handle = feed::spawn_live(provider, &self.symbol, &self.config);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;
        self.reset_market_state();
    }

    /// Start the current feed over, from the card that asked the user to fix
    /// something.
    ///
    /// The same respawn a feed switch performs, minus the switch: after the
    /// terminal is opened or the package installed, the way back has to be one
    /// click, not a restart of quantick. A replay owns the chart while it
    /// plays and has nothing to retry.
    fn restart_feed(&mut self) {
        if self.replay.is_some() {
            return;
        }
        let Some(provider) = self.config.provider_of(&self.feed_id) else {
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
        let handle = feed::spawn_live(provider, &self.symbol, &self.config);
        self.events = handle.events;
        self.book_events = handle.book_events;
        self.notices = handle.notices;
        self.feed_capabilities = handle.capabilities;
        self.notice = FeedNotice::Clear;
        self.feed_connection = FeedConnectionState::Connecting;
        self.commands = handle.commands;
        self.replay = handle.replay;
        self.book_channel_closed_reported = false;
        self.reset_market_state();
        // The live market is back and it can stream depth again; start
        // recording immediately rather than waiting for the map to be opened.
        self.ensure_book_capture();
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu_frames.record(cpu * 1000.0);
        }
        self.draw_frame(ctx, Instant::now());
    }
}

impl QuantickApp {
    /// Lay the canvas out and run every visible pane through it (§11).
    ///
    /// Single is one pane over the whole area — the same code path the split
    /// takes, with one pane in it, so the default layout can never drift from
    /// the split one.
    fn draw_canvas(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        let split = self.layout == CanvasLayout::TimeAndFlow && self.time_pane.is_some();
        // Unsplit, the flow pane is handed the whole canvas and the rest of
        // this reduces to nothing: no divider, no header, no focus rule.
        let (time_area, divider, flow_area) = if split {
            let (time, divider, flow) = split_canvas(area, self.split_fraction);
            (Some(time), Some(divider), flow)
        } else {
            (None, None, area)
        };

        let time_chart = time_area.map(|time_area| {
            // Focus before input, so the click that focuses a pane is also the
            // click that pane goes on to handle.
            self.focus_from_pointer(ui, time_area, flow_area);
            let (header, chart) = split_time_pane(time_area);
            // The time pane's own timeframe selector (§11): its BARS group,
            // beside the toolbar's, which keeps governing the flow pane.
            let mut interval_ms = self.pane(PaneSide::Time).time_interval_ms;
            let header_layout = time_header::draw(ui, header, &mut interval_ms);
            #[cfg(test)]
            {
                self.time_header_chips = header_layout.chips;
            }
            if header_layout.changed {
                let pane = self.pane_mut(PaneSide::Time);
                pane.kind = BarKind::Time;
                pane.time_interval_ms = interval_ms;
            }
            chart
        });

        {
            let Self {
                flow_pane,
                time_pane,
                toolrail,
                drawing_presets,
                style,
                tz,
                symbol,
                ..
            } = self;
            let mut chrome = PaneChrome {
                toolrail,
                presets: drawing_presets,
                style,
                tz: *tz,
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

impl QuantickApp {
    /// One frame of the application: drain, lay out the chrome, draw the
    /// chart.
    ///
    /// Everything `update` does that is not eframe's own bookkeeping, so a
    /// test can run a real frame against a headless [`egui::Context`] and read
    /// what was painted — the only honest way to assert that a chart is on
    /// screen rather than a blank rectangle.
    fn draw_frame(&mut self, ctx: &egui::Context, now: Instant) {
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_feed();
        // Apply each pane's indicator deltas before its draw reads columns.
        for pane in self.panes_mut() {
            pane.apply_indicator_events();
        }
        self.drain_book_feed();
        self.drain_notices();
        // Heartbeat for the recorder. The lifecycle calls below already start
        // it at every point that knows the market changed; this one makes
        // "always recording" true by construction, so a start command lost to
        // a momentarily full channel heals on the next frame instead of
        // leaving the session silently unrecorded. Free while it is running:
        // one bool read and an early return.
        self.ensure_book_capture();
        self.maybe_emit_summary(now);

        let bg = pane::background_color(&self.style);
        // Rail shortcuts first: Esc/1/2 must be read before any widget can
        // claim the keyboard this frame.
        self.toolrail.handle_keys(ctx);
        self.handle_drawing_keys(ctx, now);
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the corner toolbox and right dock.
        // The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_indicator_settings(ctx);
        self.poll_script_files();
        self.maintain_indicator_state();
        let status = self.status_model();
        statusbar::draw(ctx, &status, &mut self.tz);
        // The browser window and, while a session plays, the transport bar.
        if let Some(action) = self.replay_view.draw(ctx, self.replay.as_ref()) {
            self.apply_replay_action(action);
        }
        {
            // The focused pane's objects: the toolbox lists and manages what a
            // click on the canvas would act on.
            let side = self.focused_side();
            let Self {
                toolrail,
                flow_pane,
                time_pane,
                drawing_manager_open,
                ..
            } = self;
            let drawings = match side {
                PaneSide::Time => time_pane
                    .as_mut()
                    .map_or(&mut flow_pane.drawings, |pane| &mut pane.drawings),
                PaneSide::Flow => &mut flow_pane.drawings,
            };
            toolrail.draw(ctx, drawings, drawing_manager_open);
        }
        let dock_response = {
            let Self {
                dock,
                flow_pane,
                replay_view,
                replay,
                ..
            } = self;
            let orderflow = flow_pane
                .orderflow
                .as_mut()
                .expect("the flow pane is built with a tape and never drops it");
            dock.draw(
                ctx,
                &mut DockEnv {
                    orderflow,
                    replay_view,
                    replay: replay.as_ref(),
                },
            )
        };
        if dock_response.restart_book_capture {
            self.restart_book_capture();
        }
        if let Some(action) = dock_response.replay_action {
            self.apply_replay_action(action);
        }
        // The pinned inspector is chrome: declared before the central canvas
        // so the chart pays its width, exactly like the dock.
        self.draw_drawing_inspector_panel(ctx, now);
        // Respawn the feed if the feed/symbol selection changed (resets the
        // chart), then apply any bar-type change (no-op if unchanged).
        self.maybe_switch_feed();
        self.apply_spec_changes();
        self.draw_style_panel(ctx, now);
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        self.loading
            .set_active(LoadingTask::ReplaySession, self.replay_view.is_loading());
        self.loading
            .set_active(LoadingTask::BookSync, self.tape().is_syncing());

        let mut notice_action = notice_card::NoticeAction::None;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.draw_canvas(ui, area);
                loading::overlay(ui, area, &self.loading);
                if notice_card::should_draw(&self.notice, self.flow_pane.state.bars().len()) {
                    notice_action = notice_card::draw(ui, area, &self.notice);
                }
            });
        // Floating drawing controls must be registered after the opaque
        // central canvas so they stay in front of the chart.
        self.draw_drawing_inspector(ctx, now);
        self.draw_drawing_manager(ctx, now);
        self.draw_drawing_toast(ctx, now);
        if notice_action == notice_card::NoticeAction::Retry {
            self.restart_feed();
        }
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use rust_decimal::Decimal;

    use crate::config::{FeedConfig, ProviderKind};
    use crate::drawings::{ChartPoint, PresetHost};

    /// Time pane left, flow pane right, on a divider that costs both of them
    /// nothing but its own width (§11).
    #[test]
    fn the_canvas_splits_time_left_flow_right_at_the_divider() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let (time, divider, flow) = split_canvas(area, 0.5);
        assert!(time.right() <= flow.left(), "time is the left pane");
        assert_eq!(time.right(), divider.left());
        assert_eq!(divider.right(), flow.left());
        assert_eq!(divider.width(), CANVAS_DIVIDER_PX);
        assert_eq!(time.left(), area.left());
        assert_eq!(flow.right(), area.right());
        assert_eq!(
            time.width() + divider.width() + flow.width(),
            area.width(),
            "the split spends the canvas exactly once"
        );
        // Both panes keep the full height: the split is vertical only.
        assert_eq!(time.top(), area.top());
        assert_eq!(flow.bottom(), area.bottom());
    }

    /// A pane too narrow to read is not a layout, it is a lost pane. §11
    /// promises each of them a quarter of the canvas, whatever the drag says.
    #[test]
    fn neither_pane_can_be_dragged_below_a_quarter_of_the_canvas() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        for (asked, expected) in [
            (-3.0, 0.25),
            (0.0, 0.25),
            (0.1, 0.25),
            (0.9, 0.75),
            (7.0, 0.75),
        ] {
            let (time, divider, flow) = split_canvas(area, asked);
            // The divider sits *on* the split, so compare where the split is
            // rather than a pane width that has half a divider taken out of it.
            let split = (divider.center().x - area.left()) / area.width();
            assert!(
                (split - expected).abs() < 1e-3,
                "asking for {asked} must clamp to {expected}, got {split}"
            );
            let floor = area.width() * MIN_PANE_FRACTION - CANVAS_DIVIDER_PX;
            assert!(time.width() >= floor, "the time pane keeps its quarter");
            assert!(flow.width() >= floor, "and so does the flow pane");
        }
    }

    /// The header is a strip carved off the pane, not an overlay: the selector
    /// must never be painted across market data.
    #[test]
    fn the_time_pane_header_costs_the_chart_its_own_height() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 600.0));
        let (header, chart) = split_time_pane(area);
        assert_eq!(header.height(), time_header::HEIGHT_PX);
        assert_eq!(header.bottom(), chart.top(), "no gap, no overlap");
        assert_eq!(chart.bottom(), area.bottom());
        assert_eq!(header.width(), area.width());
    }

    #[test]
    fn the_live_strip_carves_between_chart_and_gutter_only_when_shown() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let off = plot_split(area, 0.0, 0);
        assert!(off.live_strip.is_none());
        assert_eq!(off.chart.right(), off.price_gutter.left());

        let on = plot_split(area, crate::live_strip::LIVE_STRIP_WIDTH_PX, 0);
        let strip = on.live_strip.expect("strip rect");
        assert_eq!(on.chart.right(), strip.left());
        assert_eq!(strip.right(), on.price_gutter.left());
        assert_eq!(strip.width(), crate::live_strip::LIVE_STRIP_WIDTH_PX);
        // The strip pays with the chart's pixels: the gutter stays put, and
        // the time axis keeps spanning exactly the chart body.
        assert_eq!(on.price_gutter, off.price_gutter);
        assert_eq!(
            on.chart.width(),
            off.chart.width() - crate::live_strip::LIVE_STRIP_WIDTH_PX
        );
        assert_eq!(on.time_strip.right(), on.chart.right());
    }

    /// The pane band is carved once, inside `plot_split`, so the rect the
    /// renderer scales prices to is the rect the input handler hit-tests
    /// against. When the two disagreed, a drawing was placed where you
    /// clicked and then selected somewhere else — by 20% of the chart height
    /// per visible pane.
    #[test]
    fn the_pane_band_comes_out_of_every_callers_chart_rect() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));

        let none = plot_split(area, 0.0, 0);
        assert!(none.indicator_panes.is_empty());

        let one = plot_split(area, 0.0, 1);
        let pane = *one
            .indicator_panes
            .first()
            .expect("one visible pane claims one rect");
        assert!(
            one.chart.height() < none.chart.height(),
            "the band is paid for out of the candles' pixels"
        );
        assert_eq!(one.chart.bottom(), pane.top(), "no gap, no overlap");
        assert_eq!(pane.bottom(), none.chart.bottom());
        assert_eq!(one.chart.width(), none.chart.width());
        // The axes stay where they were: only the candle body shrinks.
        assert_eq!(one.price_gutter, none.price_gutter);
        assert_eq!(one.time_strip, none.time_strip);

        let three = plot_split(area, 0.0, 3);
        assert_eq!(three.indicator_panes.len(), 3);
        assert!(three.chart.height() < one.chart.height());
    }

    /// Each pane zooms from the strip under it, and the split is exactly the
    /// divider — so a drag can never mean both time axes at once.
    #[test]
    fn the_time_strip_splits_at_the_lane_divider() {
        let strip = egui::Rect::from_min_max(egui::pos2(0.0, 580.0), egui::pos2(1000.0, 600.0));

        let (history, lane) = split_time_strip(strip, Some(700.0));
        let lane = lane.expect("the lane owns the strip under it");
        assert_eq!(history.left(), strip.left());
        assert_eq!(history.right(), 700.0);
        assert_eq!(lane.left(), 700.0);
        assert_eq!(lane.right(), strip.right());

        // Without a lane the candles keep the whole strip, exactly as before.
        assert_eq!(split_time_strip(strip, None), (strip, None));
        // A divider off the strip is not a split either.
        assert_eq!(split_time_strip(strip, Some(-5.0)), (strip, None));
    }

    /// The tape is inert: a gesture that lands on it must not reach the
    /// candles, and the divider belongs to the tape so the resize handle and
    /// the pan can never both fire on one pixel.
    #[test]
    fn a_gesture_on_the_tape_never_belongs_to_the_candles() {
        assert!(!gesture_hits_lane(Some(700.0), 699.9));
        assert!(gesture_hits_lane(Some(700.0), 700.0));
        assert!(gesture_hits_lane(Some(700.0), 1_200.0));
        // No lane: every pixel is the candles', exactly as before it existed.
        assert!(!gesture_hits_lane(None, 1_200.0));
    }

    #[test]
    fn the_lane_axis_reads_its_window_in_a_human_unit() {
        assert_eq!(fmt_window(800), "800 ms");
        assert_eq!(fmt_window(8_000), "8.0 s");
        assert_eq!(fmt_window(90_000), "1.5 min");
        assert_eq!(fmt_window(-1), "0 ms");
    }

    /// A minimal one-feed, two-symbol config for the app tests.
    fn test_config() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_string(),
            default_symbol: "TESTUSDT".to_string(),
            feeds: vec![FeedConfig {
                id: "binance".to_string(),
                name: "Binance".to_string(),
                provider: ProviderKind::Binance,
                symbols: vec!["TESTUSDT".to_string(), "ETHUSDT".to_string()],
                bubble_preset: None,
            }],
            metatrader: Default::default(),
        }
    }

    #[test]
    fn a_quote_driven_feed_says_so_where_the_side_note_goes() {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                // What a live Tickmill US500 session publishes once its bridge
                // says hello.
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: false,
                    traded_volume: false,
                }),
                commands: cmd_tx,
                replay: None,
            },
        );
        let _ends = (evt_tx, book_tx);

        let (label, detail) = app
            .side_note()
            .expect("a quote-driven feed discloses itself");
        assert_eq!(
            label, "prints: quote-derived",
            "a chart of one-unit prints must not read as a real tape"
        );
        // Short label, full story on hover: this row shares its space with the
        // machinery readouts, and a long label paints over them.
        assert!(label.len() < 25, "the label has to fit beside the readouts");
        assert!(
            detail.is_some_and(|text| text.contains("one synthetic print per tick")),
            "the hover has to explain what a quote-derived print is"
        );
        // And the affordances that would need a size are off with it.
        assert!(!app.capabilities().traded_volume);
        assert!(!app.capabilities().book_capture);
    }

    /// An app wired to in-memory channels, plus the test's ends of them: send
    /// feed events in, observe feed commands out. No egui, no network.
    fn test_app() -> (
        QuantickApp,
        mpsc::Sender<FeedEvent>,
        mpsc::Receiver<FeedCommand>,
        mpsc::Sender<DepthEvent>,
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        (app, evt_tx, cmd_rx, book_tx)
    }

    /// The same app, plus the notice sender its feed would hold. The other
    /// ends come back so the caller keeps the channels open, exactly as a live
    /// feed thread would.
    #[allow(clippy::type_complexity)]
    /// A library entry whose file is gone must still produce something the
    /// user can see: the click used to log a warning and leave the chart
    /// unchanged, while this function's doc promised an error slot.
    #[test]
    fn a_script_that_no_longer_reads_becomes_a_visible_error_slot() {
        let (mut app, _events, _commands, _book) = test_app();
        let dir = std::env::temp_dir().join(format!("quantick-app-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("vanishing.pine");
        std::fs::write(
            &path,
            "//@version=5
plot(close)
",
        )
        .expect("write");

        app.script_library = crate::indicators::library::ScriptLibrary::scan_dir(&dir);
        let index = app
            .script_library
            .entries()
            .iter()
            .position(|e| e.name == "vanishing.pine")
            .expect("the file was scanned");
        std::fs::remove_file(&path).expect("remove");

        let before = app.flow_pane.indicators.all().len();
        let slot = app
            .add_script_indicator(index)
            .expect("a click on a known entry claims a slot");
        assert_eq!(
            app.flow_pane.indicators.all().len(),
            before + 1,
            "a slot appeared"
        );
        let view = app
            .flow_pane
            .indicators
            .all()
            .iter()
            .find(|v| v.slot == slot)
            .expect("the slot has a view");
        assert!(view.error.is_some(), "and it carries the read failure");
        assert_eq!(view.label(), "vanishing.pine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The PR's headline behaviour had no test: everything sat on the serde
    /// layer, and the wiring is where the defects were.
    #[test]
    fn the_indicator_set_restores_from_disk_and_saves_back() {
        use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind};

        let (mut app, _events, _commands, _book) = test_app();
        let path = std::env::temp_dir().join(format!(
            "quantick-indicator-state-app-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        app.indicator_state_path = path.clone();

        // A saved set: one native with bound inputs, one hidden native, and
        // a script the library does not have.
        crate::indicators::state_file::save(
            &path,
            &[
                SavedIndicator {
                    kind: SavedKind::NativeEma,
                    hidden: false,
                    inputs: vec![SavedInput::Int(21), SavedInput::Source("close".to_owned())],
                },
                SavedIndicator {
                    kind: SavedKind::NativeCvd,
                    hidden: true,
                    inputs: Vec::new(),
                },
                SavedIndicator {
                    kind: SavedKind::Script {
                        name: "not-in-the-library.pine".to_owned(),
                    },
                    hidden: false,
                    inputs: Vec::new(),
                },
            ],
        );

        app.restore_indicator_state();
        assert_eq!(
            app.slot_kinds.len(),
            2,
            "a script the library lacks adds nothing, not a phantom slot"
        );
        assert_eq!(app.slot_kinds[0].1, SavedKind::NativeEma);
        assert_eq!(app.slot_kinds[1].1, SavedKind::NativeCvd);
        assert_eq!(app.pending_hidden.len(), 1, "the hidden flag survived");
        assert!(
            !app.indicator_state_dirty,
            "restoring is not a user edit and must not rewrite the file"
        );

        // A user edit, settled: the file must match the live set.
        app.mark_indicator_state_dirty();
        app.last_indicator_change =
            Some(Instant::now() - INDICATOR_STATE_SAVE_DEBOUNCE - Duration::from_millis(10));
        for event in app.flow_pane.indicator_worker.drain_events() {
            app.flow_pane.indicators.apply(event);
        }
        app.maintain_indicator_state();
        let written = crate::indicators::state_file::load(&path);
        assert_eq!(
            written.len(),
            app.flow_pane
                .indicators
                .all()
                .iter()
                .filter(|view| app
                    .slot_kinds
                    .iter()
                    .any(|(owner, _)| owner.slot == view.slot))
                .count(),
            "every slot with a known kind is written, and only those"
        );
        assert!(
            !app.indicator_state_dirty,
            "the debounce fired, so the change is written"
        );
        let _ = std::fs::remove_file(&path);
    }

    fn test_app_with_notices() -> (
        QuantickApp,
        mpsc::Sender<FeedNotice>,
        (mpsc::Sender<FeedEvent>, mpsc::Sender<DepthEvent>),
    ) {
        let (evt_tx, evt_rx) = mpsc::channel(64);
        let (book_tx, book_rx) = mpsc::channel(64);
        let (cmd_tx, _cmd_rx) = mpsc::channel(16);
        let (notice_tx, notice_rx) = mpsc::channel(8);
        let app = QuantickApp::new(
            test_config(),
            "binance",
            "TESTUSDT",
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: notice_rx,
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );
        (app, notice_tx, (evt_tx, book_tx))
    }

    #[test]
    fn the_newest_notice_wins_and_clear_puts_the_chart_back() {
        let (mut app, notices, _feed_ends) = test_app_with_notices();
        assert_eq!(app.notice, FeedNotice::Clear, "nothing to report at birth");

        // A burst arriving between two frames must leave the latest state, not
        // a queue of cards to page through.
        notices
            .blocking_send(FeedNotice::working("starting the MetaTrader bridge"))
            .unwrap();
        notices
            .blocking_send(FeedNotice::attention(
                "MetaTrader 5 is not running",
                "Open the terminal and log in.",
            ))
            .unwrap();
        app.drain_notices();
        assert!(
            matches!(app.notice, FeedNotice::Attention { .. }),
            "the newest notice is what the user sees, got {:?}",
            app.notice
        );

        // And a feed that recovers takes its own instruction back down.
        notices.blocking_send(FeedNotice::Clear).unwrap();
        app.drain_notices();
        assert_eq!(app.notice, FeedNotice::Clear);
    }

    #[test]
    fn only_explicit_connection_notices_drive_transport_state() {
        let (mut app, notices, _feed_ends) = test_app_with_notices();
        app.latest_trade_latency_ms = Some(42);
        assert_eq!(app.feed_connection, FeedConnectionState::Connecting);

        notices.blocking_send(FeedNotice::Connected).unwrap();
        app.drain_notices();
        assert_eq!(app.feed_connection, FeedConnectionState::Connected);
        assert_eq!(app.notice, FeedNotice::Clear);

        // The MetaTrader bridge supervisor and bridge server share this
        // channel. Progress or attention from either can arrive after the
        // server has reported Connected, so neither is a transport transition.
        notices
            .blocking_send(FeedNotice::working("late supervisor progress"))
            .unwrap();
        app.drain_notices();
        assert_eq!(app.feed_connection, FeedConnectionState::Connected);
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Live
        );

        notices
            .blocking_send(FeedNotice::attention(
                "late supervisor warning",
                "No transport action.",
            ))
            .unwrap();
        app.drain_notices();
        assert_eq!(app.feed_connection, FeedConnectionState::Connected);
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Live
        );

        notices
            .blocking_send(FeedNotice::reconnecting(
                "Hyperliquid disconnected — reconnecting",
            ))
            .unwrap();
        app.drain_notices();
        assert_eq!(app.feed_connection, FeedConnectionState::Reconnecting);
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Reconnecting,
            "a previous latency observation must not keep a disconnected socket green"
        );

        notices.blocking_send(FeedNotice::Connected).unwrap();
        app.drain_notices();
        assert_eq!(app.feed_connection, FeedConnectionState::Connected);
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Live
        );
    }

    #[test]
    fn a_feed_with_nothing_to_report_leaves_the_chart_alone() {
        // Binance and replay hand over a closed channel; draining it must be a
        // no-op rather than an error the app has to special-case.
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.drain_notices();
        assert_eq!(app.notice, FeedNotice::Clear);
    }

    /// Take the `SetBookCapture` command every test app queues at
    /// construction and return its generation. Capture follows the feed, so
    /// there is exactly one of these in flight before any test acts.
    fn take_capture_start(commands: &mut mpsc::Receiver<FeedCommand>) -> u64 {
        match commands
            .try_recv()
            .expect("capture starts with the feed, not with the toggle")
        {
            FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation,
            } => initial_generation,
            _ => panic!("unexpected command"),
        }
    }

    fn enable_heatmap_with_snapshot(
        app: &mut QuantickApp,
        commands: &mut mpsc::Receiver<FeedCommand>,
    ) {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let generation = take_capture_start(commands);
        app.tape_mut().set_depth_visible(true);
        app.tape_mut().handle_depth_event(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
        app.tape_mut().flush_for_test();
        assert_eq!(app.tape_mut().health().active_levels, 2);
    }

    #[test]
    fn loader_survives_until_every_pending_load_is_answered() {
        // Two "load older" clicks land while the initial backfill is still in
        // flight: three loads pending. The first reply must NOT hide the
        // indicator - only the last one may.
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        assert_eq!(
            app.loading.count(LoadingTask::History),
            1,
            "backfill in flight at start"
        );

        app.request_older_history();
        app.request_older_history();
        assert_eq!(app.loading.count(LoadingTask::History), 3);

        evt_tx.try_send(FeedEvent::Backfilled(Vec::new())).unwrap();
        app.drain_feed();
        assert_eq!(
            app.loading.count(LoadingTask::History),
            2,
            "older loads still pending"
        );

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_feed();
        assert_eq!(
            app.loading.count(LoadingTask::History),
            1,
            "one reply answers one load"
        );

        evt_tx
            .try_send(FeedEvent::HistoryPrepended(Vec::new()))
            .unwrap();
        app.drain_feed();
        assert_eq!(
            app.loading.count(LoadingTask::History),
            0,
            "last reply hides the loader"
        );
    }

    #[test]
    fn rejected_request_does_not_arm_the_loader() {
        // With the command channel closed the request never reaches the feed,
        // so no reply will ever come - the count must not grow.
        let (mut app, _evt_tx, cmd_rx, _book_tx) = test_app();
        drop(cmd_rx);
        app.request_older_history();
        assert_eq!(
            app.loading.count(LoadingTask::History),
            1,
            "only the initial backfill"
        );
    }

    #[test]
    fn a_source_reset_restarts_the_history_wait() {
        // Loads queued before a reset will never be answered; the refill after
        // the reset is the one load left in flight.
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        app.request_older_history();
        app.request_older_history();
        assert_eq!(app.loading.count(LoadingTask::History), 3);
        app.flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.drain_feed();
        assert_eq!(app.loading.count(LoadingTask::History), 1);
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "bar-index drawings cannot survive a source reset honestly"
        );
    }

    #[test]
    fn bar_spec_change_defers_one_frame_and_shows_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.flow_pane.tick_n = 100;
        app.flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        app.apply_spec_changes();
        assert!(app.loading.is_active(LoadingTask::BarRebuild));
        assert_eq!(
            app.flow_pane.state.spec(),
            &BarSpec::Tick(50),
            "the arming frame must paint the overlay before the rebuild runs"
        );

        app.apply_spec_changes();
        assert_eq!(app.flow_pane.state.spec(), &BarSpec::Tick(100));
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "a new bar partition must not inherit old bar-index anchors"
        );
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn a_still_moving_selector_keeps_deferring_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.flow_pane.tick_n = 100;
        app.apply_spec_changes();
        app.flow_pane.tick_n = 200; // the drag continues
        app.apply_spec_changes();
        assert_eq!(
            app.flow_pane.state.spec(),
            &BarSpec::Tick(50),
            "no rebuild mid-gesture"
        );
        assert!(app.loading.is_active(LoadingTask::BarRebuild));

        app.apply_spec_changes();
        assert_eq!(app.flow_pane.state.spec(), &BarSpec::Tick(200));
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn an_unchanged_spec_never_arms_the_rebuild_indicator() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.apply_spec_changes();
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
        assert!(app.flow_pane.pending_spec.is_none());
    }

    /// A trade a tenth of a second after the last, one unit at a walking
    /// price, so bars carry distinct times and a readable price range.
    fn trade(agg_id: u64) -> quantick_engine::Trade {
        quantick_engine::Trade {
            agg_id,
            timestamp_ms: 1_000 + agg_id as i64 * 100,
            price: Decimal::from(100) + Decimal::new(agg_id as i64 % 20, 1),
            quantity: Decimal::ONE,
            side: if agg_id.is_multiple_of(2) {
                quantick_engine::Side::Buy
            } else {
                quantick_engine::Side::Sell
            },
        }
    }

    #[test]
    fn quiet_market_keeps_the_observed_arrival_latency_live() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.feed_connection = FeedConnectionState::Connected;
        let trade = trade(1);
        let received_at_ms = trade.timestamp_ms + 42;

        app.ingest_live_trade_at(&trade, received_at_ms);

        assert_eq!(app.trade_arrival_ms(), Some(42));
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Live
        );
        assert_eq!(
            app.trade_arrival_ms(),
            Some(42),
            "reading the status again without another print must not age latency"
        );
    }

    /// The gap the removed `Stalled` state used to cover: a transport that
    /// stays open and stops delivering. No error, no disconnect, and the
    /// stored arrival figure never ages — only the tape's own age does.
    #[test]
    fn a_quiet_tape_reads_as_stale_while_arrival_stays_frozen() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.feed_connection = FeedConnectionState::Connected;
        let trade = trade(1);
        app.ingest_live_trade_at(&trade, trade.timestamp_ms + 42);

        // A moment later: fresh.
        let age = app
            .tape_age_at(trade.timestamp_ms + 500)
            .expect("a live tape has an age");
        assert!(age < metrics::STALE_TAPE_MS);
        assert_eq!(
            statusbar::tape_text(None, app.trade_arrival_ms(), Some(age)),
            "arrival 42 ms"
        );

        // A minute of silence on the same open socket.
        let age = app
            .tape_age_at(trade.timestamp_ms + 60_000)
            .expect("still a tape, just an old one");
        assert!(age > metrics::STALE_TAPE_MS, "{age} ms");
        assert_eq!(
            app.trade_arrival_ms(),
            Some(42),
            "the arrival observation is frozen, which is why it cannot report this"
        );
        assert_eq!(
            statusbar::tape_text(None, app.trade_arrival_ms(), Some(age)),
            "stale 60 s"
        );
    }

    #[test]
    fn backfill_does_not_claim_a_live_transport_latency() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        evt_tx
            .try_send(FeedEvent::Backfilled(vec![trade(1)]))
            .unwrap();

        app.drain_feed();

        assert_eq!(app.trade_arrival_ms(), None);
        assert_eq!(
            statusbar::feed_state(false, app.feed_connection),
            statusbar::FeedState::Connecting
        );
    }

    #[test]
    fn one_ui_drain_uses_one_observation_for_single_and_batched_trades() {
        use std::cell::Cell;

        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let last_trade = trade(3);
        let received_at_ms = last_trade.timestamp_ms + 75;
        evt_tx.try_send(FeedEvent::Live(trade(1))).unwrap();
        evt_tx
            .try_send(FeedEvent::LiveBatch(vec![trade(2), last_trade]))
            .unwrap();
        let clock_calls = Cell::new(0_u32);

        app.drain_feed_with_clock(|| {
            clock_calls.set(clock_calls.get() + 1);
            received_at_ms
        });

        assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
        assert_eq!(app.live_trades, 3);
        assert_eq!(app.trades_since_summary, 3);
        assert_eq!(app.trade_arrival_ms(), Some(75));
        assert_eq!(app.flow_pane.state.timeline_revision(), 3);
        assert_eq!(
            app.flow_pane.state.partial().map(|bar| bar.trade_count),
            Some(3)
        );
    }

    #[test]
    fn one_book_drain_uses_one_clock_observation() {
        use std::cell::Cell;

        let (mut app, _evt_tx, _cmd_rx, book_tx) = test_app();
        for _ in 0..2 {
            book_tx
                .try_send(DepthEvent::Status {
                    symbol: "TESTUSDT".to_owned(),
                    generation: 1,
                    status: quantick_orderbook::DepthStatus::Connecting,
                })
                .unwrap();
        }
        let clock_calls = Cell::new(0_u32);

        app.drain_book_feed_with_clock(|| {
            clock_calls.set(clock_calls.get() + 1);
            10_000
        });

        assert_eq!(clock_calls.get(), 1, "one wall-clock read per UI drain");
    }

    /// An app holding `count` backfilled trades, built into tick(1) bars — one
    /// bar per trade, the finest series a spec change can coarsen.
    fn app_with_history(count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (mut app, evt_tx, cmd_rx, _book_tx) = test_app();
        app.flow_pane.tick_n = 1;
        app.apply_spec_changes();
        app.apply_spec_changes();
        let trades: Vec<_> = (1..=count).map(trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_feed();
        assert_eq!(app.flow_pane.state.bars().len() as u64, count);
        (app, cmd_rx)
    }

    /// The dark chart: with the view panned into history, a coarser spec cuts
    /// the same trades into far fewer bars, and the old right-edge index falls
    /// off the end of the series — leaving the window over empty space, where
    /// nothing is drawn at all.
    #[test]
    fn a_rebuild_keeps_the_view_on_the_market_time_it_was_showing() {
        let (mut app, _cmd_rx) = app_with_history(400);
        // Pan back to bar 200 of 400 and remember what the edge was showing.
        app.flow_pane
            .viewport
            .pan_pixels(200.0 * 8.0, app.flow_pane.slots());
        assert!(!app.flow_pane.viewport.follows_live());
        let was_showing = app
            .flow_pane
            .right_edge_time()
            .expect("a bar under the edge");

        // Coarsen: 400 trades become 10 bars, so index 200 no longer exists.
        app.flow_pane.tick_n = 40;
        app.apply_spec_changes();
        app.apply_spec_changes();
        assert_eq!(app.flow_pane.state.bars().len(), 10);

        let slots = app.flow_pane.slots();
        let (start, end) = app.flow_pane.viewport.visible_range(800.0, slots);
        assert!(
            start < end,
            "the window must still hold bars, got {start}..{end} of {slots}"
        );
        let now_showing = app.flow_pane.right_edge_time().expect("still on a bar");
        let bar =
            &app.flow_pane.state.bars()[app.flow_pane.viewport.right_edge_bar(slots) as usize];
        assert!(
            bar.open_time <= was_showing && was_showing <= bar.close_time,
            "the edge bar ({}..{}) must span the time it was showing ({was_showing})",
            bar.open_time,
            bar.close_time
        );
        assert!(now_showing <= was_showing, "never jumps into the future");
    }

    /// Finer, not coarser: the series grows and the same market time moves to
    /// a much higher index. Following that is what keeps the user's place.
    #[test]
    fn a_finer_spec_follows_the_same_market_time_forward() {
        let (mut app, _cmd_rx) = app_with_history(400);
        app.flow_pane.tick_n = 40;
        app.apply_spec_changes();
        app.apply_spec_changes();
        app.flow_pane
            .viewport
            .pan_pixels(5.0 * 8.0, app.flow_pane.slots()); // back to bar 4 of 10
        let was_showing = app
            .flow_pane
            .right_edge_time()
            .expect("a bar under the edge");

        app.flow_pane.tick_n = 1;
        app.apply_spec_changes();
        app.apply_spec_changes();
        assert_eq!(app.flow_pane.state.bars().len(), 400);
        let edge = app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots());
        assert_eq!(
            edge, 160.0,
            "bar 4 of tick(40) opens on trade 161 — bar 160 of tick(1)"
        );
        assert_eq!(app.flow_pane.right_edge_time(), Some(was_showing));
    }

    /// A view following the live edge is already anchored to the newest bar,
    /// whatever the rebuild does to the ones behind it.
    #[test]
    fn a_rebuild_leaves_a_live_view_at_the_live_edge() {
        let (mut app, _cmd_rx) = app_with_history(400);
        assert!(app.flow_pane.viewport.follows_live());
        app.flow_pane.tick_n = 40;
        app.apply_spec_changes();
        app.apply_spec_changes();
        assert!(app.flow_pane.viewport.follows_live());
        assert_eq!(
            app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots()),
            9.0
        );
    }

    /// Every string the frame painted, panels and chart alike.
    fn painted_text(output: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::Shape, found: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => found.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// Whether the frame drew the price axis over the test's price range —
    /// the labels only exist once the chart really scaled and painted itself.
    fn has_price_axis(texts: &[String]) -> bool {
        texts.iter().any(|text| {
            text.parse::<f64>()
                .is_ok_and(|price| (95.0..=115.0).contains(&price))
        })
    }

    fn run_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
        run_frame_with_events(app, ctx, Vec::new())
    }

    fn run_frame_with_events(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        run_frame_with_modifiers(app, ctx, events, egui::Modifiers::NONE)
    }

    fn run_frame_with_modifiers(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1400.0, 900.0),
            )),
            events,
            modifiers,
            ..Default::default()
        };
        ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()))
    }

    fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn click_chart(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) {
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, true),
            ],
        );
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, false),
            ],
        );
    }

    fn drag_chart(app: &mut QuantickApp, ctx: &egui::Context, start: egui::Pos2, end: egui::Pos2) {
        run_frame_with_events(
            app,
            ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(end)]);
        run_frame_with_events(
            app,
            ctx,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        );
    }

    fn drawing_tool(id: &str) -> drawings::DrawingTool {
        drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == id)
            .expect("drawing tool is registered")
    }

    fn arm_drawing_from_toolbox(
        app: &mut QuantickApp,
        ctx: &egui::Context,
        id: &str,
    ) -> drawings::DrawingTool {
        let drawing = drawing_tool(id);
        let tool = Tool::Drawing(drawing);
        let button = app
            .toolrail
            .button_rect(tool)
            .expect("drawing button was rendered");
        click_chart(app, ctx, button.center());
        assert_eq!(
            app.toolrail.tool(),
            tool,
            "clicking {id} must arm that drawing tool"
        );
        drawing
    }

    /// Full UI interaction proof: every registered drawing is placed through
    /// egui pointer events against the real chart frame. This catches the
    /// original regression where multi-point tools silently ignored drags.
    #[test]
    fn every_toolbox_drawing_can_be_plotted_on_the_chart() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        arm_drawing_from_toolbox(&mut app, &ctx, "horizontal-line");
        click_chart(&mut app, &ctx, egui::pos2(600.0, 250.0));

        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 300.0),
            egui::pos2(800.0, 450.0),
        );

        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(650.0, 500.0),
            egui::pos2(820.0, 350.0),
        );
        click_chart(&mut app, &ctx, egui::pos2(900.0, 280.0));

        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(700.0, 620.0),
            egui::pos2(950.0, 300.0),
        );

        arm_drawing_from_toolbox(&mut app, &ctx, "fib-extension");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 650.0),
            egui::pos2(820.0, 500.0),
        );
        click_chart(&mut app, &ctx, egui::pos2(1_000.0, 350.0));

        let tools: Vec<_> = app
            .flow_pane
            .drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .collect();
        assert_eq!(tools, drawings::DRAWING_TOOLS);
        assert!(
            app.flow_pane
                .drawings
                .items()
                .iter()
                .all(|drawing| drawing.points.len() == drawing.tool.required_points())
        );
        assert_eq!(
            app.toolrail.tool(),
            Tool::Pointer,
            "placing a complete drawing restores navigation"
        );
    }

    #[test]
    fn a_drawing_can_be_selected_from_its_stroke_and_moved_without_panning() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        let before = app.flow_pane.drawings.items()[0].points[0];
        let viewport_before = app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        let after = app.flow_pane.drawings.items()[0].points[0];

        assert!(
            after.bar > before.bar,
            "dragging right moves the anchor right"
        );
        assert!(
            after.price < before.price,
            "dragging down moves the anchor to a lower price"
        );
        assert_eq!(
            app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots()),
            viewport_before,
            "moving a drawing must not pan the market underneath it"
        );
        assert_eq!(
            app.flow_pane.drawing_drag,
            DrawingDrag::None,
            "release ends the move gesture"
        );
    }

    #[test]
    fn inspector_never_blocks_drawing_drag() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let anchor = egui::pos2(300.0, 250.0);
        click_chart(&mut app, &ctx, anchor);
        run_frame(&mut app, &ctx);

        // The horizontal stroke crosses the whole pane, so wherever placement
        // put the window, some stroke pixel sits underneath it. Drag exactly
        // that pixel: the gesture must stay with the drawing.
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("placing the selected line opens its inspector");
        let line_y = anchor.y;
        let start = egui::pos2(inspector.center().x, line_y);
        assert!(
            inspector.contains(start),
            "the regression requires the floating inspector to cover this stroke pixel"
        );
        let before = app.flow_pane.drawings.items()[0].points[0];

        drag_chart(&mut app, &ctx, start, egui::pos2(start.x, line_y + 100.0));

        assert_ne!(
            app.flow_pane.drawings.items()[0].points[0].price,
            before.price,
            "the open inspector must not trap the line underneath it"
        );
    }

    #[test]
    fn drawing_actions_visible_without_scroll_at_360px() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        let output = run_frame(&mut app, &ctx);
        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        assert!(
            inspector.width() >= INSPECTOR_MIN_WIDTH_PX - 1.0,
            "the inspector must respect its minimum width; got {}",
            inspector.width()
        );
        let texts = painted_text(&output);
        for label in ["Lock drawing", "Delete drawing"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the named action {label:?} must be visible without scrolling; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn inspector_opens_beside_the_selection_inside_the_chart() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(300.0, 300.0),
            egui::pos2(400.0, 380.0),
        );
        run_frame(&mut app, &ctx);

        let inspector = ctx
            .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
            .expect("the inspector is open");
        let chart = app
            .flow_pane
            .last_chart_area
            .expect("the chart pane was laid out");
        let bbox = egui::Rect::from_min_max(egui::pos2(300.0, 300.0), egui::pos2(400.0, 380.0))
            .expand(DRAWING_ANCHOR_RADIUS_PX);
        assert!(
            !inspector.intersects(bbox),
            "the inspector must open beside the selection, not on top of it: {inspector:?} vs {bbox:?}"
        );
        assert!(
            chart.contains_rect(inspector),
            "the inspector must stay inside the chart pane (never over the axes): {inspector:?} vs {chart:?}"
        );
    }

    #[test]
    fn pinning_the_inspector_docks_it_and_frees_the_canvas() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(300.0, 300.0),
            egui::pos2(400.0, 380.0),
        );
        // Let the window settle its size and position before reading rects.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);
        let chart_before = app.flow_pane.last_chart_area.expect("chart laid out");

        let pin = app.inspector_pin_rect.expect("pin button rendered");
        click_chart(&mut app, &ctx, pin.center());
        assert!(app.inspector_pinned, "clicking Pin docks the inspector");
        run_frame(&mut app, &ctx);
        let chart_after = app.flow_pane.last_chart_area.expect("chart laid out");
        assert!(
            chart_after.width() < chart_before.width(),
            "the docked inspector must be paid for by the canvas, not float over it"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Delete drawing")),
            "the docked panel still shows the named actions"
        );
    }

    #[test]
    fn button_manager_and_keyboard_send_the_same_delete_command() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.drawing_manager_open = true;
        // Let the manager window settle its size before reading button rects.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        let delete = app
            .manager_action_rects
            .iter()
            .find(|(index, action, _)| *index == 0 && *action == "Delete")
            .map(|(_, _, rect)| *rect)
            .expect("the manager lists the drawing with a Delete action");
        click_chart(&mut app, &ctx, delete.center());
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "the manager's Delete lands the same command"
        );
        assert!(
            app.drawing_toast.is_some(),
            "the manager delete raises the same Undo toast as the keyboard"
        );
        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "one undo rewinds the manager delete, exactly like the keyboard one"
        );
    }

    #[test]
    fn the_object_manager_toggles_eye_lock_and_z_order_per_row() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        for price_y in [250.0, 350.0] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            click_chart(&mut app, &ctx, egui::pos2(700.0, price_y));
        }
        let objects = app
            .toolrail
            .objects_button_rect()
            .expect("the toolbox shows the Objects entry");
        click_chart(&mut app, &ctx, objects.center());
        assert!(
            app.drawing_manager_open,
            "the Objects button opens the manager"
        );
        run_frame(&mut app, &ctx);

        let rect_of = |app: &QuantickApp, index: usize, action: &str| {
            app.manager_action_rects
                .iter()
                .find(|(row, name, _)| *row == index && *name == action)
                .map(|(_, _, rect)| *rect)
                .expect("manager action rendered")
        };

        let eye = rect_of(&app, 0, "Eye");
        click_chart(&mut app, &ctx, eye.center());
        assert!(
            app.flow_pane.drawings.items()[0].hidden,
            "the row's eye hides it"
        );

        run_frame(&mut app, &ctx);
        let lock = rect_of(&app, 1, "Lock");
        click_chart(&mut app, &ctx, lock.center());
        assert!(
            app.flow_pane.drawings.items()[1].locked,
            "the row's lock locks it"
        );

        run_frame(&mut app, &ctx);
        let front = rect_of(&app, 0, "Front");
        let hidden_line_price = app.flow_pane.drawings.items()[0].points[0].price;
        click_chart(&mut app, &ctx, front.center());
        assert_eq!(
            app.flow_pane.drawings.items()[1].points[0].price,
            hidden_line_price,
            "Front moves the object to the top of the z-order"
        );
    }

    #[test]
    fn a_selected_drawing_exposes_its_edit_and_delete_controls() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(620.0, 300.0),
            egui::pos2(800.0, 450.0),
        );
        assert_eq!(app.flow_pane.drawings.items().len(), 1);
        assert_eq!(app.flow_pane.drawings.selected(), Some(0));

        let output = run_frame(&mut app, &ctx);
        assert_eq!(app.flow_pane.drawings.selected(), Some(0));
        let texts = painted_text(&output);
        for label in [
            "Rectangle settings",
            "Style",
            "line width (px)",
            "fill opacity",
            "Delete drawing",
        ] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "selected drawing inspector omitted {label:?}; painted text: {texts:?}"
            );
        }
    }

    #[test]
    fn rectangle_anchor_resizes_while_the_settings_window_stays_non_modal() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        let first_anchor = egui::pos2(620.0, 300.0);
        let second_anchor = egui::pos2(800.0, 450.0);
        drag_chart(&mut app, &ctx, first_anchor, second_anchor);
        let before = app.flow_pane.drawings.items()[0].points.clone();
        let viewport_before = app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots());

        let inspector = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            inspector
                .iter()
                .any(|text| text.contains("Rectangle settings")),
            "the settings window must be visible before the resize gesture"
        );

        drag_chart(&mut app, &ctx, first_anchor, egui::pos2(560.0, 240.0));
        let after = &app.flow_pane.drawings.items()[0].points;

        assert_ne!(after[0], before[0], "the dragged corner must move");
        assert_eq!(
            after[1], before[1],
            "resizing one corner must leave the opposite corner fixed"
        );
        assert_eq!(
            app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots()),
            viewport_before,
            "resizing a drawing must not pan the chart"
        );
        assert_eq!(app.flow_pane.drawing_drag, DrawingDrag::None);
        assert!(
            painted_text(&run_frame(&mut app, &ctx))
                .iter()
                .any(|text| text.contains("Rectangle settings")),
            "the non-modal settings window remains usable after resizing"
        );
    }

    fn key_press(key: egui::Key) -> egui::Event {
        key_press_with(key, egui::Modifiers::NONE)
    }

    fn key_press_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Whether any painted line segment uses `color` — proof a stroke of that
    /// colour reached the screen (or, negated, that a hidden object did not).
    fn painted_line_with_color(output: &egui::FullOutput, color: egui::Color32) -> bool {
        output.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::LineSegment { stroke, .. } => {
                stroke.color == egui::epaint::ColorMode::Solid(color)
            }
            _ => false,
        })
    }

    #[test]
    fn locked_drawing_rejects_geometry_and_keyboard_delete() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.flow_pane.drawings.set_selected_locked(true);

        let before = app.flow_pane.drawings.items()[0].points[0];
        let viewport_before = app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        assert_eq!(
            app.flow_pane.drawings.items()[0].points[0],
            before,
            "locked geometry must not move"
        );
        assert_eq!(
            app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots()),
            viewport_before,
            "the blocked gesture still belongs to the drawing - the chart must not pan"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "keyboard delete must not remove a locked drawing"
        );
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts
                .iter()
                .any(|text| text.contains("Delete locked drawing?")),
            "the same confirmation appears next to the trigger; painted: {texts:?}"
        );
    }

    #[test]
    fn one_drag_creates_one_undo_entry() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(
            app.flow_pane.drawings.undo_depth(),
            1,
            "creating the drawing is the first undo entry"
        );

        let before_drag = app.flow_pane.drawings.items()[0].points[0];
        let start = egui::pos2(1_000.0, 300.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true),
            ],
        );
        for step in [egui::pos2(1_010.0, 312.0), egui::pos2(1_025.0, 326.0)] {
            run_frame_with_events(&mut app, &ctx, vec![egui::Event::PointerMoved(step)]);
        }
        let end = egui::pos2(1_040.0, 340.0);
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        );
        assert_ne!(
            app.flow_pane.drawings.items()[0].points[0],
            before_drag,
            "the drag really moved the line"
        );

        assert_eq!(
            app.flow_pane.drawings.undo_depth(),
            2,
            "a multi-frame drag coalesces into exactly one undo entry"
        );
        assert!(app.flow_pane.drawings.undo(), "undo the drag");
        assert_eq!(
            app.flow_pane.drawings.items()[0].points[0],
            before_drag,
            "one undo rewinds the whole drag"
        );
        assert!(app.flow_pane.drawings.undo(), "undo the creation");
        assert!(app.flow_pane.drawings.items().is_empty());
    }

    #[test]
    fn delete_and_backspace_never_leak_out_of_focused_inputs() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(app.flow_pane.drawings.items().len(), 1);

        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("test-text-input")));
        run_frame_with_events(
            &mut app,
            &ctx,
            vec![
                key_press(egui::Key::Delete),
                key_press(egui::Key::Backspace),
            ],
        );
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "while an input owns the keyboard the delete keys stay in it"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "with focus released the same key deletes the selection"
        );
    }

    #[test]
    fn keyboard_delete_offers_an_undo_toast_and_ctrl_z_restores() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(app.flow_pane.drawings.items().is_empty());
        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["Drawing deleted.", "Undo"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the toast must offer {label:?}; painted: {texts:?}"
            );
        }

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Z, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "Ctrl+Z drives the same history as the toast's Undo"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::Y, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "Ctrl+Y redoes the undone delete"
        );
    }

    #[test]
    fn hidden_drawing_neither_paints_nor_hit_tests() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let stroke_position = egui::pos2(700.0, 300.0);
        click_chart(&mut app, &ctx, stroke_position);
        let marker = egui::Color32::from_rgb(1, 2, 3);
        app.flow_pane
            .drawings
            .selected_mut()
            .expect("placement selects the line")
            .style
            .color = marker;
        assert!(
            painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "the visible line paints its stroke"
        );

        app.flow_pane.drawings.set_selected_hidden(true);
        assert!(
            !painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "a hidden drawing must not paint"
        );

        app.flow_pane.drawings.select(None);
        click_chart(&mut app, &ctx, stroke_position);
        assert_eq!(
            app.flow_pane.drawings.selected(),
            None,
            "a hidden drawing must not hit-test"
        );

        let viewport_before = app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots());
        drag_chart(&mut app, &ctx, stroke_position, egui::pos2(640.0, 260.0));
        assert_ne!(
            app.flow_pane.viewport.right_edge_bar(app.flow_pane.slots()),
            viewport_before,
            "over a hidden drawing the gesture belongs to the chart again"
        );
    }

    #[test]
    fn a_bar_rebuild_clears_drawings_with_an_explicit_notice_and_no_dead_undo() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.flow_pane.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.drain_feed();
        assert!(app.flow_pane.drawings.items().is_empty());
        assert!(
            app.drawing_toast.is_some(),
            "the clear must raise the notice toast"
        );

        // A fresh egui Area sizes itself on its first frame; the text is
        // on screen from the second one.
        run_frame(&mut app, &ctx);
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Drawings cleared")),
            "losing the marks is never silent; painted: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text == "Undo"),
            "the clear toast must not offer an Undo it cannot honour"
        );
    }

    #[test]
    fn escape_walks_confirm_draft_selection_then_pointer() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // Draft first: Esc cancels it and returns to Pointer.
        arm_drawing_from_toolbox(&mut app, &ctx, "rectangle");
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(app.flow_pane.drawings.draft_len(), 1);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(
            app.flow_pane.drawings.draft().is_none(),
            "Esc cancels the draft"
        );
        assert_eq!(app.toolrail.tool(), Tool::Pointer);

        // A locked selection with a pending confirmation: Esc peels one
        // layer per press — confirm, then selection, then nothing new.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.flow_pane.drawings.set_selected_locked(true);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(app.drawing_delete_confirm, "the confirmation is pending");

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(!app.drawing_delete_confirm, "first Esc cancels the confirm");
        assert!(
            app.flow_pane.drawings.selected().is_some(),
            "the selection survives"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert_eq!(
            app.flow_pane.drawings.selected(),
            None,
            "second Esc deselects"
        );
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "nothing was deleted"
        );
    }

    #[test]
    fn backspace_steps_back_through_the_draft_anchors() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
        click_chart(&mut app, &ctx, egui::pos2(750.0, 300.0));
        assert_eq!(app.flow_pane.drawings.draft_len(), 2);

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Backspace)]);
        assert_eq!(
            app.flow_pane.drawings.draft_len(),
            1,
            "Backspace removes the last placed anchor"
        );
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "the draft workflow never deletes finished objects"
        );
    }

    #[test]
    fn alt_l_and_alt_h_protect_the_selection_from_the_keyboard() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::L, egui::Modifiers::ALT)],
            egui::Modifiers::ALT,
        );
        assert!(
            app.flow_pane.drawings.items()[0].locked,
            "Alt+L locks the selection"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::H, egui::Modifiers::ALT)],
            egui::Modifiers::ALT,
        );
        assert!(
            app.flow_pane.drawings.items()[0].hidden,
            "Alt+H hides the selection"
        );
        assert_eq!(
            app.toolrail.tool(),
            Tool::Pointer,
            "Alt+H must not arm the horizontal-line tool"
        );
    }

    #[test]
    fn ctrl_d_duplicates_the_selection_offset_and_selected() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let original_bar = app.flow_pane.drawings.items()[0].points[0].bar;

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::D, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(app.flow_pane.drawings.items().len(), 2);
        assert_eq!(app.flow_pane.drawings.selected(), Some(1));
        assert_eq!(
            app.flow_pane.drawings.items()[1].points[0].bar,
            original_bar + DUPLICATE_OFFSET_BARS
        );
    }

    #[test]
    fn arrow_nudges_move_the_selection_and_shift_multiplies_by_ten() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        let start = app.flow_pane.drawings.items()[0].points[0];
        let depth = app.flow_pane.drawings.undo_depth();

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowRight)]);
        assert_eq!(
            app.flow_pane.drawings.items()[0].points[0].bar,
            start.bar + 1.0,
            "one press is one bar"
        );
        assert_eq!(
            app.flow_pane.drawings.undo_depth(),
            depth + 1,
            "one press, one entry"
        );

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(
                egui::Key::ArrowRight,
                egui::Modifiers::SHIFT,
            )],
            egui::Modifiers::SHIFT,
        );
        assert_eq!(
            app.flow_pane.drawings.items()[0].points[0].bar,
            start.bar + 11.0,
            "Shift multiplies the nudge by ten"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowUp)]);
        assert!(
            app.flow_pane.drawings.items()[0].points[0].price > start.price,
            "ArrowUp raises the price"
        );
    }

    #[test]
    fn the_repeat_pin_keeps_the_drawing_tool_armed() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // Default: one-shot back to Pointer.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
        assert_eq!(app.toolrail.tool(), Tool::Pointer);

        // Pinned: the tool stays armed for the next object.
        app.toolrail.set_repeat(true);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 320.0));
        assert_eq!(
            app.toolrail.tool().drawing_tool().map(|tool| tool.id()),
            Some("horizontal-line"),
            "the repeat pin keeps the tool armed"
        );
        assert_eq!(app.flow_pane.drawings.items().len(), 2);
    }

    #[test]
    fn the_fib_inspector_mounts_its_level_editor_tab() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );
        assert_eq!(app.flow_pane.drawings.items().len(), 1);

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("Levels")),
            "the tool-owned tab is offered by name; painted: {texts:?}"
        );

        app.inspector_tab = InspectorTab::Extra;
        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["Preset", "Standard", "band opacity", "log scale"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the level editor must show {label:?}; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn a_placed_fib_paints_its_levels_and_labels() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );

        let texts = painted_text(&run_frame(&mut app, &ctx));
        for label in ["61.8%", "38.2%", "50.0%"] {
            assert!(
                texts.iter().any(|text| text.contains(label)),
                "the standard retracement labels paint on the chart; painted: {texts:?}"
            );
        }
    }

    #[test]
    fn a_default_preset_shapes_new_fibs_and_leaves_existing_ones_alone() {
        use crate::drawings::DrawingPayload as _;
        use crate::drawings::fib::FibPayload;

        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        // First fib: the built-in standard start.
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(600.0, 250.0),
            egui::pos2(900.0, 400.0),
        );
        let standard_levels = app.flow_pane.drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(standard_levels, 7);

        // Save a compact custom preset and make it the default for new fibs.
        let mut store = drawings::presets::PresetStore::load_from(std::env::temp_dir().join(
            format!("quantick-default-preset-test-{}.toml", std::process::id()),
        ));
        let mut compact = FibPayload::new(drawings::fib::FibKind::Retracement);
        compact.apply_preset(&drawings::fib::RETRACEMENT_PRESETS[1]);
        let exported = compact.export_preset().expect("fib exports presets");
        assert!(store.save_custom_preset("fib-retracement", "mine", exported, false));
        store.set_default_preset("fib-retracement", Some("mine".into()));
        let preset_path = store.path().to_path_buf();
        app.drawing_presets = store;

        // Second fib starts from the default preset...
        arm_drawing_from_toolbox(&mut app, &ctx, "fib-retracement");
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(400.0, 250.0),
            egui::pos2(550.0, 400.0),
        );
        let new_levels = app.flow_pane.drawings.items()[1]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(new_levels, 5, "a new fib starts from the default preset");

        // ...and the first one is untouched.
        let old_levels = app.flow_pane.drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(old_levels, 7, "the default never rewrites existing objects");
        let _ = std::fs::remove_file(preset_path);
    }

    /// The whole point, on a real frame: after a spec change with the view
    /// panned into history, the chart is drawn — not a black rectangle.
    #[test]
    fn a_rebuilt_chart_still_paints_itself() {
        let (mut app, _cmd_rx) = app_with_history(400);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx); // one frame to settle the layout
        app.flow_pane
            .viewport
            .pan_pixels(200.0 * 8.0, app.flow_pane.slots());

        app.flow_pane.tick_n = 40;
        let armed = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            armed.iter().any(|text| text.contains("rebuilding bars")),
            "the arming frame says what it is doing: {armed:?}"
        );

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the chart must be on screen after the rebuild: {texts:?}"
        );
        assert!(
            !texts.iter().any(|text| text.contains("no bars in view")),
            "and it must be showing bars, not an empty window: {texts:?}"
        );
    }

    /// The other way to empty the window — panning into the space past the
    /// newest bar, zoomed in far enough that no bar is left on screen. The
    /// chart keeps its axis and says how to get back.
    #[test]
    fn an_empty_window_says_so_instead_of_going_dark() {
        let (mut app, _cmd_rx) = app_with_history(400);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);

        app.flow_pane.viewport.zoom(8.0); // 64 px candles: only a dozen fit
        app.flow_pane
            .viewport
            .pan_pixels(-10_000.0, app.flow_pane.slots()); // into the empty future
        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            texts.iter().any(|text| text.contains("no bars in view")),
            "an empty window must explain itself: {texts:?}"
        );
        assert!(
            has_price_axis(&texts),
            "and keep the axis, so the chart never reads as hung: {texts:?}"
        );
    }

    #[test]
    fn changing_feed_falls_back_to_a_valid_symbol() {
        // Two feeds with disjoint symbol lists: switching to a feed that does
        // not offer the current symbol must snap to that feed's first symbol.
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        let config = AppConfig {
            default_feed: "a".to_string(),
            default_symbol: "AAA".to_string(),
            feeds: vec![
                FeedConfig {
                    id: "a".to_string(),
                    name: "A".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["AAA".to_string()],
                    bubble_preset: None,
                },
                FeedConfig {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["BBB".to_string()],
                    bubble_preset: None,
                },
            ],
            metatrader: Default::default(),
        };
        let mut app = QuantickApp::new(
            config,
            "a",
            "AAA",
            BarSpec::Tick(10),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        );

        app.feed_id = "b".to_string();
        app.ensure_symbol_valid();
        assert_eq!(app.symbol, "BBB", "symbol snaps to feed b's first symbol");

        // A symbol already valid for the feed is left untouched.
        app.symbol = "BBB".to_string();
        app.ensure_symbol_valid();
        assert_eq!(app.symbol, "BBB");
    }

    /// One app on `config`, opened on `feed_id`/symbol — the smallest harness
    /// that exercises the constructor path (where a feed's declared bubble
    /// preset is applied).
    fn app_on(config: AppConfig, feed_id: &str, symbol: &str) -> QuantickApp {
        let (_evt_tx, evt_rx) = mpsc::channel(8);
        let (_book_tx, book_rx) = mpsc::channel(8);
        let (cmd_tx, _cmd_rx) = mpsc::channel(8);
        QuantickApp::new(
            config,
            feed_id,
            symbol,
            BarSpec::Tick(50),
            FeedHandle {
                events: evt_rx,
                book_events: book_rx,
                notices: feed::silent_notices(),
                capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
                commands: cmd_tx,
                replay: None,
            },
        )
    }

    #[test]
    fn a_feed_declaring_a_bubble_preset_opens_wearing_it() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("live lane pie".to_string());
        let app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(app.tape().active_preset_for_test(), "live lane pie");
        assert!(
            app.tape().config_for_test().bubble_candle_summary,
            "the pie preset folds closed bars into per-price summaries"
        );
    }

    #[test]
    fn a_feed_declaring_an_unknown_bubble_preset_changes_nothing() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("no such preset".to_string());
        let with_unknown = app_on(config, "binance", "TESTUSDT");
        let untouched = app_on(test_config(), "binance", "TESTUSDT");
        assert_eq!(
            with_unknown.tape().active_preset_for_test(),
            untouched.tape().active_preset_for_test(),
            "a typo in the config must not restyle the chart"
        );
        assert_eq!(
            with_unknown.tape().config_for_test(),
            untouched.tape().config_for_test()
        );
    }

    #[test]
    fn switching_to_a_feed_with_a_declared_preset_applies_it_then() {
        // Feed "binance" declares nothing; a second feed declares the pie
        // look. Opening on the first must not apply it — moving to the
        // second must.
        let mut config = test_config();
        config.feeds.push(FeedConfig {
            id: "mt".to_string(),
            name: "MetaTrader 5".to_string(),
            provider: ProviderKind::MetaTrader,
            symbols: vec!["WINQ26".to_string()],
            bubble_preset: Some("live lane pie".to_string()),
        });
        let mut app = app_on(config, "binance", "TESTUSDT");
        let opened_with = app.tape().active_preset_for_test().to_string();
        assert_ne!(
            opened_with, "live lane pie",
            "nothing declared, nothing applied"
        );

        // The switch path runs this after installing the new feed handle.
        app.feed_id = "mt".to_string();
        app.apply_feed_bubble_preset_after_switch("binance");
        assert_eq!(app.tape().active_preset_for_test(), "live lane pie");
    }

    #[test]
    fn a_symbol_hop_inside_one_feed_keeps_the_panel_look() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("live lane pie".to_string());
        let mut app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(app.tape().active_preset_for_test(), "live lane pie");

        // The user picks a different look by hand mid-session...
        assert!(app.tape_mut().apply_preset("dense tape"));
        // ...then hops symbols inside the same feed: the hand-picked look
        // survives — the declared preset belongs to the feed, not the symbol.
        app.apply_feed_bubble_preset_after_switch("binance");
        assert_eq!(app.tape().active_preset_for_test(), "dense tape");

        // Arriving from another feed is what re-applies the declared look.
        app.apply_feed_bubble_preset_after_switch("other-feed");
        assert_eq!(app.tape().active_preset_for_test(), "live lane pie");
    }

    #[test]
    fn capture_starts_with_the_feed_and_commits_only_after_the_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();

        // Construction already asked the feed to record: capture follows the
        // market, not the toolbar.
        assert_eq!(take_capture_start(&mut cmd_rx), BOOK_GENERATION_STRIDE);
        assert!(app.tape().enabled());
        app.ensure_book_capture();
        assert!(
            cmd_rx.try_recv().is_err(),
            "a recorder already running needs no second command"
        );

        drop(cmd_rx);
        app.request_book_capture(false);
        assert!(
            app.tape().enabled(),
            "closed command channel must preserve current capture state"
        );
    }

    /// The user's complaint made executable: hiding the map is pixels only.
    /// The recorder keeps running, so reopening it finds the history whole
    /// instead of a hole where the map was closed.
    #[test]
    fn hiding_the_heatmap_never_stops_the_recorder() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        let gaps_before = app.tape_mut().health().gaps;

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        assert!(app.tape().depth_visible());

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(!app.tape().depth_visible(), "the map is hidden");
        assert!(app.tape().enabled(), "the recorder is untouched");
        assert!(
            cmd_rx.try_recv().is_err(),
            "showing or hiding the map sends no feed command"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        app.tape_mut().flush_for_test();
        assert!(app.tape().depth_visible());
        assert_eq!(
            app.tape_mut().health().gaps,
            gaps_before,
            "the toggle must not punch a coverage gap into the recording"
        );
    }

    /// The guard that keeps an always-on recorder honest: a source with no
    /// depth pipeline — a replay, or a feed missing from the config — gets no
    /// recorder and no command, however often the app asks.
    #[test]
    fn a_source_without_depth_starts_no_recorder() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);

        let generation = app.next_book_generation();
        app.tape_mut().set_enabled(false, generation);
        app.feed_id = "not-in-the-config".to_owned();
        assert!(!app.capabilities().book_capture);

        app.ensure_book_capture();
        assert!(!app.tape().enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "a source with no book is never asked to record"
        );
    }

    #[test]
    fn bubble_toggle_needs_no_feed_command_and_leaves_capture_alone() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        assert!(!app.tape().bubbles_enabled());

        app.tape_mut().set_bubbles_enabled(true);
        assert!(app.tape().bubbles_enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "aggregate trades already flow; no feed command is needed"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(
            app.tape().bubbles_enabled(),
            "hiding the book must not stop the bubbles"
        );
    }

    #[test]
    fn grouping_restart_commits_only_after_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let grouping = Decimal::new(5, 2);

        assert!(app.tape_mut().stage_capture_grouping_for_test(grouping));
        assert_eq!(app.tape_mut().health().active_levels, 2);
        app.restart_book_capture();

        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::RestartBookCapture { .. })
        ));
        assert_eq!(app.tape_mut().base_capture_grouping_for_test(), grouping);
        assert_eq!(app.tape_mut().health().active_levels, 0);
        assert_eq!(app.tape_mut().health().status, "connecting");
    }

    #[test]
    fn closed_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.tape_mut().base_capture_grouping_for_test();

        assert!(
            app.tape_mut()
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        drop(cmd_rx);
        app.restart_book_capture();

        assert_eq!(app.tape_mut().base_capture_grouping_for_test(), original);
        assert_eq!(app.tape_mut().health().active_levels, 2);
    }

    #[test]
    fn full_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.tape_mut().base_capture_grouping_for_test();
        let (full_tx, mut full_rx) = mpsc::channel(1);
        app.commands = full_tx;
        app.commands
            .try_send(FeedCommand::LoadOlder { count: 1 })
            .unwrap();

        assert!(
            app.tape_mut()
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        app.restart_book_capture();

        assert!(matches!(
            full_rx.try_recv(),
            Ok(FeedCommand::LoadOlder { count: 1 })
        ));
        assert_eq!(app.tape_mut().base_capture_grouping_for_test(), original);
        assert_eq!(app.tape_mut().health().active_levels, 2);
    }

    #[test]
    fn depth_channel_updates_heatmap_without_mutating_candles() {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        let generation = take_capture_start(&mut cmd_rx);
        let bars_before = app.flow_pane.state.bars().len();
        book_tx
            .try_send(DepthEvent::Snapshot {
                symbol: "TESTUSDT".to_owned(),
                generation,
                observed_at_ms: 1_100,
                effective_at_ms: 999,
                price_step: None,
                snapshot: BookSnapshot::new(
                    10,
                    vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                    vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                    BookCoverage::Limited {
                        levels_per_side: 1_000,
                    },
                ),
            })
            .unwrap();

        app.drain_book_feed();
        app.tape_mut().flush_for_test();
        let book = app.tape_mut().health();
        assert_eq!(book.bid_levels, 1);
        assert_eq!(book.ask_levels, 1);
        assert_eq!(app.flow_pane.state.bars().len(), bars_before);
    }

    #[test]
    fn candle_appearance_change_is_render_only() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        let capture_epoch = app.book_capture_epoch;
        let bar_spec = app.flow_pane.state.spec().clone();

        app.style.candles = CandlePreset::OutlineOnly.style();
        app.style_revision = app.style_revision.saturating_add(1);
        app.emit_style_changed(Some(CandlePreset::OutlineOnly));

        assert_eq!(app.flow_pane.state.spec(), &bar_spec);
        assert!(app.tape().enabled());
        assert_eq!(app.book_capture_epoch, capture_epoch);
        assert!(
            matches!(cmd_rx.try_recv(), Err(mpsc::error::TryRecvError::Empty)),
            "appearance changes must not restart or reconfigure market data"
        );
    }

    #[test]
    fn closed_depth_channel_is_reported_once_per_feed_handle() {
        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        drop(book_tx);

        app.drain_book_feed();
        assert!(app.book_channel_closed_reported);
        app.drain_book_feed();
        assert!(
            app.book_channel_closed_reported,
            "subsequent frames keep the one-shot diagnostic latched"
        );
    }

    #[test]
    fn replay_keeps_the_recorded_symbol_out_of_the_live_feed_snap() {
        // A recorded instrument no configured live feed offers must survive
        // the toolbar frame untouched — snapping it away would relabel the
        // whole session on the status bar and in the logs. The live path,
        // drawn through the very same frame, must keep snapping an invalid
        // selection back to the feed's list.
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        let text = "# quantick,csv,1\n# symbol=WINJ26\n# timezone=-03:00\n\
                    Date,Time,Price,Volume,Side\n\
                    2026-03-16,10:01:08.000,182035,12,B\n";
        let session = quantick_replay::Session::from_text(
            std::path::Path::new("WINJ26_2026-03-16.csv"),
            text,
            quantick_replay::ParseOptions::default(),
        )
        .expect("fixture session parses");
        app.open_replay(crate::feed::ReplayRequest {
            session: std::sync::Arc::new(session),
            options: crate::feed::ReplayOptions {
                autoplay: false,
                ..Default::default()
            },
        });
        assert_eq!(app.symbol, "WINJ26");

        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
        assert_eq!(
            app.symbol, "WINJ26",
            "a toolbar frame during replay must not relabel the session"
        );

        // The same frame path with the replay closed: validation still works.
        app.replay = None;
        app.symbol = "NOT-A-SYMBOL".to_owned();
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.draw_toolbar(ctx));
        assert_eq!(
            app.symbol, "TESTUSDT",
            "live selections keep snapping to the feed's symbol list"
        );
    }

    #[test]
    fn fmt_time_in_utc() {
        // Epoch: 1970-01-01 00:00:00 UTC, then +1h 2m 3s.
        assert_eq!(fmt_time(0, TzOffset::new(0)), "00:00:00");
        assert_eq!(fmt_time(3_723_000, TzOffset::new(0)), "01:02:03");
    }

    /// An app with `count` trades of history, split, and laid out by two real
    /// frames so both panes have reported their rects.
    fn split_app(ctx: &egui::Context, count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
        let (mut app, commands) = app_with_history(count);
        run_frame(&mut app, ctx);
        app.set_layout(CanvasLayout::TimeAndFlow);
        run_frame(&mut app, ctx);
        run_frame(&mut app, ctx);
        (app, commands)
    }

    /// Let every pane's indicator worker finish what it was sent, then apply
    /// its events — the two steps the frame loop takes, made deterministic.
    fn settle_indicators(app: &mut QuantickApp) {
        for pane in app.panes_mut() {
            pane.indicator_worker.flush();
            pane.apply_indicator_events();
        }
    }

    /// A point inside the pane on `side`, for a click that focuses it.
    fn pane_point(app: &QuantickApp, side: PaneSide) -> egui::Pos2 {
        app.pane(side)
            .last_chart_area
            .expect("the pane reported its rect")
            .center()
    }

    /// (a) The split really puts two charts on the canvas: two panes with
    /// their own laid-out rects, side by side, and a divider between them.
    #[test]
    fn enabling_the_split_lays_out_two_panes_and_a_divider() {
        let ctx = egui::Context::default();
        let (app, _commands) = split_app(&ctx, 200);

        let time = app
            .time_pane
            .as_ref()
            .expect("Time + Flow builds the time pane")
            .last_chart_area
            .expect("the time pane was laid out");
        let flow = app
            .flow_pane
            .last_chart_area
            .expect("the flow pane was laid out");
        assert!(
            time.right() <= flow.left(),
            "time pane left, flow pane right: {time:?} vs {flow:?}"
        );
        assert!(
            flow.left() - time.right() >= CANVAS_DIVIDER_PX,
            "the divider owns the pixels between them"
        );
        assert!(time.width() > 0.0 && flow.width() > 0.0);
    }

    /// Both panes paint: each keeps its own price axis, and the time pane
    /// carries the timeframe selector §11 gives it.
    #[test]
    fn both_panes_paint_and_the_time_pane_carries_its_own_selector() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let texts = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            has_price_axis(&texts),
            "the split canvas still paints a chart: {texts:?}"
        );
        for (label, _) in time_header::PRESETS {
            assert!(
                texts.iter().any(|text| text == label),
                "the time pane's header must offer {label:?}; painted: {texts:?}"
            );
        }
    }

    /// (b) One tape, two panes: the same trades reach both `ChartState`s, and
    /// each cuts them by its own spec — which is the whole point of the split.
    #[test]
    fn one_tape_feeds_both_panes_and_each_cuts_it_its_own_way() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let time = app.time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.state.trades().len(),
            app.flow_pane.state.trades().len(),
            "both panes hold the same tape"
        );
        assert_ne!(
            time.state.bars().len(),
            app.flow_pane.state.bars().len(),
            "tick(1) and M1 cannot agree on a bar count over the same trades"
        );

        // And a live trade after the split reaches both of them.
        let flow_before = app.flow_pane.state.trades().len();
        let time_before = app
            .time_pane
            .as_ref()
            .expect("time pane")
            .state
            .trades()
            .len();
        let trade = trade(500);
        app.ingest_live_trade_at(&trade, trade.timestamp_ms);
        assert_eq!(app.flow_pane.state.trades().len(), flow_before + 1);
        assert_eq!(
            app.time_pane
                .as_ref()
                .expect("time pane")
                .state
                .trades()
                .len(),
            time_before + 1,
            "a pane off the drain path would silently fall behind the market"
        );
    }

    /// Seeding a pane opened mid-session must not relabel live prints as
    /// history: the backfill divider is a data-honesty mark, not a decoration.
    #[test]
    fn a_pane_opened_late_keeps_the_backfill_boundary_honest() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(100);
        // Everything so far is backfill; now three prints arrive live.
        for id in 101..=103 {
            let trade = trade(id);
            app.ingest_live_trade_at(&trade, trade.timestamp_ms);
        }
        run_frame(&mut app, &ctx);
        app.set_layout(CanvasLayout::TimeAndFlow);

        let time = app.time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.state.trades().len(),
            103,
            "the new pane opens showing the market, not an empty chart"
        );
        assert_eq!(
            time.state.backfill_trade_count(),
            app.flow_pane.state.backfill_trade_count(),
            "the live prints must not become history in the second view"
        );
    }

    /// (c) The time pane's header governs the time pane and nothing else; the
    /// toolbar's BARS group keeps governing the flow pane (§11).
    #[test]
    fn a_timeframe_chip_moves_only_the_time_panes_spec() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_spec = app.flow_pane.state.spec().clone();
        assert_eq!(
            app.pane(PaneSide::Time).state.spec(),
            &BarSpec::Time(time_header::DEFAULT_INTERVAL_MS),
            "the time pane opens on M1, not on the flow selector's interval"
        );

        // The 15m chip, clicked where it was actually drawn.
        let (label, expected_ms) = time_header::PRESETS[2];
        let chip = app.time_header_chips[2];
        assert!(chip.is_positive(), "the {label} chip was laid out");
        click_chart(&mut app, &ctx, chip.center());
        // The spec change is deferred one frame, exactly as the toolbar's is.
        run_frame(&mut app, &ctx);
        run_frame(&mut app, &ctx);

        assert_eq!(
            app.pane(PaneSide::Time).state.spec(),
            &BarSpec::Time(expected_ms),
            "clicking {label} must re-cut the time pane"
        );
        assert_eq!(
            app.flow_pane.state.spec(),
            &flow_spec,
            "and must leave the chart beside it alone"
        );
    }

    /// (d) `Insert → Indicator` targets the focused pane (§11): the slot lands
    /// on the pane the user is working in, and only there.
    #[test]
    fn an_indicator_lands_on_the_focused_pane_only() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_before = app.flow_pane.indicators.all().len();

        let point = pane_point(&app, PaneSide::Time);

        click_chart(&mut app, &ctx, point);
        assert_eq!(
            app.focused_side(),
            PaneSide::Time,
            "clicking a pane focuses it"
        );

        app.apply_toolbar_action(ToolbarAction::AddEmaIndicator);
        settle_indicators(&mut app);

        let time = app.time_pane.as_ref().expect("time pane");
        assert_eq!(
            time.indicators.all().len(),
            1,
            "the EMA belongs to the pane that had focus"
        );
        assert!(
            time.indicators
                .all()
                .iter()
                .any(|view| view.label().contains("EMA")),
            "and it really built: {:?}",
            time.indicators
                .all()
                .iter()
                .map(|view| view.label().to_owned())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            app.flow_pane.indicators.all().len(),
            flow_before,
            "the pane beside it gains nothing"
        );
        // The persisted set is the flow pane's; a time-pane slot must not
        // enter it (see maintain_indicator_state).
        assert!(
            app.slot_kinds
                .iter()
                .all(|(owner, _)| owner.side == PaneSide::Time),
            "the only registered slot is the time pane's"
        );
    }

    /// Slot ids are per pane, so the same number means different indicators on
    /// the two of them. Removing one must not unregister the other.
    #[test]
    fn removing_a_slot_on_one_pane_leaves_the_same_number_on_the_other() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let point = pane_point(&app, PaneSide::Flow);

        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        app.apply_toolbar_action(ToolbarAction::AddCvdIndicator);
        settle_indicators(&mut app);
        assert_eq!(app.slot_kinds.len(), 2);
        let time_slot = app.time_pane.as_ref().expect("time pane").indicators.all()[0].slot;

        // Focused on the time pane: remove its slot.
        app.apply_toolbar_action(ToolbarAction::RemoveIndicator(time_slot.0));

        assert_eq!(
            app.slot_kinds.len(),
            1,
            "exactly one registration went with it"
        );
        assert_eq!(
            app.slot_kinds[0].0.side,
            PaneSide::Flow,
            "and the survivor is the flow pane's"
        );
        assert_eq!(app.flow_pane.indicators.all().len(), 1);
    }

    /// §11: flow layers stay on the flow pane. A time pane must not run a book
    /// worker, draw a lane, or claim strip pixels.
    #[test]
    fn the_time_pane_has_no_tape_and_no_flow_layers() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        app.apply_toolbar_action(ToolbarAction::SetLiveStrip(true));
        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        app.apply_toolbar_action(ToolbarAction::SetBubbles(true));
        run_frame(&mut app, &ctx);

        let time = app.time_pane.as_ref().expect("time pane");
        assert!(
            time.orderflow.is_none(),
            "no tape means no book worker behind it"
        );
        assert_eq!(
            time.live_strip_width(),
            0.0,
            "the strip is a flow layer and claims no pixels here"
        );
        assert!(
            time.last_lane_divider_x.is_none(),
            "and there is no live lane to divide"
        );
        // The toggles still reached the flow pane, which is what owns them.
        assert!(app.tape().depth_visible());
        assert!(app.tape().bubbles_enabled());
        assert!(app.flow_pane.live_strip_visible);
    }

    /// (e) Dragging the divider moves it, and stops at the quarter §11
    /// promises each pane.
    #[test]
    fn dragging_the_divider_resizes_the_panes_and_stops_at_the_minimum() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        let flow_before = app.flow_pane.last_chart_area.expect("laid out").width();
        let divider = app.canvas_divider.expect("the divider was registered");
        let grab = divider.center();

        drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 120.0, grab.y));
        run_frame(&mut app, &ctx);
        assert!(
            app.split_fraction > DEFAULT_PANE_FRACTION,
            "dragging right widens the time pane, got {}",
            app.split_fraction
        );
        assert!(
            app.flow_pane.last_chart_area.expect("laid out").width() < flow_before,
            "at the flow pane's expense"
        );

        // Now shove it far past the minimum: it stops, it does not collapse.
        for _ in 0..6 {
            let grab = app.canvas_divider.expect("registered").center();
            drag_chart(&mut app, &ctx, grab, egui::pos2(grab.x + 400.0, grab.y));
            run_frame(&mut app, &ctx);
        }
        assert!(
            (app.split_fraction - (1.0 - MIN_PANE_FRACTION)).abs() < 1e-3,
            "the flow pane keeps its quarter, got {}",
            app.split_fraction
        );
        assert!(app.flow_pane.last_chart_area.expect("laid out").width() > 0.0);
    }

    /// The keyboard's drawing grammar follows focus as well: Delete removes
    /// the selection on the pane the user is in, never its opposite number on
    /// the chart beside it.
    #[test]
    fn the_keyboard_deletes_from_the_focused_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        for side in [PaneSide::Flow, PaneSide::Time] {
            app.toolrail
                .arm(Tool::Drawing(drawing_tool("horizontal-line")));
            let point = pane_point(&app, side);
            click_chart(&mut app, &ctx, point);
        }
        assert_eq!(app.flow_pane.drawings.items().len(), 1);
        assert_eq!(app.pane(PaneSide::Time).drawings.items().len(), 1);
        assert_eq!(
            app.focused_side(),
            PaneSide::Time,
            "the last click was on the time pane"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);

        assert!(
            app.pane(PaneSide::Time).drawings.items().is_empty(),
            "Delete removes the focused pane's selection"
        );
        assert_eq!(
            app.flow_pane.drawings.items().len(),
            1,
            "and leaves the pane beside it untouched"
        );
    }

    /// The status bar's content section speaks for the focused pane (§11), so
    /// it always describes the chart the user is working in.
    #[test]
    fn the_status_bar_follows_the_focused_pane() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        let point = pane_point(&app, PaneSide::Flow);

        click_chart(&mut app, &ctx, point);
        let flow_status = app.status_model();
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        let time_status = app.status_model();

        assert_eq!(
            flow_status.spec_summary,
            app.flow_pane.state.spec().summary()
        );
        assert_eq!(
            time_status.spec_summary,
            app.pane(PaneSide::Time).state.spec().summary()
        );
        assert_ne!(
            flow_status.spec_summary, time_status.spec_summary,
            "the two panes report different specs, so the bar has to change"
        );
        // Provenance is the market's and never moves with focus.
        assert_eq!(flow_status.symbol, time_status.symbol);
        assert_eq!(flow_status.venue, time_status.venue);
    }

    /// Drawings are per pane, and the tool rail is one: an object lands on the
    /// pane under the cursor and stays out of the other's overlay.
    #[test]
    fn a_drawing_lands_on_the_pane_under_the_cursor() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);

        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);

        assert_eq!(
            app.pane(PaneSide::Time).drawings.items().len(),
            1,
            "the click landed on the time pane"
        );
        assert!(
            app.flow_pane.drawings.items().is_empty(),
            "and nowhere else"
        );

        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Flow);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.flow_pane.drawings.items().len(), 1);
        assert_eq!(
            app.pane(PaneSide::Time).drawings.items().len(),
            1,
            "placing on one pane must not add to the other"
        );
    }

    /// Going back to Single hides the context chart; it must not throw away
    /// what the user built on it, and it must keep following the market.
    #[test]
    fn leaving_the_split_keeps_the_time_panes_work_and_its_bars() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = split_app(&ctx, 200);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        let point = pane_point(&app, PaneSide::Time);
        click_chart(&mut app, &ctx, point);
        assert_eq!(app.pane(PaneSide::Time).drawings.items().len(), 1);

        app.set_layout(CanvasLayout::Single);
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.focused_side(),
            PaneSide::Flow,
            "a single canvas is the flow pane, whatever had focus"
        );

        let before = app.pane(PaneSide::Time).state.trades().len();
        let trade = trade(700);
        app.ingest_live_trade_at(&trade, trade.timestamp_ms);
        assert_eq!(
            app.pane(PaneSide::Time).state.trades().len(),
            before + 1,
            "a hidden pane keeps draining, so showing it again never catches up"
        );

        app.set_layout(CanvasLayout::TimeAndFlow);
        assert_eq!(
            app.pane(PaneSide::Time).drawings.items().len(),
            1,
            "and its drawings survived the round trip"
        );
    }

    /// The default layout is the one quantick opens on, and the split must not
    /// have quietly changed it: one pane, the whole canvas, no second worker.
    #[test]
    fn the_single_layout_still_gives_the_flow_pane_the_whole_canvas() {
        let ctx = egui::Context::default();
        let (mut app, _commands) = app_with_history(200);
        run_frame(&mut app, &ctx);

        assert_eq!(app.layout, CanvasLayout::Single);
        assert!(
            app.time_pane.is_none(),
            "an unsplit canvas builds no second pane, and no worker behind it"
        );
        let chart = app.flow_pane.last_chart_area.expect("laid out");
        // The canvas the pane was given, reconstructed from the rect it kept:
        // wider than half the window, so nothing was carved off for a divider.
        assert!(
            chart.width() > 600.0,
            "the flow pane still owns the canvas, got {chart:?}"
        );
        assert_eq!(app.focused_side(), PaneSide::Flow);
    }

    #[test]
    fn fmt_time_applies_the_offset() {
        // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
        assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
        // UTC midnight in UTC+05:30 is 05:30.
        assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
    }
}
