//! Renderer adapters for the headless contrast rules.

use super::{BUY, CANVAS, CHIP_INK, SELL, TEXT_PRIMARY};
use eframe::egui::Color32;
use quantick_appearance::{PremultipliedRgba, color, contrast};

fn headless(color: Color32) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied(color.to_array())
}

fn rendered(color: PremultipliedRgba) -> Color32 {
    let [r, g, b, a] = color.to_array();
    Color32::from_rgba_premultiplied(r, g, b, a)
}

pub fn ink(side: quantick_engine::Side) -> Color32 {
    rendered(contrast::side_ink(headless(side_color(side))))
}

pub const fn side_color(side: quantick_engine::Side) -> Color32 {
    match side {
        quantick_engine::Side::Buy => BUY,
        quantick_engine::Side::Sell => SELL,
    }
}

pub fn active_tint(accent: Color32) -> Color32 {
    rendered(color::rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        contrast::ACTIVE_TINT_ALPHA,
    ))
}

pub fn press_tint(accent: Color32) -> Color32 {
    rendered(color::rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        contrast::PRESS_TINT_ALPHA,
    ))
}

pub fn ink_on(fill: Color32) -> Color32 {
    rendered(contrast::ink_on(
        headless(fill),
        headless(CANVAS),
        headless(CHIP_INK),
        headless(TEXT_PRIMARY),
    ))
}

#[cfg(test)]
pub fn relative_luminance(color: Color32) -> f32 {
    contrast::relative_luminance(headless(color))
}

#[cfg(test)]
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    contrast::contrast_ratio(headless(a), headless(b))
}
