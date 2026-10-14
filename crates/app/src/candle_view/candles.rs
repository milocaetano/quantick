//! One bar as egui shapes: the body, the wick and the slot they occupy.

use eframe::egui;
use quantick_engine::Bar;
use quantick_indicators::Rgba8;

use super::{BarSlot, is_bullish};
use crate::chart::{PriceScale, VerticalSegment, candle_geometry};
use crate::style::CandleStyle;

pub struct CandleSurface<'a> {
    pub style: &'a CandleStyle,
    pub contour_backing: Option<egui::Color32>,
}

/// Draw one candle from pure geometry and a resolved paint description.
///
/// The heatmap and regional FLOW circles precede this function; native Tape
/// and ordinary aggression layers keep their own later passes. Candle body
/// mode and alpha always come from the configured style.
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
    surface: CandleSurface<'_>,
    bar_paint: Option<Rgba8>,
) {
    let paint = surface.style.resolved_painted(
        is_bullish(bar),
        forming,
        bar_paint.map(|c| [c.r, c.g, c.b, c.a]),
    );
    let geometry = candle_geometry(scale, bar, slot.xc, slot.half_width, paint.min_body_height);
    let body = egui::Rect::from_min_max(
        egui::pos2(geometry.body.left, geometry.body.top),
        egui::pos2(geometry.body.right, geometry.body.bottom),
    );

    let body_width = geometry.body.width();
    let body_height = geometry.body.height();
    let radius = paint
        .corner_radius
        .min(body_width / 2.0)
        .min(body_height / 2.0)
        .max(0.0);
    let rounding = egui::Rounding::same(radius);
    let outline_width = paint
        .outline_width
        .min(body_width.max(0.5))
        .min(body_height.max(0.5));
    if let Some(background) = surface.contour_backing {
        if paint.outline[3] > 0 {
            let width = quantick_chart::candle_slot::contour_backing_width(
                outline_width,
                body_width.min(body_height),
            );
            painter.rect_stroke(body, rounding, egui::Stroke::new(width, background));
        }
        if paint.wick.is_some_and(|wick| wick[3] > 0) {
            let width = paint.wick_width + 2.0 * quantick_chart::candle_slot::CONTOUR_CLEARANCE_PX;
            for segment in [geometry.upper_wick, geometry.lower_wick]
                .into_iter()
                .flatten()
            {
                draw_vertical_segment(painter, segment, egui::Stroke::new(width, background));
            }
        }
    }
    if let Some(wick) = paint.wick {
        let stroke = egui::Stroke::new(paint.wick_width, color32(wick));
        for segment in [geometry.upper_wick, geometry.lower_wick]
            .into_iter()
            .flatten()
        {
            draw_vertical_segment(painter, segment, stroke);
        }
    }
    if let Some(fill) = paint.fill
        && fill[3] > 0
    {
        painter.rect_filled(body, rounding, color32(fill));
    }

    if paint.outline[3] > 0 {
        // Keep very thick strokes from swallowing candles at the minimum zoom.
        painter.rect_stroke(
            body,
            rounding,
            egui::Stroke::new(outline_width, color32(paint.outline)),
        );
    }
}

pub fn draw_vertical_segment(
    painter: &egui::Painter,
    segment: VerticalSegment,
    stroke: egui::Stroke,
) {
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
