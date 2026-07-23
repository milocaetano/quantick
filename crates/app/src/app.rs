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

use quantick_engine::Bar;

use crate::chart::{PriceScale, to_f64};
use crate::feed::FeedEvent;
use crate::metrics::{self, FrameStats};
use crate::price_view::PriceView;
use crate::state::{BarKind, BarSpec, ChartState};
use crate::style::CandleStyle;
use crate::viewport::Viewport;

/// Convert an sRGB `[u8; 3]` style colour to an egui `Color32`.
fn color32([r, g, b]: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

const DIVIDER: egui::Color32 = egui::Color32::from_rgb(240, 185, 11);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 160, 175);
const OVERLAY: egui::Color32 = egui::Color32::from_rgb(210, 218, 226);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 99, 71);
const GRID: egui::Color32 = egui::Color32::from_rgb(35, 41, 54);
const CROSSHAIR: egui::Color32 = egui::Color32::from_rgb(110, 120, 135);
const TAG_BG: egui::Color32 = egui::Color32::from_rgb(55, 63, 80);

/// Width of the right-hand price-axis gutter, in pixels.
const AXIS_GUTTER: f32 = 60.0;
/// Height of the bottom time-axis strip, in pixels.
const TIME_STRIP: f32 = 22.0;

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);

/// Convert a UI `f64` parameter to a positive `Decimal` for a builder threshold.
fn dec_from_f64(x: f64) -> Decimal {
    Decimal::from_f64(x.max(1e-8)).unwrap_or(Decimal::ONE)
}

/// Split the padded plot area into the candle chart, the right price gutter and
/// the bottom time strip, so the input handler and the renderer agree on the
/// boundaries.
fn plot_split(area: egui::Rect) -> PlotAreas {
    let plot = area.shrink(16.0);
    let split_x = (plot.right() - AXIS_GUTTER).max(plot.left() + 20.0);
    let split_y = (plot.bottom() - TIME_STRIP).max(plot.top() + 20.0);
    PlotAreas {
        chart: egui::Rect::from_min_max(plot.min, egui::pos2(split_x, split_y)),
        price_gutter: egui::Rect::from_min_max(
            egui::pos2(split_x, plot.top()),
            egui::pos2(plot.right(), split_y),
        ),
        time_strip: egui::Rect::from_min_max(
            egui::pos2(plot.left(), split_y),
            egui::pos2(split_x, plot.bottom()),
        ),
    }
}

/// The three interactive regions of the plot.
struct PlotAreas {
    chart: egui::Rect,
    price_gutter: egui::Rect,
    time_strip: egui::Rect,
}

/// Format an epoch-millisecond timestamp as `HH:MM:SS` (UTC) for the time axis.
fn fmt_time(ms: i64) -> String {
    let secs = ms.div_euclid(1000).rem_euclid(86_400);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}")
}

/// The quantick chart window.
pub struct QuantickApp {
    state: ChartState,
    events: mpsc::Receiver<FeedEvent>,
    symbol: String,

    // Bar-type selector state (one parameter retained per kind).
    kind: BarKind,
    tick_n: u64,
    volume_units: f64,
    dollar_notional: f64,
    time_interval_ms: i64,

    // Pan/zoom navigation over the bar series.
    viewport: Viewport,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    price_view: PriceView,
    // Last frame's auto-fit price range and chart height, for pixel↔price maths
    // in the input handler (which runs before the draw computes them).
    last_auto_range: Option<(f64, f64)>,
    last_chart_height: f32,
    // Pointer position over the plot this frame, for the crosshair.
    hover_pos: Option<egui::Pos2>,

    // Candle appearance + whether the style panel is open.
    style: CandleStyle,
    show_style: bool,

    frames: FrameStats,
    last_frame: Option<Instant>,
    latest_trade_ms: Option<i64>,
    live_trades: u64,
    trades_since_summary: u64,
    last_summary: Instant,
}

