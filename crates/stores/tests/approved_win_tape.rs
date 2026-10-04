//! The approved WIN native tape, pinned byte for byte.
//!
//! The trader approved the WIN native tape — volume dots at each execution's
//! own time and price, pies, memberships that hold once settled, its own
//! clock and its own price fit — at commit aafcc21f. The golden file beside
//! this test is what that commit drew for a real WINV26 recording replayed
//! through the harness below: every mark's exact price, quantity-weighted
//! time, buy and sell quantities, radius, pie split, position and members,
//! the tape's clock ticks and its fitted price window, at six instants rolling
//! across the recording. Every later commit must draw the same bytes, beside
//! the candles (`native_tape`) and alone (`tape_only`), at the same lane width.
//!
//! The harness is the app's tape path without a window: the order-flow view
//! (`crates/app/src/orderflow_view*`), its book worker run synchronously after
//! each frame (`orderflow_worker.rs`), the pane's lay-out, price fit and
//! projection request (`pane/draw_chart.rs`, `frame_layout.rs`,
//! `frame_stages.rs`), the replay clock (`tab/tape_clock.rs`) and the painter's
//! tape frame (`orderflow_render/bubbles.rs`, `pane/render_registry/axes.rs`).
//! Each mode is drawn through both tape paths and compared with the one
//! golden: the path the app draws, which keeps the published tape and lays
//! the accepted prints beside it (`with_pending_overlay`,
//! `project_tape_frame_with_overlay`), and the complete path, which folds the
//! whole pending tape every frame (`with_pending_tape`, `project_tape_frame`).
//! Every decision is the engine's, the chart crate's or the shipped preset's;
//! this file only calls them in the order the app does, so a change to that
//! app glue has to be mirrored here or this test stops watching it. The clock
//! labels are measured on a fixed label slot, because the real width is a
//! font metric.
//!
//! When this fails, the tape changed. Do not edit the golden to match and do
//! not relax the comparison: the approved tape changes only with the trader's
//! approval, and then the new golden is the file the failing run wrote, read
//! and approved first.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quantick_chart::geometry::tape_price_window;
use quantick_chart::price_view::PriceView;
use quantick_chart::state::ChartState;
use quantick_chart::viewport::Viewport;
use quantick_engine::fixture::parse_trades;
use quantick_engine::{Bar, BarSpec, Side, Trade};
use quantick_orderflow::config::theme::OrderflowRenderStyle;
use quantick_orderflow::engine::{BookEngine, BookPublished, ProjectionRequest, VisibleOrderflow};
use quantick_orderflow::projection::{
    AggressionPrimitive, PendingTape, TapeDotFrame, TapeDotGeometry, TapeDotMemory,
    TapeHorizontalGeometry, draws_bubble, project_tape_frame, project_tape_frame_with_overlay,
};
use quantick_orderflow::tape_clock::TapeClock;
use quantick_orderflow::{
    DotRungMemory, HeatmapConfig, LiveEdge, PaneGeometry, PriceWindow, lane_bars, lane_time_ticks,
    reserved_span_ms,
};
use quantick_stores::bubble_assets::{AssetBubbles, AssetBubblesFile, AssetSource, AssetTrack};
use quantick_stores::bubble_presets;
use quantick_stores::config::AppConfig;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

/// 3,267 unchanged prints of WINV26 on 2026-09-22, 09:33:20 to 09:35:00.
const TRADES: &str = include_str!("fixtures/win_2026_09_22_approved_tape.csv");
/// What aafcc21f drew for them. See the module comment before touching it.
const GOLDEN: &str = include_str!("fixtures/win_2026_09_22_approved_tape.golden.txt");

