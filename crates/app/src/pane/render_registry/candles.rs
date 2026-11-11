use super::super::draw_frame::DrawFrame;
use super::{Contribution, Package};
use crate::pane::constants::SIDEBAR_BODY_FRAC;
use crate::{
    candle_view::{BarSlot, draw_candle, draw_pixel_candle, is_bullish},
    indicators::IndicatorViews,
    viewport::Viewport,
};
use eframe::egui;
use quantick_chart::candle_casing::bar_casing;
use quantick_chart::candle_pixels::{CandleRun, PixelBodies};
use quantick_stores::footprint_presentation::CandleStyles;
use rust_decimal::prelude::ToPrimitive as _;
pub(super) const PACKAGE: Package = Package {
    layers: &[],
    contributions: &[
        Contribution::CandleClear(clear),
        Contribution::Candles(paint),
    ],
};
pub(in crate::pane) struct CandlePass<'a, 'b> {
    pub frame: &'a DrawFrame<'b>,
    pub painter: &'a egui::Painter,
    pub viewport: &'a Viewport,
    pub indicators: &'a IndicatorViews,
    pub clear_depth: bool,
    pub half: f32,
    pub candle_lane: f32,
    pub content_half: f32,
    /// Dressed by the footprint only where a bar has a ladder.
    pub styles: CandleStyles<'a>,
    /// Candles on whole device pixels, and which width they take
    /// (`quantick_chart::candle_pixels::pixel_bodies`).
    pub pixel: Option<PixelBodies>,
}
impl CandlePass<'_, '_> {
    fn slot(&self, index: usize) -> BarSlot {
        let xc = self
            .viewport
            .x_center(index, self.frame.right, self.frame.total);
        let lane = self.styles.lane(index, self.candle_lane);
        if lane > 0.0 {
            let sliver = (lane * SIDEBAR_BODY_FRAC).max(1.0);
            BarSlot {
                xc: xc - self.content_half + sliver + 1.0,
                half_width: sliver,
            }
        } else {
            BarSlot {
                xc,
                half_width: self.half,
            }
        }
    }
    /// The run this frame's whole-pixel candles are laid out in, if any.
    fn pixel_run(&self) -> Option<CandleRun> {
        let bodies = self.pixel.filter(|_| self.candle_lane <= 0.0)?;
        let ppp = self.painter.ctx().pixels_per_point();
        let (pitch, body, style) = (self.frame.cw, 2.0 * self.half, self.styles.frame());
        Some(CandleRun::for_frame(bodies, pitch, body, style, ppp))
    }
    fn bars(&self, mut visit: impl FnMut(usize, &quantick_engine::Bar, bool)) {
        for (offset, bar) in self
            .frame
            .visible_prefix
            .iter()
            .chain(self.frame.visible_state)
            .enumerate()
        {
            visit(self.frame.closed_start + offset, bar, false);
        }
        if let Some(bar) = self.frame.partial_visible {
            visit(self.frame.closed_total, bar, true);
        }
    }
}
fn clear(pass: &mut CandlePass<'_, '_>) {
    if !pass.clear_depth {
        return;
    }
    pass.bars(|index, bar, _| {
        let xc = pass
            .viewport
            .x_center(index, pass.frame.right, pass.frame.total);
        let (top, bottom) = pass.frame.scale.band(
            bar.high.to_f64().unwrap_or(0.0),
            bar.low.to_f64().unwrap_or(0.0),
        );
        pass.painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(xc - pass.half, top),
                egui::pos2(xc + pass.half, bottom),
            ),
            egui::Rounding::ZERO,
            pass.frame.canvas_background,
        );
    });
}
fn paint(pass: &mut CandlePass<'_, '_>) {
    let painted = pass.indicators.paints_any();
    let mut run = pass.pixel_run();
    let color = |index: usize, forming| {
        painted
            .then(|| pass.indicators.slot_paint(index..index + 1, forming))
            .flatten()
    };
    let resolved = |index, bar: &quantick_engine::Bar, forming| {
        let rgba = color(index, forming).map(|c| [c.r, c.g, c.b, c.a]);
        let style = pass.styles.of(index);
        style.resolved_painted(is_bullish(bar), forming, rgba)
    };
    let palette = crate::theme::get(pass.painter.ctx());
    if let Some(width) = palette.resolved.candle_casing_px {
        // Every plate before any candle: a plate never covers its neighbour.
        let (mut plates, ink, ppp) = (run, palette.casing(), pass.painter.ctx().pixels_per_point());
        pass.bars(|index, bar, forming| {
            let BarSlot { xc, half_width } = pass.slot(index);
            let paint = resolved(index, bar, forming);
            let bar = (&pass.frame.scale, bar);
            for plate in bar_casing(plates.as_mut(), bar, (xc, half_width), &paint, (ppp, width)) {
                let min = egui::pos2(plate.left, plate.top);
                let rect = egui::Rect::from_min_max(min, egui::pos2(plate.right, plate.bottom));
                pass.painter.rect_filled(rect, egui::Rounding::ZERO, ink);
            }
        });
    }
    pass.bars(|index, bar, forming| {
        let color = color(index, forming);
        let style = pass.styles.of(index);
        if let Some(run) = run.as_mut() {
            let xc = pass.slot(index).xc;
            let paint = resolved(index, bar, forming);
            draw_pixel_candle(pass.painter, run, xc, &pass.frame.scale, bar, &paint);
            return;
        }
        draw_candle(
            pass.painter,
            pass.slot(index),
            &pass.frame.scale,
            bar,
            forming,
            style,
            color,
        );
    });
}