impl QuantickApp {
    /// Create the app for `symbol` starting from `spec`, draining `events`.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        spec: BarSpec,
        events: mpsc::Receiver<FeedEvent>,
    ) -> Self {
        // Defaults for every kind, with the initial spec's parameter applied.
        let mut tick_n = 50;
        let mut volume_units = 5.0;
        let mut dollar_notional = 500_000.0;
        let mut time_interval_ms = 1_000;
        match &spec {
            BarSpec::Tick(n) => tick_n = *n,
            BarSpec::Volume(u) => volume_units = u.to_f64().unwrap_or(volume_units),
            BarSpec::Dollar(d) => dollar_notional = d.to_f64().unwrap_or(dollar_notional),
            BarSpec::Time(ms) => time_interval_ms = *ms,
        }

        Self {
            kind: spec.kind(),
            state: ChartState::new(spec),
            events,
            symbol: symbol.into(),
            tick_n,
            volume_units,
            dollar_notional,
            time_interval_ms,
            viewport: Viewport::new(),
            price_view: PriceView::new(),
            last_auto_range: None,
            last_chart_height: 1.0,
            hover_pos: None,
            style: CandleStyle::default(),
            show_style: false,
            frames: FrameStats::new(120),
            last_frame: None,
            latest_trade_ms: None,
            live_trades: 0,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        }
    }

    /// The bar spec implied by the current selector state.
    fn current_spec(&self) -> BarSpec {
        match self.kind {
            BarKind::Tick => BarSpec::Tick(self.tick_n.max(1)),
            BarKind::Volume => BarSpec::Volume(dec_from_f64(self.volume_units)),
            BarKind::Dollar => BarSpec::Dollar(dec_from_f64(self.dollar_notional)),
            BarKind::Time => BarSpec::Time(self.time_interval_ms.max(1)),
        }
    }

    /// The bar-type selector: a combo for the kind and a drag for its parameter.
    fn draw_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("bar type:");
            egui::ComboBox::from_id_salt("bar_kind")
                .selected_text(self.kind.label())
                .show_ui(ui, |ui| {
                    for kind in BarKind::ALL {
                        ui.selectable_value(&mut self.kind, kind, kind.label());
                    }
                });
            ui.separator();
            match self.kind {
                BarKind::Tick => {
                    ui.label("N trades");
                    ui.add(egui::DragValue::new(&mut self.tick_n).range(1.0..=5000.0));
                }
                BarKind::Volume => {
                    ui.label("units");
                    ui.add(
                        egui::DragValue::new(&mut self.volume_units)
                            .range(0.1..=1000.0)
                            .speed(0.1),
                    );
                }
                BarKind::Dollar => {
                    ui.label("notional");
                    ui.add(
                        egui::DragValue::new(&mut self.dollar_notional)
                            .range(1000.0..=1_000_000_000.0)
                            .speed(1000.0),
                    );
                }
                BarKind::Time => {
                    ui.label("interval ms");
                    ui.add(
                        egui::DragValue::new(&mut self.time_interval_ms)
                            .range(100.0..=600_000.0)
                            .speed(100.0),
                    );
                }
            }
            ui.separator();
            if ui.button("⚙ style").clicked() {
                self.show_style = !self.show_style;
            }
        });
    }

    /// The current background colour as an egui `Color32`.
    fn bg(&self) -> egui::Color32 {
        color32(self.style.background)
    }

    /// The floating candle-style settings panel (shown when `show_style`).
    fn draw_style_panel(&mut self, ctx: &egui::Context) {
        let mut open = self.show_style;
        egui::Window::new("candle style")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                let s = &mut self.style;
                ui.horizontal(|ui| {
                    ui.label("up (bull)");
                    ui.color_edit_button_srgb(&mut s.bull);
                });
                ui.horizontal(|ui| {
                    ui.label("down (bear)");
                    ui.color_edit_button_srgb(&mut s.bear);
                });
                ui.checkbox(&mut s.show_wicks, "show wicks");
                ui.checkbox(&mut s.wick_matches_body, "wick matches body");
                if !s.wick_matches_body {
                    ui.horizontal(|ui| {
                        ui.label("wick");
                        ui.color_edit_button_srgb(&mut s.wick);
                    });
                }
                ui.add(egui::Slider::new(&mut s.body_width_frac, 0.1..=1.0).text("body width"));
                ui.horizontal(|ui| {
                    ui.label("background");
                    ui.color_edit_button_srgb(&mut s.background);
                });
                ui.separator();
                if ui.button("reset to defaults").clicked() {
                    *s = CandleStyle::default();
                }
            });
        self.show_style = open;
    }

    /// Drain every feed event available this frame into the engine, tracking the
    /// latest trade timestamp and live-trade counts for the metrics.
    fn drain_feed(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => {
                    if let Some(last) = trades.last() {
                        self.latest_trade_ms = Some(last.timestamp_ms);
                    }
                    self.state.ingest_backfill(&trades);
                }
                Ok(FeedEvent::Live(trade)) => {
                    self.latest_trade_ms = Some(trade.timestamp_ms);
                    self.live_trades += 1;
                    self.trades_since_summary += 1;
                    self.state.ingest_live(&trade);
                }
                Err(_) => break,
            }
        }
    }

    /// Periodically log a perf summary and warn on threshold breaches.
    fn maybe_emit_summary(&mut self, now: Instant) {
        let elapsed = now - self.last_summary;
        if elapsed < SUMMARY_INTERVAL {
            return;
        }
        let rate = self.trades_since_summary as f64 / elapsed.as_secs_f64();
        let lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), self.latest_trade_ms);
        let avg = self.frames.avg_ms().unwrap_or(0.0);
        let worst = self.frames.worst_ms().unwrap_or(0.0);
        let fps = self.frames.fps().unwrap_or(0.0);

        tracing::info!(
            target: "quantick::app",
            fps = fps as i64,
            frame_avg_ms = avg,
            frame_worst_ms = worst,
            feed_lag_ms = lag,
            trades_per_s = rate,
            live_trades = self.live_trades,
            bar_spec = self.state.spec().summary(),
            "perf summary"
        );
        if avg > metrics::SLOW_FRAME_MS {
            tracing::warn!(
                target: "quantick::app",
                frame_avg_ms = avg,
                threshold_ms = metrics::SLOW_FRAME_MS,
                "slow frames: the chart is not keeping up"
            );
        }
        if let Some(l) = lag
            && l > metrics::HIGH_LAG_MS
        {
            tracing::warn!(
                target: "quantick::app",
                feed_lag_ms = l,
                threshold_ms = metrics::HIGH_LAG_MS,
                "high feed lag: trades are arriving well behind their timestamps"
            );
        }

        self.trades_since_summary = 0;
        self.last_summary = now;
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the chart → pan time (x, moves the whole chart) and price (y);
    /// - scroll over the chart → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click → reset to the live edge and auto-fit price.
    fn handle_navigation(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let areas = plot_split(area);
        let auto = self.last_auto_range;
        let height = self.last_chart_height;
        let total = self.state.bars().len() + usize::from(self.state.partial().is_some());

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            egui::Id::new("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = chart.hover_pos();
        if total > 0 && chart.dragged() {
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
                // Scroll up (positive) zooms in — wider candles.
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
            }
        }

        // Bottom time strip: drag or scroll to zoom the candle spacing.
        let time = ui.interact(
            areas.time_strip,
            egui::Id::new("time_nav"),
            egui::Sense::click_and_drag(),
        );
        if time.dragged() {
            // Drag right → wider candles (zoom in); left → narrower (zoom out).
            self.viewport.zoom((time.drag_delta().x / 120.0).exp());
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / 300.0));
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
        if total == 0 {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", self.symbol),
                egui::FontId::proportional(16.0),
                MUTED,
            );
            return;
        }

        let areas = plot_split(area);
        let chart_rect = areas.chart;

        let (start, end) = self.viewport.visible_range(chart_rect.width(), total);

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
        let half = (cw * self.style.clamped_width_frac() / 2.0).max(0.5);
        let right = chart_rect.right();

        // Grid + price labels first, behind the candles.
        self.draw_price_axis(painter, chart_rect, &scale);

        // Candles, clipped to the chart body so they don't spill into the axes.
        let clip = painter.with_clip_rect(chart_rect);
        for (offset, bar) in visible_closed.iter().enumerate() {
            let index = closed_start + offset;
            let xc = self.viewport.x_center(index, right, total);
            draw_candle(&clip, xc, half, &scale, bar, false, &self.style);
        }
        if let Some(partial) = partial_visible {
            let xc = self.viewport.x_center(closed.len(), right, total);
            draw_candle(&clip, xc, half, &scale, partial, true, &self.style);
        }

        self.draw_backfill_divider(painter, chart_rect, total, cw);
        self.draw_time_strip(painter, areas.time_strip, closed, start, end, total);
        self.draw_crosshair(painter, chart_rect, &scale);
        self.draw_header(painter, chart_rect);

        // Cache the auto range + height for next frame's input handler, which
        // runs before the draw and needs them for pixel↔price conversion.
        self.last_auto_range = Some(auto_range);
        self.last_chart_height = chart_rect.height();
    }

    /// Bottom time strip: a top border and a few `HH:MM:SS` labels for the
    /// visible bars. Draggable left/right to zoom the candle spacing.
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
            egui::Stroke::new(1.0_f32, GRID),
        );
        let font = egui::FontId::monospace(10.0);
        let y = strip.center().y;
        // Up to ~6 evenly-spaced labels across the visible closed bars.
        let visible = end.saturating_sub(start);
        if visible == 0 {
            return;
        }
        let step = (visible / 6).max(1);
        let mut index = start;
        while index < end {
            if let Some(bar) = closed.get(index) {
                let x = self.viewport.x_center(index, strip.right(), total);
                if strip.x_range().contains(x) {
                    painter.text(
                        egui::pos2(x, y),
                        egui::Align2::CENTER_CENTER,
                        fmt_time(bar.open_time),
                        font.clone(),
                        MUTED,
                    );
                }
            }
            index += step;
        }
    }

    /// Right-hand price axis: round-number gridlines and labels.
    fn draw_price_axis(&self, painter: &egui::Painter, chart_rect: egui::Rect, scale: &PriceScale) {
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
                egui::Stroke::new(1.0_f32, GRID),
            );
            painter.text(
                egui::pos2(chart_rect.right() + 6.0, y),
                egui::Align2::LEFT_CENTER,
                format!("{tick:.2}"),
                font.clone(),
                MUTED,
            );
        }
        // The axis dividing line.
        painter.line_segment(
            [
                egui::pos2(chart_rect.right(), chart_rect.top()),
                egui::pos2(chart_rect.right(), chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, GRID),
        );
    }

    /// Crosshair following the pointer, with the price shown on the axis.
    fn draw_crosshair(&self, painter: &egui::Painter, chart_rect: egui::Rect, scale: &PriceScale) {
        let Some(pos) = self.hover_pos else {
            return;
        };
        if !chart_rect.contains(pos) {
            return;
        }
        let stroke = egui::Stroke::new(1.0_f32, CROSSHAIR);
        painter.line_segment(
            [
                egui::pos2(pos.x, chart_rect.top()),
                egui::pos2(pos.x, chart_rect.bottom()),
            ],
            stroke,
        );
        painter.line_segment(
            [
                egui::pos2(chart_rect.left(), pos.y),
                egui::pos2(chart_rect.right(), pos.y),
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
        let text_pos = egui::pos2(chart_rect.right() + 6.0, pos.y - galley.size().y / 2.0);
        let bg = egui::Rect::from_min_size(
            text_pos - egui::vec2(3.0, 1.0),
            galley.size() + egui::vec2(6.0, 2.0),
        );
        painter.rect_filled(bg, egui::Rounding::same(2.0), TAG_BG);
        painter.galley(text_pos, galley, egui::Color32::WHITE);
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls inside the chart body.
    fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
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
        let x = self.viewport.x_center(boundary, chart_rect.right(), total) - candle_width / 2.0;
        if x < chart_rect.left() || x > chart_rect.right() {
            return; // off-screen
        }
        painter.line_segment(
            [
                egui::pos2(x, chart_rect.top()),
                egui::pos2(x, chart_rect.bottom()),
            ],
            egui::Stroke::new(1.0_f32, DIVIDER),
        );
        let font = egui::FontId::proportional(11.0);
        painter.text(
            egui::pos2(x - 4.0, chart_rect.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "backfill",
            font.clone(),
            MUTED,
        );
        painter.text(
            egui::pos2(x + 4.0, chart_rect.bottom() - 4.0),
            egui::Align2::LEFT_BOTTOM,
            "live",
            font,
            DIVIDER,
        );
    }

    fn draw_header(&self, painter: &egui::Painter, plot: egui::Rect) {
        let bars = self.state.bars();
        let (backfilled, live) = match self.state.backfill_boundary() {
            Some(b) => (b, bars.len().saturating_sub(b)),
            None => (0, bars.len()),
        };
        let mode = if self.viewport.follows_live() {
            "● live"
        } else {
            "history · double-click for live"
        };
        let price_mode = if self.price_view.is_auto() {
            ""
        } else {
            " · price: manual (double-click to auto-fit)"
        };
        let header = format!(
            "{} · {} · {} backfilled + {} live bars · {}{}",
            self.symbol,
            self.state.spec().summary(),
            backfilled,
            live,
            mode,
            price_mode
        );
        painter.text(
            egui::pos2(plot.left(), plot.top()),
            egui::Align2::LEFT_TOP,
            header,
            egui::FontId::proportional(13.0),
            MUTED,
        );
    }

    /// A top-right overlay with FPS, frame time and feed lag; values that breach
    /// a threshold are drawn in the warning colour.
    fn draw_overlay(&self, painter: &egui::Painter, area: egui::Rect) {
        let avg = self.frames.avg_ms();
        let lag = metrics::feed_lag_ms(metrics::wall_clock_ms(), self.latest_trade_ms);

        let fps_color = if avg.is_some_and(|a| a > metrics::SLOW_FRAME_MS) {
            WARN
        } else {
            OVERLAY
        };
        let lag_color = if lag.is_some_and(|l| l > metrics::HIGH_LAG_MS) {
            WARN
        } else {
            OVERLAY
        };
        let lag_text = match lag {
            Some(l) => format!("feed lag {l} ms"),
            None => "feed lag —".to_string(),
        };

        let lines: [(String, egui::Color32); 4] = [
            (
                format!(
                    "{:>4.0} fps  {:>5.1} ms",
                    self.frames.fps().unwrap_or(0.0),
                    avg.unwrap_or(0.0)
                ),
                fps_color,
            ),
            (
                format!("worst {:>5.1} ms", self.frames.worst_ms().unwrap_or(0.0)),
                OVERLAY,
            ),
            (lag_text, lag_color),
            (format!("{} live trades", self.live_trades), OVERLAY),
        ];

        let font = egui::FontId::monospace(12.0);
        let pad = 8.0;
        let line_h = 16.0;
        let box_w = 180.0;
        let box_h = lines.len() as f32 * line_h + pad;
        let top_right = egui::pos2(area.right() - 8.0 - box_w, area.top() + 8.0);
        let backdrop = egui::Rect::from_min_size(top_right, egui::vec2(box_w, box_h));
        painter.rect_filled(
            backdrop,
            egui::Rounding::same(4.0),
            egui::Color32::from_black_alpha(150),
        );

        let mut y = backdrop.top() + pad / 2.0;
        let right = backdrop.right() - pad;
        for (text, color) in &lines {
            painter.text(
                egui::pos2(right, y),
                egui::Align2::RIGHT_TOP,
                text,
                font.clone(),
                *color,
            );
            y += line_h;
        }
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            self.frames.record((now - last).as_secs_f32() * 1000.0);
        }
        self.last_frame = Some(now);

        self.drain_feed();
        self.maybe_emit_summary(now);

        let bg = self.bg();
        egui::TopBottomPanel::top("controls")
            .frame(egui::Frame::none().fill(bg).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(OVERLAY);
                self.draw_controls(ui);
            });
        // Apply any selector change (no-op if unchanged).
        self.state.set_spec(self.current_spec());
        self.draw_style_panel(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(bg))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.handle_navigation(ui, area);
                self.draw_chart(ui.painter(), area);
                self.draw_overlay(ui.painter(), area);
            });
        // Live feed: keep polling the channel ~60×/s without busy-spinning.
        ctx.request_repaint_after(Duration::from_millis(16));
    }
}

