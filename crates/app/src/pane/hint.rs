//! The plate that follows the pointer while a drawing is being placed. The
//! two rules that decide where an anchor lands live in
//! `quantick_drawings::gesture`.

use eframe::egui;

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
