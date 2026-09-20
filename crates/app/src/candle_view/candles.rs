//! One bar as egui shapes: the body, the wick and the slot they occupy.

use eframe::egui;
use quantick_engine::Bar;
use quantick_indicators::Rgba8;

use crate::chart::{PriceScale, VerticalSegment, candle_geometry};
use crate::style::CandleStyle;

/// Whether a bar reads as bullish, and so which of the two candle colours it
/// wears.
///
/// A bar that closed exactly where it opened counts as bullish, which also
/// makes a freshly opened forming bar — where close still equals open — start
/// on the up colour rather than flickering.
///
/// Shared so that everything colouring by direction agrees by construction:
/// the candle and the last-price chip sit on the same canvas at the same
/// price, and a reader would take any disagreement between them as data.
#[must_use]
pub fn is_bullish(bar: &Bar) -> bool {
    bar.close >= bar.open
}

/// Where one candle's column sits on screen: the centre of its slot and half
/// the body width, both in pixels. The pair travels together because neither
/// says anything alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSlot {
    /// Centre x of the bar's slot.
    pub xc: f32,
    /// Half the body width.
    pub half_width: f32,
}

/// Draw one candle from pure geometry and a resolved paint description.
///
/// The heatmap is painted before this function and aggression bubbles after it.
/// Translucent or absent fills therefore reveal liquidity without allowing
/// candle contours to cover the aggression markers.
///
/// `bar_paint` is the colour an indicator asked this bar to wear (Pine's
/// `barcolor`); `None` — the ordinary case — draws the trader's own direction
/// colours.
pub fn draw_candle(
    painter: &egui::Painter,
    slot: BarSlot,
    scale: &PriceScale,
    bar: &Bar,
    forming: bool,
    style: &CandleStyle,
    bar_paint: Option<Rgba8>,
) {
    let paint = style.resolved_painted(
        is_bullish(bar),
        forming,
        bar_paint.map(|c| [c.r, c.g, c.b, c.a]),
    );
    let geometry = candle_geometry(scale, bar, slot.xc, slot.half_width, paint.min_body_height);
    let body = egui::Rect::from_min_max(
        egui::pos2(geometry.body.left, geometry.body.top),
        egui::pos2(geometry.body.right, geometry.body.bottom),
    );

    if let Some(wick) = paint.wick {
        let stroke = egui::Stroke::new(paint.wick_width, color32(wick));
        if let Some(segment) = geometry.upper_wick {
            draw_vertical_segment(painter, segment, stroke);
        }
        if let Some(segment) = geometry.lower_wick {
            draw_vertical_segment(painter, segment, stroke);
        }
    }

    let body_width = geometry.body.width();
    let body_height = geometry.body.height();
    let radius = paint
        .corner_radius
        .min(body_width / 2.0)
        .min(body_height / 2.0)
        .max(0.0);
    let rounding = egui::Rounding::same(radius);
    if let Some(fill) = paint.fill
        && fill[3] > 0
    {
        painter.rect_filled(body, rounding, color32(fill));
    }

    if paint.outline[3] > 0 {
        // Keep very thick strokes from swallowing candles at the minimum zoom.
        let width = paint
            .outline_width
            .min(body_width.max(0.5))
            .min(body_height.max(0.5));
        painter.rect_stroke(
            body,
            rounding,
            egui::Stroke::new(width, color32(paint.outline)),
        );
    }
}

pub fn draw_vertical_segment(painter: &egui::Painter, segment: VerticalSegment, stroke: egui::Stroke) {
    if segment.length() > 0.0 {
        painter.line_segment(
            [
                egui::pos2(segment.x, segment.top),
                egui::pos2(segment.x, segment.bottom),
            ],
            stroke,
        );
    }
}

pub fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}