/// 09:33:20.000 at -03:00, where the replay starts.
const SLICE_START_MS: i64 = 1_790_080_400_000;
/// 09:35:00.000, where it stops.
const SLICE_END_MS: i64 = 1_790_080_500_000;
/// One painted frame per 50 ms of market time.
const FRAME_STEP_MS: i64 = 50;
/// The frames written to the golden, after the replay starts. Whole frames,
/// deliberately off the round second so the open native window is mid-way.
const INSTANTS_MS: [i64; 6] = [20_000, 35_050, 50_100, 65_150, 80_200, 99_950];
/// The trader's flow pane builds 2,000-print candles.
const BARS: &str = "tick:2000";
/// The tape's band, equal in both modes.
const LANE_WIDTH_PX: f32 = 350.0;
/// Beside the candles the preset's lane share gives the band above.
const BESIDE_CHART_WIDTH_PX: f32 = 1_000.0;
/// A short flow pane: dots at neighbouring prices touch there, so the golden
/// holds merges across prices as well as along time.
const CHART_HEIGHT_PX: f32 = 300.0;
/// B3 local time, for the golden's clock.
const LOCAL_OFFSET_MS: i64 = -3 * 3_600_000;
/// One `HH:MM:SS` clock label and its two gaps: the monospace label measures
/// about 48 px at 10 px, and the axis keeps 8 px either side.
const CLOCK_LABEL_SLOT_PX: f32 = 64.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// `native_tape = true`, `tape_only = false`: the tape beside the candles.
    Beside,
    /// `tape_only = true`: the tape alone, full width.
    TapeOnly,
}

impl Mode {
    fn chart_width(self) -> f32 {
        match self {
            Self::Beside => BESIDE_CHART_WIDTH_PX,
            Self::TapeOnly => LANE_WIDTH_PX,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Beside => "native tape beside the candles",
            Self::TapeOnly => "tape only",
        }
    }
}

/// Which of the two tape paths a replay paints through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    /// What the app draws: the published tape kept, the accepted prints
    /// beside it as the cells they touch.
    Overlay,
    /// The complete pending projection, folded whole on every frame.
    Complete,
}

impl Path {
    fn label(self) -> &'static str {
        match self {
            Self::Overlay => "overlay",
            Self::Complete => "complete",
        }
    }

    /// The painter's tape frame for `frame`, through this path's call.
    #[allow(clippy::too_many_arguments)]
    fn tape_frame(
        self,
        marks: Vec<AggressionPrimitive>,
        memory: &mut TapeDotMemory,
        style: &OrderflowRenderStyle,
        geometry: TapeDotGeometry,
        time: Option<(LiveEdge, i64)>,
        prices: Option<PriceWindow>,
        frame: &VisibleOrderflow,
    ) -> Option<TapeDotFrame> {
        let facts = frame.projection.tape_facts.as_deref();
        match self {
            Self::Overlay => project_tape_frame_with_overlay(
                marks,
                Some(memory),
                style,
                geometry,
                time,
                prices,
                facts,
                frame.tape_overlay.as_deref(),
            ),
            Self::Complete => {
                project_tape_frame(marks, Some(memory), style, geometry, time, prices, facts)
            }
        }
    }
}

// The four places the modes enter the harness. At aafcc21f tape only was the
// one native tape: the golden was written with the first three reading
// `tape_only` where they now read `native_tape`, and with the preset itself
// opening tape only.

/// `HeatmapConfig::native_tape` names the tape every processing site builds.
fn native(config: &HeatmapConfig) -> bool {
    config.native_tape()
}

/// The painter draws the factual tape when the frame was built as one.
fn factual_tape(volume_dots: bool, style: &OrderflowRenderStyle) -> bool {
    volume_dots && style.dot_sizing.is_some_and(|sizing| sizing.native_tape)
}

/// The price fit's padding for the tape's largest dot.
fn tape_padding_px(config: &HeatmapConfig, chart_width: f32, chart_height: f32) -> f32 {
    TapeHorizontalGeometry::native_price_inset_px(config, chart_width, chart_height)
}

/// What a WINV26 tab opens on: the shipped feed config's WIN asset, resolved
/// with nothing stored for it — the same resolution the app's tab runs.
fn win_asset_settings() -> AssetBubbles {
    let feeds: AppConfig =
        toml::from_str(include_str!("../../app/config/feeds.toml")).expect("shipped feeds parse");
    let asset = feeds
        .feed("metatrader-b3")
        .expect("the B3 feed")
        .bubble_asset("WINV26");
    let (track, settings) = AssetTrack::resolve(
        asset,
        &bubble_presets::embedded(),
        &AssetBubblesFile::default(),
        false,
    );
    assert_eq!(track.source(), AssetSource::Declared);
    assert_eq!(settings.look.name, "mini index regions");
    settings
}

/// Apply the mode's switches over the shipped WIN asset's look.
fn switch_mode(config: &mut HeatmapConfig, mode: Mode) {
    match mode {
        Mode::Beside => {
            config.live_lane.native_tape = true;
            config.live_lane.tape_only = false;
        }
        Mode::TapeOnly => config.live_lane.tape_only = true,
    }
}

