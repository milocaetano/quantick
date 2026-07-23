//! The egui application: turns engine [`Bar`]s into candlesticks.
//!
//! All the coordinate math lives in [`crate::chart`] (pure, tested); this layer
//! just maps positions to egui shapes and owns the window state.

use eframe::egui;
use quantick_engine::Bar;

use crate::chart::{PriceScale, TimeAxis, to_f64};

/// Teal-green for up bars, red for down bars — a conventional chart palette.
const UP: egui::Color32 = egui::Color32::from_rgb(38, 166, 154);
const DOWN: egui::Color32 = egui::Color32::from_rgb(239, 83, 80);
const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(19, 23, 34);

/// The quantick chart window.
pub struct QuantickApp {
    bars: Vec<Bar>,
    partial: Option<Bar>,
}

impl QuantickApp {
    /// Create the app from a set of closed bars and an optional forming bar.
    #[must_use]
    pub fn new(bars: Vec<Bar>, partial: Option<Bar>) -> Self {
        Self { bars, partial }
    }

    fn draw_chart(&self, painter: &egui::Painter, area: egui::Rect) {
        painter.rect_filled(area, egui::Rounding::ZERO, BACKGROUND);
        let plot = area.shrink(16.0);

        let count = self.bars.len() + usize::from(self.partial.is_some());
        let Some(scale) = PriceScale::auto(
            &self.bars,
            self.partial.as_ref(),
            plot.top(),
            plot.bottom(),
            0.05,
        ) else {
            painter.text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "waiting for bars…",
                egui::FontId::proportional(16.0),
                egui::Color32::GRAY,
            );
            return;
        };
        let axis = TimeAxis::new(plot.left(), plot.right(), count, 0.7, 24.0);

        for (index, bar) in self.bars.iter().enumerate() {
            draw_candle(painter, &axis, &scale, index, bar, false);
        }
        if let Some(partial) = &self.partial {
            draw_candle(painter, &axis, &scale, self.bars.len(), partial, true);
        }
    }
}

impl eframe::App for QuantickApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND))
            .show(ctx, |ui| {
                let area = ui.available_rect_before_wrap();
                self.draw_chart(ui.painter(), area);
            });
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

    // Wick: high → low.
    painter.line_segment(
        [
            egui::pos2(xc, scale.y(to_f64(bar.high))),
            egui::pos2(xc, scale.y(to_f64(bar.low))),
        ],
        egui::Stroke::new(1.0_f32, color),
    );

    // Body: open → close, with a minimum height so a doji is still visible.
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
