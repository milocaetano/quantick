//! The pane's own decoration: the last-price line, the history seam, the
//! live chip and the lane's axis furniture.

use eframe::egui;

use crate::indicator_worker::MAX_LANE_RUNGS;
use crate::style::ChartStyle;
use crate::theme;

/// Alpha of the last-price line: legible at a glance without competing with a
/// candle or a bubble for attention.
pub const LAST_PRICE_LINE_ALPHA: f32 = 0.55;

/// Dash length, in pixels, of the last-price line. Dashed so it never reads as
/// a level someone drew.
pub const LAST_PRICE_DASH_PX: f32 = 4.0;

/// See [`LAST_PRICE_DASH_PX`].
pub const LAST_PRICE_GAP_PX: f32 = 4.0;

/// Ink on the last-price chip. The chip is filled with a saturated candle
/// colour, so its text is the one place on the chrome that goes dark.
pub const LAST_PRICE_CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x12, 0x1A);

/// Dash length, in pixels, of the venue↔prints seam marker. Long enough to
/// read as deliberate beside the solid backfill divider, short enough not to
/// be mistaken for one.
pub const SEAM_DASH_PX: f32 = 5.0;

/// See [`SEAM_DASH_PX`].
pub const SEAM_GAP_PX: f32 = 4.0;

/// Font size of the caption beside a dashed vertical mark, in points.
///
/// Shared by the venue seam and the tape-gap mark rather than written at each:
/// they are the same kind of caption answering the same question about the bars
/// either side of a line, and two copies would drift the first time one moved.
pub const SEAM_LABEL_PT: f32 = 11.0;

/// Gap between a dashed vertical mark and its caption, in pixels — and the
/// caption's drop from the top of the pane.
pub const SEAM_LABEL_INSET_PX: f32 = 4.0;

/// A dashed vertical rule down `rect` at `x`.
///
/// The same construction the heatmap's own boundary marks use: egui has no
/// dashed line primitive for a single segment, so the dashes are drawn.
pub fn draw_dashed_vertical(
    painter: &egui::Painter,
    x: f32,
    rect: egui::Rect,
    dash: f32,
    gap: f32,
    color: egui::Color32,
) {
    let dash = dash.max(0.5);
    let gap = gap.max(0.0);
    let stroke = egui::Stroke::new(1.0_f32, color);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + dash).min(rect.bottom())),
            ],
            stroke,
        );
        y += dash + gap;
    }
}

/// Half-width, in pixels, of the grab area over the live lane's divider.
///
/// The line itself stays a hairline — it marks where the present begins and a
/// thick rule there would read as a wall in the data. The handle around it is
/// what makes it draggable, and the resize cursor is the only thing that says
/// so.
pub const LANE_HANDLE_HALF_WIDTH_PX: f32 = 5.0;

/// Type size of the axis under the tape.
pub const LANE_AXIS_FONT_PX: f32 = 10.0;

/// Breathing room between the tape's two axis labels, and between the warning
/// and the strip's own right edge.
pub const LANE_AXIS_GAP_PX: f32 = 8.0;

/// Pixels of lane the ladder spends one rung on.
///
/// A rung is a full evaluation of every hosted indicator, so this is the knob
/// that trades cost against smoothness. Six pixels draws a curve that reads as
/// continuous at the sizes the lane is ever given, and keeps a wide lane well
/// under [`MAX_LANE_RUNGS`].
pub const LANE_RUNG_PX: f32 = 6.0;

/// How many rungs a lane this wide is worth sampling at.
///
/// Zero for a chart with no lane — the signal that no ladder is walked at all.
pub fn lane_rungs(lane_width_px: f32) -> usize {
    if !lane_width_px.is_finite() || lane_width_px <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rungs = (lane_width_px / LANE_RUNG_PX) as usize;
    rungs.clamp(1, MAX_LANE_RUNGS)
}

/// Wheel travel that doubles or halves what a time axis shows.
///
/// One number for the candles, the lane and every pane body, so a scroll means
/// the same amount of zoom wherever the pointer happens to be resting. It was
/// already one number — written out four times.
pub const SCROLL_ZOOM_PX: f32 = 300.0;

/// Width of the jump-to-live chip on the time strip, in pixels.
pub const LIVE_CHIP_WIDTH_PX: f32 = 56.0;

/// Gap between the chip and the strip's right edge, in pixels.
pub const LIVE_CHIP_MARGIN_PX: f32 = 6.0;

/// Vertical inset of the chip inside the strip, in pixels.
pub const LIVE_CHIP_VPAD_PX: f32 = 3.0;

/// Where the jump-to-live chip sits (audit F6): right-aligned inside the
/// history segment of the time strip — the live end of the axis, which is
/// where the eye looks for the way back. One geometry for the input region
/// and the paint, so the click can never miss the pixels.
pub fn live_chip_rect(history_strip: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX - LIVE_CHIP_WIDTH_PX,
            history_strip.top() + LIVE_CHIP_VPAD_PX,
        ),
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX,
            history_strip.bottom() - LIVE_CHIP_VPAD_PX,
        ),
    )
}

/// Paint the jump-to-live chip: solid accent, dark ink — the chip language
/// of the price gutter, because this too is a statement about the axis.
/// Accent, not amber: it is a control, not a provenance statement.
pub fn draw_live_chip(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, egui::Rounding::same(3.0), theme::ACCENT);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "» live",
        egui::FontId::proportional(11.0),
        theme::CHIP_INK,
    );
}

/// Whether a freshly folded prefix differs from the one already installed.
///
/// Length and the two end open-times, not a full comparison: the fold is
/// deterministic over the same base, so two runs agreeing on how many bars
/// they produced and which windows the first and last cover agree on
/// everything between. The full compare was ~129k `Decimal`s on every frame
/// of a settled interval drag.
pub fn prefix_differs(current: &[quantick_engine::Bar], next: &[quantick_engine::Bar]) -> bool {
    if current.len() != next.len() {
        return true;
    }
    let ends = |bars: &[quantick_engine::Bar]| {
        (
            bars.first().map(|bar| bar.open_time),
            bars.last().map(|bar| bar.open_time),
        )
    };
    ends(current) != ends(next)
}

/// Convert an explicit unmultiplied RGBA style colour to egui.
pub fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The canvas background colour `style` asks for.
pub fn background_color(style: &ChartStyle) -> egui::Color32 {
    color32(style.canvas.background_rgba())
}

/// The chart-grid colour `style` asks for. `TRANSPARENT` disables grid painting
/// without branching throughout the axis code.
pub fn grid_color(style: &ChartStyle) -> egui::Color32 {
    style
        .canvas
        .grid_rgba()
        .map_or(egui::Color32::TRANSPARENT, color32)
}
