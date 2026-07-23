//! The egui application: drains the live feed, renders bars, surfaces metrics.
//!
//! Coordinate math lives in [`crate::chart`] (pure, tested), trade → bar logic
//! in [`crate::state`] (pure, tested), and metric math in [`crate::metrics`]
//! (pure, tested). This layer owns the clocks and the tracing, drains the feed
//! channel each frame, and turns everything into egui shapes — candles, the
//! backfill/live divider, and a performance overlay.

use std::time::{Duration, Instant};

use eframe::egui;
use tokio::sync::mpsc;

use quantick_engine::Bar;

use crate::chart::{PriceScale, TimeAxis, to_f64};
use crate::feed::FeedEvent;
use crate::metrics::{self, FrameStats};
use crate::state::ChartState;

const UP: egui::Color32 = egui::Color32::from_rgb(38, 166, 154);
const DOWN: egui::Color32 = egui::Color32::from_rgb(239, 83, 80);
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);
const DIVIDER: egui::Color32 = egui::Color32::from_rgb(240, 185, 11);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 160, 175);
const OVERLAY: egui::Color32 = egui::Color32::from_rgb(210, 218, 226);
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 99, 71);

/// How often the perf summary is logged (not every frame).
const SUMMARY_INTERVAL: Duration = Duration::from_secs(2);

/// The quantick chart window.
pub struct QuantickApp {
    state: ChartState,
    events: mpsc::Receiver<FeedEvent>,
    symbol: String,
    tick_size: u64,

    frames: FrameStats,
    last_frame: Option<Instant>,
    latest_trade_ms: Option<i64>,
    live_trades: u64,
    trades_since_summary: u64,
    last_summary: Instant,
}

impl QuantickApp {
    /// Create the app for `symbol`, building tick bars of `tick_size`, draining
    /// `events` from the feed thread.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        tick_size: u64,
        events: mpsc::Receiver<FeedEvent>,
    ) -> Self {
        Self {
            state: ChartState::new(tick_size),
            events,
            symbol: symbol.into(),
            tick_size,
            frames: FrameStats::new(120),
            last_frame: None,
            latest_trade_ms: None,
            live_trades: 0,
            trades_since_summary: 0,
            last_summary: Instant::now(),
        }
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
                // Empty (nothing pending) or Disconnected (feed gone): stop.
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

    fn draw_chart(&self, painter: &egui::Painter, area: egui::Rect) {
        painter.rect_filled(area, egui::Rounding::ZERO, BACKGROUND);
        let plot = area.shrink(16.0);

        let bars = self.state.bars();
        let partial = self.state.partial();
        let count = bars.len() + usize::from(partial.is_some());

        let Some(scale) = PriceScale::auto(bars, partial, plot.top(), plot.bottom(), 0.05) else {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                format!("connecting to {} …", self.symbol),
                egui::FontId::proportional(16.0),
                MUTED,
            );
            return;
        };
        let axis = TimeAxis::new(plot.left(), plot.right(), count, 0.7, 24.0);

        for (index, bar) in bars.iter().enumerate() {
            draw_candle(painter, &axis, &scale, index, bar, false);
        }
        if let Some(partial) = partial {
            draw_candle(painter, &axis, &scale, bars.len(), partial, true);
        }

        self.draw_backfill_divider(painter, &axis, plot);
        self.draw_header(painter, plot);
    }

    /// A vertical marker separating backfilled history (left) from live (right).
    fn draw_backfill_divider(&self, painter: &egui::Painter, axis: &TimeAxis, plot: egui::Rect) {
        let Some(boundary) = self.state.backfill_boundary() else {
            return;
        };
        if boundary == 0 {
            return; // nothing was backfilled
        }
        let x = axis.x_left(boundary).clamp(plot.left(), plot.right());
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
        let header = format!(
            "{} · tick({}) · {} backfilled + {} live bars",
            self.symbol, self.tick_size, backfilled, live
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

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
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
