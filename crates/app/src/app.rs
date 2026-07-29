//! The egui application: drains the live feed, renders bars, surfaces metrics,
//! and lets the user switch bar type live.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! and the bar-type dispatch in [`crate::state`] (pure, tested), and metric math
//! in [`crate::metrics`] (pure, tested). This layer owns the clocks, the tracing
//! and the widgets, drains the feed each frame, and turns everything into egui
//! shapes.

use std::time::{Duration, Instant};

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use tokio::sync::mpsc;

use quantick_feed_binance::depth::DepthEvent;

use crate::candle_view::{draw_candle, draw_style_window};
use crate::chart::PriceScale;
use crate::config::{AppConfig, FeedCapabilities, ProviderKind};
use crate::dock::{Dock, DockEnv, DockTab};
use crate::feed::{self, FeedCommand, FeedEvent, FeedHandle, FeedNotice, ReplayLink};
use crate::loading::{self, LoadingTask, LoadingTracker};
use crate::metrics::{self, FrameStats};
use crate::notice_card;
use crate::orderflow_view::OrderflowView;
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

/// Split the padded plot area into the candle chart, the optional live strip,
/// the right price gutter and the bottom time strip, so the input handler and
/// the renderer agree on the boundaries. `live_strip_width` of zero means the
/// strip is off and the chart runs straight into the gutter, exactly as it
/// did before the strip existed.
fn plot_split(area: egui::Rect, live_strip_width: f32) -> PlotAreas {
    let plot = area.shrink(16.0);
    let strip_width = live_strip_width.max(0.0);
    let gutter_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_x = (gutter_x - strip_width).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    PlotAreas {
        chart: egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y)),
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
    chart: egui::Rect,
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
    commands: mpsc::Sender<FeedCommand>,
    orderflow: OrderflowView,
    book_capture_epoch: u64,
    book_channel_closed_reported: bool,
    /// Whether the user wants the live strip shown. The pixels it actually
    /// gets are still capability-gated — see [`Self::live_strip_width`].
    live_strip_visible: bool,

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

    // The two chrome zones the design model reserves beside the chart: the
    // tabbed right dock (settings) and the left tool rail (chart tools).
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
    // Pointer position over the plot this frame, for the crosshair.
    hover_pos: Option<egui::Pos2>,

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
            notice: FeedNotice::Clear,
            commands: feed.commands,
            orderflow: OrderflowView::new(symbol.clone()),
            book_capture_epoch: 0,
            book_channel_closed_reported: false,
            live_strip_visible: false,
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
            hover_pos: None,
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
            latest_trade_ms: None,
            live_trades: 0,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        };
        // Recording is not a display choice: it starts with the feed, so
        // hiding the map later never leaves a hole in what was captured.
        app.ensure_book_capture();
        // The map itself stays hidden until asked for — a layer nobody
        // requested must cost no projection. Dev/ops can open it without a
        // click; capture is already running either way.
        app.orderflow
            .set_depth_visible(std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|v| v == "1"));
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
        // A recorded session streams trades and nothing else: there is no depth
        // in the file and no venue to page older history from. Answering
        // honestly here is the whole gate — every affordance already asks the
        // capability rather than the provider name, so the heatmap toggle and
        // "load older" disable themselves during replay.
        if self.replay.is_some() {
            return FeedCapabilities::none();
        }
        self.config
            .provider_of(&self.feed_id)
            .map_or(FeedCapabilities::none(), ProviderKind::capabilities)
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
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
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
        self.history_trades = 0;
        // The old feed's unanswered loads died with its channel; the new feed
        // opens with exactly one backfill in flight.
        self.loading.restart(LoadingTask::History);
        self.latest_trade_ms = None;
        self.orderflow.reset_for_symbol(self.symbol.clone());

        self.active = (self.feed_id.clone(), self.symbol.clone());
        self.ensure_book_capture();
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
                self.state.set_spec(desired);
                self.loading.set_active(LoadingTask::BarRebuild, false);
            }
        }
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
    /// latest trade timestamp and live-trade counts for the metrics.
    fn drain_feed(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    self.loading.end(LoadingTask::History);
                    if let Some(last) = trades.last() {
                        self.latest_trade_ms = Some(last.timestamp_ms);
                    }
                    self.history_trades += trades.len();
                    self.state.ingest_backfill(&trades);
                }
                Ok(FeedEvent::HistoryPrepended(trades)) => {
                    // The reply — even an empty one — answers exactly one
                    // pending load; the indicator survives until the last one.
                    self.loading.end(LoadingTask::History);
                    // Older bars shift every index up; keep the view steady.
                    self.history_trades += trades.len();
                    let added = self.state.prepend_history(&trades);
                    self.viewport.shift_right_edge(added);
                }
                Ok(FeedEvent::Live(trade)) => self.ingest_live_trade(&trade),
                Ok(FeedEvent::LiveBatch(trades)) => {
                    for trade in &trades {
                        self.ingest_live_trade(trade);
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
            self.notice = notice;
        }
    }

    /// Ingest one live trade: bars, order flow and the metrics that follow it.
    fn ingest_live_trade(&mut self, trade: &quantick_engine::Trade) {
        self.latest_trade_ms = Some(trade.timestamp_ms);
        self.live_trades += 1;
        self.trades_since_summary += 1;
        self.orderflow.record_trade(trade);
        self.state.ingest_live(trade);
    }

    /// Throw away everything loaded and wait for the source to refill it.
    ///
    /// Sent by a source that rewound — seeking a replay, for instance. The
    /// chart is rebuilt from the history that follows rather than patched,
    /// because bars that already closed cannot be reopened.
    fn reset_market_state(&mut self) {
        self.state = ChartState::new(self.current_spec());
        self.viewport = Viewport::new();
        self.price_view = PriceView::new();
        self.last_auto_range = None;
        self.hover_pos = None;
        self.history_trades = 0;
        self.latest_trade_ms = None;
        self.last_lane_divider_x = None;
        // The refill arrives as one backfill batch; keep the loading indicator
        // up until it lands. Requests sent to the source before the reset will
        // never be answered, so the count restarts rather than accumulates.
        self.loading.restart(LoadingTask::History);
        self.orderflow.reset_for_symbol(self.symbol.clone());
    }

    /// Drain a bounded number of synchronized depth events. The separate
    /// channel and budget ensure heatmap work cannot block candle ingestion.
    fn drain_book_feed(&mut self) {
        for _ in 0..BOOK_DRAIN_BUDGET {
            match self.book_events.try_recv() {
                Ok(event) => self.orderflow.handle_depth_event(event),
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

    /// How far behind the wall clock the newest trade is — the health signal
    /// for a live feed.
    ///
    /// `None` while a session is replaying: those prints are as old as the day
    /// they were recorded, so measuring them against today's clock reports a
    /// four-month lag and warns about a connection that is working perfectly.
    fn trade_lag_ms(&self) -> Option<i64> {
        if self.replay.is_some() {
            return None;
        }
        metrics::feed_lag_ms(metrics::wall_clock_ms(), self.latest_trade_ms)
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = self.trade_lag_ms();
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let cpu_avg = self.cpu_frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);
        let book = self.orderflow.health();
        let book_lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), book.last_event_ms);
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
            feed_lag_ms = lag,
            trades_per_s = rate,
            live_trades = self.live_trades,
            bar_spec = self.state.spec().summary(),
            book_enabled = book.enabled,
            book_status = book.status,
            book_generation = book.generation,
            book_last_update_id = book.last_update_id,
            book_last_event_ms = book.last_event_ms,
            book_snapshot_observed_ms = book.last_snapshot_observed_ms,
            book_lag_ms = book_lag,
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
                event_code = "HEATMAP_HIGH_LAG",
                symbol = self.symbol.as_str(),
                book_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                book_status = book.status,
                action = "inspect_depth_connection",
                "order-book events are behind wall clock"
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
    fn handle_navigation(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let areas = plot_split(area, self.live_strip_width());
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.state.bars().len() + usize::from(self.state.partial().is_some());
        let divider = self.last_lane_divider_x;
        let in_lane = |position: egui::Pos2| gesture_hits_lane(divider, position.x);

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            egui::Id::new("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = chart.hover_pos();
        // Where the press landed, not where the pointer is now: a pan that
        // started on the candles keeps working when it crosses the divider.
        let dragging_candles = chart
            .interact_pointer_pos()
            .is_some_and(|press| !in_lane(press));
        if total > 0 && chart.dragged() && dragging_candles {
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
        let areas = plot_split(area, self.live_strip_width());
        let chart_rect = areas.chart;
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

        // Auto-fit the visible bars, then apply any manual price pan/zoom.
        let Some(auto_scale) = PriceScale::auto(
            visible_closed,
            partial_visible,
            chart_rect.top(),
            chart_rect.bottom(),
            0.05,
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
            closed_start,
            visible_closed,
            partial_visible,
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

        // Above the flow layers: everything else on the canvas is read against
        // it. Drawn on the unclipped painter so the chip reaches the gutter.
        if let Some(bar) = partial.or_else(|| closed.last()) {
            self.draw_last_price(painter, chart_rect, axis_x, &scale, bar);
        }
        // The candles' own mark, so it is placed and clipped in their pane.
        self.draw_backfill_divider(painter, history_rect, total, cw);
        self.draw_time_strip(painter, areas.time_strip, closed, start, end, total);
        self.draw_lane_time_axis(
            painter,
            split_time_strip(areas.time_strip, self.last_lane_divider_x).1,
            self.orderflow.live_lane_window_ms(closed),
        );
        self.draw_crosshair(painter, chart_rect, axis_x, &scale);
        self.orderflow.draw_status_badge(painter, chart_rect);

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.last_auto_range = Some(auto_range);
        self.last_chart_height = chart_rect.height();
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

    /// Data-honesty label for how the aggressor side of each trade is known,
    /// or `None` when the venue reports true sides (§8 — the status bar's
    /// middle section).
    fn side_note(&self) -> Option<String> {
        if let Some(link) = &self.replay {
            Some(match link.session.header.side_source.as_deref() {
                Some(source) => format!("side: {source}"),
                None => "side: not recorded".to_owned(),
            })
        } else {
            // The running feed, not the still-uncommitted selection.
            self.config.side_note(&self.active.0).map(str::to_owned)
        }
    }

    /// Everything the status bar reports this frame.
    fn status_model(&self) -> statusbar::StatusModel {
        let bars = self.state.bars();
        let (backfilled, live) = match self.state.backfill_boundary() {
            Some(boundary) => (boundary, bars.len().saturating_sub(boundary)),
            None => (0, bars.len()),
        };
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
            feed_lag_ms: self.trade_lag_ms(),
            spec_summary: self.state.spec().summary(),
            bar_progress: self
                .state
                .progress()
                .map(|(progress, unit)| fmt_progress(&progress, unit)),
            backfilled_bars: backfilled,
            live_bars: live,
            side_note: self.side_note(),
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
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
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
        // The old feed's trouble is not the new feed's: switching away from a
        // blocked source must not leave its instruction on screen.
        self.notice = FeedNotice::Clear;
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
        self.notice = FeedNotice::Clear;
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
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu_frames.record(cpu * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_feed();
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
        // Chrome panels claim their zones outside-in (§5): menu and toolbar
        // on top, the status line at the very bottom with the replay
        // transport directly above it, then the tool rail and the dock on the
        // sides. The chart keeps whatever remains.
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        let status = self.status_model();
        statusbar::draw(ctx, &status, &mut self.tz);
        // The browser window and, while a session plays, the transport bar.
        if let Some(action) = self.replay_view.draw(ctx, self.replay.as_ref()) {
            self.apply_replay_action(action);
        }
        self.toolrail.draw(ctx);
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

        let off = plot_split(area, 0.0);
        assert!(off.live_strip.is_none());
        assert_eq!(off.chart.right(), off.price_gutter.left());

        let on = plot_split(area, crate::live_strip::LIVE_STRIP_WIDTH_PX);
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
            }],
            metatrader: Default::default(),
        }
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

        evt_tx.try_send(FeedEvent::Reset).unwrap();
        app.drain_feed();
        assert_eq!(app.loading.count(LoadingTask::History), 1);
    }

    #[test]
    fn bar_spec_change_defers_one_frame_and_shows_the_rebuild() {
        let (mut app, _evt_tx, _cmd_rx, _book_tx) = test_app();
        app.tick_n = 100;

        app.apply_spec_change();
        assert!(app.loading.is_active(LoadingTask::BarRebuild));
        assert_eq!(
            app.state.spec(),
            &BarSpec::Tick(50),
            "the arming frame must paint the overlay before the rebuild runs"
        );

        app.apply_spec_change();
        assert_eq!(app.state.spec(), &BarSpec::Tick(100));
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
                },
                FeedConfig {
                    id: "b".to_string(),
                    name: "B".to_string(),
                    provider: ProviderKind::Binance,
                    symbols: vec!["BBB".to_string()],
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
