use super::super::draw_frame::DrawFrame;
use super::{Contribution, Package};
use crate::{
    candle_view::{BarSlot, draw_candle},
    indicators::IndicatorViews,
    style::CandleStyle,
    viewport::Viewport,
};
use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;
const SIDEBAR_BODY_FRAC: f32 = 0.35;
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
    pub style: &'a CandleStyle,
}
impl CandlePass<'_, '_> {
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
    pass.bars(|index, bar, forming| {
        let xc = pass
            .viewport
            .x_center(index, pass.frame.right, pass.frame.total);
        let color = painted
            .then(|| pass.indicators.slot_paint(index..index + 1, forming))
            .flatten();
        let slot = if pass.candle_lane > 0.0 {
            let sliver = (pass.candle_lane * SIDEBAR_BODY_FRAC).max(1.0);
            BarSlot {
                xc: xc - pass.content_half + sliver + 1.0,
                half_width: sliver,
            }
        } else {
            BarSlot {
                xc,
                half_width: pass.half,
            }
        };
        draw_candle(
            pass.painter,
            slot,
            &pass.frame.scale,
            bar,
            forming,
            pass.style,
            color,
        );
    });
}