/// Draw one candlestick. A `forming` candle is outlined and translucent so it
/// reads as "still open" versus a solid closed candle.
fn draw_candle(
    painter: &egui::Painter,
    xc: f32,
    half: f32,
    scale: &PriceScale,
    bar: &Bar,
    forming: bool,
    style: &CandleStyle,
) {
    let up = bar.close >= bar.open;
    let color = color32(style.body_color(up));

    if style.show_wicks {
        painter.line_segment(
            [
                egui::pos2(xc, scale.y(to_f64(bar.high))),
                egui::pos2(xc, scale.y(to_f64(bar.low))),
            ],
            egui::Stroke::new(1.0_f32, color32(style.wick_color(up))),
        );
    }

    let y_open = scale.y(to_f64(bar.open));
    let y_close = scale.y(to_f64(bar.close));
    let top = y_open.min(y_close);
    let bottom = y_open.max(y_close).max(top + 1.0);
    let body = egui::Rect::from_min_max(egui::pos2(xc - half, top), egui::pos2(xc + half, bottom));

    if forming {
        painter.rect_filled(body, egui::Rounding::ZERO, color.gamma_multiply(0.35));
        painter.rect_stroke(
            body,
            egui::Rounding::ZERO,
            egui::Stroke::new(1.5_f32, color),
        );
    } else {
        painter.rect_filled(body, egui::Rounding::ZERO, color);
    }
}
