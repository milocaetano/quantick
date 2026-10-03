//! Keyboard bindings and visible hints for candle layers.
use super::ChartLayer;
use eframe::egui;

pub(crate) fn binding(layer: ChartLayer) -> Option<egui::KeyboardShortcut> {
    let key = match layer {
        ChartLayer::Bubbles => egui::Key::B,
        ChartLayer::Footprint => egui::Key::F,
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(egui::Modifiers::CTRL, key))
}

pub(crate) fn label(layer: ChartLayer) -> Option<&'static str> {
    match layer {
        ChartLayer::Bubbles => Some("Ctrl+B"),
        ChartLayer::Footprint => Some("Ctrl+F"),
        _ => None,
    }
}