/// `BookCommand`, the order-flow view's messages to its worker.
enum Command {
    Trade(Trade),
    TapeTrade {
        ordinal: u64,
        trade: Trade,
    },
    TapeEpoch(u64),
    ApplyVisualConfig(HeatmapConfig),
    TapePriceGrid {
        step: Decimal,
        reference_price: Option<Decimal>,
    },
    Project(ProjectionRequest),
}

/// The book worker's loop (`orderflow_worker::run`), one batch at a time.
struct Worker {
    engine: BookEngine,
    queue: Vec<Command>,
    last_request: Option<ProjectionRequest>,
    tape_epoch: u64,
    through_ordinal: u64,
    tape_receipt: Option<(u64, u64)>,
    mailbox: (BookPublished, Option<(u64, u64)>),
}

impl Worker {
    fn new() -> Self {
        Self {
            engine: BookEngine::new("WINV26"),
            queue: Vec::new(),
            last_request: None,
            tape_epoch: 0,
            through_ordinal: 0,
            tape_receipt: None,
            mailbox: (BookPublished::initial(), None),
        }
    }

    fn send(&mut self, command: Command) {
        self.queue.push(command);
    }

    /// Apply every queued command, build the newest layout and publish.
    fn run_batch(&mut self, cache_now: Instant) {
        if self.queue.is_empty() {
            return;
        }
        let mut incoming = None;
        for command in std::mem::take(&mut self.queue) {
            match command {
                Command::Trade(trade) => self.engine.record_trade(&trade),
                Command::TapeTrade { ordinal, trade } => {
                    self.engine.record_trade(&trade);
                    self.through_ordinal = ordinal;
                }
                Command::TapeEpoch(epoch) => {
                    self.tape_epoch = epoch;
                    self.through_ordinal = 0;
                    self.tape_receipt = None;
                    self.last_request = None;
                    incoming = None;
                }
                Command::ApplyVisualConfig(config) => self.engine.apply_visual_config(config),
                Command::TapePriceGrid {
                    step,
                    reference_price,
                } => self.engine.size_from_tape(step, reference_price),
                Command::Project(request) => incoming = Some(request),
            }
        }
        if let Some(request) = incoming {
            self.engine.note_price_window(request.price_range);
            self.last_request = Some(request);
        }
        if let Some(request) = &self.last_request
            && self.engine.any_layer_enabled()
            && self.engine.project_at(request, cache_now).is_some()
        {
            self.tape_receipt = Some((self.tape_epoch, self.through_ordinal));
        }
        self.mailbox = (self.engine.published(), self.tape_receipt);
    }
}

/// `OrderflowView`: the pane's order-flow state and its immediate tape.
struct View {
    config: HeatmapConfig,
    published: BookPublished,
    last_seen_base: Decimal,
    last_tape_price_grid: Option<(Decimal, Option<Decimal>)>,
    dot_rungs: DotRungMemory,
    tape_clock: TapeClock,
    pending_tape: PendingTape,
    tape_dots: TapeDotMemory,
    worker: Worker,
}

impl View {
    /// A chart opens on the presets file's active preset.
    fn new() -> Self {
        let presets = bubble_presets::embedded();
        let mut config = HeatmapConfig::default();
        presets
            .get(&presets.active)
            .expect("the shipped presets name their active preset")
            .apply_to(&mut config);
        Self {
            last_seen_base: config.price_grouping,
            config,
            published: BookPublished::initial(),
            last_tape_price_grid: None,
            dot_rungs: DotRungMemory::default(),
            tape_clock: TapeClock::default(),
            pending_tape: PendingTape::default(),
            tape_dots: TapeDotMemory::default(),
            worker: Worker::new(),
        }
    }

    fn immediate_tape(&self) -> bool {
        native(&self.config) && self.config.volume_dots.enabled
    }

    fn lane_now_ms(&self) -> Option<i64> {
        if native(&self.config) {
            self.tape_clock.now_ms()
        } else {
            None
        }
    }

    fn set_replay_clock_at(&mut self, position_ms: i64, applied: Option<i64>, next: Option<i64>) {
        if native(&self.config) {
            self.tape_clock.replay_at(position_ms, applied, next);
        } else {
            self.tape_clock.reset();
        }
    }

