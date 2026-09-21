//! The plate that follows the pointer while a drawing is being placed,
//! and the two rules that decide where an anchor lands.

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::chart::PriceScale;
use crate::drawings;
use crate::theme;

/// Chip metrics for the placement hint: it rides beside the cursor without
/// sitting under it, and it is the same 10 px plate the ruler readout uses.
pub const HINT_CURSOR_OFFSET_PX: egui::Vec2 = egui::vec2(14.0, 14.0);

pub const HINT_TEXT_PX: f32 = 10.0;

pub const HINT_PAD_X_PX: f32 = 5.0;

pub const HINT_PAD_Y_PX: f32 = 3.0;

pub const HINT_RADIUS_PX: f32 = 3.0;

pub const HINT_PLATE: egui::Color32 = egui::Color32::from_rgba_premultiplied(14, 18, 26, 216);

/// Tell the trader what the next click does, beside the cursor.
///
/// A tool that knows says so in words (`placement_hint`); one that does not
/// still reports its progress, because "2/3" beats an object that appears to
/// have stopped responding. Nothing is drawn once the last anchor is placed —
/// there is no next click to describe.
pub fn paint_placement_hint(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    cursor: egui::Pos2,
    tool: drawings::DrawingTool,
    placed: usize,
) {
    let required = tool.required_points();
    if required < 2 || placed == 0 || placed >= required {
        return;
    }
    let text = tool
        .placement_hint(placed)
        .map_or_else(|| format!("{placed}/{required}"), str::to_owned);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(HINT_TEXT_PX),
        theme::TEXT_PRIMARY,
    );
    let size = galley.size() + egui::vec2(2.0 * HINT_PAD_X_PX, 2.0 * HINT_PAD_Y_PX);
    // Flip to the other side of the cursor rather than let the chip leave the
    // chart: a hint half off-screen is worse than no hint.
    let mut min = cursor + HINT_CURSOR_OFFSET_PX;
    if min.x + size.x > chart_rect.right() {
        min.x = cursor.x - HINT_CURSOR_OFFSET_PX.x - size.x;
    }
    if min.y + size.y > chart_rect.bottom() {
        min.y = cursor.y - HINT_CURSOR_OFFSET_PX.y - size.y;
    }
    let plate = egui::Rect::from_min_size(min, size);
    painter.rect_filled(plate, egui::Rounding::same(HINT_RADIUS_PX), HINT_PLATE);
    painter.galley(
        plate.min + egui::vec2(HINT_PAD_X_PX, HINT_PAD_Y_PX),
        galley,
        theme::TEXT_PRIMARY,
    );
}

/// The open / high / low / close of `candle` nearest to the pointer on
/// screen, when one is within reach.
///
/// This is the difference between a line that *looks* drawn off the swing
/// high and one that is (`docs/ux/drawing-tools-2026-08.md` §D6). Nothing in
/// reach returns `None` and the free price is used — a magnet that always
/// snaps is a magnet you cannot draw a diagonal with.
/// Clamp a fractional bar coordinate onto the bars that exist:
/// `0 ..= total - 1`. The candle magnet's time half — a snap that reads a
/// candle must stand on one.
pub fn snap_bar_to_tape(bar: f32, total: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    bar.clamp(0.0, total.saturating_sub(1) as f32)
}

pub fn magnet_price_of(
    candle: &quantick_engine::Bar,
    pointer_y: f32,
    scale: &PriceScale,
    reach_px: f32,
) -> Option<f64> {
    [candle.open, candle.high, candle.low, candle.close]
        .into_iter()
        .filter_map(|price| {
            let price = price.to_f64()?;
            let distance = (scale.y(price) - pointer_y).abs();
            (distance <= reach_px).then_some((distance, price))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, price)| price)
}
