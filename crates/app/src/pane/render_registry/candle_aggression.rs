//! One quiet, opt-in weighted-price dot per candle from its native trade ladder.
use super::{Contribution, FootprintPass, Package};
use crate::orderflow_render::{PIE_START_ANGLE, SphereShading, add_shaded_sector};
use crate::theme;
use eframe::egui;
use quantick_layers::ChartLayer;
use quantick_orderflow::projection::{
    CandleDotView, CandleFootprint, CandleFootprintSource, PriceWindow, project_candle_dots,
};
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
    let first = frame.visible.0.saturating_sub(frame.first_state_slot);
    let end = frame.visible.1.saturating_sub(frame.first_state_slot);
    let closed = frame
        .footprints
        .iter()
        .enumerate()
        .skip(first)
        .take(end.saturating_sub(first))
        .map(|(index, ladder)| (frame.first_state_slot + index, ladder));
    let partial = pass
        .current_partial
        .filter(|_| frame.visible.0 <= frame.partial_slot && frame.partial_slot < frame.visible.1)
        .map(|ladder| (frame.partial_slot, ladder));
    let projected = project_candle_dots(
        closed.chain(partial).map(|(slot, ladder)| CandleFootprint {
            slot,
            ladder,
            source: CandleFootprintSource::TradeBuilt,
        }),
        pass.native_grid,
        CandleDotView {
            prices,
            candle_width_px: frame.candle_width,
        },
    );
    let buy = theme::BUY.gamma_multiply(DOT_OPACITY);
    let sell = theme::SELL.gamma_multiply(DOT_OPACITY);
    let mut mesh = egui::Mesh::default();
    for dot in projected.marks {
        let Some(price) = dot.price.to_f64() else {
            continue;
        };
        let center = egui::pos2((frame.x_center)(dot.slot), frame.scale.y(price));
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