    fn commit_config_changes(&mut self, before: &HeatmapConfig) {
        self.config.sanitize();
        if self.config == *before {
            return;
        }
        self.tape_dots.clear();
        if native(&self.config) != native(before)
            || self.config.volume_dots.enabled != before.volume_dots.enabled
        {
            let epoch = self.pending_tape.reset();
            self.published.frame = None;
            self.worker.send(Command::TapeEpoch(epoch));
        }
        assert_eq!(self.config.price_grouping, before.price_grouping);
        self.worker
            .send(Command::ApplyVisualConfig(self.config.clone()));
    }

    fn set_projection_demand(&mut self, wanted: bool) {
        if self.config.projection_demand == wanted {
            return;
        }
        let before = self.config.clone();
        self.config.projection_demand = wanted;
        self.commit_config_changes(&before);
    }

    fn record_trade(&mut self, trade: &Trade) {
        if !self.config.any_layer_enabled() {
            return;
        }
        self.pending_tape.observe_opening_burst(trade.timestamp_ms);
        if self.immediate_tape() {
            if !self.pending_tape.started() {
                self.worker
                    .send(Command::ApplyVisualConfig(self.config.clone()));
                self.worker
                    .send(Command::TapeEpoch(self.pending_tape.epoch()));
            }
            let ordinal = self.pending_tape.record(trade, &self.config);
            self.worker.send(Command::TapeTrade {
                ordinal,
                trade: trade.clone(),
            });
        } else {
            self.worker.send(Command::Trade(trade.clone()));
        }
    }

    fn observe_tape_price_grid(&mut self, step: Decimal, reference_price: Option<Decimal>) {
        let grid = (step, reference_price);
        if self.last_tape_price_grid == Some(grid) {
            return;
        }
        self.last_tape_price_grid = Some(grid);
        self.worker.send(Command::TapePriceGrid {
            step,
            reference_price,
        });
    }

    fn sync_published(&mut self) {
        let (mut book, receipt) = self.worker.mailbox.clone();
        match receipt {
            Some((epoch, through)) if epoch == self.pending_tape.epoch() => {
                self.pending_tape.acknowledge(through);
            }
            _ => book.frame = None,
        }
        self.published = book;
        let base = self.published.base_price_grouping;
        if base != self.last_seen_base {
            self.last_seen_base = base;
            self.config.price_grouping = base;
        }
    }

    /// `(band width, the instant its right edge stands for)`.
    fn live_lane(&mut self, chart_width: f32) -> Option<(f32, i64)> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let end_ms = self.lane_now_ms().or(self.published.live_end_ms)?;
        Some((self.config.live_lane.resolved_width_px(chart_width), end_ms))
    }

    fn tape_price_range(&self) -> Option<(f64, f64)> {
        if self.immediate_tape() {
            let window_ms = self.config.lane_window_ms(15_000);
            let retained = self
                .lane_now_ms()
                .and_then(|now| self.tape_dots.price_range(now, window_ms));
            return self
                .pending_tape
                .price_range(
                    self.published
                        .frame
                        .as_ref()
                        .map(|frame| frame.projection.as_ref()),
                    self.lane_now_ms(),
                    window_ms,
                )
                .into_iter()
                .chain(retained)
                .reduce(|(low, high), (next_low, next_high)| {
                    (low.min(next_low), high.max(next_high))
                });
        }
        let frame = self.published.frame.as_deref()?;
        quantick_orderflow::projection::tape_price_range(&frame.projection.aggressions)
    }

    #[allow(clippy::too_many_arguments)]
    fn project_visible(
        &mut self,
        timeline_revision: u64,
        first_bar_index: usize,
        closed: &[Bar],
        partial: Option<&Bar>,
        lane: bool,
        on_newest_bar: bool,
        lane_reference_ms: Option<i64>,
        price_range: (f64, f64),
        geometry: PaneGeometry,
        path: Path,
    ) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.any_layer_enabled() {
            return None;
        }
        self.sync_published();
        let tape_span = self.tape_price_range().map(|(low, high)| high - low);
        let dot_zoom = self
            .dot_rungs
            .choose(geometry, &self.config, price_range, tape_span);
        let request = ProjectionRequest {
            timeline_revision,
            first_bar_index,
            closed: closed.to_vec(),
            partial: partial.cloned(),
            lane,
            on_newest_bar,
            lane_reference_ms,
            lane_now_ms: self.lane_now_ms(),
            price_range,
            dot_zoom: Some(dot_zoom),
        };
        let frame = if !self.immediate_tape() || self.pending_tape.is_empty() {
            self.published.frame.clone()
        } else {
            let with_pending = match path {
                Path::Overlay => VisibleOrderflow::with_pending_overlay,
                Path::Complete => VisibleOrderflow::with_pending_tape,
            };
            with_pending(
                &self.pending_tape,
                &self.config,
                &request,
                self.published.frame.as_deref(),
            )
            .map(Arc::new)
        };
        self.worker.send(Command::Project(request));
        frame
    }
}

