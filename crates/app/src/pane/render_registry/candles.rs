use super::super::draw_frame::DrawFrame;
use super::{Contribution, Package};
use crate::pane::constants::SIDEBAR_BODY_FRAC;
use crate::{
    candle_view::{BarSlot, draw_candle, draw_pixel_candle, is_bullish},
    indicators::IndicatorViews,
    viewport::Viewport,
};
use eframe::egui;
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
        if self.candle_lane > 0.0 {
            let sliver = (self.candle_lane * SIDEBAR_BODY_FRAC).max(1.0);
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
    pass.bars(|index, bar, forming| {
        let color = painted
            .then(|| pass.indicators.slot_paint(index..index + 1, forming))
            .flatten();
        let style = pass.styles.of(index);
        if let Some(run) = run.as_mut() {
            let xc = pass.slot(index).xc;
            let rgba = color.map(|c| [c.r, c.g, c.b, c.a]);
            let paint = style.resolved_painted(is_bullish(bar), forming, rgba);
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
