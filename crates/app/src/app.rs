//! The egui application: drains the live feed and renders bars as they form.
//!
//! All coordinate math lives in [`crate::chart`] (pure, tested) and all trade →
//! bar logic in [`crate::state`] (pure, tested). This layer drains the feed
//! channel each frame, hands trades to the engine via [`ChartState`], and turns
//! the resulting bars into egui shapes — including an honest divider between
//! backfilled history and live ticks.

use std::time::Duration;

use eframe::egui;
use tokio::sync::mpsc;

use quantick_engine::Bar;

use crate::chart::{PriceScale, TimeAxis, to_f64};
use crate::feed::FeedEvent;
use crate::state::ChartState;

const UP: egui::Color32 = egui::Color32::from_rgb(38, 166, 154);
const DOWN: egui::Color32 = egui::Color32::from_rgb(239, 83, 80);
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);
const DIVIDER: egui::Color32 = egui::Color32::from_rgb(240, 185, 11);
const MUTED: egui::Color32 = egui::Color32::from_rgb(150, 160, 175);

/// The quantick chart window.
pub struct QuantickApp {
    state: ChartState,
    events: mpsc::Receiver<FeedEvent>,
    symbol: String,
    tick_size: u64,
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
        }
    }

    /// Drain every feed event available this frame into the engine.
    fn drain_feed(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(FeedEvent::Backfilled(trades)) => self.state.ingest_backfill(&trades),
                Ok(FeedEvent::Live(trade)) => self.state.ingest_live(&trade),
                // Empty (nothing pending) or Disconnected (feed gone): stop draining.
                Err(_) => break,
            }
        }
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
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_feed();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.draw_chart(ui.painter(), area);
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