/// `ChartPane` with its tab's replay clock.
struct Pane {
    view: View,
    state: ChartState,
    viewport: Viewport,
    price_view: PriceView,
    auto_range: Option<(f64, f64)>,
    chart_width: f32,
    path: Path,
    /// Painted frames that carried their accepted prints beside the
    /// published tape.
    overlaid: usize,
}

impl Pane {
    fn new(mode: Mode, path: Path) -> Self {
        let mut view = View::new();
        let before = view.config.clone();
        win_asset_settings().look.apply_to(&mut view.config);
        switch_mode(&mut view.config, mode);
        view.commit_config_changes(&before);
        assert!(native(&view.config), "the approved tape is the native tape");
        assert_eq!(view.config.tape_only(), mode == Mode::TapeOnly);
        assert_eq!(
            view.config
                .live_lane
                .resolved_width_px(mode.chart_width())
                .to_bits(),
            LANE_WIDTH_PX.to_bits(),
            "both modes draw the tape on the same band"
        );
        let spec = BarSpec::parse(BARS).expect("the flow pane's bars");
        Self {
            view,
            state: ChartState::new(spec),
            viewport: Viewport::new(),
            price_view: PriceView::new(),
            auto_range: None,
            chart_width: mode.chart_width(),
            path,
            overlaid: 0,
        }
    }

    /// `ChartPane::ingest_live_trade`: the tape, the bars, then the grid.
    fn ingest(&mut self, trade: &Trade) {
        self.view.record_trade(trade);
        self.state.ingest_live(trade);
        if let Some(step) = self.state.tape_price_step() {
            self.view
                .observe_tape_price_grid(step, self.state.tape_reference_price());
        }
    }

    /// One painted frame at the replay's playhead; `out` receives the tape.
    fn frame(&mut self, position_ms: i64, next_ms: Option<i64>, out: Option<&mut String>) {
        let applied = self.state.trades().last().map(|trade| trade.timestamp_ms);
        self.view.set_replay_clock_at(position_ms, applied, next_ms);
        let Some(drawn) = self.paint() else {
            if let Some(out) = out {
                out.push_str("no tape drawn\n");
            }
            return;
        };
        if let Some(out) = out {
            drawn.write(out);
        }
    }

