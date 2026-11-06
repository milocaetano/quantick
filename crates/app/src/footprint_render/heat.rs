//! Renderer conversion for the configuration-owned footprint heat ramp.
use eframe::egui;
pub(super) use quantick_chart::footprint_projection::heat::{HeatScale, heat_scale, heat_step};
use quantick_engine::Side;

pub(super) const HEAT_INK_FLIP_STEP: usize = 4;

pub(super) fn heat_fill(palette: &crate::theme::Palette, side: Side, step: usize) -> egui::Color32 {
    (match side {
        Side::Buy => palette.heat_buy,
        Side::Sell => palette.heat_sell,
    })[step.min(5)]
}
pub(super) fn heat_ink(palette: &crate::theme::Palette, step: usize) -> egui::Color32 {
    palette.heat_ink[step.min(5)]
}
