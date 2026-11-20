//! The pinned close series and each signal at its original slot.
use eframe::egui;
use quantick_ui_model::strategy_describe::{HistoricalClose, Marker};
use rust_decimal::prelude::ToPrimitive;

pub(super) fn plot(ui: &mut egui::Ui, history: &[HistoricalClose], markers: &[Marker]) {
    if history.is_empty() {
        return;
    }
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
    let inner = rect.shrink(7.0);
    let painter = ui.painter_at(rect);
    let theme = crate::theme::get(ui.ctx());
    painter.rect_filled(rect, 3.0, ui.visuals().faint_bg_color);
    let prices: Vec<f64> = history
        .iter()
        .map(|s| s.close.bar.close.to_f64().expect("decimal fits f64"))
        .collect();
    let (low, high) = prices
        .iter()
        .fold((prices[0], prices[0]), |(low, high), &p| {
            (low.min(p), high.max(p))
        });
    let start = history[0].close.slot;
    let span = history
        .last()
        .expect("nonempty series")
        .close
        .slot
        .saturating_sub(start)
        .max(1);
    let points: Vec<_> = history
        .iter()
        .zip(prices)
        .map(|(sample, price)| {
            let x = sample.close.slot.saturating_sub(start) as f32 / span as f32;
            let y = if high > low {
                ((price - low) / (high - low)) as f32
            } else {
                0.5
            };
            egui::pos2(
                inner.left() + x * inner.width(),
                inner.bottom() - y * inner.height(),
            )
        })
        .collect();
    for pair in points.windows(2) {
        painter.line_segment(
            [pair[0], pair[1]],
            egui::Stroke::new(1.0_f32, ui.visuals().text_color()),
        );
    }
    for marker in markers {
        let Ok(at) = history.binary_search_by_key(&marker.slot, |s| s.close.slot) else {
            continue;
        };
        let tip = points[at];
        let (dy, color) = match marker.side {
            quantick_engine::Side::Buy => (6.0, theme.buy()),
            quantick_engine::Side::Sell => (-6.0, theme.sell()),
        };
        let triangle = vec![tip, tip + egui::vec2(-4.0, dy), tip + egui::vec2(4.0, dy)];
        painter.add(egui::Shape::convex_polygon(
            triangle,
            color,
            egui::Stroke::NONE,
        ));
    }
}