    fn paint(&mut self) -> Option<Drawn> {
        let Self {
            view,
            state,
            viewport,
            price_view,
            auto_range,
            chart_width,
            path,
            overlaid,
        } = self;
        let (chart_width, chart_height) = (*chart_width, CHART_HEIGHT_PX);
        let closed = state.bars();
        let partial = state.partial();
        let total = closed.len() + usize::from(partial.is_some());
        if total == 0 {
            return None;
        }
        let tape_only = view.config.tape_only();
        let native_tape = native(&view.config);
        if tape_only {
            viewport.snap_to_live();
        }
        let live_lane = view.live_lane(chart_width);
        let lane_width = if tape_only {
            chart_width
        } else {
            live_lane.map_or(0.0, |(width, _)| width)
        };
        // `lane_divider_x`, with the chart's left edge at zero.
        let divider = (lane_width.is_finite() && lane_width > 0.0 && lane_width <= chart_width)
            .then_some(chart_width - lane_width);
        let history_width = divider.unwrap_or(chart_width);
        viewport.clamp_to_window(history_width, total);
        let (start, end) = viewport.visible_range(history_width, total);
        let padding = tape_padding_px(&view.config, chart_width, chart_height);

        // The price fit: the tape's prints and the last price.
        let tape_range = view.tape_price_range();
        assert!(native_tape, "the approved tape fits its own prints");
        let newest = partial.or_else(|| closed.last());
        let fitted = tape_price_window(
            tape_range,
            newest.and_then(|bar| bar.close.to_f64()),
            *auto_range,
            0.0,
            chart_height,
            padding,
        )?;
        let fitted = fitted.range();
        let scale = price_view.scale(fitted, 0.0, chart_height);
        let price_range = scale.range();
        let closed_total = closed.len();
        let closed_start = start.min(closed_total);
        let closed_end = end.min(closed_total);
        let partial_visible = partial.filter(|_| closed_total >= start && closed_total < end);

        // The projection request, every frame.
        view.set_projection_demand(true);
        let window_ms = view.config.lane_window_ms(reserved_span_ms(closed));
        let frame = view.project_visible(
            state.timeline_revision(),
            closed_start,
            &closed[closed_start..closed_end],
            partial_visible,
            lane_width > 0.0,
            end == total,
            Some(reserved_span_ms(closed)),
            price_range,
            PaneGeometry {
                px_per_bar: viewport.px_per_bar(),
                lane_width_px: lane_width,
                lane_window_ms: window_ms,
                height_px: chart_height,
                lane_bars: lane_bars(closed, partial, window_ms),
            },
            *path,
        );
        *auto_range = Some(fitted);
        let frame = frame?;

        // The painter's tape frame.
        let mut style = OrderflowRenderStyle::from_config(&view.config, [0, 0, 0, 255]);
        style.dot_sizing = frame
            .volume_dots
            .as_ref()
            .and_then(|scale| view.dot_rungs.sizing(scale, chart_height));
        let tape_prices = PriceWindow::from_f64_range(price_range);
        let tape_time = match (frame.live_edge, frame.volume_dots.as_ref()) {
            (Some(mut edge), Some(dots)) => {
                edge.now_ms = view.lane_now_ms().unwrap_or(edge.now_ms);
                edge.window_ms = view.config.lane_window_ms(edge.reference_ms);
                Some((edge, dots.tape_window_ms))
            }
            _ => None,
        };
        let marks: Vec<AggressionPrimitive> = frame
            .projection
            .aggressions
            .iter()
            .filter(|mark| draws_bubble(&style, mark))
            .cloned()
            .collect();
        if !style.aggression_layer && !style.lane_aggression_layer {
            return None;
        }
        let mut style = style.sanitized();
        if !factual_tape(frame.projection.volume_dots, &style) {
            return None;
        }
        let lane_left = divider.unwrap_or(chart_width);
        let geometry =
            TapeHorizontalGeometry::resolve(chart_width - lane_left, chart_height, &style.bubbles);
        style.bubbles.max_radius = geometry.max_radius;
        let lane_start = 1.0 - 1.0 / frame.slot_count.max(1) as f64;
        *overlaid += usize::from(*path == Path::Overlay && frame.tape_overlay.is_some());
        let tape = path.tape_frame(
            marks,
            &mut view.tape_dots,
            &style,
            TapeDotGeometry {
                left_x: lane_start,
                right_x: 1.0,
                width_px: geometry.span_px,
                height_px: chart_height,
            },
            tape_time,
            tape_prices,
            &frame,
        )?;
        style.bubbles.max_radius = tape.max_radius;
        let sizing = style.dot_sizing.expect("a factual tape has its sizing");

        // The tape's own clock under it.
        let clock_ms = live_lane.map(|(_, end_ms)| end_ms);
        let clock_geometry = TapeHorizontalGeometry::resolve(
            chart_width - lane_left,
            chart_height,
            &view.config.bubbles,
        );
        let lane_window_ms = view.config.lane_window_ms(reserved_span_ms(closed));
        let ticks = clock_ms.map(|end_ms| {
            let room = (clock_geometry.span_px / CLOCK_LABEL_SLOT_PX)
                .floor()
                .max(0.0) as usize;
            let start_ms = end_ms - lane_window_ms;
            lane_time_ticks(end_ms, lane_window_ms, room)
                .into_iter()
                .map(|instant| {
                    let x = clock_geometry.x((instant - start_ms) as f64 / lane_window_ms as f64);
                    (instant, x)
                })
                .collect::<Vec<_>>()
        });

        let marks = tape
            .marks
            .iter()
            .map(|mark| {
                // Every mark is on the tape, so only the tape's full size
                // is read; the painter overrides it with the frame's own.
                assert!(mark.live, "the tape draws only tape marks");
                let fulls = (tape.full_quantity, tape.full_quantity);
                let radius = sizing.draw(&style.bubbles, &style.live_lane, mark, fulls).1;
                let x = geometry.x((mark.x - lane_start) / (1.0 - lane_start));
                let y = mark.y as f32 * chart_height;
                DrawnMark {
                    mark: mark.clone(),
                    radius,
                    x,
                    y,
                }
            })
            .collect();
        Some(Drawn {
            clock_ms,
            edge: tape_time
                .map(|(edge, dot_window_ms)| (edge.now_ms, edge.window_ms, dot_window_ms)),
            price_range,
            geometry,
            max_radius: tape.max_radius,
            full_quantity: tape.full_quantity,
            ticks: ticks.unwrap_or_default(),
            marks,
        })
    }
}

