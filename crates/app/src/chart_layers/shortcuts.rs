//! Keyboard bindings and visible hints for candle layers.
use super::ChartLayer;
use eframe::egui;

/// On a focused flow pane: the tape alone, and from there back beside the
/// candles. Read before the new-tab shortcut, whose `Ctrl+T` it would also match.
pub(crate) const TAPE_VIEW_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::T,
);

pub(crate) fn binding(layer: ChartLayer) -> Option<egui::KeyboardShortcut> {
    let key = match layer {
        ChartLayer::Bubbles => egui::Key::B,
        ChartLayer::Footprint => egui::Key::F,
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(egui::Modifiers::CTRL, key))
}
