//! One quiet, opt-in weighted-price dot per candle, or per held group of
//! candles when zoomed out, from the native trade ladders.
use super::{Contribution, FootprintPass, Package};
use crate::orderflow_render::{PIE_START_ANGLE, SphereShading, add_shaded_sector};
use crate::theme;
use eframe::egui;
use quantick_layers::ChartLayer;
use quantick_orderflow::projection::{CandleDotView, PriceWindow, project_candle_dots};
use rust_decimal::{
    Decimal,
    prelude::{FromPrimitive as _, ToPrimitive as _},
};

const DOT_OPACITY: f32 = 0.35;
pub(super) const PACKAGE: Package = Package {
    layers: &[ChartLayer::CandleAggression],
    contributions: &[Contribution::Footprint(paint)],
};

fn paint(pass: &mut FootprintPass<'_>) {
    if !pass.candle_aggression {
        return;
    }
    let frame = pass.frame;
    let (low, high) = frame.scale.range();
    let Some(prices) = Decimal::from_f64(low)
        .zip(Decimal::from_f64(high))
        .and_then(|(low, high)| PriceWindow::new(low, high))
    else {
        return;
    };
    let view = CandleDotView {
        prices,
        candle_width_px: frame.candle_width,
        visible: frame.visible,
        candles_per_mark: pass.lod.candle_groups.choose(frame.candle_width),
    };
    let partial = pass
        .current_partial
        .map(|ladder| (frame.partial_slot, ladder));
    let inputs = view.trade_built(frame.footprints, frame.first_state_slot, partial);
    let projected = project_candle_dots(inputs, pass.native_grid, view);
    let buy = theme::BUY.gamma_multiply(DOT_OPACITY);
    let sell = theme::SELL.gamma_multiply(DOT_OPACITY);
    let mut mesh = egui::Mesh::default();
    for dot in projected.marks {
        let Some(price) = dot.price.to_f64() else {
            continue;
        };
        // The group's centre: its first and last candle, halfway.
        let x = ((frame.x_center)(dot.slot) + (frame.x_center)(dot.last_slot)) / 2.0;
        let center = egui::pos2(x, frame.scale.y(price));
        if dot.buy_quantity.is_zero() || dot.sell_quantity.is_zero() {
            frame.painter.circle_filled(
                center,
                dot.radius_px,
                if dot.buy_quantity.is_zero() {
                    sell
                } else {
                    buy
                },
            );
        } else {
            let total = dot.buy_quantity.saturating_add(dot.sell_quantity);
            let share = dot
                .buy_quantity
                .checked_div(total)
                .and_then(|ratio| ratio.to_f32())
                .unwrap_or(0.5);
            let sweep = share * std::f32::consts::TAU;
            add_shaded_sector(
                &mut mesh,
                center,
                dot.radius_px,
                PIE_START_ANGLE,
                sweep,
                SphereShading::flat(buy),
            );
            add_shaded_sector(
                &mut mesh,
                center,
                dot.radius_px,
                PIE_START_ANGLE + sweep,
                std::f32::consts::TAU - sweep,
                SphereShading::flat(sell),
            );
        }
    }
    if !mesh.is_empty() {
        frame.painter.add(egui::Shape::mesh(mesh));
    }
}
