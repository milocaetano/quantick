//! Renderer adapters for the headless contrast rules.

use super::resolved::rendered;
#[cfg(test)]
use super::{BUY, SELL};
use eframe::egui::Color32;
use quantick_appearance::{PremultipliedRgba, contrast};

fn headless(color: Color32) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied(color.to_array())
}

#[cfg(test)]
pub fn ink(side: quantick_engine::Side) -> Color32 {
    rendered(contrast::side_ink(headless(side_color(side))))
}

#[cfg(test)]
pub const fn side_color(side: quantick_engine::Side) -> Color32 {
    match side {
        quantick_engine::Side::Buy => BUY,
        quantick_engine::Side::Sell => SELL,
    }
}

impl super::Palette {
    pub fn side_color(&self, side: quantick_engine::Side) -> Color32 {
        self.side_color_for(side == quantick_engine::Side::Buy)
    }
    pub fn ink(&self, side: quantick_engine::Side) -> Color32 {
        self.side_ink_for(side == quantick_engine::Side::Buy)
    }
    pub fn ink_on(&self, fill: Color32) -> Color32 {
        rendered(self.resolved.ink_on(headless(fill)))
    }
}

pub fn with_opacity(color: Color32, alpha: u8) -> Color32 {
    rendered(quantick_appearance::color::with_opacity(
        headless(color),
        alpha,
    ))
}

pub fn active_tint(accent: Color32) -> Color32 {
    with_opacity(accent, contrast::ACTIVE_TINT_ALPHA)
}
pub fn press_tint(accent: Color32) -> Color32 {
    with_opacity(accent, contrast::PRESS_TINT_ALPHA)
}
#[cfg(test)]
pub fn relative_luminance(color: Color32) -> f32 {
    contrast::relative_luminance(headless(color))
}
