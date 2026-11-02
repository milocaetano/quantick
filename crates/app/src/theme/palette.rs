//! Historical Dark fixtures for renderer regression tests.
use eframe::egui::Color32;
const fn rendered(color: quantick_appearance::PremultipliedRgba) -> Color32 {
    let [r, g, b, a] = color.to_array();
    Color32::from_rgba_premultiplied(r, g, b, a)
}
pub const ACCENT: Color32 = rendered(quantick_appearance::palette::ACCENT);
pub const BORDER: Color32 = rendered(quantick_appearance::palette::BORDER);
pub const BUY: Color32 = rendered(quantick_appearance::palette::BUY);
pub const CANVAS: Color32 = rendered(quantick_appearance::palette::CANVAS);
pub const CASING: Color32 = rendered(quantick_appearance::palette::CASING);
pub const CHIP_INK: Color32 = rendered(quantick_appearance::palette::CHIP_INK);
pub const CONTROL: Color32 = rendered(quantick_appearance::palette::CONTROL);
pub const DRAW_CYAN: Color32 = rendered(quantick_appearance::palette::DRAW_CYAN);
pub const POC: Color32 = rendered(quantick_appearance::palette::POC);
pub const SELL: Color32 = rendered(quantick_appearance::palette::SELL);
pub const TEXT_MUTED: Color32 = rendered(quantick_appearance::palette::TEXT_MUTED);
pub const TEXT_PRIMARY: Color32 = rendered(quantick_appearance::palette::TEXT_PRIMARY);
