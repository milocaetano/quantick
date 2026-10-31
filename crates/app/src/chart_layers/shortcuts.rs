//! Keyboard bindings and visible hints for candle layers.
use super::ChartLayer;
use eframe::egui;

/// On a focused flow pane: the tape alone, and from there back beside the
/// candles. Read before the new-tab shortcut, whose `Ctrl+T` it would also match.
pub(crate) const TAPE_VIEW_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::T,
);

/// Consumes the tape key however it is held; answers whether it asks for a
/// change: only a fresh press outside a text field, as a held key's repeats
/// would strobe the views.
pub(crate) fn take_tape_view_press(ctx: &egui::Context) -> bool {
    let shortcut = TAPE_VIEW_SHORTCUT;
    let fresh = ctx.input_mut(|input| {
        let fresh = input.events.iter().any(|event| {
            matches!(event, egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                repeat: false,
                ..
            } if *key == shortcut.logical_key && modifiers.matches_logically(shortcut.modifiers))
        });
        input.consume_shortcut(&shortcut);
        fresh
    });
    fresh && !ctx.wants_keyboard_input()
}

pub(crate) fn binding(layer: ChartLayer) -> Option<egui::KeyboardShortcut> {
    let key = match layer {
        ChartLayer::Bubbles => egui::Key::B,
        ChartLayer::Footprint => egui::Key::F,
        _ => return None,
    };
    Some(egui::KeyboardShortcut::new(egui::Modifiers::CTRL, key))
}