struct DrawnMark {
    mark: AggressionPrimitive,
    radius: f32,
    /// From the lane's left edge, in pixels.
    x: f32,
    /// From the chart's top, in pixels.
    y: f32,
}

struct Drawn {
    clock_ms: Option<i64>,
    /// `(now, window, native window)` the tape was placed at.
    edge: Option<(i64, i64, i64)>,
    price_range: (f64, f64),
    geometry: TapeHorizontalGeometry,
    max_radius: f32,
    full_quantity: Decimal,
    ticks: Vec<(i64, f32)>,
    marks: Vec<DrawnMark>,
}

impl Drawn {
    fn write(&self, out: &mut String) {
        let _ = writeln!(
            out,
            "clock {}",
            self.clock_ms.map_or_else(|| "none".to_owned(), local_time)
        );
        match self.edge {
            Some((now_ms, window_ms, dot_window_ms)) => {
                let _ = writeln!(
                    out,
                    "edge now={} window_ms={window_ms} native_window_ms={dot_window_ms}",
                    local_time(now_ms)
                );
            }
            None => out.push_str("edge none\n"),
        }
        let _ = writeln!(
            out,
            "price_window low={:.4} high={:.4}",
            self.price_range.0, self.price_range.1
        );
        let _ = writeln!(
            out,
            "geometry max_radius={:.3} inset_px={:.3} span_px={:.3} price_inset_px={:.3}",
            self.geometry.max_radius,
            self.geometry.inset_px,
            self.geometry.span_px,
            self.geometry.price_inset_px
        );
        out.push_str("ticks");
        for (instant, x) in &self.ticks {
            let _ = write!(out, " {}@{x:.3}", local_time(*instant));
        }
        out.push('\n');
        let _ = writeln!(
            out,
            "marks {} full_quantity={} max_radius={:.3}",
            self.marks.len(),
            self.full_quantity.normalize(),
            self.max_radius
        );
        for (index, drawn) in self.marks.iter().enumerate() {
            let mark = &drawn.mark;
            let time = (mark.timestamp_quantity / mark.quantity).round_dp(3);
            let _ = writeln!(
                out,
                "{} price={} time={} span_ms={} qty={} buy={} sell={} share={:.4} side={} trades={} r={:.3} x={:.3} y={:.3} ids={}",
                index + 1,
                mark.price.normalize(),
                time.normalize(),
                mark.last_timestamp_ms - mark.first_timestamp_ms,
                mark.quantity.normalize(),
                mark.buy_quantity.normalize(),
                (mark.quantity - mark.buy_quantity).normalize(),
                mark.buy_share,
                match mark.side {
                    Side::Buy => "B",
                    Side::Sell => "S",
                },
                mark.trade_count,
                drawn.radius,
                drawn.x,
                drawn.y,
                id_ranges(&mark.agg_ids)
            );
        }
    }
}

/// `HH:MM:SS.mmm` at B3's -03:00.
fn local_time(timestamp_ms: i64) -> String {
    let day_ms = (timestamp_ms + LOCAL_OFFSET_MS).rem_euclid(86_400_000);
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        day_ms / 3_600_000,
        day_ms / 60_000 % 60,
        day_ms / 1_000 % 60,
        day_ms % 1_000
    )
}

