//! Historical Dark fixtures for renderer regression tests.
use eframe::egui::Color32;
const fn rendered(color: quantick_appearance::PremultipliedRgba) -> Color32 {
    let [r, g, b, a] = color.to_array();
    Color32::from_rgba_premultiplied(r, g, b, a)
}
pub const ACCENT: Color32 = rendered(quantick_appearance::palette::ACCENT);
pub const AMBER: Color32 = rendered(quantick_appearance::palette::AMBER);
pub const BARE_HOVER: Color32 = rendered(quantick_appearance::palette::BARE_HOVER);
pub const BORDER: Color32 = rendered(quantick_appearance::palette::BORDER);
pub const BUY: Color32 = rendered(quantick_appearance::palette::BUY);
pub const CANVAS: Color32 = rendered(quantick_appearance::palette::CANVAS);
pub const CASING: Color32 = rendered(quantick_appearance::palette::CASING);
pub const CHIP_INK: Color32 = rendered(quantick_appearance::palette::CHIP_INK);
pub const CHROME: Color32 = rendered(quantick_appearance::palette::CHROME);
pub const CONTROL: Color32 = rendered(quantick_appearance::palette::CONTROL);
pub const DRAW_CYAN: Color32 = rendered(quantick_appearance::palette::DRAW_CYAN);
pub const POC: Color32 = rendered(quantick_appearance::palette::POC);
pub const SELL: Color32 = rendered(quantick_appearance::palette::SELL);
pub const TEXT_MUTED: Color32 = rendered(quantick_appearance::palette::TEXT_MUTED);
pub const TEXT_PRIMARY: Color32 = rendered(quantick_appearance::palette::TEXT_PRIMARY);

pub const FLOW_SURFACE: Color32 = Color32::from_rgb(30, 36, 51);
pub const FLOW_KEY_DOWN: Color32 = Color32::from_rgb(20, 25, 36);
pub const FLOW_KEY_DOWN_RIM: Color32 = Color32::from_black_alpha(102);
pub const FLOW_KEY_UP_TOP: Color32 = Color32::from_rgb(61, 72, 94);
pub const FLOW_KEY_UP_BOTTOM: Color32 = Color32::from_rgb(46, 55, 73);
pub const FLOW_KEY_UP_LIP: Color32 = Color32::from_rgba_premultiplied(56, 56, 56, 56);
pub const FLOW_KEY_UP_SHADOW: Color32 = Color32::from_black_alpha(128);
pub const WARN: Color32 = rendered(quantick_appearance::palette::WARN);
