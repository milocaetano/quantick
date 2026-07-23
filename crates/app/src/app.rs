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

use crate::chart::{PriceScale, TimeAxis, to_f64};
use crate::feed::FeedEvent;
use crate::metrics::{self, FrameStats};
use crate::state::{BarKind, BarSpec, ChartState};
use crate::viewport::Viewport;

const UP: egui::Color32 = egui::Color32::from_rgb(38, 166, 154);
const DOWN: egui::Color32 = egui::Color32::from_rgb(239, 83, 80);
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);
const DIVIDER: egui::Color32 = egui::Color32::from_rgb(240, 185, 11);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 160, 175);
const OVERLAY: egui::Color32 = egui::Color32::from_rgb(210, 218, 226);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 99, 71);

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);

/// Convert a UI `f64` parameter to a positive `Decimal` for a builder threshold.
fn dec_from_f64(x: f64) -> Decimal {
    Decimal::from_f64(x.max(1e-8)).unwrap_or(Decimal::ONE)
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
        });
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

    /// Handle mouse navigation over the plot: drag to pan, scroll to zoom,
    /// double-click to snap back to the live edge.
    fn handle_navigation(&mut self, ui: &egui::Ui, area: egui::Rect) {
        let plot = area.shrink(16.0);
        let total = self.state.bars().len() + usize::from(self.state.partial().is_some());
        if total == 0 {
            return;
        }
        let response = ui.interact(
            plot,
            egui::Id::new("chart_nav"),
            egui::Sense::click_and_drag(),
        );

        if response.dragged() {
            let slot = plot.width() / self.viewport.visible_bars().max(1.0);
            if slot > 0.0 {
                self.viewport.pan(response.drag_delta().x / slot, total);
            }
        }
        if response.double_clicked() {
            self.viewport.snap_to_live();
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in; each notch is a gentle step.
                self.viewport.zoom(2.0_f32.powf(-scroll / 300.0));
            }
        }
    }

    fn draw_chart(&self, painter: &egui::Painter, area: egui::Rect) {
        painter.rect_filled(area, egui::Rounding::ZERO, BACKGROUND);
        let plot = area.shrink(16.0);

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

        let (start, end) = self.viewport.visible_range(total);
        let visible_count = end.saturating_sub(start).max(1);

        // The visible closed bars, plus the partial if it falls in view.
        let closed_start = start.min(closed.len());
        let closed_end = end.min(closed.len());
        let visible_closed = &closed[closed_start..closed_end];
        let partial_visible = partial.filter(|_| closed.len() >= start && closed.len() < end);

        let Some(scale) = PriceScale::auto(
            visible_closed,
            partial_visible,
            plot.top(),
            plot.bottom(),
            0.05,
        ) else {
            return;
        };
        let axis = TimeAxis::new(plot.left(), plot.right(), visible_count, 0.7, 24.0);

        for (offset, bar) in visible_closed.iter().enumerate() {
            let local = (closed_start - start) + offset;
            draw_candle(painter, &axis, &scale, local, bar, false);
        }
        if let Some(partial) = partial_visible {
            draw_candle(painter, &axis, &scale, closed.len() - start, partial, true);
        }

        self.draw_backfill_divider(painter, &axis, plot, start, end);
        self.draw_header(painter, plot);
    }

    /// A vertical marker separating backfilled history (left) from live (right),
    /// drawn only when the boundary falls in the visible `[start, end)` window.
    fn draw_backfill_divider(
        &self,
        painter: &egui::Painter,
        axis: &TimeAxis,
        plot: egui::Rect,
        start: usize,
        end: usize,
    ) {
        let Some(boundary) = self.state.backfill_boundary() else {
            return;
        };
        if boundary == 0 || boundary < start || boundary > end {
            return; // nothing backfilled, or the divider is off-screen
        }
        let x = axis
            .x_left(boundary - start)
            .clamp(plot.left(), plot.right());
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, DIVIDER),
        );
        let font = egui::FontId::proportional(11.0);
        painter.text(
            egui::pos2(x - 4.0, plot.bottom() - 4.0),
            egui::Align2::RIGHT_BOTTOM,
            "backfill",
            font.clone(),
            MUTED,
        );
        painter.text(
            egui::pos2(x + 4.0, plot.bottom() - 4.0),
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
        let header = format!(
            "{} · {} · {} backfilled + {} live bars · {}",
            self.symbol,
            self.state.spec().summary(),
            backfilled,
            live,
            mode
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

        egui::TopBottomPanel::top("controls")
            .frame(egui::Frame::none().fill(BACKGROUND).inner_margin(8.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(OVERLAY);
                self.draw_controls(ui);
            });
        // Apply any selector change (no-op if unchanged).
        self.state.set_spec(self.current_spec());

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND))
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
    axis: &TimeAxis,
    scale: &PriceScale,
    index: usize,
    bar: &Bar,
    forming: bool,
) {
    let xc = axis.x_center(index);
    let half = axis.bar_width() / 2.0;
    let color = if bar.close >= bar.open { UP } else { DOWN };

    painter.line_segment(
        [
            egui::pos2(xc, scale.y(to_f64(bar.high))),
            egui::pos2(xc, scale.y(to_f64(bar.low))),
        ],
        egui::Stroke::new(1.0_f32, color),
    );

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