/// A group's members as ascending runs: `199320-199324,199327`.
fn id_ranges(ids: &[u64]) -> String {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for id in ids {
        match runs.last_mut() {
            Some((_, last)) if *last + 1 == id => *last = id,
            _ => runs.push((id, id)),
        }
    }
    runs.iter()
        .map(|(first, last)| {
            if first == last {
                first.to_string()
            } else {
                format!("{first}-{last}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Replay the fixture through the pane, painting through `path`, and write
/// the tape at each instant; also returns the frames the overlay drew.
fn replay(mode: Mode, path: Path) -> (String, usize) {
    let trades = parse_trades(TRADES).expect("the fixture is an engine trade file");
    assert_eq!(trades.len(), 3_267);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# The approved WIN native tape as aafcc21f drew it. A test compares every byte:\n\
         # read crates/stores/tests/approved_win_tape.rs before changing anything here.\n\
         fixture win_2026_09_22_approved_tape.csv trades={} ids={}-{}\n\
         preset \"mini index regions\" bars={BARS} lane={LANE_WIDTH_PX}x{CHART_HEIGHT_PX}px frame_step_ms={FRAME_STEP_MS}",
        trades.len(),
        trades[0].agg_id,
        trades[trades.len() - 1].agg_id
    );
    let mut pane = Pane::new(mode, path);
    let started = Instant::now();
    let mut next = 0;
    let mut position_ms = SLICE_START_MS;
    while position_ms <= SLICE_END_MS {
        // The replay releases every print at or before its playhead.
        while next < trades.len() && trades[next].timestamp_ms <= position_ms {
            pane.ingest(&trades[next]);
            next += 1;
        }
        let elapsed_ms = position_ms - SLICE_START_MS;
        let instant = INSTANTS_MS.iter().position(|at| *at == elapsed_ms);
        if let Some(index) = instant {
            let _ = writeln!(
                out,
                "\ninstant {} at {}",
                index + 1,
                local_time(position_ms)
            );
        }
        let next_ms = trades.get(next).map(|trade| trade.timestamp_ms);
        pane.frame(position_ms, next_ms, instant.map(|_| &mut out));
        pane.view
            .worker
            .run_batch(started + Duration::from_millis(elapsed_ms as u64));
        position_ms += FRAME_STEP_MS;
    }
    (out, pane.overlaid)
}

/// Both paths draw the approved tape, and the app's path does lay accepted
/// prints beside the published tape, so the overlay is what it checks.
fn assert_approved(mode: Mode) {
    let (overlay, overlaid) = replay(mode, Path::Overlay);
    assert_same_tape(mode, Path::Overlay, &overlay);
    let (complete, _) = replay(mode, Path::Complete);
    assert_same_tape(mode, Path::Complete, &complete);
    assert!(
        overlaid > 0,
        "the app's path drew no frame beside the published tape ({})",
        mode.label()
    );
}

fn assert_same_tape(mode: Mode, path: Path, actual: &str) {
    if actual == GOLDEN {
        return;
    }
    let written = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "approved_win_tape.{}.{}.txt",
        match mode {
            Mode::Beside => "native_tape",
            Mode::TapeOnly => "tape_only",
        },
        path.label()
    ));
    let saved = std::fs::write(&written, actual).map_or_else(
        |error| format!("could not be written ({error})"),
        |()| format!("is in {}", written.display()),
    );
    let golden: Vec<&str> = GOLDEN.lines().collect();
    let current: Vec<&str> = actual.lines().collect();
    let end = "<end of file>";
    let (line, approved, now) = (0..golden.len().max(current.len()))
        .find(|index| golden.get(*index) != current.get(*index))
        .map_or(
            (
                golden.len(),
                "<the same lines>",
                "<a different final newline>",
            ),
            |index| {
                (
                    index + 1,
                    golden.get(index).copied().unwrap_or(end),
                    current.get(index).copied().unwrap_or(end),
                )
            },
        );
    panic!(
        "The approved WIN tape changed ({}, {} path). The trader approved this tape at aafcc21f, \
         and it must not change without the trader's approval.\n\
         First difference, line {line} of the golden:\n  approved: {approved}\n  now:      {now}\n\
         The whole new tape {saved}. Do not edit the golden to match: fix the change, \
         or show the trader both tapes and replace the golden only once the trader approves.",
        mode.label(),
        path.label(),
    );
}

#[test]
fn the_native_tape_beside_the_candles_draws_the_approved_win_tape() {
    assert_approved(Mode::Beside);
}

#[test]
fn tape_only_draws_the_approved_win_tape() {
    assert_approved(Mode::TapeOnly);
}
