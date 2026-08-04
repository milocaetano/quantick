//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use tokio::sync::{mpsc, watch};

use quantick_feed_binance::depth::DepthEvent;
use smallvec::SmallVec;

use crate::candle_view::{draw_candle, draw_style_window};
use crate::chart::{self, PriceScale};
use crate::chart_layers::{self, ChartLayer};
use crate::config::{AppConfig, FeedCapabilities};
use crate::dock::{Dock, DockEnv, DockTab};
use crate::drawings::{
    self, ChartPoint, DeleteOutcome, DrawContext, Drawings, MAX_DRAWING_FILL_ALPHA,
    MAX_DRAWING_WIDTH_PX, MIN_DRAWING_WIDTH_PX, PresetHost,
};
use crate::feed::{
    self, FeedCommand, FeedConnectionState, FeedEvent, FeedHandle, FeedNotice, ReplayLink,
};
use crate::indicator_panel::{self, SettingsDialog, SettingsOutcome};
use crate::indicator_render::{self, PlotX};
use crate::indicator_worker::{
    IndicatorCommand, IndicatorEvent, IndicatorSource, IndicatorWorker, SlotId,
};
use crate::indicators::IndicatorViews;
use crate::indicators::library::ScriptLibrary;
use crate::indicators::state_file::{self, SavedIndicator, SavedInput, SavedKind};
use crate::loading::{self, LoadingTask, LoadingTracker};
use crate::metrics::{self, FrameStats};
use crate::notice_card;
use crate::orderflow_view::{OrderflowView, VisibleBarTimeline};
use crate::price_view::PriceView;
use crate::replay_view::{ReplayAction, ReplayView};
use crate::state::{BarKind, BarSpec, ChartState};
use crate::statusbar;
use crate::style::{CandlePreset, ChartStyle};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolbar::{self, ToolbarAction};
use crate::toolrail::{Tool, ToolRail};
use crate::viewport::Viewport;

/// Convert an explicit unmultiplied RGBA style colour to egui.
fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Width of the right-hand price-axis gutter, in pixels (§5 zone 9).
const AXIS_GUTTER: f32 = 64.0;
/// Height of the bottom time-axis strip, in pixels (§5 zone 6).
const TIME_STRIP: f32 = 24.0;
/// Hit radius for selecting a drawing anchor, in logical pixels.
const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;
/// Hit radius for a selected drawing's editable anchor.
const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;
/// Minimum pointer travel that turns one press/release into drag placement.
const DRAWING_DRAG_THRESHOLD_PX: f32 = 4.0;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DrawingDrag {
    #[default]
    None,
    Translate,
    Anchor {
        drawing_index: usize,
        point_index: usize,
    },
    /// The press landed on a locked drawing: the gesture belongs to the
    /// object (the chart must not pan) but the geometry stays put.
    Blocked,
}

impl DrawingDrag {
    const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

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

/// Alpha of the last-price line: legible at a glance without competing with a
/// candle or a bubble for attention.
const LAST_PRICE_LINE_ALPHA: f32 = 0.55;
/// Dash length, in pixels, of the last-price line. Dashed so it never reads as
/// a level someone drew.
const LAST_PRICE_DASH_PX: f32 = 4.0;
/// See [`LAST_PRICE_DASH_PX`].
const LAST_PRICE_GAP_PX: f32 = 4.0;
/// Ink on the last-price chip. The chip is filled with a saturated candle
/// colour, so its text is the one place on the chrome that goes dark.
const LAST_PRICE_CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x12, 0x1A);

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);
/// Coalesce slider drags into one diagnostic event after the value settles.
const STYLE_LOG_DEBOUNCE: Duration = Duration::from_millis(350);
/// Each UI capture epoch reserves room for reconnect generations. This keeps
/// late events from an aborted task below the next accepted generation floor.
const BOOK_GENERATION_STRIDE: u64 = 1_000_000;
/// Bound depth work per frame so a burst cannot starve egui input/rendering.
const BOOK_DRAIN_BUDGET: usize = 2_048;
/// Font size, in points, of the "nothing in view" line drawn where the candles
/// would be. Matches the "connecting…" line: same voice, same weight.
const EMPTY_VIEW_FONT_SIZE: f32 = 16.0;

/// Convert a UI `f64` parameter to a positive `Decimal` for a builder threshold.
fn dec_from_f64(x: f64) -> Decimal {
    Decimal::from_f64(x.max(1e-8)).unwrap_or(Decimal::ONE)
}

/// Half-width, in pixels, of the grab area over the live lane's divider.
///
/// The line itself stays a hairline — it marks where the present begins and a
/// thick rule there would read as a wall in the data. The handle around it is
/// what makes it draggable, and the resize cursor is the only thing that says
/// so.
const LANE_HANDLE_HALF_WIDTH_PX: f32 = 5.0;

/// Pixels of drag on the lane's own time strip that double or halve its window.
///
/// Matches the candles' own feel: dragging the time axis zooms it by
/// `exp(dx / 120)`, so the two panes answer a drag at the same rate even
/// though they are zooming different things.
const LANE_ZOOM_DRAG_PX: f32 = 120.0;

/// Split the padded plot area into the candle chart, the indicator panes, the
/// optional live strip, the right price gutter and the bottom time strip, so
/// the input handler and the renderer agree on the boundaries.
/// `live_strip_width` of zero means the strip is off and the chart runs
/// straight into the gutter, exactly as it did before the strip existed.
///
/// `pane_count` is the number of *visible* pane indicators: the band they
/// claim is carved here, once, rather than by each caller — a chart rect that
/// two call sites disagree about is two price scales for the same pixels.
fn plot_split(area: egui::Rect, live_strip_width: f32, pane_count: usize) -> PlotAreas {
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

/// Whether a pointer at `x` is over the live lane rather than the candles.
///
/// The divider itself counts as the lane, so the gesture that resizes it and
/// the gesture that pans the candles can never both fire on the same pixel.
/// Without a lane every pixel belongs to the candles, exactly as before.
fn gesture_hits_lane(divider_x: Option<f32>, x: f32) -> bool {
    divider_x.is_some_and(|divider| x >= divider)
}

/// Split the bottom time strip at the lane's divider: the candles' own time
/// axis on the left, the lane's on the right.
///
/// Each pane zooms from the strip under it, which is the only place a zoom
/// gesture can say *which* time axis it means. Without a divider the whole
/// strip belongs to the candles, exactly as it did before the lane had a zoom.
fn split_time_strip(strip: egui::Rect, divider_x: Option<f32>) -> (egui::Rect, Option<egui::Rect>) {
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

/// The interactive regions of the plot, plus the optional live strip.
struct PlotAreas {
    /// The candle body, with the indicator pane band already taken out of it.
    /// Every consumer — renderer and input handler alike — reads the chart
    /// rect from here, which is what keeps the price scale a drawing is
    /// placed against identical to the one it is hit-tested against.
    chart: egui::Rect,
    /// Stacked indicator panes below the candles, top to bottom. Empty when
    /// no pane indicator is visible.
    indicator_panes: Vec<egui::Rect>,
    /// Present only while the strip is shown; sits between `chart` and
    /// `price_gutter` and is not an input region.
    live_strip: Option<egui::Rect>,
    price_gutter: egui::Rect,
    time_strip: egui::Rect,
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
fn fmt_window(milliseconds: i64) -> String {
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
fn fmt_time(ms: i64, tz: TzOffset) -> String {
    let local = ms.saturating_add(tz.offset_ms());
    let secs = local.div_euclid(1000).rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// The quantick chart window.
pub struct QuantickApp {
    state: ChartState,
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
    orderflow: OrderflowView,
    /// Background thread owning the `IndicatorHost`; the UI only sends
    /// commands and applies the delta events back.
    indicator_worker: IndicatorWorker,
    /// The UI's copy of every indicator's plot columns (see
    /// [`crate::indicators`]).
    indicators: IndicatorViews,
    /// Loadable `.pine` scripts (embedded + indicators dir), scanned at
    /// startup. A file-backed script then follows its file: `poll_script_files`
    /// checks mtimes on a debounce and reloads on a save.
    script_library: ScriptLibrary,
    /// The open indicator-settings dialog, if any (one at a time).
    indicator_settings: Option<SettingsDialog>,
    /// File-backed script slots: (slot, library index, last seen mtime) —
    /// what the hot-reload poll walks.
    script_files: Vec<(SlotId, usize, std::time::SystemTime)>,
    /// How each live slot restores (the persistence identity per slot).
    slot_kinds: Vec<(SlotId, SavedKind)>,
    /// Slots restored as hidden, applied when their Rebuilt lands.
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
    /// Whether the user wants the live strip shown. The pixels it actually
    /// gets are still capability-gated — see [`Self::live_strip_width`].
    live_strip_visible: bool,

    // Layer visibility (the right-click menu on the canvas). Only the layers
    // no other field owns are held here; every other entry resolves to its
    // existing owner in `layer_visible`/`set_layer_visible`, so the menu can
    // never hold a second opinion about a pixel.
    /// Layers switched off that this struct is the owner of.
    hidden_layers: BTreeSet<ChartLayer>,
    /// Where layer visibility persists.
    chart_layers_path: std::path::PathBuf,
    /// The visibility already on disk, as a bitmask. Compared with the live one
    /// once per frame so a switch is saved whoever flipped it — the menu, the
    /// toolbar, the dock or the appearance panel. Nine bool reads and an
    /// integer compare: cheaper than teaching four call sites to remember.
    saved_layer_mask: u16,

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
    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    drawings: Drawings,

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

    // Bar-type selector state (one parameter retained per kind).
    kind: BarKind,
    // The spec the selectors ask for, applied one frame after they settle so
    // the frame carrying the change paints the loading overlay before the
    // synchronous rebuild holds this thread. See apply_spec_change.
    pending_spec: Option<BarSpec>,
    tick_n: u64,
    volume_units: f64,
    dollar_notional: f64,
    time_interval_ms: i64,
    imbalance_target: u64,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    viewport: Viewport,
    // Where the history pane ended last frame — the lane's divider, and the
    // handle that resizes it. The input pass runs before the draw computes it.
    last_lane_divider_x: Option<f32>,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    price_view: PriceView,
    // Last frame's auto-fit price range and chart height, for pixel↔price maths
    // in the input handler (which runs before the draw computes them).
    last_auto_range: Option<(f64, f64)>,
    last_chart_height: f32,
    last_chart_top: f32,
    // Pointer position over the plot this frame, for the crosshair.
    hover_pos: Option<egui::Pos2>,
    // Drawing placement/movement state. Anchors are chart coordinates; only
    // the current hover and press position are transient pixels.
    drawing_hover: Option<ChartPoint>,
    drawing_press_position: Option<egui::Pos2>,
    drawing_press_started_empty: bool,
    drawing_drag: DrawingDrag,
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
    // The chart pane from the last frame (excludes axes and the live lane),
    // for inspector placement and manager centring.
    last_chart_area: Option<egui::Rect>,
    drawing_manager_open: bool,
    // Custom drawing presets (named payload exports + default-for-new),
    // persisted across restarts in a versioned file.
    drawing_presets: drawings::presets::PresetStore,
    /// Where each layer's switch landed in the last menu frame, so a test can
    /// click the real widget instead of calling the setter behind it.
    #[cfg(test)]
    layer_menu_rects: Vec<(ChartLayer, egui::Rect)>,
    #[cfg(test)]
    inspector_pin_rect: Option<egui::Rect>,
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
        // Defaults for every kind, with the initial spec's parameter applied.
        let mut tick_n = 50;
        let mut volume_units = 5.0;
        let mut dollar_notional = 500_000.0;
        let mut time_interval_ms = 1_000;
        let mut imbalance_target = 100;
        match &spec {
            BarSpec::Tick(n) => tick_n = *n,
            BarSpec::Volume(u) => volume_units = u.to_f64().unwrap_or(volume_units),
            BarSpec::Dollar(d) => dollar_notional = d.to_f64().unwrap_or(dollar_notional),
            BarSpec::Time(ms) => time_interval_ms = *ms,
            BarSpec::Imbalance(target) => imbalance_target = *target,
        }

        let mut loading = LoadingTracker::new();
        // The feed starts backfilling the moment it is spawned, so the chart
        // opens with that one load already in flight.
        loading.begin(LoadingTask::History);

        let mut app = Self {
            kind: spec.kind(),
            state: ChartState::new(spec),
            events: feed.events,
            book_events: feed.book_events,
            notices: feed.notices,
            feed_capabilities: feed.capabilities,
            notice: FeedNotice::Clear,
            feed_connection: FeedConnectionState::Connecting,
            commands: feed.commands,
            orderflow: OrderflowView::new(symbol.clone()),
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            script_library: ScriptLibrary::scan(),
            indicator_settings: None,
            script_files: Vec::new(),
            slot_kinds: Vec::new(),
            pending_hidden: Vec::new(),
            indicator_state_path: state_file::default_path(),
            indicator_state_dirty: false,
            last_indicator_change: None,
            last_script_poll: Instant::now(),
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            live_strip_visible: false,
            hidden_layers: BTreeSet::new(),
            chart_layers_path: chart_layers::default_path(),
            saved_layer_mask: 0,
            active: (feed_id.clone(), symbol.clone()),
            replay: feed.replay,
            replay_view: ReplayView::new(),
            dock: Dock::new(),
            toolrail: ToolRail::new(),
            drawings: Drawings::default(),
            config,
            feed_id,
            symbol,
            history_step: 2000,
            history_trades: 0,
            loading,
            pending_spec: None,
            tick_n,
            volume_units,
            dollar_notional,
            time_interval_ms,
            imbalance_target,
            viewport: Viewport::new(),
            price_view: PriceView::new(),
            last_lane_divider_x: None,
            last_auto_range: None,
            last_chart_height: 1.0,
            last_chart_top: 0.0,
            hover_pos: None,
            drawing_hover: None,
            drawing_press_position: None,
            drawing_press_started_empty: false,
            drawing_drag: DrawingDrag::None,
            drawing_delete_confirm: false,
            inspector_edit_baseline: None,
            drawing_toast: None,
            inspector_tab: InspectorTab::default(),
            inspector_pinned: false,
            inspector_moved: false,
            inspector_last_selection: None,
            last_chart_area: None,
            drawing_manager_open: false,
            drawing_presets: drawings::presets::PresetStore::load_from(
                drawings::presets::PresetStore::default_path(),
            ),
            #[cfg(test)]
            layer_menu_rects: Vec::new(),
            #[cfg(test)]
            inspector_pin_rect: None,
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
        // requested must cost no projection. Capture is already running either
        // way, so this is a display choice and nothing else.
        app.orderflow.set_depth_visible(false);
        // What the user last had on the canvas, applied over those defaults and
        // under the autostart hooks below: an env var is an explicit request
        // for this run and must still win (see `restore_chart_layers`).
        app.restore_chart_layers();
        // Dev/ops can open the map without a click.
        if std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|value| value == "1") {
            app.orderflow.set_depth_visible(true);
        }
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            app.live_strip_visible = true;
        }
        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            app.orderflow.set_bubbles_enabled(true);
        }
        // Same convenience for indicators: open with the two M1 natives on
        // (EMA overlay + CVD pane), through the same code path the toolbar
        // menu takes, so a scripted validation run needs no clicks.
        if std::env::var("QUANTICK_INDICATORS_AUTOSTART").is_ok_and(|value| value == "1") {
            app.add_indicator(IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            });
            app.add_indicator(IndicatorSource::NativeCvd);
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
        // An env var is not a user edit: what the autostart hooks switched on
        // must not be written back as though the user had asked for it every
        // launch from now on. Same rule the indicator state follows.
        app.saved_layer_mask = app.layer_mask();
        app
    }

    /// The bar spec implied by the current selector state.
    fn current_spec(&self) -> BarSpec {
        match self.kind {
            BarKind::Tick => BarSpec::Tick(self.tick_n.max(1)),
            BarKind::Volume => BarSpec::Volume(dec_from_f64(self.volume_units)),
            BarKind::Dollar => BarSpec::Dollar(dec_from_f64(self.dollar_notional)),
            BarKind::Time => BarSpec::Time(self.time_interval_ms.max(1)),
            BarKind::Imbalance => BarSpec::Imbalance(self.imbalance_target.max(1)),
        }
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
        let heatmap_on = self.orderflow.depth_visible();
        let bubbles_on = self.orderflow.bubbles_enabled();
        let mut model = toolbar::ToolbarModel {
            feeds,
            feed_id: &mut self.feed_id,
            feed_display_name,
            symbols,
            symbol: &mut self.symbol,
            replay,
            kind: &mut self.kind,
            tick_n: &mut self.tick_n,
            volume_units: &mut self.volume_units,
            dollar_notional: &mut self.dollar_notional,
            time_interval_ms: &mut self.time_interval_ms,
            imbalance_target: &mut self.imbalance_target,
            history_step: &mut self.history_step,
            history_trades: self.history_trades,
            capabilities,
            heatmap_on,
            bubbles_on,
            live_strip_on: self.live_strip_visible,
            dock_visible: self.dock.visible(),
            appearance_open: self.show_style,
            indicators: self
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
                .collect(),
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
            ToolbarAction::SetHeatmap(shown) => self.orderflow.set_depth_visible(shown),
            ToolbarAction::SetBubbles(enabled) => self.orderflow.set_bubbles_enabled(enabled),
            ToolbarAction::SetLiveStrip(shown) => self.live_strip_visible = shown,
            ToolbarAction::OpenDockTab(tab) => self.dock.open_tab(tab),
            ToolbarAction::ToggleDock => self.dock.toggle_visible(),
            ToolbarAction::ToggleAppearance => self.show_style = !self.show_style,
            ToolbarAction::AddEmaIndicator => {
                let slot = self.add_indicator(IndicatorSource::NativeEma {
                    len: DEFAULT_EMA_LEN,
                    source: quantick_indicators::SourceId::Close,
                });
                self.slot_kinds.push((slot, SavedKind::NativeEma));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::AddCvdIndicator => {
                let slot = self.add_indicator(IndicatorSource::NativeCvd);
                self.slot_kinds.push((slot, SavedKind::NativeCvd));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::ToggleIndicatorHidden(slot) => {
                self.indicators.toggle_hidden(SlotId(slot));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::RemoveIndicator(slot) => {
                // UI first (the entry vanishes this frame), worker second;
                // events already in flight for the slot are dropped on apply.
                self.indicators.remove(SlotId(slot));
                self.indicator_worker
                    .send(IndicatorCommand::Remove(SlotId(slot)));
                self.slot_kinds.retain(|(s, _)| *s != SlotId(slot));
                self.script_files.retain(|(s, ..)| *s != SlotId(slot));
                self.mark_indicator_state_dirty();
            }
            ToolbarAction::AddScriptIndicator(index) => {
                self.add_script_indicator(index);
            }
            ToolbarAction::OpenIndicatorSettings(slot) => {
                let slot = SlotId(slot);
                if let Some(view) = self.indicators.all().iter().find(|v| v.slot == slot) {
                    self.indicator_settings = Some(SettingsDialog {
                        slot,
                        title: view.label().to_owned(),
                        draft: view.input_values.clone(),
                    });
                }
            }
        }
    }

    /// Draw the settings dialog and execute its outcome. Apply goes through
    /// the worker (construct anew, replace, replay) — the same path every
    /// input change takes, UI or not.
    fn draw_indicator_settings(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.indicator_settings.as_mut() else {
            return;
        };
        let Some(view) = self
            .indicators
            .all()
            .iter()
            .find(|view| view.slot == dialog.slot)
        else {
            // The indicator was removed under the dialog.
            self.indicator_settings = None;
            return;
        };
        match indicator_panel::draw(ctx, dialog, &view.descriptor.inputs) {
            SettingsOutcome::Open => {}
            SettingsOutcome::Cancel => self.indicator_settings = None,
            SettingsOutcome::Apply => {
                let dialog = self.indicator_settings.take().expect("dialog is open");
                self.indicator_worker.send(IndicatorCommand::SetInputs {
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
        match self.script_library.read(index) {
            Some(Ok(text)) => {
                let slot = self.add_indicator(IndicatorSource::Script {
                    name: name.clone(),
                    text,
                });
                // Watch the file so a save reloads it. Registered here, with
                // the add, so the two cannot drift apart.
                if let Some((_, mtime)) = self.script_library.file_info(index) {
                    self.script_files.push((slot, index, mtime));
                }
                self.slot_kinds.push((slot, SavedKind::Script { name }));
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
                let slot = self.indicators.allocate_slot();
                self.indicators.apply(IndicatorEvent::Rebuilt {
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
                self.indicators.apply(IndicatorEvent::Error {
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

    /// Width reserved for the live strip this frame. No capability gate any
    /// more: the aggression histogram runs on the trade stream, which every
    /// source provides (replay included), and without book data the strip
    /// honestly degrades to that histogram alone.
    fn live_strip_width(&self) -> f32 {
        if self.live_strip_visible {
            crate::live_strip::LIVE_STRIP_WIDTH_PX
        } else {
            0.0
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
        if self.orderflow.enabled() || !self.capabilities().book_capture {
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
            Ok(()) => self.orderflow.set_enabled(enabled, generation),
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
        if !self.orderflow.enabled() {
            return;
        }
        let generation = self.next_book_generation();
        match self.commands.try_send(FeedCommand::RestartBookCapture {
            initial_generation: generation,
        }) {
            Ok(()) => self.orderflow.accept_capture_grouping_restart(generation),
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.orderflow
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
                self.orderflow
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

        // Rebuild the chart from scratch for the new stream, keeping the current
        // bar spec. Retained trades from the old symbol must not leak in.
        self.state = ChartState::new(self.current_spec());
        self.viewport = Viewport::new();
        self.price_view = PriceView::new();
        self.last_auto_range = None;
        self.hover_pos = None;
        self.reset_drawing_overlay();
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.loading.restart(LoadingTask::History);
        self.latest_trade_latency_ms = None;
        self.orderflow.reset_for_symbol(self.symbol.clone());

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
        let applied = self.orderflow.apply_preset(&name);
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

    /// Apply a bar-type/parameter change one frame after the selectors settle.
    ///
    /// Switching the spec replays every retained trade synchronously, which
    /// can hold this thread long enough to notice on a deep history. Deferring
    /// the rebuild by one frame lets the frame that carries the change paint
    /// the loading overlay first, so the wait reads as the chart working
    /// rather than the app hanging. A selector still moving (a dragged
    /// parameter) keeps pushing the pending spec forward, which also debounces
    /// the rebuild to one per gesture.
    fn apply_spec_change(&mut self) {
        let desired = self.current_spec();
        if desired == *self.state.spec() {
            // Selection and chart agree — nothing is pending any more (a feed
            // switch or reset may have rebuilt the state under a pending spec).
            if self.pending_spec.take().is_some() {
                self.loading.set_active(LoadingTask::BarRebuild, false);
            }
            return;
        }
        match self.pending_spec.take() {
            // The frame that changed the selector: arm the indicator, paint.
            None => {
                self.pending_spec = Some(desired);
                self.loading.set_active(LoadingTask::BarRebuild, true);
            }
            // Still moving: wait for the selector to settle for a frame.
            Some(pending) if pending != desired => self.pending_spec = Some(desired),
            // Settled since last frame: do the rebuild.
            Some(_) => {
                // Where the user is looking, in market time — the one thing a
                // rebuild preserves. The new series cuts the same trades into
                // a different number of bars, so the old right-edge *index*
                // may not exist in it at all: keeping it would leave the
                // window past the end of the data, drawing nothing.
                let anchor = self.right_edge_time();
                self.state.set_spec(desired);
                self.send_indicator_rebuild();
                self.reset_drawing_overlay();
                self.viewport.reanchor(
                    anchor.and_then(|ms| self.state.slot_at_time(ms)),
                    self.slots(),
                );
                self.loading.set_active(LoadingTask::BarRebuild, false);
            }
        }
    }

    /// How many bar slots the chart draws: the closed bars plus the forming
    /// one, which occupies the slot after them.
    fn slots(&self) -> usize {
        self.state.bars().len() + usize::from(self.state.partial().is_some())
    }

    /// The market time under the right edge of the candles' pane, or `None`
    /// while the view follows live (the right edge is the newest bar by
    /// definition, so there is nothing to remember) or when there are no bars.
    fn right_edge_time(&self) -> Option<i64> {
        if self.viewport.follows_live() {
            return None;
        }
        let slots = self.slots();
        let edge = self.viewport.right_edge_bar(slots);
        // Panning into the empty space past the newest bar puts the edge off
        // the series; the newest bar is the market time it is closest to.
        let slot = (edge.floor().max(0.0) as usize).min(slots.saturating_sub(1));
        self.state.slot_open_time(slot)
    }

    /// The current background colour as an egui `Color32`.
    fn bg(&self) -> egui::Color32 {
        color32(self.style.canvas.background_rgba())
    }

    /// The current chart-grid colour. `TRANSPARENT` disables grid painting
    /// without branching throughout the axis code.
    fn grid(&self) -> egui::Color32 {
        self.style
            .canvas
            .grid_rgba()
            .map_or(egui::Color32::TRANSPARENT, color32)
    }

    /// Whether `layer` is painted right now.
    ///
    /// Every arm reads the one field that already owns that layer — the menu
    /// keeps no copy, so it and the toolbar/dock cannot disagree about a pixel.
    /// Only the four layers nothing else owned before this menu existed live in
    /// `hidden_layers`.
    fn layer_visible(&self, layer: ChartLayer) -> bool {
        match layer {
            ChartLayer::Heatmap => self.orderflow.depth_visible(),
            ChartLayer::Bubbles => self.orderflow.bubbles_enabled(),
            ChartLayer::LiveStrip => self.live_strip_visible,
            ChartLayer::LaneMarks => self.orderflow.lane_marks_visible(),
            ChartLayer::DepthGaps => self.orderflow.gaps_visible(),
            ChartLayer::Grid => self.style.canvas.grid_enabled,
            // The toolbox's global eye already owns this one, undo history and
            // all; the menu is a second door to the same switch.
            ChartLayer::Drawings => !self.drawings.all_hidden(),
            ChartLayer::LastPrice | ChartLayer::BackfillDivider | ChartLayer::Crosshair => {
                !self.hidden_layers.contains(&layer)
            }
        }
    }

    /// Show or hide `layer`, writing through to whoever owns it.
    ///
    /// Display only: nothing here stops depth capture, bar building or
    /// indicator computation, so unhiding repaints the retained past instead of
    /// opening a hole in it.
    fn set_layer_visible(&mut self, layer: ChartLayer, visible: bool) {
        match layer {
            ChartLayer::Heatmap => self.orderflow.set_depth_visible(visible),
            ChartLayer::Bubbles => self.orderflow.set_bubbles_enabled(visible),
            ChartLayer::LiveStrip => self.live_strip_visible = visible,
            ChartLayer::LaneMarks => self.orderflow.set_lane_marks_visible(visible),
            ChartLayer::DepthGaps => self.orderflow.set_gaps_visible(visible),
            ChartLayer::Grid => {
                self.style.canvas.grid_enabled = visible;
                // The appearance panel's own edits bump this; the renderer and
                // the style log both read it to know something moved.
                self.style_revision = self.style_revision.saturating_add(1);
            }
            ChartLayer::Drawings => self.drawings.set_all_hidden(!visible),
            ChartLayer::LastPrice | ChartLayer::BackfillDivider | ChartLayer::Crosshair => {
                if visible {
                    self.hidden_layers.remove(&layer);
                } else {
                    self.hidden_layers.insert(layer);
                }
            }
        }
    }

    /// Why `layer` cannot be shown for the running feed, if it cannot.
    ///
    /// A layer the source cannot produce is *unavailable*, not hidden: the menu
    /// shows the entry disabled with the reason, the same wording the toolbar
    /// uses, rather than offering a switch that would do nothing.
    ///
    /// `capabilities` is passed in rather than read here so one menu frame
    /// resolves the running feed once instead of once per entry.
    fn layer_blocked(layer: ChartLayer, capabilities: FeedCapabilities) -> Option<&'static str> {
        match layer {
            ChartLayer::Heatmap | ChartLayer::DepthGaps => (!capabilities.book_capture)
                .then_some("order-book capture is not available for this source"),
            ChartLayer::Bubbles => (!capabilities.traded_volume)
                .then_some("this source quotes prices but prints no traded volume"),
            _ => None,
        }
    }

    /// Current visibility of every layer this app persists, for the state file.
    fn persisted_layer_states(&self) -> std::collections::BTreeMap<ChartLayer, bool> {
        ChartLayer::ALL
            .into_iter()
            .filter(|layer| layer.persisted())
            .map(|layer| (layer, self.layer_visible(layer)))
            .collect()
    }

    /// The same visibility as one bit per persisted layer, for change
    /// detection. `ALL` is ten entries, so the mask can never outgrow `u16`.
    fn layer_mask(&self) -> u16 {
        ChartLayer::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, layer)| layer.persisted() && self.layer_visible(*layer))
            .fold(0_u16, |mask, (bit, _)| mask | (1 << bit))
    }

    /// Save the layer visibility when it differs from what is on disk.
    ///
    /// Called once per frame instead of from each switch: the layers are owned
    /// by four different pieces of chrome, and a save hook on each is four
    /// chances to forget one.
    fn maintain_chart_layers(&mut self) {
        let mask = self.layer_mask();
        if mask == self.saved_layer_mask {
            return;
        }
        self.save_chart_layers();
        self.saved_layer_mask = mask;
    }

    /// Apply the saved layer visibility.
    ///
    /// Runs before the autostart env vars so an explicit `QUANTICK_*_AUTOSTART`
    /// still wins for the run it was set on: a validation session asks for the
    /// heatmap on the command line and gets it, whatever the file remembers.
    fn restore_chart_layers(&mut self) {
        let saved = chart_layers::load(&self.chart_layers_path);
        // Whatever the file said (including nothing at all) is now on disk;
        // only a change from here is worth another write.
        if saved.is_empty() {
            self.saved_layer_mask = self.layer_mask();
            return;
        }
        let hidden = saved.values().filter(|visible| !**visible).count();
        for (layer, visible) in saved {
            self.set_layer_visible(layer, visible);
        }
        self.saved_layer_mask = self.layer_mask();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_RESTORED",
            path = %self.chart_layers_path.display(),
            hidden,
            "chart layer visibility restored"
        );
    }

    /// Write the layer visibility out. Only ever reached from
    /// [`Self::maintain_chart_layers`], which has already established that the
    /// canvas differs from the file — a switch is a click, so there is nothing
    /// here to debounce (unlike indicator inputs, which a drag changes at frame
    /// rate).
    fn save_chart_layers(&self) {
        chart_layers::save(&self.chart_layers_path, &self.persisted_layer_states());
    }

    /// The canvas right-click menu: one entry per chart layer, then one per
    /// active indicator.
    ///
    /// The indicator entries drive `IndicatorViews::toggle_hidden` — the same
    /// state the toolbar's eye writes — so an indicator hidden here shows as
    /// hidden there, and the indicator state file remains its single home.
    fn draw_layer_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("chart layers")
                .size(11.0)
                .color(theme::TEXT_MUTED),
        );
        #[cfg(test)]
        self.layer_menu_rects.clear();
        let capabilities = self.capabilities();
        for layer in ChartLayer::ALL {
            let blocked = Self::layer_blocked(layer, capabilities);
            let mut visible = self.layer_visible(layer);
            let response = ui
                .add_enabled(
                    blocked.is_none(),
                    egui::Checkbox::new(&mut visible, layer.label()),
                )
                .on_hover_text(layer.hint());
            #[cfg(test)]
            self.layer_menu_rects.push((layer, response.rect));
            if let Some(reason) = blocked {
                response.on_disabled_hover_text(reason);
            } else if response.changed() {
                self.set_layer_visible(layer, visible);
            }
        }

        // Borrowed straight from the view list — no per-frame copy of the
        // labels — and the one mutation waits until the loop lets go.
        let mut toggled = None;
        if !self.indicators.all().is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new("indicators")
                    .size(11.0)
                    .color(theme::TEXT_MUTED),
            );
            for view in self.indicators.all() {
                let mut visible = !view.hidden;
                if ui
                    .checkbox(&mut visible, view.label())
                    .on_hover_text("hide/show without removing (no recompute)")
                    .changed()
                {
                    toggled = Some(view.slot);
                }
            }
        }
        if let Some(slot) = toggled {
            self.indicators.toggle_hidden(slot);
            self.mark_indicator_state_dirty();
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
                    self.state.ingest_backfill(&trades);
                    self.indicator_worker
                        .send(IndicatorCommand::Backfilled(self.state.bars().to_vec()));
                    self.indicator_worker.send(IndicatorCommand::PartialUpdated(
                        self.state.partial().cloned(),
                    ));
                }
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    // The reply — even an empty one — answers exactly one
                    // pending load; the indicator survives until the last one.
                    self.loading.end(LoadingTask::History);
                    // Older bars shift every index up; keep the view steady.
                    self.history_trades += trades.len();
                    let added = self.state.prepend_history(&trades);
                    self.viewport.shift_right_edge(added);
                    self.drawings.shift_bars(added);
                    // Indicator columns shift with them: the rebuild below is
                    // a round-trip away, and until it lands every value would
                    // otherwise be drawn `added` slots off its own candle.
                    self.indicators.shift_rows(added);
                    // Older trades re-cut every bar; replay from scratch.
                    self.send_indicator_rebuild();
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
    fn ingest_live_trade_at(&mut self, trade: &quantick_engine::Trade, received_at_ms: i64) {
        self.latest_trade_latency_ms =
            metrics::feed_lag_ms(received_at_ms, Some(trade.timestamp_ms));
        self.latest_trade_ms = Some(trade.timestamp_ms);
        self.live_trades += 1;
        self.trades_since_summary += 1;
        self.orderflow.record_trade(trade);
        let bars_before = self.state.bars().len();
        self.state.ingest_live(trade);
        // At most one bar closes per trade (an atomic market event is never
        // split), so "grew" identifies exactly the bar that closed.
        if self.state.bars().len() > bars_before
            && let Some(closed) = self.state.bars().last()
        {
            self.indicator_worker
                .send(IndicatorCommand::BarClosed(closed.clone()));
        }
        self.indicator_worker.send(IndicatorCommand::PartialUpdated(
            self.state.partial().cloned(),
        ));
    }

    /// Ask the worker to replay the chart's bars from scratch — the one
    /// command behind spec switches, prepended history and source resets, so
    /// indicators inherit correct behavior for every rebuild path.
    fn send_indicator_rebuild(&mut self) {
        self.indicator_worker.send(IndicatorCommand::Rebuild(
            self.state.bars().to_vec(),
            self.state.partial().cloned(),
        ));
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
        let mut reloads: Vec<(SlotId, String, String)> = Vec::new();
        for (slot, index, seen_mtime) in &mut self.script_files {
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
                    reloads.push((*slot, name, text));
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
        for (slot, name, text) in reloads {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_SCRIPT_RELOAD",
                script = %name,
                action = "recompile_and_replay",
                "indicator script changed on disk"
            );
            self.indicator_worker.send(IndicatorCommand::Reload {
                slot,
                source: IndicatorSource::Script { name, text },
            });
        }
    }

    /// Reserve a slot and ask the worker to instantiate `source` behind it.
    fn add_indicator(&mut self, source: IndicatorSource) -> SlotId {
        let slot = self.indicators.allocate_slot();
        self.indicator_worker
            .send(IndicatorCommand::Add { slot, source });
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
    fn restore_indicator_state(&mut self) {
        let saved = state_file::load(&self.indicator_state_path);
        for entry in saved {
            let slot = match &entry.kind {
                SavedKind::NativeEma => {
                    let slot = self.add_indicator(IndicatorSource::NativeEma {
                        len: DEFAULT_EMA_LEN,
                        source: quantick_indicators::SourceId::Close,
                    });
                    self.slot_kinds.push((slot, SavedKind::NativeEma));
                    Some(slot)
                }
                SavedKind::NativeCvd => {
                    let slot = self.add_indicator(IndicatorSource::NativeCvd);
                    self.slot_kinds.push((slot, SavedKind::NativeCvd));
                    Some(slot)
                }
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
                self.indicator_worker
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
    fn maintain_indicator_state(&mut self) {
        if !self.pending_hidden.is_empty() {
            let existing: Vec<SlotId> = self
                .pending_hidden
                .iter()
                .copied()
                .filter(|slot| self.indicators.all().iter().any(|v| v.slot == *slot))
                .collect();
            for slot in &existing {
                self.indicators.toggle_hidden(*slot);
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
                .indicators
                .all()
                .iter()
                .filter_map(|view| {
                    let kind_ref = self
                        .slot_kinds
                        .iter()
                        .find(|(slot, _)| *slot == view.slot)
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
        self.state = ChartState::new(self.current_spec());
        // Indicators follow the chart into the empty state; the refill's
        // Backfilled event replays them (replay seek funnels through here,
        // so seeking inherits correct indicator behavior for free).
        self.send_indicator_rebuild();
        self.viewport = Viewport::new();
        self.price_view = PriceView::new();
        self.last_auto_range = None;
        self.hover_pos = None;
        self.reset_drawing_overlay();
        self.history_trades = 0;
        self.latest_trade_latency_ms = None;
        self.latest_trade_ms = None;
        self.last_lane_divider_x = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.loading.restart(LoadingTask::History);
        self.orderflow.reset_for_symbol(self.symbol.clone());
    }

    /// Bar-index anchors are meaningful only for the market/spec that created
    /// them. Clear them on a source or aggregation rebuild rather than
    /// silently attaching a mark to different market data — and say so.
    fn reset_drawing_overlay(&mut self) {
        let had_drawings = !self.drawings.items().is_empty();
        self.drawings.clear();
        self.toolrail.arm(Tool::Pointer);
        self.drawing_hover = None;
        self.drawing_press_position = None;
        self.drawing_press_started_empty = false;
        self.drawing_drag = DrawingDrag::None;
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
                    self.orderflow.handle_depth_event_at(event, received_at_ms);
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    if self.orderflow.enabled() && !self.book_channel_closed_reported {
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
        let newest = match (self.latest_trade_ms, self.orderflow.last_event_ms()) {
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
        let book = self.orderflow.health();
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
            bar_spec = self.state.spec().summary(),
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
        self.orderflow.reset_summary_counters();
        self.last_summary = now;
    }

    /// Convert a chart pixel into an overlay anchor. The x coordinate is a
    /// fractional bar slot, so drawings follow pan/zoom instead of being stuck
    /// to one screen pixel.
    fn drawing_point_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
    ) -> Option<ChartPoint> {
        let (auto_lo, auto_hi) = self.last_auto_range?;
        if total == 0 || self.last_chart_height <= 1.0 {
            return None;
        }
        let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.last_chart_top,
            self.last_chart_top + self.last_chart_height,
        );
        let bar = self.viewport.right_edge_bar(total) + 0.5
            - (history_right - pos.x) / self.viewport.candle_width();
        Some(ChartPoint {
            bar,
            price: scale.price_at(pos.y),
        })
    }

    /// Placement consumes clicks while a drawing tool is armed, preventing a
    /// mark from also panning the chart. A completed object returns to Pointer,
    /// matching the one-shot TradingView interaction.
    fn handle_drawing_placement(&mut self, ui: &egui::Ui, area: egui::Rect) -> bool {
        let Some(tool) = self.toolrail.tool().drawing_tool() else {
            self.drawings.cancel_draft();
            self.drawing_hover = None;
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
            return false;
        };
        let areas = plot_split(
            area,
            self.live_strip_width(),
            self.indicators.visible_panes().count(),
        );
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
        let history = egui::Rect::from_min_max(
            areas.chart.min,
            egui::pos2(history_right, areas.chart.bottom()),
        );
        let response = ui.interact(
            history,
            egui::Id::new("drawing_placement"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = response.hover_pos();
        self.drawing_hover = response
            .hover_pos()
            .and_then(|position| self.drawing_point_at(position, history_right, self.slots()));
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        let pressed_position = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if let Some(position) = pressed_position.filter(|position| history.contains(*position))
            && let Some(point) = self.drawing_point_at(position, history_right, self.slots())
        {
            self.drawing_press_started_empty = self.drawings.draft_len() == 0;
            self.drawing_press_position = Some(position);
            self.place_drawing_point(tool, point);
        }

        let released_position = ui.input(|input| {
            input
                .pointer
                .primary_released()
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        if tool.required_points() > 1
            && self.drawing_press_started_empty
            && let Some(start) = self.drawing_press_position
            && let Some(position) = released_position
            && history.contains(position)
            && start.distance(position) >= DRAWING_DRAG_THRESHOLD_PX
            && let Some(point) = self.drawing_point_at(position, history_right, self.slots())
        {
            self.place_drawing_point(tool, point);
        }
        if released_position.is_some() {
            self.drawing_press_position = None;
            self.drawing_press_started_empty = false;
        }
        true
    }

    fn place_drawing_point(&mut self, tool: drawings::DrawingTool, point: ChartPoint) {
        // A new object starts from the user's explicit default preset when
        // one is set; existing objects are never touched by that choice.
        let presets = &self.drawing_presets;
        let completed = self.drawings.place_with(tool, point, |tool| {
            let mut payload = tool.default_payload();
            if let Some(name) = presets.default_preset(tool.id())
                && let Some(value) = presets.load_custom_preset(tool.id(), &name)
            {
                payload.import_preset(&value);
            }
            payload
        });
        if completed {
            // One-shot by default; the toolbox repeat pin keeps the tool
            // armed for the next object.
            if !self.toolrail.repeat() {
                self.toolrail.arm(Tool::Pointer);
            }
            self.drawing_hover = None;
        }
    }

    fn projected_drawing_points(
        &self,
        drawing: &drawings::Drawing,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> SmallVec<[egui::Pos2; 4]> {
        drawing
            .points
            .iter()
            .map(|point| self.drawing_screen_point(*point, history_right, total, scale))
            .collect()
    }

    fn drawing_at(
        &self,
        pos: egui::Pos2,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        self.drawings
            .items()
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, _)| self.drawings.is_visible(*index))
            .find_map(|(index, drawing)| {
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                };
                drawing
                    .tool
                    .hit_test(chart_rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
                    .then_some(index)
            })
    }

    /// Alt+click: deterministic z-order cycling through every visible object
    /// under the pointer. From the current selection, the next hit beneath
    /// it wins; past the bottom it wraps back to the top.
    fn drawing_below_selection(
        &self,
        pos: egui::Pos2,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        let hits: Vec<usize> = (0..self.drawings.items().len())
            .rev()
            .filter(|&index| self.drawings.is_visible(index))
            .filter(|&index| {
                let drawing = &self.drawings.items()[index];
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                };
                drawing
                    .tool
                    .hit_test(chart_rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            })
            .collect();
        match self
            .drawings
            .selected()
            .and_then(|current| hits.iter().position(|&index| index == current))
        {
            Some(at) => Some(hits[(at + 1) % hits.len()]),
            None => hits.first().copied(),
        }
    }

    fn drawing_anchor_in(
        &self,
        drawing_index: usize,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<usize> {
        if !self.drawings.is_visible(drawing_index) {
            return None;
        }
        let drawing = self.drawings.items().get(drawing_index)?;
        self.projected_drawing_points(drawing, history_right, total, scale)
            .iter()
            .enumerate()
            .map(|(point_index, point)| (point_index, point.distance_sq(pos)))
            .filter(|(_, distance_sq)| {
                *distance_sq <= DRAWING_ANCHOR_RADIUS_PX * DRAWING_ANCHOR_RADIUS_PX
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(point_index, _)| point_index)
    }

    fn drawing_anchor_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Option<(usize, usize)> {
        let selected = self.drawings.selected();
        if let Some(drawing_index) = selected
            && let Some(point_index) =
                self.drawing_anchor_in(drawing_index, pos, history_right, total, scale)
        {
            return Some((drawing_index, point_index));
        }
        (0..self.drawings.items().len())
            .rev()
            .filter(|drawing_index| Some(*drawing_index) != selected)
            .find_map(|drawing_index| {
                self.drawing_anchor_in(drawing_index, pos, history_right, total, scale)
                    .map(|point_index| (drawing_index, point_index))
            })
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the candles → pan time (x, moves the whole chart) and price (y);
    /// - scroll over them → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click → reset to the live edge and auto-fit price.
    ///
    /// The live lane is a pane of its own and answers to none of it: a gesture
    /// that starts inside the tape moves nothing, and scrolling there zooms the
    /// tape's own window instead of the candles.
    /// Arming a tool brings back the layer it draws on.
    ///
    /// A crosshair that draws no cross, or a line tool that places invisible
    /// objects, reads as a broken tool rather than as a hidden layer — and
    /// reaching for the tool is the user saying they want to see it.
    fn unhide_layer_for_armed_tool(&mut self) {
        let layer = match self.toolrail.tool() {
            Tool::Crosshair => ChartLayer::Crosshair,
            Tool::Drawing(_) => ChartLayer::Drawings,
            Tool::Pointer => return,
        };
        if !self.layer_visible(layer) {
            self.set_layer_visible(layer, true);
        }
    }

    fn handle_navigation(&mut self, ui: &egui::Ui, area: egui::Rect) {
        self.unhide_layer_for_armed_tool();
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.last_chart_area = Some(
            plot_split(
                area,
                self.live_strip_width(),
                self.indicators.visible_panes().count(),
            )
            .chart,
        );
        if self.handle_drawing_placement(ui, area) {
            return;
        }
        let areas = plot_split(
            area,
            self.live_strip_width(),
            self.indicators.visible_panes().count(),
        );
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.slots();
        let divider = self.last_lane_divider_x;
        let in_lane = |position: egui::Pos2| gesture_hits_lane(divider, position.x);

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            egui::Id::new("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = chart.hover_pos();
        // Right-click: what is on the canvas, and what is not. Secondary button
        // only, so it shares no gesture with the pan, the zoom or the drawing
        // tools — a pan that ends anywhere never opens it.
        chart.context_menu(|ui| self.draw_layer_menu(ui));
        // While the menu is open the pointer is reading it, not the chart, so
        // no crosshair chases it across the candles behind it.
        if chart.context_menu_opened() {
            self.hover_pos = None;
        }
        let drawing_scale = auto.map(|(auto_lo, auto_hi)| {
            let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
            PriceScale::from_range(lo, hi, areas.chart.top(), areas.chart.bottom())
        });
        let history_right = self.last_lane_divider_x.unwrap_or(areas.chart.right());
        let drawing_area = egui::Rect::from_min_max(
            areas.chart.min,
            egui::pos2(history_right, areas.chart.bottom()),
        );
        let (primary_pressed, primary_down, primary_released, pointer_position, pointer_delta) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.latest_pos(),
                    input.pointer.delta(),
                )
            });
        let mut drawing_drag_consumes_gesture = false;
        if self.toolrail.tool() == Tool::Pointer {
            // Hover feedback: a resize cursor over a selected anchor, a move
            // cursor over any visible body, and not-allowed over locked
            // geometry (visible objects in the viewport only — bounded work).
            if let Some(position) =
                pointer_position.filter(|position| drawing_area.contains(*position))
                && let Some(scale) = drawing_scale
            {
                if let Some(selected) = self.drawings.selected()
                    && self
                        .drawing_anchor_in(selected, position, history_right, total, &scale)
                        .is_some()
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[selected].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::ResizeNwSe
                        });
                } else if let Some(hovered) =
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[hovered].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::Move
                        });
                }
            }
            if chart.clicked()
                && let Some(position) = chart.interact_pointer_pos()
                && let Some(scale) = drawing_scale
            {
                // Alt+click walks down the z-order through overlapping
                // objects; a plain click selects the topmost hit.
                let selected = if ui.input(|input| input.modifiers.alt) {
                    self.drawing_below_selection(
                        position,
                        areas.chart,
                        history_right,
                        total,
                        &scale,
                    )
                } else {
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                };
                self.drawings.select(selected);
            }
            // Floating inspectors normally own pointer input over their whole
            // rectangle. Read the raw press so an already-selected drawing
            // remains draggable even when its stroke or handle is underneath
            // that inspector. Only an actual drawing hit takes precedence;
            // every other inspector click remains a style interaction.
            let mut drawing_drag_started = false;
            if primary_pressed
                && let Some(position) =
                    pointer_position.filter(|position| drawing_area.contains(*position))
                && let Some(scale) = drawing_scale
            {
                if let Some((drawing_index, point_index)) =
                    self.drawing_anchor_at(position, history_right, total, &scale)
                {
                    self.drawings.select(Some(drawing_index));
                    self.drawing_drag = if self.drawings.items()[drawing_index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Anchor {
                            drawing_index,
                            point_index,
                        }
                    };
                } else if let Some(index) =
                    self.drawing_at(position, areas.chart, history_right, total, &scale)
                {
                    self.drawings.select(Some(index));
                    self.drawing_drag = if self.drawings.items()[index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Translate
                    };
                }
                // A press that hits no geometry is not ours to interpret: it
                // belongs to whatever egui routed it to (inspector, manager,
                // chart pan). Deselection happens through the egui-routed
                // click above, which already respects floating windows.
                drawing_drag_started = self.drawing_drag.is_active();
            }
            if primary_down && !drawing_drag_started {
                match self.drawing_drag {
                    DrawingDrag::Anchor {
                        drawing_index,
                        point_index,
                    } => {
                        if let Some(position) = pointer_position {
                            let position = egui::pos2(
                                position.x.clamp(areas.chart.left(), history_right),
                                position.y.clamp(areas.chart.top(), areas.chart.bottom()),
                            );
                            if let Some(point) =
                                self.drawing_point_at(position, history_right, total)
                            {
                                self.drawings.move_anchor(drawing_index, point_index, point);
                            }
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                    }
                    DrawingDrag::Translate => {
                        if let Some(scale) = drawing_scale {
                            let (lo, hi) = scale.range();
                            let delta_bar = pointer_delta.x / self.viewport.candle_width();
                            let delta_price =
                                -f64::from(pointer_delta.y / areas.chart.height()) * (hi - lo);
                            self.drawings.translate_selected(delta_bar, delta_price);
                        }
                    }
                    DrawingDrag::Blocked => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                    DrawingDrag::None => {}
                }
            }
            drawing_drag_consumes_gesture = self.drawing_drag.is_active();
            if primary_released {
                // One gesture, one undo entry — recorded only if it moved.
                self.drawings.commit_gesture();
                self.drawing_drag = DrawingDrag::None;
            }
        } else {
            self.drawing_drag = DrawingDrag::None;
        }
        // Where the press landed, not where the pointer is now: a pan that
        // started on the candles keeps working when it crosses the divider.
        let dragging_candles = chart
            .interact_pointer_pos()
            .is_some_and(|press| !in_lane(press));
        if total > 0 && chart.dragged() && dragging_candles && !drawing_drag_consumes_gesture {
            let drag = chart.drag_delta();
            self.viewport.pan_pixels(drag.x, total);
            if let Some(auto) = auto
                && drag.y != 0.0
                && height > 1.0
            {
                let (lo, hi) = self.price_view.resolve(auto);
                let price_per_px = (hi - lo) / f64::from(height);
                self.price_view.pan(f64::from(drag.y) * price_per_px, auto);
            }
        }
        if chart.double_clicked() {
            self.viewport.snap_to_live();
            self.price_view.reset();
        }
        if chart.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in. Over the tape that means less
                // market time in the band; over the candles, wider candles.
                if chart.hover_pos().is_some_and(in_lane) {
                    self.orderflow.zoom_live_lane(2.0_f32.powf(scroll / 300.0));
                } else {
                    self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
                }
            }
        }

        // The lane's divider, as a resize handle. Registered after the chart
        // body so it takes the drag that would otherwise pan the candles
        // behind it, and it is the only place the pointer changes shape: the
        // line stays a hairline, the cursor is what says it can be moved.
        let divider = self.last_lane_divider_x.map(|x| {
            ui.interact(
                egui::Rect::from_min_max(
                    egui::pos2(x - LANE_HANDLE_HALF_WIDTH_PX, areas.chart.top()),
                    egui::pos2(x + LANE_HANDLE_HALF_WIDTH_PX, areas.chart.bottom()),
                ),
                egui::Id::new("lane_divider"),
                egui::Sense::drag(),
            )
        });
        if let Some(divider) = &divider {
            if divider.hovered() || divider.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if divider.dragged() {
                // Drag left → a wider tape, at the expense of the candles.
                self.orderflow
                    .resize_live_lane(divider.drag_delta().x, areas.chart.width());
            }
        }

        // Bottom time strip: drag or scroll to zoom. The segment under the
        // lane zooms the lane's window, the rest zooms the candle spacing —
        // each pane's own time axis, under the pane it belongs to.
        let (history_strip, lane_strip) =
            split_time_strip(areas.time_strip, self.last_lane_divider_x);
        let time = ui.interact(
            history_strip,
            egui::Id::new("time_nav"),
            egui::Sense::click_and_drag(),
        );
        if time.dragged() {
            // Drag right → wider candles (zoom in); left → narrower (zoom out).
            self.viewport
                .zoom((time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
            }
        }
        if let Some(lane_strip) = lane_strip {
            let lane_time = ui.interact(
                lane_strip,
                egui::Id::new("lane_time_nav"),
                egui::Sense::click_and_drag(),
            );
            if lane_time.dragged() {
                // Drag right → less market time in the band (zoom in), so
                // prints run across it faster and further apart.
                self.orderflow
                    .zoom_live_lane((lane_time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
            }
            if lane_time.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    self.orderflow.zoom_live_lane(2.0_f32.powf(scroll / 300.0));
                }
            }
        }

        // Right price gutter: drag or scroll to zoom the price scale.
        let price = ui.interact(
            areas.price_gutter,
            egui::Id::new("price_nav"),
            egui::Sense::click_and_drag(),
        );
        if let Some(auto) = auto {
            if price.dragged() {
                // Drag up → compress span (bigger candles); down → expand.
                self.price_view
                    .zoom(f64::from(price.drag_delta().y / 150.0).exp(), auto);
            }
            if price.double_clicked() {
                self.price_view.reset();
            }
            if price.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    self.price_view.zoom(f64::from(-scroll / 200.0).exp(), auto);
                }
            }
        }
    }

    fn draw_chart(&mut self, painter: &egui::Painter, area: egui::Rect) {
        painter.rect_filled(area, egui::Rounding::ZERO, self.bg());

        let closed = self.state.bars();
        let partial = self.state.partial();
        let total = closed.len() + usize::from(partial.is_some());
        let areas = plot_split(
            area,
            self.live_strip_width(),
            self.indicators.visible_panes().count(),
        );
        // Indicator panes claimed the bottom band inside `plot_split`, so the
        // rect the candles scale to is the same one the input handler uses.
        let chart_rect = areas.chart;
        let pane_rects = areas.indicator_panes.clone();
        if total == 0 {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", self.symbol),
                egui::FontId::proportional(16.0),
                theme::TEXT_MUTED,
            );
            self.orderflow.draw_status_badge(painter, chart_rect);
            return;
        }

        // The live lane: a pane of its own, pinned to the right edge of the
        // chart, showing a fixed window of market time that always ends at
        // now. Fixed width, fixed pixels-per-ms: a print enters at the right
        // edge and slides left until it leaves into the slot of its own bar.
        //
        // It belongs to the tape rather than to the forming bar, which is what
        // keeps a bar close from emptying it — the reset that made the book
        // look like it was restarting every few seconds. And it is a pane
        // rather than a reservation inside the viewport, which is what keeps
        // every chart movement out of it: panning, zooming and dragging move
        // the candles beside the tape and never the tape itself, so the most
        // recent prints are on screen whatever the rest of the chart is doing.
        let lane_width_px = self
            .orderflow
            .live_lane_width_px(chart_rect.width())
            .unwrap_or(0.0);
        // Everything left of the divider is the candles' pane. They pan and
        // zoom inside it exactly as they did when it was the whole chart.
        self.last_lane_divider_x =
            crate::orderflow_render::lane_divider_x(chart_rect, lane_width_px);
        let history_rect = egui::Rect::from_min_max(
            chart_rect.min,
            egui::pos2(
                self.last_lane_divider_x
                    .unwrap_or_else(|| chart_rect.right()),
                chart_rect.bottom(),
            ),
        );

        let (start, end) = self.viewport.visible_range(history_rect.width(), total);

        // The visible closed bars, plus the partial if it falls in view.
        let closed_start = start.min(closed.len());
        let closed_end = end.min(closed.len());
        let visible_closed = &closed[closed_start..closed_end];
        let partial_visible = partial.filter(|_| closed.len() >= start && closed.len() < end);

        // Auto-fit the visible bars, then apply any manual price pan/zoom. A
        // window with no bars in it still gets a scale (the last one, then the
        // newest bar), because a chart that draws nothing at all is
        // indistinguishable from a hung app — which is exactly how the blank
        // frame after a rebuild read.
        let nothing_in_view = visible_closed.is_empty() && partial_visible.is_none();
        let Some(auto_scale) = chart::price_window(
            visible_closed,
            partial_visible,
            self.last_auto_range,
            partial.or_else(|| closed.last()),
            chart_rect.top(),
            chart_rect.bottom(),
        ) else {
            return;
        };
        let auto_range = auto_scale.range();
        let (lo, hi) = self.price_view.resolve(auto_range);
        let scale = PriceScale::from_range(lo, hi, chart_rect.top(), chart_rect.bottom());

        let cw = self.viewport.candle_width();
        let half = (cw * self.style.candles.clamped_width_frac() / 2.0).max(0.5);
        let right = history_rect.right();

        // Resting liquidity is the bottom visual layer. Projection is pure with
        // respect to candles and uses the same bar-warped viewport coordinates.
        // The projection builds a lane exactly when the layout draws one. Tied
        // to `lane_width_px` rather than restated, because the two decide the
        // same thing: with them apart, the newest prints would be clustered and
        // sized as lane prints and then squeezed into a single candle slot.
        let orderflow_frame = self.orderflow.project_visible(
            VisibleBarTimeline::new(
                self.state.timeline_revision(),
                closed_start,
                visible_closed,
                partial_visible,
            ),
            lane_width_px > 0.0,
            end == total,
            scale.range(),
        );
        let canvas_background = self.bg();
        if let Some(frame) = &orderflow_frame {
            self.orderflow.draw_background(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
            );
        }

        // Grid + price labels first, behind the candles. Labels anchor on the
        // gutter's edge, past the live strip when one is shown.
        let axis_x = areas.price_gutter.left();
        self.draw_price_axis(painter, chart_rect, axis_x, &scale);

        // Candles, clipped to their own pane: panning far enough into history
        // sends the newest bars off the right of it, and they scroll out of
        // sight behind the tape instead of being drawn over it.
        let clip = painter.with_clip_rect(history_rect);
        // Clear the heat behind each candle's high–low span so a translucent
        // candle stays a clean divider — no liquidity band shows through it.
        // Where the price swept, the wall reads as consumed; bands survive only
        // in the gaps between candles and above/below each bar.
        if orderflow_frame.is_some() && self.orderflow.depth_visible() {
            let clear_bar = |xc: f32, bar: &quantick_engine::Bar| {
                let top = scale.y(bar.high.to_f64().unwrap_or(0.0));
                let bottom = scale.y(bar.low.to_f64().unwrap_or(0.0));
                clip.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xc - half, top),
                        egui::pos2(xc + half, bottom),
                    ),
                    egui::Rounding::ZERO,
                    canvas_background,
                );
            };
            for (offset, bar) in visible_closed.iter().enumerate() {
                clear_bar(
                    self.viewport.x_center(closed_start + offset, right, total),
                    bar,
                );
            }
            if let Some(partial) = partial_visible {
                clear_bar(self.viewport.x_center(closed.len(), right, total), partial);
            }
        }
        for (offset, bar) in visible_closed.iter().enumerate() {
            let index = closed_start + offset;
            let xc = self.viewport.x_center(index, right, total);
            draw_candle(&clip, xc, half, &scale, bar, false, &self.style.candles);
        }
        if let Some(partial) = partial_visible {
            let xc = self.viewport.x_center(closed.len(), right, total);
            draw_candle(&clip, xc, half, &scale, partial, true, &self.style.candles);
        }
        // Overlay indicator plots ride the candles' own clip, scale and
        // x-mapping — after candles, before aggression bubbles (the same
        // paint-order slot draw objects take).
        let plot_x = PlotX {
            viewport: &self.viewport,
            right,
            total,
        };
        // Slot -> (high_y, low_y) in pixels, for above/below-bar markers.
        let bar_extents = |slot: usize| -> Option<(f32, f32)> {
            let bar = if slot < closed.len() {
                Some(&closed[slot])
            } else if slot == closed.len() {
                partial
            } else {
                None
            }?;
            Some((
                scale.y(chart::to_f64(bar.high)),
                scale.y(chart::to_f64(bar.low)),
            ))
        };
        indicator_render::draw_overlays(
            &clip,
            self.indicators.visible_overlays(),
            &plot_x,
            &scale,
            start,
            end,
            partial_visible.map(|_| closed.len()),
            &bar_extents,
        );
        // Draw objects (lines/boxes/labels) share the overlays' paint slot:
        // after candles, before aggression bubbles.
        for view in self.indicators.visible_overlays() {
            indicator_render::draw_objects(
                &clip,
                view.render_objects(),
                &plot_x,
                |v| scale.y(v),
                start,
                end,
            );
        }
        // Pane indicators stack in the band carved off above, sharing the
        // candles' x-mapping so bars and their flow read as one chart.
        for (view, pane) in self.indicators.visible_panes().zip(&pane_rects) {
            let pane = egui::Rect::from_min_max(
                egui::pos2(history_rect.left(), pane.top()),
                egui::pos2(history_rect.right(), pane.bottom()),
            );
            indicator_render::draw_pane(
                painter,
                pane,
                view,
                &plot_x,
                start,
                end,
                partial_visible.map(|_| closed.len()),
                canvas_background,
            );
        }
        if let Some(frame) = &orderflow_frame {
            self.orderflow.draw_aggressions(
                painter,
                chart_rect,
                &self.viewport,
                total,
                frame,
                canvas_background,
                lane_width_px,
            );
        }

        // The live strip: the book right now plus the forming bar's
        // aggression histogram, beside the axis the price labels live on.
        // Its own rect, so chart layers never bleed into it. The histogram
        // follows `partial` (not its visible filter): the strip reports the
        // bar forming now even while the user pans through history.
        if let Some(strip) = areas.live_strip {
            self.orderflow.draw_live_strip(
                painter,
                strip,
                &scale,
                canvas_background,
                partial.map(|bar| bar.open_time),
            );
        }

        // Drawings sit above market layers and remain anchored to chart space,
        // not the screen, while the viewport moves beneath them. Their own
        // visibility (per object and globally) is settled inside.
        self.draw_drawings(painter, chart_rect, right, total, &scale);

        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        if self.layer_visible(ChartLayer::LastPrice)
            && let Some(bar) = partial.or_else(|| closed.last())
        {
            self.draw_last_price(painter, chart_rect, axis_x, &scale, bar);
        }
        // The candles' own mark, so it is placed and clipped in their pane.
        if self.layer_visible(ChartLayer::BackfillDivider) {
            self.draw_backfill_divider(painter, history_rect, total, cw);
        }
        self.draw_time_strip(painter, areas.time_strip, closed, start, end, total);
        self.draw_lane_time_axis(
            painter,
            split_time_strip(areas.time_strip, self.last_lane_divider_x).1,
            self.orderflow.live_lane_window_ms(closed),
        );
        // Panned off the data (or a rebuild re-cut the series under the
        // window): the chart is whole — axis, tape, badges — but there is
        // nothing in the candles' pane, so say so and say the way back.
        if nothing_in_view {
            painter.text(
                history_rect.center(),
                egui::Align2::CENTER_CENTER,
                "no bars in view — double-click to return to the live edge",
                egui::FontId::proportional(EMPTY_VIEW_FONT_SIZE),
                theme::TEXT_MUTED,
            );
        }
        if self.layer_visible(ChartLayer::Crosshair) {
            self.draw_crosshair(painter, chart_rect, axis_x, &scale);
        }
        // The status badge is not a layer: it reports whether the source is
        // healthy, and a chart with every layer off must still say that.
        self.orderflow.draw_status_badge(painter, chart_rect);

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.last_auto_range = Some(auto_range);
        self.last_chart_height = chart_rect.height();
        self.last_chart_top = chart_rect.top();
    }

    /// Bottom time strip: a top border and a few `HH:MM:SS` labels for the
    /// visible bars. Draggable left/right to zoom the candle spacing.
    ///
    /// The labels stay under the candles' own pane; the segment past the lane's
    /// divider is the tape's time axis and reads its window instead
    /// ([`Self::draw_lane_time_axis`]).
    fn draw_time_strip(
        &self,
        painter: &egui::Painter,
        strip: egui::Rect,
        closed: &[quantick_engine::Bar],
        start: usize,
        end: usize,
        total: usize,
    ) {
        painter.line_segment(
            [
                egui::pos2(strip.left(), strip.top()),
                egui::pos2(strip.right(), strip.top()),
            ],
            egui::Stroke::new(1.0_f32, self.grid()),
        );
        let font = egui::FontId::monospace(10.0);
        let y = strip.center().y;
        // Up to ~6 evenly-spaced labels across the visible closed bars.
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let (history_strip, _) = split_time_strip(strip, self.last_lane_divider_x);
        let step = (visible / 6).max(1);
        let mut index = start;
        while index < end {
            if let Some(bar) = closed.get(index) {
                let x = self.viewport.x_center(index, history_strip.right(), total);
                if history_strip.x_range().contains(x) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time(bar.open_time, self.tz),
                        font.clone(),
                        theme::TEXT_MUTED,
                    );
                }
            }
            index += step;
        }
    }

    /// The live lane's own time axis: how much market time the tape is
    /// showing, under the tape.
    ///
    /// The lane has no bar boundaries to label — it is one continuous window —
    /// so its axis reads the window itself. It is also the only readout of what
    /// the lane's zoom is currently worth, which is what makes dragging here
    /// something other than guesswork.
    fn draw_lane_time_axis(
        &self,
        painter: &egui::Painter,
        lane_strip: Option<egui::Rect>,
        window_ms: i64,
    ) {
        let Some(strip) = lane_strip else {
            return;
        };
        painter.text(
            strip.center(),
            egui::Align2::CENTER_CENTER,
            format!("tape · {}", fmt_window(window_ms)),
            egui::FontId::monospace(10.0),
            theme::TEXT_MUTED,
        );
    }

    /// Right-hand price axis: round-number gridlines and labels. `axis_x` is
    /// the gutter's left edge — the chart's right edge normally, the live
    /// strip's right edge while the strip sits between them.
    fn draw_price_axis(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
    ) {
        let (lo, hi) = scale.range();
        let font = egui::FontId::monospace(11.0);
        for tick in crate::chart::nice_ticks(lo, hi, 8) {
            let y = scale.y(tick);
            if y < chart_rect.top() || y > chart_rect.bottom() {
                continue;
            }
            painter.line_segment(
                [
                    egui::pos2(chart_rect.left(), y),
                    egui::pos2(chart_rect.right(), y),
                ],
                egui::Stroke::new(1.0_f32, self.grid()),
            );
            painter.text(
                egui::pos2(axis_x + 6.0, y),
                egui::Align2::LEFT_CENTER,
                format!("{tick:.2}"),
                font.clone(),
                theme::TEXT_MUTED,
            );
        }
        // The axis dividing line.
        painter.line_segment(
            [
                egui::pos2(axis_x, chart_rect.top()),
                egui::pos2(axis_x, chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, self.grid()),
        );
    }

    /// The current price: a dashed line across the chart and a solid chip on
    /// the price axis, coloured by the direction of the bar carrying it.
    ///
    /// This is the always-on answer to "am I above or below?" — the question
    /// every other mark on the canvas is read against, and the one a wall of
    /// resting liquidity cannot answer on its own.
    fn draw_last_price(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
        bar: &quantick_engine::Bar,
    ) {
        let Some(price) = bar.close.to_f64() else {
            return;
        };
        let y = scale.y(price);
        if y < chart_rect.top() || y > chart_rect.bottom() {
            return;
        }
        // Same predicate and same two colours the candle wears, so the chip
        // and the bar it reports can never disagree about direction.
        let rgb = if crate::candle_view::is_bullish(bar) {
            self.style.candles.bull_outline
        } else {
            self.style.candles.bear_outline
        };
        let color = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

        // Runs through the live strip when one is shown (`axis_x` then sits
        // past it): the depth silhouette is read against this exact line.
        painter.extend(egui::Shape::dashed_line(
            &[egui::pos2(chart_rect.left(), y), egui::pos2(axis_x, y)],
            egui::Stroke::new(1.0_f32, color.gamma_multiply(LAST_PRICE_LINE_ALPHA)),
            LAST_PRICE_DASH_PX,
            LAST_PRICE_GAP_PX,
        ));

        // Same geometry as the crosshair tag, so the two never disagree about
        // where a price sits on the axis.
        let galley = painter.layout_no_wrap(
            format!("{price:.2}"),
            egui::FontId::monospace(11.0),
            LAST_PRICE_CHIP_TEXT,
        );
        let text_pos = egui::pos2(axis_x + 6.0, y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), color);
        painter.galley(text_pos, galley, LAST_PRICE_CHIP_TEXT);
    }

    fn drawing_screen_point(
        &self,
        point: ChartPoint,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> egui::Pos2 {
        egui::pos2(
            self.viewport
                .x_at_bar_position(point.bar, history_right, total),
            scale.y(point.price),
        )
    }

    /// Paint the completed drawing objects. This runs once per frame and is
    /// O(number of drawings); it never touches the per-trade ingestion path.
    fn draw_drawings(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) {
        let clipped = painter.with_clip_rect(chart_rect);
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            let selected = self.drawings.selected() == Some(index);
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                style: drawing.style,
                selected,
                halo: false,
            };
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
            drawing.tool.paint(
                &clipped,
                chart_rect,
                drawing.style,
                &points,
                &ctxt,
                selected && !drawing.locked,
            );
        }

        if let Some(draft) = self.drawings.draft() {
            let mut points = self.projected_drawing_points(draft, history_right, total, scale);
            // The preview completes the geometry with the hovered anchor, in
            // both screen and chart space, so payload-driven tools can show
            // their real shape while placing.
            let mut anchors: SmallVec<[ChartPoint; 4]> = SmallVec::from_slice(&draft.points);
            if points.len() < draft.tool.required_points()
                && let Some(hover) = self.drawing_hover
            {
                points.push(self.drawing_screen_point(hover, history_right, total, scale));
                anchors.push(hover);
            }
            let ctxt = DrawContext {
                payload: draft.payload.as_ref(),
                anchors: &anchors,
                scale,
                style: draft.style,
                selected: false,
                halo: false,
            };
            draft
                .tool
                .paint(&clipped, chart_rect, draft.style, &points, &ctxt, false);
        }
    }

    /// Crosshair following the pointer, with the price shown on the axis.
    /// Drawn only while the Crosshair tool is armed on the rail (§7 — the
    /// hover crosshair is a mode, not an always-on layer).
    fn draw_crosshair(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        axis_x: f32,
        scale: &PriceScale,
    ) {
        if self.toolrail.tool() != Tool::Crosshair {
            return;
        }
        let Some(pos) = self.hover_pos else {
            return;
        };
        if !chart_rect.contains(pos) {
            return;
        }
        let stroke = egui::Stroke::new(1.0_f32, theme::TEXT_FAINT);
        painter.line_segment(
            [
                egui::pos2(pos.x, chart_rect.top()),
                egui::pos2(pos.x, chart_rect.bottom()),
            ],
            stroke,
        );
        // Reaches the axis through the live strip when one is shown, so the
        // cursor height can be read against the depth silhouette too.
        painter.line_segment(
            [
                egui::pos2(chart_rect.left(), pos.y),
                egui::pos2(axis_x, pos.y),
            ],
            stroke,
        );

        // Price tag on the axis at the cursor height.
        let price = scale.price_at(pos.y);
        let galley = painter.layout_no_wrap(
            format!("{price:.2}"),
            egui::FontId::monospace(11.0),
            egui::Color32::WHITE,
        );
        let text_pos = egui::pos2(axis_x + 6.0, pos.y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), theme::TAG_BG);
        painter.galley(text_pos, galley, egui::Color32::WHITE);
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls inside the candles' pane.
    ///
    /// `pane` is the candles' own rect — the chart minus the live lane — since
    /// that is the space the viewport maps bar indices into.
    fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        pane: egui::Rect,
        total: usize,
        candle_width: f32,
    ) {
        let Some(boundary) = self.state.backfill_boundary() else {
            return;
        };
        if boundary == 0 {
            return; // nothing backfilled
        }
        // The divider sits at the left edge of the first live bar.
        let x = self.viewport.x_center(boundary, pane.right(), total) - candle_width / 2.0;
        if x < pane.left() || x > pane.right() {
            return; // off-screen
        }
        painter.line_segment(
            [egui::pos2(x, pane.top()), egui::pos2(x, pane.bottom())],
            egui::Stroke::new(1.0_f32, theme::AMBER),
        );
        let font = egui::FontId::proportional(11.0);
        painter.text(
            egui::pos2(x - 4.0, pane.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "backfill",
            font.clone(),
            theme::TEXT_MUTED,
        );
        painter.text(
            egui::pos2(x + 4.0, pane.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "live",
            font,
            theme::AMBER,
        );
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
    fn status_model(&self) -> statusbar::StatusModel {
        let bars = self.state.bars();
        let (backfilled, live) = match self.state.backfill_boundary() {
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
            spec_summary: self.state.spec().summary(),
            bar_progress: self
                .state
                .progress()
                .map(|(progress, unit)| fmt_progress(&progress, unit)),
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: note.clone().map(|(label, _)| label),
            side_detail: note.and_then(|(_, detail)| detail),
            follows_live: self.viewport.follows_live(),
            price_auto: self.price_view.is_auto(),
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
        match self.drawings.delete_selected(false) {
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
            } else if self.drawings.draft().is_some() {
                self.drawings.cancel_draft();
                self.toolrail.arm(Tool::Pointer);
            } else if self.drawings.selected().is_some() {
                self.drawings.select(None);
            } else {
                self.toolrail.arm(Tool::Pointer);
            }
        }
        if self.drawings.draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                self.drawings.remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(now);
        }
        if keys.undo {
            self.drawings.undo();
        }
        if keys.redo {
            self.drawings.redo();
        }
        if keys.lock
            && let Some(index) = self.drawings.selected()
        {
            let locked = self.drawings.items()[index].locked;
            self.drawings.set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = self.drawings.selected()
        {
            let hidden = self.drawings.items()[index].hidden;
            self.drawings.set_selected_hidden(!hidden);
        }
        if keys.duplicate {
            self.drawings.duplicate_selected(DUPLICATE_OFFSET_BARS);
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0) && self.drawings.selected().is_some() {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of price per
            // vertical step. Each press lands as one undo entry.
            let price_per_px = self.last_auto_range.map_or(0.0, |auto| {
                let (lo, hi) = self.price_view.resolve(auto);
                (hi - lo) / f64::from(self.last_chart_height.max(1.0))
            });
            self.drawings.begin_gesture();
            self.drawings
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            self.drawings.commit_gesture();
        }
    }

    /// Commit a pending inspector edit gesture as one undo entry.
    fn commit_inspector_gesture(&mut self) {
        if let Some((index, before)) = self.inspector_edit_baseline.take() {
            self.drawings.record_edit_of(index, before);
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
            self.drawings.undo();
            self.drawing_toast = None;
        }
    }

    /// Everything the inspector shows for the selected object, shared by the
    /// floating window and the pinned dock panel. Sections are driven by the
    /// tool's capabilities — an unsupported property is absent, not disabled.
    fn drawing_inspector_body(&mut self, ui: &mut egui::Ui, index: usize) -> InspectorActions {
        let mut actions = InspectorActions::default();
        let drawing = &self.drawings.items()[index];
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
        let price_speed = self.last_auto_range.map_or(1.0, |(lo, hi)| {
            ((hi - lo) / PRICE_DRAG_STEPS).abs().max(1e-9)
        });
        let Self {
            drawings,
            drawing_presets,
            ..
        } = self;
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
            let hidden = self.drawings.items()[index].hidden;
            self.drawings.set_selected_hidden(!hidden);
        }
        if actions.toggle_lock {
            let locked = self.drawings.items()[index].locked;
            self.drawings.set_selected_locked(!locked);
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
            if self.drawings.delete_selected(true) == DeleteOutcome::Deleted {
                self.drawing_toast = Some(DrawingToast {
                    message: "Drawing deleted.",
                    shown_at: now,
                    offers_undo: true,
                });
            }
        }
        if actions.close {
            self.drawings.select(None);
            self.drawing_delete_confirm = false;
        }
    }

    /// Where a freshly opened floating inspector should sit: 12 px right of
    /// the object's bounding box, falling back to left, below and above, and
    /// always clamped into the chart pane — which already excludes the price
    /// and time axes, so the popup can never cover either or leave the view.
    fn inspector_target_position(&self, ctx: &egui::Context, index: usize) -> Option<egui::Pos2> {
        let chart = self.last_chart_area?;
        let total = self.slots();
        let (auto_lo, auto_hi) = self.last_auto_range?;
        let (lo, hi) = self.price_view.resolve((auto_lo, auto_hi));
        let scale = PriceScale::from_range(
            lo,
            hi,
            self.last_chart_top,
            self.last_chart_top + self.last_chart_height,
        );
        let history_right = self.last_lane_divider_x.unwrap_or(chart.right());
        let drawing = self.drawings.items().get(index)?;
        let points = self.projected_drawing_points(drawing, history_right, total, &scale);
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
        let Some(index) = self.drawings.selected() else {
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
        Some((index, self.drawings.items()[index].clone()))
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
        let inspector_interactable = !self.drawing_drag.is_active();
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
                let count = self.drawings.items().len();
                if count == 0 {
                    ui.label("No drawings yet.");
                }
                // Walked in reverse: the manager lists top-most first, the
                // same order hit-testing resolves overlap.
                for index in (0..count).rev() {
                    let drawing = &self.drawings.items()[index];
                    let selected = self.drawings.selected() == Some(index);
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
            self.drawings.select(Some(index));
            // Centre the viewport on the object's bar span.
            if let Some(chart) = self.last_chart_area {
                let points = &self.drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    self.viewport
                        .center_on_bar(mid, chart.width(), self.slots());
                }
            }
        }
        if let Some(index) = eye_row {
            let hidden = self.drawings.items()[index].hidden;
            self.drawings.set_hidden_at(index, !hidden);
        }
        if let Some(index) = lock_row {
            let locked = self.drawings.items()[index].locked;
            self.drawings.set_locked_at(index, !locked);
        }
        if let Some(index) = front_row {
            self.drawings.bring_to_front(index);
        }
        if let Some(index) = delete_row {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            self.drawings.select(Some(index));
            self.request_delete_selected(now);
        }
        if show_all {
            self.drawings.set_all_hidden(false);
        }
        if unlock_all {
            self.drawings.set_all_locked(false);
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
        self.orderflow.set_enabled(false, generation);
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
        // Apply the indicator worker's deltas before the draw reads columns.
        for event in self.indicator_worker.drain_events() {
            self.indicators.apply(event);
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

        let bg = self.bg();
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
        self.maintain_chart_layers();
        let status = self.status_model();
        statusbar::draw(ctx, &status, &mut self.tz);
        // The browser window and, while a session plays, the transport bar.
        if let Some(action) = self.replay_view.draw(ctx, self.replay.as_ref()) {
            self.apply_replay_action(action);
        }
        {
            let Self {
                toolrail,
                drawings,
                drawing_manager_open,
                ..
            } = self;
            toolrail.draw(ctx, drawings, drawing_manager_open);
        }
        let dock_response = {
            let Self {
                dock,
                orderflow,
                replay_view,
                replay,
                ..
            } = self;
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
        self.apply_spec_change();
        self.draw_style_panel(ctx, now);
        // Waits owned by other components, mirrored level-style each frame so
        // the overlay needs no push notifications from either.
        self.loading
            .set_active(LoadingTask::ReplaySession, self.replay_view.is_loading());
        self.loading
            .set_active(LoadingTask::BookSync, self.orderflow.is_syncing());

        let mut notice_action = notice_card::NoticeAction::None;
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.handle_navigation(ui, area);
                self.draw_chart(ui.painter(), area);
                loading::overlay(ui, area, &self.loading);
                if notice_card::should_draw(&self.notice, self.state.bars().len()) {
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

    use crate::config::{FeedConfig, ProviderKind};

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

    /// The layer menu writes through to whoever owns the layer, and touches
    /// nothing else. A menu holding its own copy of "is the heatmap on" would
    /// disagree with the toolbar the moment either one was used.
    #[test]
    fn each_layer_switch_moves_exactly_one_owner() {
        let (mut app, _events, _commands, _book) = test_app();
        // What the chart opens with: the market layers are opt-in, the chart's
        // own chrome is on. This is the state the file's absence must preserve.
        for (layer, expected) in [
            (ChartLayer::Heatmap, false),
            (ChartLayer::Bubbles, false),
            (ChartLayer::LiveStrip, false),
            (ChartLayer::LaneMarks, true),
            (ChartLayer::DepthGaps, true),
            (ChartLayer::Grid, true),
            (ChartLayer::LastPrice, true),
            (ChartLayer::BackfillDivider, true),
            (ChartLayer::Crosshair, true),
            (ChartLayer::Drawings, true),
        ] {
            assert_eq!(
                app.layer_visible(layer),
                expected,
                "{} opens in the wrong state",
                layer.id()
            );
        }

        for layer in ChartLayer::ALL {
            let before: Vec<bool> = ChartLayer::ALL
                .into_iter()
                .map(|other| app.layer_visible(other))
                .collect();
            let flipped = !app.layer_visible(layer);
            app.set_layer_visible(layer, flipped);
            for (other, was) in ChartLayer::ALL.into_iter().zip(before) {
                let expected = if other == layer { flipped } else { was };
                assert_eq!(
                    app.layer_visible(other),
                    expected,
                    "switching {} moved {} too",
                    layer.id(),
                    other.id()
                );
            }
            app.set_layer_visible(layer, !flipped);
        }
    }

    /// Hiding is a view state, never a kill switch: the recorder keeps running
    /// behind a hidden heatmap, so unhiding repaints the retained past instead
    /// of opening a hole in it.
    #[test]
    fn hiding_a_layer_never_stops_the_data_behind_it() {
        let (mut app, _events, _commands, _book) = test_app();
        app.ensure_book_capture();
        assert!(app.orderflow.enabled(), "capture is on before the test");

        app.set_layer_visible(ChartLayer::Heatmap, true);
        assert!(app.layer_visible(ChartLayer::Heatmap));
        app.set_layer_visible(ChartLayer::Heatmap, false);
        assert!(!app.layer_visible(ChartLayer::Heatmap));
        assert!(
            app.orderflow.enabled(),
            "the map went off screen; the recording must not stop with it"
        );

        // Same for the drawings: the layer switch hides them, it never removes
        // them, and the objects come back with their anchors intact.
        let before = app.drawings.items().len();
        app.set_layer_visible(ChartLayer::Drawings, false);
        assert_eq!(app.drawings.items().len(), before, "hiding deletes nothing");
        app.set_layer_visible(ChartLayer::Drawings, true);
        assert!(app.layer_visible(ChartLayer::Drawings));
    }

    /// Close the app with layers hidden, open it again, and the canvas comes
    /// back the way it was left — through the same restore the constructor runs.
    #[test]
    fn layer_visibility_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("quantick-app-layers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();
        app.saved_layer_mask = app.layer_mask();
        app.set_layer_visible(ChartLayer::Crosshair, false);
        app.set_layer_visible(ChartLayer::BackfillDivider, false);
        // Switched through the toolbar's own action rather than the menu: the
        // save must follow the state, not the widget that moved it.
        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        // A market layer switched *on* has to come back on, which is why the
        // file records each layer's state instead of a list of hidden ones.
        app.set_layer_visible(ChartLayer::Bubbles, true);
        app.maintain_chart_layers();
        assert_eq!(
            app.saved_layer_mask,
            app.layer_mask(),
            "a settled canvas writes nothing further"
        );

        let (mut restored, _events, _commands, _book) = test_app();
        restored.chart_layers_path = path.clone();
        restored.restore_chart_layers();
        for (layer, expected) in [
            (ChartLayer::Crosshair, false),
            (ChartLayer::BackfillDivider, false),
            (ChartLayer::Heatmap, true),
            (ChartLayer::Bubbles, true),
            (ChartLayer::LastPrice, true),
            (ChartLayer::Drawings, true),
        ] {
            assert_eq!(
                restored.layer_visible(layer),
                expected,
                "{} did not survive the restart",
                layer.id()
            );
        }
        // The lane marks belong to the order-flow preset; this file must not
        // have taken a second opinion on them.
        let text = std::fs::read_to_string(&path).expect("state file");
        assert!(!text.contains("lane_marks"), "{text}");
        std::fs::remove_file(&path).ok();
    }

    /// The menu itself: every layer gets a switch, a layer the feed cannot
    /// produce is offered disabled rather than as a lie, and clicking a real
    /// checkbox hides the layer behind it.
    #[test]
    fn the_layer_menu_offers_every_layer_and_its_switches_work() {
        let dir = std::env::temp_dir().join(format!("quantick-app-menu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 700.0));
        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();

        let menu_frame = |app: &mut QuantickApp, events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.draw_layer_menu(ui));
                },
            );
        };

        menu_frame(&mut app, Vec::new());
        assert_eq!(
            app.layer_menu_rects.len(),
            ChartLayer::ALL.len(),
            "every layer needs a switch, or it cannot be turned off at all"
        );

        let crosshair = app
            .layer_menu_rects
            .iter()
            .find(|(layer, _)| *layer == ChartLayer::Crosshair)
            .expect("the crosshair has a switch")
            .1
            .center();
        assert!(app.layer_visible(ChartLayer::Crosshair));
        menu_frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(crosshair),
                egui::Event::PointerButton {
                    pos: crosshair,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
        menu_frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos: crosshair,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            }],
        );
        assert!(
            !app.layer_visible(ChartLayer::Crosshair),
            "clicking the switch has to switch the layer"
        );

        // A capability the source lacks is disabled, not silently absent: a
        // recording has no book, and the entry says so instead of offering a
        // switch that would do nothing.
        assert!(QuantickApp::layer_blocked(ChartLayer::Heatmap, app.capabilities()).is_none());
        let (mut quote_only, _events, _commands, _book) = test_app_without_depth();
        let quotes_only = quote_only.capabilities();
        assert!(
            QuantickApp::layer_blocked(ChartLayer::Heatmap, quotes_only).is_some(),
            "a source with no book cannot promise a heatmap"
        );
        assert!(QuantickApp::layer_blocked(ChartLayer::Bubbles, quotes_only).is_some());
        assert!(QuantickApp::layer_blocked(ChartLayer::Grid, quotes_only).is_none());
        menu_frame(&mut quote_only, Vec::new());
        assert_eq!(
            quote_only.layer_menu_rects.len(),
            ChartLayer::ALL.len(),
            "an unavailable layer is still listed, just not switchable"
        );
        std::fs::remove_file(&path).ok();
    }

    /// A switched-off layer really stops painting.
    ///
    /// The switches above only prove the *state* moved; this one draws a real
    /// chart twice and counts the shapes, so a gate someone forgets to place
    /// (or later deletes) shows up as a menu entry that changes nothing.
    #[test]
    fn a_hidden_layer_paints_nothing() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 600.0));
        let (mut app, _commands) = app_with_history(120);
        // The crosshair is a mode: it paints under a pointer, with its own tool
        // armed. Both are set here so the layer has something to switch off.
        app.toolrail.arm(Tool::Crosshair);
        app.hover_pos = Some(screen.center());

        let shapes = |app: &mut QuantickApp| -> usize {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = ui.available_rect_before_wrap();
                        app.hover_pos = Some(area.center());
                        app.draw_chart(ui.painter(), area);
                    });
                },
            );
            output.shapes.len()
        };

        let all_on = shapes(&mut app);
        for layer in [
            ChartLayer::LastPrice,
            ChartLayer::BackfillDivider,
            ChartLayer::Crosshair,
        ] {
            app.set_layer_visible(layer, false);
            let off = shapes(&mut app);
            assert!(
                off < all_on,
                "{} kept painting after it was switched off ({off} shapes vs {all_on})",
                layer.id()
            );
            app.set_layer_visible(layer, true);
            assert_eq!(
                shapes(&mut app),
                all_on,
                "{} did not come back exactly as it was",
                layer.id()
            );
        }
    }

    /// Hidden objects answer no pointer, and reaching for a tool brings its own
    /// layer back — a crosshair that draws no cross, or a line tool that places
    /// invisible objects, reads as a broken tool rather than a hidden layer.
    #[test]
    fn arming_a_tool_unhides_the_layer_it_draws_on() {
        let dir = std::env::temp_dir().join(format!("quantick-app-tool-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("chart-layers.toml");
        let _ = std::fs::remove_file(&path);

        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let (mut app, _events, _commands, _book) = test_app();
        app.chart_layers_path = path.clone();

        let navigate = |app: &mut QuantickApp| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let area = ui.available_rect_before_wrap();
                        app.handle_navigation(ui, area);
                    });
                },
            );
        };

        for (tool, layer) in [
            (
                Tool::Drawing(drawings::DRAWING_TOOLS[0]),
                ChartLayer::Drawings,
            ),
            (Tool::Crosshair, ChartLayer::Crosshair),
        ] {
            app.toolrail.arm(Tool::Pointer);
            app.set_layer_visible(layer, false);
            navigate(&mut app);
            assert!(
                !app.layer_visible(layer),
                "{} must stay hidden while nothing needs it",
                layer.id()
            );
            app.toolrail.arm(tool);
            navigate(&mut app);
            assert!(
                app.layer_visible(layer),
                "arming its tool has to bring {} back",
                layer.id()
            );
        }
        std::fs::remove_file(&path).ok();
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

    /// The same app on a source that quotes prices and nothing else: no book,
    /// no traded volume — what a live CFD bridge publishes.
    fn test_app_without_depth() -> (
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
                capabilities: feed::fixed_capabilities(FeedCapabilities {
                    book_capture: false,
                    history_paging: false,
                    traded_volume: false,
                }),
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

        let before = app.indicators.all().len();
        let slot = app
            .add_script_indicator(index)
            .expect("a click on a known entry claims a slot");
        assert_eq!(app.indicators.all().len(), before + 1, "a slot appeared");
        let view = app
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
        for event in app.indicator_worker.drain_events() {
            app.indicators.apply(event);
        }
        app.maintain_indicator_state();
        let written = crate::indicators::state_file::load(&path);
        assert_eq!(
            written.len(),
            app.indicators
                .all()
                .iter()
                .filter(|view| app.slot_kinds.iter().any(|(slot, _)| *slot == view.slot))
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
        app.orderflow.set_depth_visible(true);
        app.orderflow.handle_depth_event(DepthEvent::Snapshot {
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
        app.orderflow.flush_for_test();
        assert_eq!(app.orderflow.health().active_levels, 2);
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
        app.drawings.place(
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
            app.drawings.items().is_empty(),
            "bar-index drawings cannot survive a source reset honestly"
        );
    }

    #[test]
    fn bar_spec_change_defers_one_frame_and_shows_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.tick_n = 100;
        app.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        app.apply_spec_change();
        assert!(app.loading.is_active(LoadingTask::BarRebuild));
        assert_eq!(
            app.state.spec(),
            &BarSpec::Tick(50),
            "the arming frame must paint the overlay before the rebuild runs"
        );

        app.apply_spec_change();
        assert_eq!(app.state.spec(), &BarSpec::Tick(100));
        assert!(
            app.drawings.items().is_empty(),
            "a new bar partition must not inherit old bar-index anchors"
        );
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn a_still_moving_selector_keeps_deferring_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.tick_n = 100;
        app.apply_spec_change();
        app.tick_n = 200; // the drag continues
        app.apply_spec_change();
        assert_eq!(
            app.state.spec(),
            &BarSpec::Tick(50),
            "no rebuild mid-gesture"
        );
        assert!(app.loading.is_active(LoadingTask::BarRebuild));

        app.apply_spec_change();
        assert_eq!(app.state.spec(), &BarSpec::Tick(200));
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
    }

    #[test]
    fn an_unchanged_spec_never_arms_the_rebuild_indicator() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.apply_spec_change();
        assert!(!app.loading.is_active(LoadingTask::BarRebuild));
        assert!(app.pending_spec.is_none());
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
        assert_eq!(app.state.timeline_revision(), 3);
        assert_eq!(app.state.partial().map(|bar| bar.trade_count), Some(3));
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
        app.tick_n = 1;
        app.apply_spec_change();
        app.apply_spec_change();
        let trades: Vec<_> = (1..=count).map(trade).collect();
        evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
        app.drain_feed();
        assert_eq!(app.state.bars().len() as u64, count);
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
        app.viewport.pan_pixels(200.0 * 8.0, app.slots());
        assert!(!app.viewport.follows_live());
        let was_showing = app.right_edge_time().expect("a bar under the edge");

        // Coarsen: 400 trades become 10 bars, so index 200 no longer exists.
        app.tick_n = 40;
        app.apply_spec_change();
        app.apply_spec_change();
        assert_eq!(app.state.bars().len(), 10);

        let slots = app.slots();
        let (start, end) = app.viewport.visible_range(800.0, slots);
        assert!(
            start < end,
            "the window must still hold bars, got {start}..{end} of {slots}"
        );
        let now_showing = app.right_edge_time().expect("still on a bar");
        let bar = &app.state.bars()[app.viewport.right_edge_bar(slots) as usize];
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
        app.tick_n = 40;
        app.apply_spec_change();
        app.apply_spec_change();
        app.viewport.pan_pixels(5.0 * 8.0, app.slots()); // back to bar 4 of 10
        let was_showing = app.right_edge_time().expect("a bar under the edge");

        app.tick_n = 1;
        app.apply_spec_change();
        app.apply_spec_change();
        assert_eq!(app.state.bars().len(), 400);
        let edge = app.viewport.right_edge_bar(app.slots());
        assert_eq!(
            edge, 160.0,
            "bar 4 of tick(40) opens on trade 161 — bar 160 of tick(1)"
        );
        assert_eq!(app.right_edge_time(), Some(was_showing));
    }

    /// A view following the live edge is already anchored to the newest bar,
    /// whatever the rebuild does to the ones behind it.
    #[test]
    fn a_rebuild_leaves_a_live_view_at_the_live_edge() {
        let (mut app, _cmd_rx) = app_with_history(400);
        assert!(app.viewport.follows_live());
        app.tick_n = 40;
        app.apply_spec_change();
        app.apply_spec_change();
        assert!(app.viewport.follows_live());
        assert_eq!(app.viewport.right_edge_bar(app.slots()), 9.0);
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
            .drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .collect();
        assert_eq!(tools, drawings::DRAWING_TOOLS);
        assert!(
            app.drawings
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

        let before = app.drawings.items()[0].points[0];
        let viewport_before = app.viewport.right_edge_bar(app.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        let after = app.drawings.items()[0].points[0];

        assert!(
            after.bar > before.bar,
            "dragging right moves the anchor right"
        );
        assert!(
            after.price < before.price,
            "dragging down moves the anchor to a lower price"
        );
        assert_eq!(
            app.viewport.right_edge_bar(app.slots()),
            viewport_before,
            "moving a drawing must not pan the market underneath it"
        );
        assert_eq!(
            app.drawing_drag,
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
        let before = app.drawings.items()[0].points[0];

        drag_chart(&mut app, &ctx, start, egui::pos2(start.x, line_y + 100.0));

        assert_ne!(
            app.drawings.items()[0].points[0].price,
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
        let chart = app.last_chart_area.expect("the chart pane was laid out");
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
        let chart_before = app.last_chart_area.expect("chart laid out");

        let pin = app.inspector_pin_rect.expect("pin button rendered");
        click_chart(&mut app, &ctx, pin.center());
        assert!(app.inspector_pinned, "clicking Pin docks the inspector");
        run_frame(&mut app, &ctx);
        let chart_after = app.last_chart_area.expect("chart laid out");
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
            app.drawings.items().is_empty(),
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
            app.drawings.items().len(),
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
        assert!(app.drawings.items()[0].hidden, "the row's eye hides it");

        run_frame(&mut app, &ctx);
        let lock = rect_of(&app, 1, "Lock");
        click_chart(&mut app, &ctx, lock.center());
        assert!(app.drawings.items()[1].locked, "the row's lock locks it");

        run_frame(&mut app, &ctx);
        let front = rect_of(&app, 0, "Front");
        let hidden_line_price = app.drawings.items()[0].points[0].price;
        click_chart(&mut app, &ctx, front.center());
        assert_eq!(
            app.drawings.items()[1].points[0].price,
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
        assert_eq!(app.drawings.items().len(), 1);
        assert_eq!(app.drawings.selected(), Some(0));

        let output = run_frame(&mut app, &ctx);
        assert_eq!(app.drawings.selected(), Some(0));
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
        let before = app.drawings.items()[0].points.clone();
        let viewport_before = app.viewport.right_edge_bar(app.slots());

        let inspector = painted_text(&run_frame(&mut app, &ctx));
        assert!(
            inspector
                .iter()
                .any(|text| text.contains("Rectangle settings")),
            "the settings window must be visible before the resize gesture"
        );

        drag_chart(&mut app, &ctx, first_anchor, egui::pos2(560.0, 240.0));
        let after = &app.drawings.items()[0].points;

        assert_ne!(after[0], before[0], "the dragged corner must move");
        assert_eq!(
            after[1], before[1],
            "resizing one corner must leave the opposite corner fixed"
        );
        assert_eq!(
            app.viewport.right_edge_bar(app.slots()),
            viewport_before,
            "resizing a drawing must not pan the chart"
        );
        assert_eq!(app.drawing_drag, DrawingDrag::None);
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
        app.drawings.set_selected_locked(true);

        let before = app.drawings.items()[0].points[0];
        let viewport_before = app.viewport.right_edge_bar(app.slots());
        drag_chart(
            &mut app,
            &ctx,
            egui::pos2(1_000.0, 300.0),
            egui::pos2(1_040.0, 340.0),
        );
        assert_eq!(
            app.drawings.items()[0].points[0],
            before,
            "locked geometry must not move"
        );
        assert_eq!(
            app.viewport.right_edge_bar(app.slots()),
            viewport_before,
            "the blocked gesture still belongs to the drawing - the chart must not pan"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert_eq!(
            app.drawings.items().len(),
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
            app.drawings.undo_depth(),
            1,
            "creating the drawing is the first undo entry"
        );

        let before_drag = app.drawings.items()[0].points[0];
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
            app.drawings.items()[0].points[0],
            before_drag,
            "the drag really moved the line"
        );

        assert_eq!(
            app.drawings.undo_depth(),
            2,
            "a multi-frame drag coalesces into exactly one undo entry"
        );
        assert!(app.drawings.undo(), "undo the drag");
        assert_eq!(
            app.drawings.items()[0].points[0],
            before_drag,
            "one undo rewinds the whole drag"
        );
        assert!(app.drawings.undo(), "undo the creation");
        assert!(app.drawings.items().is_empty());
    }

    #[test]
    fn delete_and_backspace_never_leak_out_of_focused_inputs() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        assert_eq!(app.drawings.items().len(), 1);

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
            app.drawings.items().len(),
            1,
            "while an input owns the keyboard the delete keys stay in it"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(
            app.drawings.items().is_empty(),
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
        assert!(app.drawings.items().is_empty());
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
            app.drawings.items().len(),
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
            app.drawings.items().is_empty(),
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
        app.drawings
            .selected_mut()
            .expect("placement selects the line")
            .style
            .color = marker;
        assert!(
            painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "the visible line paints its stroke"
        );

        app.drawings.set_selected_hidden(true);
        assert!(
            !painted_line_with_color(&run_frame(&mut app, &ctx), marker),
            "a hidden drawing must not paint"
        );

        app.drawings.select(None);
        click_chart(&mut app, &ctx, stroke_position);
        assert_eq!(
            app.drawings.selected(),
            None,
            "a hidden drawing must not hit-test"
        );

        let viewport_before = app.viewport.right_edge_bar(app.slots());
        drag_chart(&mut app, &ctx, stroke_position, egui::pos2(640.0, 260.0));
        assert_ne!(
            app.viewport.right_edge_bar(app.slots()),
            viewport_before,
            "over a hidden drawing the gesture belongs to the chart again"
        );
    }

    #[test]
    fn a_bar_rebuild_clears_drawings_with_an_explicit_notice_and_no_dead_undo() {
        let (mut app, evt_tx, _cmd_rx, _book_tx) = test_app();
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        app.drawings.place(
            drawing_tool("horizontal-line"),
            ChartPoint {
                bar: 1.0,
                price: 100.0,
            },
        );

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.drain_feed();
        assert!(app.drawings.items().is_empty());
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
        assert_eq!(app.drawings.draft_len(), 1);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(app.drawings.draft().is_none(), "Esc cancels the draft");
        assert_eq!(app.toolrail.tool(), Tool::Pointer);

        // A locked selection with a pending confirmation: Esc peels one
        // layer per press — confirm, then selection, then nothing new.
        app.toolrail
            .arm(Tool::Drawing(drawing_tool("horizontal-line")));
        click_chart(&mut app, &ctx, egui::pos2(700.0, 300.0));
        app.drawings.set_selected_locked(true);
        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Delete)]);
        assert!(app.drawing_delete_confirm, "the confirmation is pending");

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert!(!app.drawing_delete_confirm, "first Esc cancels the confirm");
        assert!(app.drawings.selected().is_some(), "the selection survives");

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Escape)]);
        assert_eq!(app.drawings.selected(), None, "second Esc deselects");
        assert_eq!(app.drawings.items().len(), 1, "nothing was deleted");
    }

    #[test]
    fn backspace_steps_back_through_the_draft_anchors() {
        let (mut app, _commands) = app_with_history(200);
        let ctx = egui::Context::default();
        run_frame(&mut app, &ctx);
        arm_drawing_from_toolbox(&mut app, &ctx, "parallel-channel");
        click_chart(&mut app, &ctx, egui::pos2(650.0, 280.0));
        click_chart(&mut app, &ctx, egui::pos2(750.0, 300.0));
        assert_eq!(app.drawings.draft_len(), 2);

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::Backspace)]);
        assert_eq!(
            app.drawings.draft_len(),
            1,
            "Backspace removes the last placed anchor"
        );
        assert!(
            app.drawings.items().is_empty(),
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
        assert!(app.drawings.items()[0].locked, "Alt+L locks the selection");

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::H, egui::Modifiers::ALT)],
            egui::Modifiers::ALT,
        );
        assert!(app.drawings.items()[0].hidden, "Alt+H hides the selection");
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
        let original_bar = app.drawings.items()[0].points[0].bar;

        run_frame_with_modifiers(
            &mut app,
            &ctx,
            vec![key_press_with(egui::Key::D, egui::Modifiers::COMMAND)],
            egui::Modifiers::COMMAND,
        );
        assert_eq!(app.drawings.items().len(), 2);
        assert_eq!(app.drawings.selected(), Some(1));
        assert_eq!(
            app.drawings.items()[1].points[0].bar,
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
        let start = app.drawings.items()[0].points[0];
        let depth = app.drawings.undo_depth();

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowRight)]);
        assert_eq!(
            app.drawings.items()[0].points[0].bar,
            start.bar + 1.0,
            "one press is one bar"
        );
        assert_eq!(app.drawings.undo_depth(), depth + 1, "one press, one entry");

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
            app.drawings.items()[0].points[0].bar,
            start.bar + 11.0,
            "Shift multiplies the nudge by ten"
        );

        run_frame_with_events(&mut app, &ctx, vec![key_press(egui::Key::ArrowUp)]);
        assert!(
            app.drawings.items()[0].points[0].price > start.price,
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
        assert_eq!(app.drawings.items().len(), 2);
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
        assert_eq!(app.drawings.items().len(), 1);

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
        let standard_levels = app.drawings.items()[0]
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
        let new_levels = app.drawings.items()[1]
            .payload
            .as_any()
            .downcast_ref::<FibPayload>()
            .expect("fib payload")
            .levels
            .len();
        assert_eq!(new_levels, 5, "a new fib starts from the default preset");

        // ...and the first one is untouched.
        let old_levels = app.drawings.items()[0]
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
        app.viewport.pan_pixels(200.0 * 8.0, app.slots());

        app.tick_n = 40;
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

        app.viewport.zoom(8.0); // 64 px candles: only a dozen fit
        app.viewport.pan_pixels(-10_000.0, app.slots()); // into the empty future
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
        assert_eq!(app.orderflow.active_preset_for_test(), "live lane pie");
        assert!(
            app.orderflow.config_for_test().bubble_candle_summary,
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
            with_unknown.orderflow.active_preset_for_test(),
            untouched.orderflow.active_preset_for_test(),
            "a typo in the config must not restyle the chart"
        );
        assert_eq!(
            with_unknown.orderflow.config_for_test(),
            untouched.orderflow.config_for_test()
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
        let opened_with = app.orderflow.active_preset_for_test().to_string();
        assert_ne!(
            opened_with, "live lane pie",
            "nothing declared, nothing applied"
        );

        // The switch path runs this after installing the new feed handle.
        app.feed_id = "mt".to_string();
        app.apply_feed_bubble_preset_after_switch("binance");
        assert_eq!(app.orderflow.active_preset_for_test(), "live lane pie");
    }

    #[test]
    fn a_symbol_hop_inside_one_feed_keeps_the_panel_look() {
        let mut config = test_config();
        config.feeds[0].bubble_preset = Some("live lane pie".to_string());
        let mut app = app_on(config, "binance", "TESTUSDT");
        assert_eq!(app.orderflow.active_preset_for_test(), "live lane pie");

        // The user picks a different look by hand mid-session...
        assert!(app.orderflow.apply_preset("dense tape"));
        // ...then hops symbols inside the same feed: the hand-picked look
        // survives — the declared preset belongs to the feed, not the symbol.
        app.apply_feed_bubble_preset_after_switch("binance");
        assert_eq!(app.orderflow.active_preset_for_test(), "dense tape");

        // Arriving from another feed is what re-applies the declared look.
        app.apply_feed_bubble_preset_after_switch("other-feed");
        assert_eq!(app.orderflow.active_preset_for_test(), "live lane pie");
    }

    #[test]
    fn capture_starts_with_the_feed_and_commits_only_after_the_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();

        // Construction already asked the feed to record: capture follows the
        // market, not the toolbar.
        assert_eq!(take_capture_start(&mut cmd_rx), BOOK_GENERATION_STRIDE);
        assert!(app.orderflow.enabled());
        app.ensure_book_capture();
        assert!(
            cmd_rx.try_recv().is_err(),
            "a recorder already running needs no second command"
        );

        drop(cmd_rx);
        app.request_book_capture(false);
        assert!(
            app.orderflow.enabled(),
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
        let gaps_before = app.orderflow.health().gaps;

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        assert!(app.orderflow.depth_visible());

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(!app.orderflow.depth_visible(), "the map is hidden");
        assert!(app.orderflow.enabled(), "the recorder is untouched");
        assert!(
            cmd_rx.try_recv().is_err(),
            "showing or hiding the map sends no feed command"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(true));
        app.orderflow.flush_for_test();
        assert!(app.orderflow.depth_visible());
        assert_eq!(
            app.orderflow.health().gaps,
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
        app.orderflow.set_enabled(false, generation);
        app.feed_id = "not-in-the-config".to_owned();
        assert!(!app.capabilities().book_capture);

        app.ensure_book_capture();
        assert!(!app.orderflow.enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "a source with no book is never asked to record"
        );
    }

    #[test]
    fn bubble_toggle_needs_no_feed_command_and_leaves_capture_alone() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        assert!(!app.orderflow.bubbles_enabled());

        app.orderflow.set_bubbles_enabled(true);
        assert!(app.orderflow.bubbles_enabled());
        assert!(
            cmd_rx.try_recv().is_err(),
            "aggregate trades already flow; no feed command is needed"
        );

        app.apply_toolbar_action(ToolbarAction::SetHeatmap(false));
        assert!(
            app.orderflow.bubbles_enabled(),
            "hiding the book must not stop the bubbles"
        );
    }

    #[test]
    fn grouping_restart_commits_only_after_command_is_queued() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let grouping = Decimal::new(5, 2);

        assert!(app.orderflow.stage_capture_grouping_for_test(grouping));
        assert_eq!(app.orderflow.health().active_levels, 2);
        app.restart_book_capture();

        assert!(matches!(
            cmd_rx.try_recv(),
            Ok(FeedCommand::RestartBookCapture { .. })
        ));
        assert_eq!(app.orderflow.base_capture_grouping_for_test(), grouping);
        assert_eq!(app.orderflow.health().active_levels, 0);
        assert_eq!(app.orderflow.health().status, "connecting");
    }

    #[test]
    fn closed_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.orderflow.base_capture_grouping_for_test();

        assert!(
            app.orderflow
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        drop(cmd_rx);
        app.restart_book_capture();

        assert_eq!(app.orderflow.base_capture_grouping_for_test(), original);
        assert_eq!(app.orderflow.health().active_levels, 2);
    }

    #[test]
    fn full_restart_channel_rolls_back_grouping_without_losing_history() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        enable_heatmap_with_snapshot(&mut app, &mut cmd_rx);
        let original = app.orderflow.base_capture_grouping_for_test();
        let (full_tx, mut full_rx) = mpsc::channel(1);
        app.commands = full_tx;
        app.commands
            .try_send(FeedCommand::LoadOlder { count: 1 })
            .unwrap();

        assert!(
            app.orderflow
                .stage_capture_grouping_for_test(Decimal::new(5, 2))
        );
        app.restart_book_capture();

        assert!(matches!(
            full_rx.try_recv(),
            Ok(FeedCommand::LoadOlder { count: 1 })
        ));
        assert_eq!(app.orderflow.base_capture_grouping_for_test(), original);
        assert_eq!(app.orderflow.health().active_levels, 2);
    }

    #[test]
    fn depth_channel_updates_heatmap_without_mutating_candles() {
        use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

        let (mut app, _evt_tx, mut cmd_rx, book_tx) = test_app();
        let generation = take_capture_start(&mut cmd_rx);
        let bars_before = app.state.bars().len();
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
        app.orderflow.flush_for_test();
        let book = app.orderflow.health();
        assert_eq!(book.bid_levels, 1);
        assert_eq!(book.ask_levels, 1);
        assert_eq!(app.state.bars().len(), bars_before);
    }

    #[test]
    fn candle_appearance_change_is_render_only() {
        let (mut app, _evt_tx, mut cmd_rx, _book_tx) = test_app();
        take_capture_start(&mut cmd_rx);
        let capture_epoch = app.book_capture_epoch;
        let bar_spec = app.state.spec().clone();

        app.style.candles = CandlePreset::OutlineOnly.style();
        app.style_revision = app.style_revision.saturating_add(1);
        app.emit_style_changed(Some(CandlePreset::OutlineOnly));

        assert_eq!(app.state.spec(), &bar_spec);
        assert!(app.orderflow.enabled());
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

    #[test]
    fn fmt_time_applies_the_offset() {
        // UTC midnight shown in UTC−03:00 is 21:00 of the previous day.
        assert_eq!(fmt_time(0, TzOffset::new(-180)), "21:00:00");
        // UTC midnight in UTC+05:30 is 05:30.
        assert_eq!(fmt_time(0, TzOffset::new(330)), "05:30:00");
    }
}
